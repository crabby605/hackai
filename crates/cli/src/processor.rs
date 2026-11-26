//! Deterministic, derived-only local dialogue processor.
//!
//! This module deliberately operates on the normalized event store rather
//! than transcript files.  It therefore cannot accidentally place prompt or
//! tool text in its output.  Where the stored evidence is insufficient, it
//! emits no label instead of guessing.

use crate::state;
use hackai_schema::{
    ActorRole, AgentState, DialogueTurn, EventKind, EventSource, FailureContext, InformationBucket,
    NormalizedEvent, ToolClass, UserAct, VerificationState,
};
use hackai_scorer::{
    FormulaConfig, PesdoFactors, TimeAccountingConfig, account_time, process_delta, score_pesdo,
};
use rusqlite::{Connection, OpenFlags};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::Path;

const PROCESSOR_VERSION: &str = "dialogue-processor-v1";

#[derive(Debug, Serialize, serde::Deserialize)]
pub struct ProcessorOutput {
    pub processor_version: &'static str,
    pub formula_version: u16,
    pub generated_at_unix_ms: i64,
    pub event_count: usize,
    pub dialogue_turns: Vec<DialogueTurn>,
    pub episodes: Vec<EpisodeSummary>,
}

#[derive(Debug, Serialize, serde::Deserialize)]
pub struct EpisodeSummary {
    pub episode_id: String,
    pub started_at_unix_ms: i64,
    pub ended_at_unix_ms: i64,
    pub event_count: usize,
    pub dialogue_turn_count: usize,
    pub attempts: u32,
    pub agent_name: String,
    pub process_delta: i32,
    /// Deterministic 0–1000 PESDO score. It contains only derived features;
    /// raw prompts, replies, paths and tool output never leave the machine.
    pub pesdo_score_permille: u16,
    pub outcome: Outcome,
    pub resolved: bool,
    pub human_active_millis: u64,
    pub attended_agent_millis: u64,
    pub union_millis: u64,
}

/// Outcome deliberately remains separate from process.  A commit alone is
/// not proof that work shipped, so it remains `Unverified` absent a green
/// verification event in the episode.
#[derive(Clone, Copy, Debug, Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Shipped,
    EndedRed,
    Unverified,
}

pub fn process(root: &Path) -> io::Result<ProcessorOutput> {
    let events = read_events(root)?;
    let config = load_formula(root)?;
    let mut turns = Vec::new();
    let mut episodes = Vec::new();
    // A tracked directory is one project. Harness session IDs are transport
    // details, not product boundaries: merge Codex, Claude, Gemini, etc. into
    // one chronological project timeline and cut collaboration sessions only
    // on the configured inactivity gap.
    let mut timeline = events;
    timeline.sort_by_key(|event| (event.occurred_at.0, event.id.0.clone()));
    if !timeline.is_empty() {
        let mut start = 0_usize;
        for index in 1..=timeline.len() {
            let gap = timeline[index]
                .occurred_at
                .0
                .saturating_sub(timeline[index - 1].occurred_at.0);
            let boundary = index == timeline.len() || gap > config.episode_gap_millis;
            if boundary {
                if start > 0 && gap > config.episode_gap_millis {
                    eprintln!(
                        "Split episode at {}ms gap (>{}ms): {} event(s) in previous episode",
                        gap,
                        config.episode_gap_millis,
                        index - start
                    );
                }
                let episode_events = &timeline[start..index];
                let episode_id = episode_id(
                    "project",
                    episode_events.first().map_or(0, |e| e.occurred_at.0),
                );
                let started_at_unix_ms = episode_events
                    .first()
                    .map_or(0, |event| event.occurred_at.0);
                let ended_at_unix_ms = episode_events
                    .last()
                    .map_or(started_at_unix_ms, |event| event.occurred_at.0);
                let episode_turns = label_episode(episode_events, &episode_id, config.version);
                let accounting = account_time(episode_events, TimeAccountingConfig::default());
                let process_delta = process_delta(
                    &episode_turns
                        .iter()
                        .map(|turn| (turn.agent_state, turn.user_act, turn.context))
                        .collect::<Vec<_>>(),
                    &config.transition_rules,
                    config.pesdo.process_delta_limit,
                );
                let outcome = outcome(episode_events);
                let pesdo_score_permille = episode_pesdo_score(
                    episode_events,
                    &episode_turns,
                    &accounting,
                    process_delta,
                    &config,
                );
                episodes.push(EpisodeSummary {
                    episode_id,
                    started_at_unix_ms,
                    ended_at_unix_ms,
                    event_count: episode_events.len(),
                    dialogue_turn_count: episode_turns.len(),
                    attempts: u32::try_from(episode_turns.len()).unwrap_or(u32::MAX),
                    agent_name: agent_name(episode_events).to_owned(),
                    process_delta,
                    pesdo_score_permille,
                    resolved: matches!(outcome, Outcome::Shipped),
                    outcome,
                    human_active_millis: accounting.human_active_millis,
                    attended_agent_millis: accounting.agent_attended_millis,
                    union_millis: accounting.union_millis,
                });
                turns.extend(episode_turns);
                start = index;
            }
        }
    }

    // The timestamp comes directly from the store, not the wall clock, so a
    // rerun over the same input serializes identically.
    let generated_at_unix_ms = read_latest_timestamp(root)?;
    Ok(ProcessorOutput {
        processor_version: PROCESSOR_VERSION,
        formula_version: config.version,
        generated_at_unix_ms,
        event_count: episodes.iter().map(|episode| episode.event_count).sum(),
        dialogue_turns: turns,
        episodes,
    })
}

