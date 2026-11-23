//! Rich encoding for dialogue replay and evidence bundles.
//!
//! This module handles the structured encoding of user/agent interactions
//! for replay and analysis, without storing raw prompt/response text.

use hackai_schema::{
    ActorRole, AgentState, DialogueTurn, EventKind, EventSource, InformationBucket,
    NormalizedEvent, ToolClass, UserAct, VerificationState,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// A replay-safe event record that can be serialized and replayed
/// to reconstruct the dialogue flow without raw text.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)] // Public API for replay encoding
pub struct ReplayEvent {
    pub event_id: String,
    pub ts_utc_ms: i64,
    pub harness: String,
    pub event_kind: String,
    pub role: String,
    pub session_id_hash: String,
    pub project_id_hash: Option<String>,
    pub prompt_features: Option<PromptFeaturesReplay>,
    pub tool_features: Option<ToolFeaturesReplay>,
    pub change_features: Option<ChangeFeaturesReplay>,
    pub dialogue: Option<DialogueTurnReplay>,
}

/// Prompt features safe for replay
#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)] // Public API for replay encoding
pub struct PromptFeaturesReplay {
    pub length_bucket: LengthBucket,
    pub anchor_count_bucket: CountBucket,
    pub fullcaps: bool,
    pub simhash64: Option<String>,
    pub is_approval: bool,
    pub is_hypothesis: bool,
    pub is_low_effort: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[allow(dead_code)] // Public API for replay encoding
pub enum LengthBucket {
    Empty,
    Short,
    Medium,
    Long,
    VeryLong,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[allow(dead_code)] // Public API for replay encoding
pub enum CountBucket {
    Zero,
    One,
    TwoToThree,
    FourPlus,
}

/// Tool features safe for replay
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolFeaturesReplay {
    pub class: String,
    pub started_at: i64,
    pub finished_at: Option<i64>,
    pub exit_code: Option<i32>,
    pub error_signature: Option<String>,
    pub is_steering: bool,
    pub output_low_entropy: bool,
}

/// Change features safe for replay
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangeFeaturesReplay {
    pub human_added: u32,
    pub human_deleted: u32,
    pub agent_added: u32,
    pub agent_deleted: u32,
    pub files_changed: Option<u32>,
}

/// Dialogue turn for replay
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DialogueTurnReplay {
    pub user_act: String,
    pub act_confidence_permille: u16,
    pub context: FailureContextReplay,
    pub new_information: String,
    pub prompt_length_bucket: u8,
    pub fullcaps: bool,
    pub repeat_distance_bucket: u8,
    pub answers_question: bool,
    pub latency_bucket: u8,
    pub agent_state: String,
    pub verification: String,
    pub files_changed_bucket: u8,
    pub tool_errors: u16,
    pub reason_codes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FailureContextReplay {
    pub evidence: bool,
    pub expectation: bool,
    pub location: bool,
}

/// Episode summary for replay
#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)] // Public API for replay encoding
pub struct EpisodeReplay {
    pub episode_id: String,
    pub started_at: i64,
    pub ended_at: i64,
    pub turn_count: usize,
    pub label: String,
    pub label_confidence: u16,
    pub label_reason_codes: Vec<String>,
    pub human_active_ms: u64,
    pub agent_attended_ms: u64,
    pub union_ms: u64,
    pub outcome: String,
    pub process_delta: i32,
}

/// Full replay bundle
#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)] // Public API for replay encoding
pub struct ReplayBundle {
    pub schema_version: u16,
    pub generated_at_unix_ms: i64,
    pub processor_version: String,
    pub formula_version: String,
    pub events: Vec<ReplayEvent>,
    pub episodes: Vec<EpisodeReplay>,
    pub dialogue_turns: Vec<DialogueTurn>,
}

/// Convert normalized events to replay events
#[allow(dead_code)] // Public API for replay encoding
pub fn events_to_replay(events: &[NormalizedEvent<String, String>]) -> Vec<ReplayEvent> {
    events
        .iter()
        .map(|event| {
            let harness = match event.source {
                EventSource::ClaudeCode => "claude_code",
                EventSource::Codex => "codex",
                EventSource::OpenCode => "opencode",
                EventSource::Gemini => "gemini",
                EventSource::Aider => "aider",
                EventSource::QwenCode => "qwen_code",
                EventSource::KimiCode => "kimi_code",
                EventSource::ZCode => "z_code",
                EventSource::Copilot => "copilot",
                EventSource::ManualCli => "manual",
            };

            let event_kind = match event.kind {
                EventKind::PromptSubmitted => "prompt_submit",
                EventKind::ToolStarted => "tool_call",
                EventKind::ToolFinished => "tool_result",
                EventKind::HumanEdit => "human_edit",
                EventKind::GitCommit => "commit",
                EventKind::SessionEnded => "session_end",
                EventKind::Annotation => "annotation",
            };

            let role = match event.role {
                ActorRole::Human => "human",
                ActorRole::Agent => "agent",
                ActorRole::Harness => "harness",
            };

            ReplayEvent {
                event_id: event.id.0.clone(),
                ts_utc_ms: event.occurred_at.0,
                harness: harness.to_string(),
                event_kind: event_kind.to_string(),
                role: role.to_string(),
                session_id_hash: hash_session(&event.session),
                project_id_hash: Some(hash_session(&event.project)),
                prompt_features: event.prompt.as_ref().map(prompt_to_replay),
                tool_features: event.tool.as_ref().map(tool_to_replay),
                change_features: event.change.as_ref().map(change_to_replay),
                dialogue: None, // Filled by processor
            }
        })
        .collect()
}

#[allow(dead_code)] // Used by events_to_replay
fn prompt_to_replay(p: &hackai_schema::PromptFeatures) -> PromptFeaturesReplay {
    let length_bucket = match p.char_len {
        0 => LengthBucket::Empty,
        1..=40 => LengthBucket::Short,
        41..=200 => LengthBucket::Medium,
        201..=800 => LengthBucket::Long,
        _ => LengthBucket::VeryLong,
    };
    let anchor_count_bucket = CountBucket::Zero; // Would need raw text for real count
    PromptFeaturesReplay {
        length_bucket,
        anchor_count_bucket,
        fullcaps: false, // Would need raw text
        simhash64: None,
        is_approval: p.is_approval,
        is_hypothesis: p.is_hypothesis,
        is_low_effort: p.is_low_effort,
    }
}

#[allow(dead_code)] // Used by events_to_replay
fn tool_to_replay(t: &hackai_schema::ToolFeatures) -> ToolFeaturesReplay {
    let class = match t.class {
        ToolClass::Shell => "shell",
        ToolClass::FileRead => "file_read",
        ToolClass::FileWrite => "file_write",
        ToolClass::FilePatch => "file_patch",
        ToolClass::Search => "search",
        ToolClass::Test => "test",
        ToolClass::Build => "build",
        ToolClass::Lint => "lint",
        ToolClass::Run => "run",
        ToolClass::Debug => "debug",
        ToolClass::Git => "git",
        ToolClass::Network => "network",
        ToolClass::Other => "other",
    };
    ToolFeaturesReplay {
        class: class.to_string(),
        started_at: t.started_at.0,
        finished_at: t.finished_at.map(|ft| ft.0),
        exit_code: t.exit_code,
        error_signature: t.error_signature.as_ref().map(|h| h.0.clone()),
        is_steering: t.is_steering,
        output_low_entropy: t.output_low_entropy,
    }
}

#[allow(dead_code)] // Used by events_to_replay
fn change_to_replay(c: &hackai_schema::ChangeFeatures) -> ChangeFeaturesReplay {
    ChangeFeaturesReplay {
        human_added: c.human_added,
        human_deleted: c.human_deleted,
        agent_added: c.agent_added,
        agent_deleted: c.agent_deleted,
        files_changed: None,
    }
}

#[allow(dead_code)] // Used by events_to_replay
fn hash_session(s: &str) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(s.as_bytes()))
}

/// Build replay bundle from processor output
#[allow(dead_code)] // Public API for replay encoding
pub fn build_replay_bundle(
    processor_output: &crate::processor::ProcessorOutput,
    events: &[NormalizedEvent<String, String>],
) -> ReplayBundle {
    let mut replay_events = events_to_replay(events);

    // Enrich with dialogue turns
    let dialogue_by_event: BTreeMap<String, &DialogueTurn> = processor_output
        .dialogue_turns
        .iter()
        .map(|turn| (turn.event_id.0.clone(), turn))
        .collect();

    for replay_event in &mut replay_events {
        if let Some(turn) = dialogue_by_event.get(&replay_event.event_id) {
            replay_event.dialogue = Some(dialogue_turn_to_replay(turn));
        }
    }

    let episodes = processor_output
        .episodes
        .iter()
        .map(|ep| EpisodeReplay {
            episode_id: ep.episode_id.clone(),
            started_at: ep.human_active_millis as i64, // placeholder
            ended_at: (ep.human_active_millis + ep.attended_agent_millis) as i64,
            turn_count: ep.dialogue_turn_count,
            label: "unlabeled".to_string(), // Would be filled by labeler
            label_confidence: 0,
            label_reason_codes: vec![],
            human_active_ms: ep.human_active_millis,
            agent_attended_ms: ep.attended_agent_millis,
            union_ms: ep.union_millis,
            outcome: format!("{:?}", ep.outcome).to_lowercase(),
            process_delta: ep.process_delta,
        })
        .collect();

    ReplayBundle {
        schema_version: 1,
        generated_at_unix_ms: processor_output.generated_at_unix_ms,
        processor_version: processor_output.processor_version.to_string(),
        formula_version: format!("v{}", processor_output.formula_version),
        events: replay_events,
        episodes,
        dialogue_turns: processor_output.dialogue_turns.clone(),
    }
}

#[allow(dead_code)] // Used by build_replay_bundle
fn dialogue_turn_to_replay(turn: &DialogueTurn) -> DialogueTurnReplay {
    let user_act = match turn.user_act {
        UserAct::Specify => "specify",
        UserAct::Nudge => "nudge",
        UserAct::ReportFailure => "report_failure",
        UserAct::Steer => "steer",
        UserAct::Decide => "decide",
        UserAct::Approve => "approve",
        UserAct::RejectOrRevert => "reject_or_revert",
        UserAct::Interrupt => "interrupt",
        UserAct::AskToUnderstand => "ask_to_understand",
        UserAct::Meta => "meta",
    };

    let agent_state = match turn.agent_state {
        AgentState::Asked => "asked",
        AgentState::Planned => "planned",
        AgentState::Explained => "explained",
        AgentState::EditedGreen => "edited_green",
        AgentState::EditedRed => "edited_red",
        AgentState::EditedUnverified => "edited_unverified",
        AgentState::ClaimedDoneUnverified => "claimed_done_unverified",
        AgentState::ToolError => "tool_error",
        AgentState::Blocked => "blocked",
        AgentState::Looping => "looping",
    };

    let verification = match turn.verification {
        VerificationState::Green => "green",
        VerificationState::Red => "red",
        VerificationState::Unverified => "unverified",
        VerificationState::NotApplicable => "not_applicable",
    };

    let new_information = match turn.new_information {
        InformationBucket::None => "none",
        InformationBucket::Low => "low",
        InformationBucket::Medium => "medium",
        InformationBucket::High => "high",
    };

    DialogueTurnReplay {
        user_act: user_act.to_string(),
        act_confidence_permille: turn.act_confidence_permille,
        context: FailureContextReplay {
            evidence: turn.context.evidence,
            expectation: turn.context.expectation,
            location: turn.context.location,
        },
        new_information: new_information.to_string(),
        prompt_length_bucket: turn.prompt_length_bucket,
        fullcaps: turn.fullcaps,
        repeat_distance_bucket: turn.repeat_distance_bucket,
        answers_question: turn.answers_question,
        latency_bucket: turn.latency_bucket,
        agent_state: agent_state.to_string(),
        verification: verification.to_string(),
        files_changed_bucket: turn.files_changed_bucket,
        tool_errors: turn.tool_errors,
        reason_codes: turn.reason_codes.clone(),
    }
}