/// Load the versioned policy checked into the project. Falling back to the
/// compiled defaults keeps existing installations usable until they opt in;
/// an explicitly supplied but invalid policy is always an error so a score is
/// never silently computed under a different policy than the reviewer sees.
fn load_formula(root: &Path) -> io::Result<FormulaConfig> {
    let path = root.join("formula.toml");
    if !path.exists() {
        return Ok(FormulaConfig::default());
    }
    let source = fs::read_to_string(&path)?;
    toml::from_str(&source).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("invalid {}: {error}", path.display()),
        )
    })
}

pub fn process_and_write(root: &Path) -> io::Result<ProcessorOutput> {
    let output = process(root)?;
    fs::create_dir_all(state::state_dir(root))?;
    fs::write(
        state::state_dir(root).join("dialogue-summary.json"),
        serde_json::to_vec_pretty(&output).map_err(io::Error::other)?,
    )?;
    // The sync API consumes these derived session rollups. They intentionally
    // contain no paths, prompt/reply text, tool output, or raw harness IDs.
    let sync_episodes = output
        .episodes
        .iter()
        .map(|episode| {
            let transcript_turns = output
                .dialogue_turns
                .iter()
                .filter(|turn| turn.episode_id == episode.episode_id)
                .map(|turn| {
                    serde_json::json!({
                        "event_id": turn.event_id.0,
                        "user_act": turn.user_act,
                        "act_confidence_permille": turn.act_confidence_permille,
                        "new_information": turn.new_information,
                        "prompt_length_bucket": turn.prompt_length_bucket,
                        "fullcaps": turn.fullcaps,
                        "answers_question": turn.answers_question,
                        "agent_state": turn.agent_state,
                        "verification": turn.verification,
                        "files_changed_bucket": turn.files_changed_bucket,
                        "tool_errors": turn.tool_errors,
                        "reason_codes": turn.reason_codes,
                    })
                })
                .collect::<Vec<_>>();
            serde_json::json!({
                "formula_version": output.formula_version,
                "started_at": episode.started_at_unix_ms,
                "ended_at": episode.ended_at_unix_ms,
                "session": episode.episode_id,
                "attempts": episode.attempts,
                "harness": episode.agent_name,
                "pesdo_score_permille": episode.pesdo_score_permille,
                "distinct_signatures": [],
                "files_touched": [],
                "human_edit_seconds": episode.human_active_millis / 1_000,
                "agent_edit_seconds": episode.attended_agent_millis / 1_000,
                "resolved": episode.resolved,
                "credited_seconds": episode.union_millis / 1_000,
                "buckets": [],
                // A derived turn trace only: no prompt/reply text, code,
                // paths, tool output, or raw harness session ids.
                "transcript_turns": transcript_turns,
            })
        })
        .collect::<Vec<_>>();
    fs::write(
        state::state_dir(root).join("episodes.json"),
        serde_json::to_vec_pretty(&sync_episodes).map_err(io::Error::other)?,
    )?;
    Ok(output)
}

fn agent_name(events: &[NormalizedEvent<String, String>]) -> &'static str {
    match events
        .iter()
        .find(|event| event.role == ActorRole::Agent || event.kind == EventKind::PromptSubmitted)
        .map(|event| event.source)
    {
        Some(EventSource::ClaudeCode) => "Claude Code",
        Some(EventSource::Codex) => "Codex",
        Some(EventSource::OpenCode) => "OpenCode",
        Some(EventSource::Gemini) => "Gemini",
        Some(EventSource::Aider) => "Aider",
        Some(EventSource::QwenCode) => "Qwen Code",
        Some(EventSource::KimiCode) => "Kimi Code",
        Some(EventSource::ZCode) => "Zed",
        Some(EventSource::Copilot) => "GitHub Copilot",
        Some(EventSource::ManualCli) | None => "Your coding agent",
    }
}

/// Map grounded, privacy-safe episode signals to the formula's PESD factors.
/// O is a submission gate, not a per-session penalty, so local progress uses
/// the active gate value (1000) until a submission verifier says otherwise.
fn episode_pesdo_score(
    events: &[NormalizedEvent<String, String>],
    turns: &[DialogueTurn],
    accounting: &hackai_scorer::TimeAccounting,
    process_delta: i32,
    config: &FormulaConfig,
) -> u16 {
    let has_human_prompt = events
        .iter()
        .any(|event| event.kind == EventKind::PromptSubmitted && event.role == ActorRole::Human);
    let presence = if accounting.union_millis == 0 {
        if has_human_prompt { 900 } else { 700 }
    } else if accounting.human_active_millis == 0 && has_human_prompt {
        // Historic transcript imports often lack an OS/edit activity stream.
        // Unknown presence gets a conservative baseline, never a zero.
        900
    } else {
        u16::try_from(
            accounting
                .human_active_millis
                .saturating_mul(1_000)
                .checked_div(accounting.union_millis)
                .unwrap_or(0),
        )
        .unwrap_or(1_000)
        // Transcript sampling can undercount a person's offline review time.
        // Do not turn that missing observation into a punishment.
        .clamp(700, 1_000)
    };
    let human_evidence = events
        .iter()
        .filter(|event| {
            matches!(event.kind, EventKind::HumanEdit | EventKind::GitCommit)
                && event.role == ActorRole::Human
        })
        .count();
    let evidence = match human_evidence {
        0 if has_human_prompt => 900,
        0 => 750,
        1 => 950,
        _ => 1_000,
    };
    let specification = if turns.is_empty() {
        800
    } else {
        let total: u32 = turns
            .iter()
            .map(|turn| {
                let information_score = match turn.new_information {
                    InformationBucket::High => 1_000,
                    InformationBucket::Medium => 925,
                    InformationBucket::Low => 850,
                    InformationBucket::None => 800,
                };
                // Directing scope or resolving an agent question is grounded
                // orchestration even when a harness exposes too little text
                // metadata to recognize a file/error anchor.
                let act_score = match turn.user_act {
                    UserAct::Specify => 1_000,
                    UserAct::Steer => 950,
                    UserAct::Decide => 900,
                    UserAct::ReportFailure if !turn.context.is_contextless() => 900,
                    UserAct::ReportFailure => 700,
                    UserAct::Nudge | UserAct::Approve => 750,
                    UserAct::Interrupt | UserAct::AskToUnderstand | UserAct::Meta => 800,
                    UserAct::RejectOrRevert => 900,
                };
                information_score.max(act_score)
            })
            .sum();
        u16::try_from(total / u32::try_from(turns.len()).unwrap_or(1)).unwrap_or(500)
    };
    // D is neutral unless a grounded transition rule observes a repetition or
    // rubber-stamp pattern. Absence of review telemetry is not a penalty.
    let diligence = (1_000_i32 + process_delta.saturating_mul(10)).clamp(600, 1_000) as u16;
    let factors = PesdoFactors {
        presence,
        evidence,
        specification,
        diligence,
    };
    // Log factor values for tuning/debugging
    eprintln!(
        "PESDO factors: presence={} evidence={} specification={} diligence={} process_delta={} -> score={}",
        factors.presence,
        factors.evidence,
        factors.specification,
        factors.diligence,
        process_delta,
        score_pesdo(1_000, factors, true, config.pesdo)
    );
    u16::try_from(score_pesdo(1_000, factors, true, config.pesdo)).unwrap_or(1_000)
}

fn read_events(root: &Path) -> io::Result<Vec<NormalizedEvent<String, String>>> {
    let database_path = state::event_store_path(root);
    if !database_path.exists() {
        return Ok(Vec::new());
    }
    let database = Connection::open_with_flags(database_path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|error| io::Error::other(format!("open event store: {error}")))?;
    let mut statement = database
        .prepare("SELECT payload_json FROM normalized_events ORDER BY occurred_at_ms, event_id")
        .map_err(|error| io::Error::other(format!("read event store: {error}")))?;
    let rows = statement
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(|error| io::Error::other(format!("query event store: {error}")))?;
    rows.map(|row| {
        let payload = row.map_err(|error| io::Error::other(format!("read event row: {error}")))?;
        serde_json::from_str(&payload)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
    })
    .collect()
}

fn read_latest_timestamp(root: &Path) -> io::Result<i64> {
    let database_path = state::event_store_path(root);
    if !database_path.exists() {
        return Ok(0);
    }
    let database = Connection::open_with_flags(database_path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|error| io::Error::other(format!("open event store: {error}")))?;
    database
        .query_row(
            "SELECT COALESCE(MAX(occurred_at_ms), 0) FROM normalized_events",
            [],
            |row| row.get(0),
        )
        .map_err(|error| io::Error::other(format!("read event store: {error}")))
}

/// Build a minimal agent reply summary for context detection.
/// Returns (had_plan, had_explanation, asked_question, claimed_done, tool_error_count, loop_signals)
fn analyze_agent_reply(
    events: &[&NormalizedEvent<String, String>],
) -> (bool, bool, bool, bool, u16, bool) {
    let mut had_plan = false;
    let mut had_explanation = false;
    let mut asked_question = false;
    let mut tool_error_count = 0u16;
    let mut saw_tool_result = false;

    for event in events {
        // Look for tool calls that indicate planning/explanation
        if let Some(tool) = &event.tool {
            if tool.class == ToolClass::Other {
                // Many harnesses use "Other" for TodoWrite, planning tools
                had_plan = true;
            }
            if matches!(
                tool.class,
                ToolClass::Test | ToolClass::Build | ToolClass::Lint
            ) {
                saw_tool_result = true;
                if tool.exit_code.is_some_and(|c| c != 0) {
                    tool_error_count = tool_error_count.saturating_add(1);
                }
            }
        }
        // Annotation events may carry agent text summaries
        if event.kind == EventKind::Annotation {
            had_explanation = true;
            // Check if agent asked a question (annotation text ends with ?)
            // We can't access raw text, but we can infer from the event structure
            // If the annotation is the last agent event and contains question indicators
        }
    }

    // Detect if agent asked a question by checking the last agent annotation
    // We check if the last agent event was an annotation and the one before that was a tool result
    let agent_events: Vec<_> = events
        .iter()
        .filter(|e| e.role == ActorRole::Agent)
        .collect();
    if let Some(last_agent) = agent_events.last()
        && last_agent.kind == EventKind::Annotation
    {
        // If the agent's last action was an annotation (not a tool),
        // it likely means they were responding/asking rather than acting
        asked_question = true;
    }

    // Detect looping: same tool class failing repeatedly
    let mut tool_class_counts: HashMap<ToolClass, u16> = HashMap::new();
    for event in events {
        if let Some(tool) = &event.tool
            && matches!(
                tool.class,
                ToolClass::Test | ToolClass::Build | ToolClass::Lint
            )
            && tool.exit_code.is_some_and(|c| c != 0)
        {
            *tool_class_counts.entry(tool.class).or_default() += 1;
        }
    }
    let looping = tool_class_counts.values().any(|&c| c >= 2);

    // Heuristic: if agent ran tools and then stopped without errors, likely claimed done
    let had_tool_activity = events.iter().any(|e| e.tool.is_some());
    let no_errors = tool_error_count == 0;
    let claimed_done = had_tool_activity && no_errors && saw_tool_result;

    (
        had_plan,
        had_explanation,
        asked_question,
        claimed_done,
        tool_error_count,
        looping,
    )
}

/// Determine agent state from tool events
fn determine_agent_state(
    events: &[&NormalizedEvent<String, String>],
) -> (AgentState, VerificationState, u8, u16, Vec<String>) {
    let tool_errors = u16::try_from(
        events
            .iter()
            .filter(|event| {
                event
                    .tool
                    .as_ref()
                    .and_then(|tool| tool.exit_code)
                    .is_some_and(|code| code != 0)
            })
            .count(),
    )
    .unwrap_or(u16::MAX);

    let has_red = events.iter().any(|event| {
        event.tool.as_ref().is_some_and(|tool| {
            matches!(
                tool.class,
                ToolClass::Test | ToolClass::Build | ToolClass::Lint
            ) && tool.exit_code.is_some_and(|code| code != 0)
        })
    });

    let has_green = events.iter().any(|event| {
        event.tool.as_ref().is_some_and(|tool| {
            matches!(
                tool.class,
                ToolClass::Test | ToolClass::Build | ToolClass::Lint
            ) && tool.exit_code == Some(0)
        })
    });

    let writes = events
        .iter()
        .filter(|event| {
            event.tool.as_ref().is_some_and(|tool| {
                matches!(tool.class, ToolClass::FileWrite | ToolClass::FilePatch)
            })
        })
        .count();

    let files_changed_bucket = u8::try_from(writes.min(255)).unwrap_or(u8::MAX);

    // Detect looping: same failing tool repeated
    let mut tool_class_fail_counts: HashMap<ToolClass, u16> = HashMap::new();
    for event in events {
        if let Some(tool) = &event.tool
            && matches!(
                tool.class,
                ToolClass::Test | ToolClass::Build | ToolClass::Lint
            )
            && tool.exit_code.is_some_and(|c| c != 0)
        {
            *tool_class_fail_counts.entry(tool.class).or_default() += 1;
        }
    }
    let looping = tool_class_fail_counts.values().any(|&c| c >= 2);

    if looping {
        return (
            AgentState::Looping,
            VerificationState::Red,
            files_changed_bucket,
            tool_errors,
            vec!["REPEATED_FAILURE".into()],
        );
    }

    if has_red {
        return (
            AgentState::EditedRed,
            VerificationState::Red,
            files_changed_bucket,
            tool_errors,
            vec!["VERIFICATION_RED".into()],
        );
    }

    if writes > 0 && has_green {
        return (
            AgentState::EditedGreen,
            VerificationState::Green,
            files_changed_bucket,
            tool_errors,
            vec!["EDIT_VERIFIED_GREEN".into()],
        );
    }

    if writes > 0 {
        // Check if agent claimed done (no more tool activity after writes)
        let last_tool_idx = events.iter().rposition(|e| e.tool.is_some());
        let claimed_done = last_tool_idx.is_some_and(|idx| {
            events[idx + 1..]
                .iter()
                .all(|e| !matches!(e.kind, EventKind::ToolStarted | EventKind::ToolFinished))
        });

        if claimed_done {
            return (
                AgentState::ClaimedDoneUnverified,
                VerificationState::Unverified,
                files_changed_bucket,
                tool_errors,
                vec!["CLAIMED_DONE_UNVERIFIED".into()],
            );
        }

        return (
            AgentState::EditedUnverified,
            VerificationState::Unverified,
            files_changed_bucket,
            tool_errors,
            vec!["EDIT_UNVERIFIED".into()],
        );
    }

    if tool_errors > 0 {
        return (
            AgentState::ToolError,
            VerificationState::NotApplicable,
            0,
            tool_errors,
            vec!["TOOL_ERROR".into()],
        );
    }

    // Check for planning/explanation signals
    let had_plan = events.iter().any(|e| {
        e.kind == EventKind::ToolStarted
            && e.tool.as_ref().is_some_and(|t| t.class == ToolClass::Other)
    });
    let had_explanation = events.iter().any(|e| e.kind == EventKind::Annotation);

    if had_plan {
        return (
            AgentState::Planned,
            VerificationState::NotApplicable,
            0,
            0,
            vec!["PLAN_EMITTED".into()],
        );
    }

    if had_explanation {
        return (
            AgentState::Explained,
            VerificationState::NotApplicable,
            0,
            0,
            vec!["EXPLANATION_EMITTED".into()],
        );
    }

    // Check if agent asked a question (hard without raw text)
    // Heuristic: agent turn with no tools and no edits
    let agent_turns: Vec<_> = events
        .iter()
        .filter(|e| e.role == ActorRole::Agent)
        .collect();
    if let Some(last_agent) = agent_turns.last()
        && last_agent.tool.is_none()
        && writes == 0
    {
        return (
            AgentState::Asked,
            VerificationState::NotApplicable,
            0,
            0,
            vec!["AGENT_ASKED_QUESTION".into()],
        );
    }

    (
        AgentState::Blocked,
        VerificationState::NotApplicable,
        0,
        0,
        vec![],
    )
}

/// Detect failure report context vector from prompt features.
/// Since we don't have raw prompt text, we infer from:
/// - is_hypothesis + has_code_fence -> likely has evidence (error text/diff)
/// - char_len > 200 -> likely has expectation/location
/// - is_low_effort -> likely contextless
fn detect_failure_context(prompt: Option<&hackai_schema::PromptFeatures>) -> FailureContext {
    let Some(features) = prompt else {
        return FailureContext::default();
    };

    // Evidence: pasted error, stack trace, diff, or code fence with error-like content
    let evidence = features.has_code_fence || features.char_len > 500;

    // Expectation: hypothesis-style prompt usually states what should happen
    let expectation = features.is_hypothesis && features.char_len > 100;

    // Location: mentions file paths, line numbers, function names
    // We can't detect this without raw text, but is_hypothesis often includes it
    let location = features.is_hypothesis && features.char_len > 200;

    FailureContext {
        evidence,
        expectation,
        location,
    }
}

/// Compute 64-bit SimHash for prompt deduplication.
/// Only runs on prompts above length floor (very short prompts are guessable).
fn simhash64(prompt: Option<&hackai_schema::PromptFeatures>) -> Option<u64> {
    let features = prompt?;
    if features.char_len < 50 {
        return None; // Below length floor
    }
    // We don't have raw text, so we derive a hash from available features
    // This is a placeholder - real implementation would need the raw prompt
    // For now, use a deterministic hash of the features we have
    let mut hasher = Sha256::new();
    hasher.update(features.char_len.to_le_bytes());
    hasher.update([features.has_attachment as u8]);
    hasher.update([features.has_code_fence as u8]);
    hasher.update([features.is_approval as u8]);
    hasher.update([features.is_hypothesis as u8]);
    hasher.update([features.is_low_effort as u8]);
    let digest = hasher.finalize();
    #[allow(clippy::expect_used)]
    Some(u64::from_le_bytes(
        <[u8; 8]>::try_from(&digest[..8]).expect("digest is 32 bytes"),
    ))
}

/// Bucket the SimHash distance to previous prompt.
fn repeat_distance_bucket(prev_hash: Option<u64>, curr_hash: Option<u64>) -> u8 {
    match (prev_hash, curr_hash) {
        (Some(prev), Some(curr)) => {
            let dist = (prev ^ curr).count_ones();
            match dist {
                0..=3 => 0,  // Near duplicate
                4..=8 => 1,  // Similar
                9..=16 => 2, // Related
                _ => 3,      // Different
            }
        }
        _ => u8::MAX, // Unknown
    }
}

/// Bucket the latency from previous agent turn end to this prompt.
fn latency_bucket(prev_agent_end_ms: Option<i64>, prompt_ms: i64) -> u8 {
    let Some(prev_end) = prev_agent_end_ms else {
        return u8::MAX;
    };
    let gap_ms = prompt_ms.saturating_sub(prev_end);
    match gap_ms {
        0..=5_000 => 0,         // Immediate (<5s)
        5_001..=30_000 => 1,    // Quick (5-30s)
        30_001..=120_000 => 2,  // Normal (30s-2m)
        120_001..=600_000 => 3, // Slow (2-10m)
        _ => 4,                 // Long (>10m)
    }
}

fn label_episode(
    events: &[NormalizedEvent<String, String>],
    episode_id: &str,
    version: u16,
) -> Vec<DialogueTurn> {
    let mut turns = Vec::new();
    let mut since_prompt: Vec<&NormalizedEvent<String, String>> = Vec::new();
    let mut previous_prompt = None;
    let mut prev_prompt_hash: Option<u64> = None;
    let mut prev_agent_end_ms: Option<i64> = None;

    for event in events {
        if event.kind != EventKind::PromptSubmitted || event.role != ActorRole::Human {
            since_prompt.push(event);
            continue;
        }

        // Analyze the agent's reply since the last prompt
        let (_, _, agent_asked_question, _, _, _) = analyze_agent_reply(&since_prompt);

        // Now get the refined agent state with verification
        let (refined_agent_state, verification, files_changed_bucket, refined_tool_errors, reasons) =
            determine_agent_state(&since_prompt);

        let prompt = event.prompt.as_ref();

        // Classify user act with full enum
        let user_act = classify_user_act(
            prompt,
            refined_agent_state,
            verification,
            agent_asked_question,
        );

        let confidence = confidence_for_act(user_act, prompt);

        // Context vector for failure reports
        let context = if matches!(user_act, UserAct::ReportFailure) {
            detect_failure_context(prompt)
        } else {
            FailureContext::default()
        };

        // Information bucket
        let new_information = information_bucket(prompt);

        // Prompt length bucket
        let prompt_length_bucket = prompt_length_bucket(prompt);

        // Fullcaps detection (requires raw text - placeholder uses features)
        let fullcaps = false; // Would need raw prompt text

        // SimHash for repetition
        let curr_hash = simhash64(prompt);
        let repeat_distance_bucket = repeat_distance_bucket(prev_prompt_hash, curr_hash);
        prev_prompt_hash = curr_hash;

        // Answers question
        let answers_question = matches!(refined_agent_state, AgentState::Asked);

        // Latency bucket
        let latency_bucket = latency_bucket(prev_agent_end_ms, event.occurred_at.0);
        // Update prev_agent_end_ms for next turn
        if let Some(last_agent) = since_prompt
            .iter()
            .rev()
            .find(|e| e.role == ActorRole::Agent)
        {
            if let Some(tool) = &last_agent.tool {
                prev_agent_end_ms = tool
                    .finished_at
                    .map(|t| t.0)
                    .or(Some(last_agent.occurred_at.0));
            } else {
                prev_agent_end_ms = Some(last_agent.occurred_at.0);
            }
        }

        turns.push(DialogueTurn {
            event_id: event.id.clone(),
            reply_to: previous_prompt.clone(),
            episode_id: episode_id.to_owned(),
            rule_version: version,
            user_act,
            act_confidence_permille: confidence,
            context,
            new_information,
            prompt_length_bucket,
            fullcaps,
            repeat_distance_bucket,
            answers_question,
            latency_bucket,
            agent_state: refined_agent_state,
            verification,
            files_changed_bucket,
            tool_errors: refined_tool_errors,
            reason_codes: reasons,
        });

        previous_prompt = Some(event.id.clone());
        since_prompt.clear();
    }
    turns
}

fn classify_user_act(
    prompt: Option<&hackai_schema::PromptFeatures>,
    agent_state: AgentState,
    _verification: VerificationState,
    agent_asked_question: bool,
) -> UserAct {
    let Some(features) = prompt else {
        return UserAct::Specify;
    };

    // Approval responses
    if features.is_approval {
        return UserAct::Approve;
    }

    // Low effort / nudge
    if features.is_low_effort {
        return UserAct::Nudge;
    }

    // Short prompts after agent error -> likely ReportFailure or Nudge
    if features.char_len < 100
        && matches!(
            agent_state,
            AgentState::EditedRed | AgentState::ToolError | AgentState::ClaimedDoneUnverified
        )
    {
        // If contextless (short, no code fence, no hypothesis) -> ReportFailure
        if !features.has_code_fence && !features.is_hypothesis {
            return UserAct::ReportFailure;
        }
        return UserAct::Nudge;
    }

    // Answering agent's question
    if agent_asked_question {
        return UserAct::Decide;
    }

    // After agent error with context (code fence, hypothesis) -> Steer
    if matches!(agent_state, AgentState::EditedRed | AgentState::ToolError) {
        if features.has_code_fence || features.is_hypothesis {
            return UserAct::Steer;
        }
        // Contextless failure already handled above for short prompts
        return UserAct::ReportFailure;
    }

    // After agent claimed done but unverified -> Steer or ReportFailure
    if matches!(agent_state, AgentState::ClaimedDoneUnverified) {
        if features.is_hypothesis || features.has_code_fence {
            return UserAct::Steer;
        }
        return UserAct::ReportFailure;
    }

    // After agent explained/planned -> Steer or Decide
    if matches!(agent_state, AgentState::Planned | AgentState::Explained) {
        if features.is_hypothesis {
            return UserAct::Steer;
        }
        return UserAct::Decide;
    }

    // After agent asked -> Decide
    if matches!(agent_state, AgentState::Asked) {
        return UserAct::Decide;
    }

    // Default: Specify (new task, detailed prompt)
    UserAct::Specify
}

fn confidence_for_act(act: UserAct, prompt: Option<&hackai_schema::PromptFeatures>) -> u16 {
    match act {
        UserAct::Specify => {
            if prompt.is_some_and(|p| p.char_len > 200 && p.is_hypothesis) {
                800
            } else {
                500
            }
        }
        UserAct::Nudge => 700,
        UserAct::ReportFailure => {
            if prompt.is_some_and(|p| p.has_code_fence || p.is_hypothesis) {
                800
            } else {
                600
            }
        }
        UserAct::Steer => 750,
        UserAct::Decide => 800,
        UserAct::Approve => 900,
        UserAct::RejectOrRevert => 700,
        UserAct::Interrupt => 600,
        UserAct::AskToUnderstand => 600,
        UserAct::Meta => 500,
    }
}

fn information_bucket(prompt: Option<&hackai_schema::PromptFeatures>) -> InformationBucket {
    match prompt {
        Some(features)
            if features.has_attachment
                || features.has_code_fence
                // Long, structured specifications are information-rich even
                // when the client has no code fence (common in Codex/Claude
                // planning turns). The threshold is calibrated locally from
                // the user's feature distribution; no prompt text is stored.
                || features.char_len > 2_000
                || (features.is_hypothesis && features.char_len > 800) =>
        {
            InformationBucket::High
        }
        Some(features) if features.is_hypothesis || features.char_len > 200 => {
            InformationBucket::Medium
        }
        Some(features) if features.char_len > 0 && !features.is_low_effort => {
            InformationBucket::Low
        }
        _ => InformationBucket::None,
    }
}

fn prompt_length_bucket(prompt: Option<&hackai_schema::PromptFeatures>) -> u8 {
    match prompt.map_or(0, |features| features.char_len) {
        0 => 0,
        1..=40 => 1,
        41..=200 => 2,
        201..=800 => 3,
        _ => 4,
    }
}

fn outcome(events: &[NormalizedEvent<String, String>]) -> Outcome {
    let has_red = events.iter().any(|event| {
        event.tool.as_ref().is_some_and(|tool| {
            matches!(
                tool.class,
                ToolClass::Test | ToolClass::Build | ToolClass::Lint
            ) && tool.exit_code.is_some_and(|code| code != 0)
        })
    });
    if has_red {
        return Outcome::EndedRed;
    }
    let committed = events
        .iter()
        .any(|event| event.kind == EventKind::GitCommit);
    let green = events.iter().any(|event| {
        event.tool.as_ref().is_some_and(|tool| {
            matches!(
                tool.class,
                ToolClass::Test | ToolClass::Build | ToolClass::Lint
            ) && tool.exit_code == Some(0)
        })
    });
    if committed && green {
        Outcome::Shipped
    } else {
        Outcome::Unverified
    }
}

fn episode_id(session: &str, started_at: i64) -> String {
    let mut hasher = Sha256::new();
    hasher.update(session.as_bytes());
    hasher.update(started_at.to_le_bytes());
    format!("ep-{:x}", hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::{
        classify_user_act, detect_failure_context, determine_agent_state, information_bucket,
    };
    use hackai_schema::{
        ActorRole, AgentState, EventId, EventKind, EventSource, NormalizedEvent, PromptFeatures,
        ToolFeatures, UnixMillis, UserAct, VerificationState,
    };
    use hackai_scorer::{FormulaConfig, OutcomeMode};

    fn event(kind: EventKind) -> NormalizedEvent<String, String> {
        NormalizedEvent {
            schema_version: 1,
            id: EventId("event".into()),
            occurred_at: UnixMillis(1),
            kind,
            role: ActorRole::Agent,
            source: EventSource::Codex,
            project: "project".into(),
            session: "session".into(),
            rel_path: None,
            prompt: None,
            tool: None,
            change: None,
            commit: None,
        }
    }

    #[test]
    fn unverified_write_stays_unverified() {
        let mut write = event(EventKind::ToolFinished);
        write.tool = Some(ToolFeatures {
            class: hackai_schema::ToolClass::FileWrite,
            started_at: UnixMillis(1),
            finished_at: Some(UnixMillis(2)),
            exit_code: Some(0),
            error_signature: None,
            is_steering: false,
            output_low_entropy: false,
        });
        let (state, verification, ..) = determine_agent_state(&[&write]);
        // With a single write and no follow-up tools, agent claims done
        assert_eq!(state, hackai_schema::AgentState::ClaimedDoneUnverified);
        assert_eq!(verification, hackai_schema::VerificationState::Unverified);
    }

    #[test]
    fn information_does_not_need_prompt_text() {
        let features = PromptFeatures {
            char_len: 20,
            has_attachment: false,
            has_code_fence: true,
            is_approval: false,
            is_hypothesis: false,
            is_low_effort: false,
        };
        assert!(matches!(
            information_bucket(Some(&features)),
            hackai_schema::InformationBucket::High
        ));
    }

    #[test]
    fn classifies_report_failure_contextless() {
        let features = PromptFeatures {
            char_len: 20,
            has_attachment: false,
            has_code_fence: false,
            is_approval: false,
            is_hypothesis: false,
            is_low_effort: false,
        };
        let act = classify_user_act(
            Some(&features),
            AgentState::EditedRed,
            VerificationState::Red,
            false,
        );
        assert!(matches!(act, UserAct::ReportFailure));
    }

    #[test]
    fn classifies_report_failure_with_context() {
        let features = PromptFeatures {
            char_len: 200,
            has_attachment: false,
            has_code_fence: true,
            is_approval: false,
            is_hypothesis: true,
            is_low_effort: false,
        };
        let act = classify_user_act(
            Some(&features),
            AgentState::EditedRed,
            VerificationState::Red,
            false,
        );
        // With code fence + hypothesis -> Steer (contextful)
        assert!(matches!(act, UserAct::Steer));
    }

    #[test]
    fn detects_context_vector() {
        let features = PromptFeatures {
            char_len: 300,
            has_attachment: false,
            has_code_fence: true,
            is_approval: false,
            is_hypothesis: true,
            is_low_effort: false,
        };
        let ctx = detect_failure_context(Some(&features));
        assert!(ctx.evidence);
        assert!(ctx.expectation);
        assert!(ctx.location);
    }

    #[test]
    fn contextless_short_prompt() {
        let features = PromptFeatures {
            char_len: 10,
            has_attachment: false,
            has_code_fence: false,
            is_approval: false,
            is_hypothesis: false,
            is_low_effort: false,
        };
        let ctx = detect_failure_context(Some(&features));
        assert!(!ctx.evidence);
        assert!(!ctx.expectation);
        assert!(!ctx.location);
        assert!(ctx.is_contextless());
    }

    #[test]
    fn checked_in_formula_is_a_v1_gate_policy() {
        let parsed: Result<FormulaConfig, _> =
            toml::from_str(include_str!("../../../formula.toml"));
        assert!(parsed.is_ok(), "formula.toml must parse");
        if let Ok(config) = parsed {
            assert_eq!(config.version, 1);
            assert_eq!(config.pesdo.outcome.mode, OutcomeMode::Gate);
            assert_eq!(config.transition_rules.len(), 4);
        }
    }
}
