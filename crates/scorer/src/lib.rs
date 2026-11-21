#![forbid(unsafe_code)]
//! Deterministic integer-only scoring formula, version 1.
//! All constants live in `FormulaConfig` so they can be calibrated without code changes.
use hackai_schema::{
    ActorRole, ErrorSignatureHash, EventId, EventKind, NormalizedEvent, SaltedHmacHunkHash,
    ToolClass, UnixMillis,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};

pub const FORMULA_VERSION: u16 = 1;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct FormulaConfig {
    pub version: u16,
    pub bucket_millis: i64,
    pub think_time_cap_millis: i64,
    pub cold_start_cap_millis: i64,
    pub heartbeat_timeout_millis: i64,
    pub episode_gap_millis: i64,
    pub stall_info_reset_millis: i64,
    pub stall_warning_buckets: u32,
    pub stall_zero_buckets: u32,
    pub dark_flow_window_millis: i64,
    pub near_duplicate_threshold: u16,
    pub min_read_time_per_line: u64,
    pub large_diff_lines: u32,
    pub cross_machine_bucket_cap_seconds: u32,
    pub daily_review_flag_hours: u32,
    pub weights: EvidenceWeights,
}

impl Default for FormulaConfig {
    fn default() -> Self {
        Self {
            version: FORMULA_VERSION,
            bucket_millis: 5 * 60 * 1_000,
            think_time_cap_millis: 10 * 60 * 1_000,
            cold_start_cap_millis: 2 * 60 * 1_000,
            heartbeat_timeout_millis: 2 * 60 * 1_000,
            episode_gap_millis: 30 * 60 * 1_000,
            stall_info_reset_millis: 5 * 60 * 1_000,
            stall_warning_buckets: 2,
            stall_zero_buckets: 3,
            dark_flow_window_millis: 30 * 60 * 1_000,
            near_duplicate_threshold: 40,
            min_read_time_per_line: 2,
            large_diff_lines: 50,
            cross_machine_bucket_cap_seconds: 300,
            daily_review_flag_hours: 10,
            weights: EvidenceWeights::default(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct EvidenceWeights {
    pub driven: u16,
    pub steered: u16,
    pub directed: u16,
    pub watching: u16,
}

impl Default for EvidenceWeights {
    fn default() -> Self {
        Self {
            driven: 1000,
            steered: 600,
            directed: 300,
            watching: 0,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum EvidenceClass {
    Driven,
    Steered,
    Directed,
    Watching,
}

impl EvidenceClass {
    pub fn weight(&self, weights: &EvidenceWeights) -> u16 {
        match self {
            EvidenceClass::Driven => weights.driven,
            EvidenceClass::Steered => weights.steered,
            EvidenceClass::Directed => weights.directed,
            EvidenceClass::Watching => weights.watching,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum DarkFlowPenalty {
    NearDuplicate,
    AutoAccept,
    LargeDiffNoTest,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CreditedBucket<P> {
    pub bucket_start: UnixMillis,
    pub bucket_end: UnixMillis,
    pub present_seconds: u64,
    pub evidence_class: EvidenceClass,
    pub evidence_weight: u16,
    pub stall_factor: u16,
    pub dark_flow_factor: u16,
    pub credited_seconds: u64,
    pub evidence_ids: Vec<EventId>,
    pub info_events: Vec<EventId>,
    pub penalties: Vec<DarkFlowPenalty>,
    pub error_signatures: Vec<ErrorSignatureHash>,
    pub files_touched: Vec<P>,
    pub human_edit_seconds: u64,
    pub agent_edit_seconds: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ProductiveEpisode<P, I> {
    pub formula_version: u16,
    pub started_at: UnixMillis,
    pub ended_at: UnixMillis,
    pub project: I,
    pub session: String,
    pub attempts: u32,
    pub distinct_signatures: Vec<ErrorSignatureHash>,
    pub files_touched: Vec<P>,
    pub human_edit_seconds: u64,
    pub agent_edit_seconds: u64,
    pub resolved: bool,
    pub time_to_resolution_seconds: Option<u64>,
    pub credited_seconds: u64,
    pub valid: bool,
    pub buckets: Vec<CreditedBucket<P>>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct HumanAction {
    occurred_at: UnixMillis,
    project: String,
    session: String,
    is_agent_output: bool,
    event_id: EventId,
    kind: HumanActionKind,
}

impl Default for HumanAction {
    fn default() -> Self {
        Self {
            occurred_at: UnixMillis(0),
            project: String::new(),
            session: String::new(),
            is_agent_output: false,
            event_id: EventId(String::new()),
            kind: HumanActionKind::Prompt,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, Default)]
enum HumanActionKind {
    #[default]
    Prompt,
    Approval,
    Rejection,
    Interrupt,
    HumanEdit,
    Heartbeat,
}

fn bucket_start(millis: i64, bucket_millis: i64) -> i64 {
    millis.div_euclid(bucket_millis) * bucket_millis
}

fn bucket_end(start: i64, bucket_millis: i64) -> i64 {
    start.saturating_add(bucket_millis)
}

/// Check if an event represents a human action that establishes presence
fn is_human_presence_event<P, I>(event: &NormalizedEvent<P, I>) -> Option<HumanActionKind> {
    match event.kind {
        EventKind::PromptSubmitted => {
            let prompt = event.prompt.as_ref()?;
            if prompt.is_approval {
                Some(HumanActionKind::Approval)
            } else if prompt.is_low_effort {
                Some(HumanActionKind::Prompt)
            } else {
                Some(HumanActionKind::Prompt)
            }
        }
        EventKind::ToolFinished => {
            let tool = event.tool.as_ref()?;
            if tool.is_steering {
                Some(HumanActionKind::Interrupt)
            } else if tool.exit_code == Some(0) && tool.error_signature.is_none() {
                None
            } else {
                Some(HumanActionKind::Prompt)
            }
        }
        EventKind::HumanEdit => Some(HumanActionKind::HumanEdit),
        EventKind::GitCommit => Some(HumanActionKind::HumanEdit),
        _ => None,
    }
}

/// Classify the evidence strength for a bucket based on events in it
fn classify_evidence<P, I>(
    events: &[&NormalizedEvent<P, I>],
    config: &FormulaConfig,
) -> (EvidenceClass, Vec<EventId>) {
    let mut evidence_ids = Vec::new();
    let mut has_driven = false;
    let mut has_steered = false;
    let mut has_directed = false;

    for event in events {
        match event.kind {
            EventKind::HumanEdit => {
                let change = event.change.as_ref();
                if change.is_some_and(|c| !c.hunk_hashes.is_empty()) {
                    has_driven = true;
                    evidence_ids.push(event.id.clone());
                }
            }
            EventKind::PromptSubmitted => {
                let prompt = event.prompt.as_ref();
                if prompt.is_some_and(|p| p.is_hypothesis && !p.is_low_effort) {
                    has_driven = true;
                    evidence_ids.push(event.id.clone());
                } else if prompt.is_some_and(|p| !p.is_low_effort && !p.is_approval) {
                    has_directed = true;
                    evidence_ids.push(event.id.clone());
                }
            }
            EventKind::ToolFinished => {
                let tool = event.tool.as_ref();
                if tool.is_some_and(|t| t.is_steering) {
                    has_steered = true;
                    evidence_ids.push(event.id.clone());
                } else if tool.is_some_and(|t| {
                    matches!(
                        t.class,
                        ToolClass::Test | ToolClass::Build | ToolClass::Debug | ToolClass::Lint
                    )
                }) {
                    has_steered = true;
                    evidence_ids.push(event.id.clone());
                }
            }
            _ => {}
        }
    }

    let class = if has_driven {
        EvidenceClass::Driven
    } else if has_steered {
        EvidenceClass::Steered
    } else if has_directed {
        EvidenceClass::Directed
    } else {
        EvidenceClass::Watching
    };

    (class, evidence_ids)
}

/// Detect information events that reset the stall counter
fn is_information_event<P: Clone + Eq + std::hash::Hash, I: Clone + ToString>(
    event: &NormalizedEvent<P, I>,
    seen_signatures: &HashSet<ErrorSignatureHash>,
    seen_files: &HashSet<P>,
    seen_hunks: &HashSet<SaltedHmacHunkHash>,
) -> bool {
    match event.kind {
        EventKind::ToolFinished => {
            if let Some(tool) = &event.tool {
                if let Some(sig) = &tool.error_signature
                    && !seen_signatures.contains(sig)
                {
                    return true;
                }
                if matches!(
                    tool.class,
                    ToolClass::Test | ToolClass::Build | ToolClass::Run
                ) && tool.error_signature.is_none()
                {
                    return true;
                }
            }
        }
        EventKind::HumanEdit => {
            if let Some(change) = &event.change {
                for hunk in &change.hunk_hashes {
                    if !seen_hunks.contains(hunk) {
                        return true;
                    }
                }
            }
        }
        EventKind::PromptSubmitted => {
            if let Some(prompt) = &event.prompt {
                if prompt.is_hypothesis && !prompt.is_low_effort {
                    return true;
                }
            }
        }
        _ => {}
    }
    if let Some(path) = &event.rel_path {
        if !seen_files.contains(path) {
            return true;
        }
    }
    false
}

/// SimHash-based near-duplicate detection for prompts (simplified)
fn prompt_simhash(content: &str) -> u64 {
    let mut hash = 0u64;
    for word in content.split(|c: char| !c.is_ascii_alphanumeric()) {
        if word.len() >= 3 {
            let mut h = std::collections::hash_map::DefaultHasher::new();
            use std::hash::{Hash, Hasher};
            word.hash(&mut h);
            hash ^= h.finish();
        }
    }
    hash
}

fn hamming_distance(a: u64, b: u64) -> u32 {
    (a ^ b).count_ones()
}

fn are_near_duplicate(a: &str, b: &str, threshold: u16) -> bool {
    let h1 = prompt_simhash(a);
    let h2 = prompt_simhash(b);
    let dist = hamming_distance(h1, h2);
    let max_bits = 64;
    let similarity = 100 - (dist * 100 / max_bits);
    similarity >= threshold.into()
}

/// Build human action timeline per user across all sessions/projects
fn build_human_timeline<P: Clone + Eq + std::hash::Hash, I: Clone + ToString>(
    events: &[NormalizedEvent<P, I>],
    config: &FormulaConfig,
) -> BTreeMap<UnixMillis, Vec<HumanAction>> {
    let mut timeline: BTreeMap<UnixMillis, Vec<HumanAction>> = BTreeMap::new();

    for event in events {
        if let Some(kind) = is_human_presence_event(event) {
            let project = event.project.to_string();
            let action = HumanAction {
                occurred_at: event.occurred_at,
                project: project.clone(),
                session: event.session.clone(),
                is_agent_output: false,
                event_id: event.id.clone(),
                kind,
            };
            timeline.entry(event.occurred_at).or_default().push(action);
        } else if event.kind == EventKind::ToolFinished && event.role == ActorRole::Agent {
            if let Some(tool) = &event.tool {
                if let Some(fin) = tool.finished_at {
                    let action = HumanAction {
                        occurred_at: fin,
                        project: event.project.to_string(),
                        session: event.session.clone(),
                        is_agent_output: true,
                        event_id: event.id.clone(),
                        kind: HumanActionKind::Prompt,
                    };
                    timeline.entry(fin).or_default().push(action);
                }
            }
        }
    }

    timeline
}

/// Compute present seconds (P) for each bucket
fn compute_present_seconds(
    timeline: &BTreeMap<UnixMillis, Vec<HumanAction>>,
    config: &FormulaConfig,
) -> BTreeMap<(String, UnixMillis), u64> {
    let mut present: BTreeMap<(String, UnixMillis), u64> = BTreeMap::new();
    let mut last_action_per_project: HashMap<String, Option<HumanAction>> = HashMap::new();

    for (time, actions) in timeline {
        let bucket = bucket_start(time.0, config.bucket_millis);
        let bucket_key = UnixMillis(bucket);

        for action in actions {
            let project = &action.project;
            let last = last_action_per_project.entry(project.clone()).or_default();

            let gap_cap = if last.as_ref().is_some_and(|a| a.is_agent_output) {
                config.think_time_cap_millis
            } else {
                config.cold_start_cap_millis
            };

            if let Some(last_action) = last {
                let gap = time.0.saturating_sub(last_action.occurred_at.0);
                let capped_gap = gap.min(gap_cap);
                if capped_gap > 0 {
                    let gap_bucket = bucket_start(last_action.occurred_at.0, config.bucket_millis);
                    let gap_bucket_key = UnixMillis(gap_bucket);
                    *present
                        .entry((project.clone(), gap_bucket_key))
                        .or_default() += (capped_gap / 1_000) as u64;
                }
            }

            if !action.is_agent_output {
                let hb_bucket = bucket_start(action.occurred_at.0, config.bucket_millis);
                let hb_bucket_key = UnixMillis(hb_bucket);
                *present.entry((project.clone(), hb_bucket_key)).or_default() +=
                    (config.heartbeat_timeout_millis / 1_000) as u64;
            }

            *last = Some(action.clone());
        }
    }

    present
}

/// Compute stall factor (S) per bucket
fn compute_stall_factor<P: Clone + Eq + std::hash::Hash, I: Clone + ToString>(
    events: &[NormalizedEvent<P, I>],
    present: &BTreeMap<(String, UnixMillis), u64>,
    config: &FormulaConfig,
) -> BTreeMap<(String, UnixMillis), u16> {
    let mut stall: BTreeMap<(String, UnixMillis), u16> = BTreeMap::new();
    let mut seen_signatures: HashMap<String, HashSet<ErrorSignatureHash>> = HashMap::new();
    let mut seen_files: HashMap<String, HashSet<P>> = HashMap::new();
    let mut seen_hunks: HashMap<String, HashSet<SaltedHmacHunkHash>> = HashMap::new();
    let mut consecutive_no_info: HashMap<String, u32> = HashMap::new();

    let mut ordered: Vec<_> = events.iter().collect();
    ordered.sort_by_key(|e| e.occurred_at.0);

    for event in ordered {
        let project = event.project.to_string();
        let bucket = bucket_start(event.occurred_at.0, config.bucket_millis);
        let bucket_key = UnixMillis(bucket);

        let has_info = is_information_event(
            event,
            seen_signatures.entry(project.clone()).or_default(),
            seen_files.entry(project.clone()).or_default(),
            seen_hunks.entry(project.clone()).or_default(),
        );

        let bucket_present = present
            .get(&(project.clone(), bucket_key))
            .cloned()
            .unwrap_or(0);

        if has_info {
            consecutive_no_info.insert(project.clone(), 0);
        } else if bucket_present > 0 {
            let entry = consecutive_no_info.entry(project.clone()).or_default();
            *entry = entry.saturating_add(1).min(config.stall_zero_buckets);
        }

        let class = if present.contains_key(&(project.clone(), bucket_key)) {
            EvidenceClass::Directed
        } else {
            continue;
        };

        let stall_factor = match class {
            EvidenceClass::Driven | EvidenceClass::Steered => 1000,
            EvidenceClass::Directed | EvidenceClass::Watching => {
                let n = consecutive_no_info.get(&project).cloned().unwrap_or(0);
                if n <= 1 {
                    1000
                } else if n == 2 {
                    500
                } else {
                    0
                }
            }
        };

        stall.entry((project, bucket_key)).or_insert(stall_factor);
    }

    stall
}

/// Compute dark-flow factor (D) per bucket
fn compute_dark_flow_factor<P: Clone + Eq + std::hash::Hash, I: Clone + ToString>(
    events: &[NormalizedEvent<P, I>],
    config: &FormulaConfig,
) -> BTreeMap<(String, UnixMillis), (u16, Vec<DarkFlowPenalty>)> {
    let mut dark_flow: BTreeMap<(String, UnixMillis), (u16, Vec<DarkFlowPenalty>)> =
        BTreeMap::new();
    let window_millis = config.dark_flow_window_millis;

    let mut prompts_per_window: BTreeMap<(String, UnixMillis), Vec<String>> = BTreeMap::new();

    for event in events {
        let project = event.project.to_string();
        let bucket = bucket_start(event.occurred_at.0, config.bucket_millis);
        let window_start = bucket - (window_millis / 2);
        let window_key = UnixMillis(window_start.max(0).div_euclid(window_millis) * window_millis);

        match event.kind {
            EventKind::PromptSubmitted => {
                if let Some(prompt) = &event.prompt {
                    if prompt.is_hypothesis {
                        // Placeholder - would need actual prompt content
                        prompts_per_window
                            .entry((project.clone(), window_key))
                            .or_default()
                            .push(event.id.0.clone());
                    }
                }
            }
            _ => {}
        }
    }

    for ((project, window), prompts) in prompts_per_window {
        if prompts.len() < 2 {
            continue;
        }

        let mut near_dup_count = 0;
        for i in 0..prompts.len() {
            for j in i + 1..prompts.len() {
                if are_near_duplicate(&prompts[i], &prompts[j], config.near_duplicate_threshold) {
                    near_dup_count += 1;
                }
            }
        }

        let total_pairs = prompts.len() * (prompts.len() - 1) / 2;
        let ratio = if total_pairs > 0 {
            (near_dup_count * 100) / total_pairs
        } else {
            0
        };

        if ratio >= config.near_duplicate_threshold.into() {
            let mut penalties = Vec::new();
            penalties.push(DarkFlowPenalty::NearDuplicate);
            dark_flow
                .entry((project, window))
                .or_insert((1000, Vec::new()))
                .1
                .extend(penalties);
        }
    }

    let mut result = BTreeMap::new();
    for ((project, window), (_, penalties)) in dark_flow {
        let window_end = window.0 + window_millis;
        let mut bucket = window.0;
        while bucket < window_end {
            let bucket_key = UnixMillis(bucket);
            let base_factor = 1000i32;
            let penalty_total: i32 = penalties
                .iter()
                .map(|p| match p {
                    DarkFlowPenalty::NearDuplicate => -300,
                    DarkFlowPenalty::AutoAccept => -300,
                    DarkFlowPenalty::LargeDiffNoTest => -200,
                })
                .sum();
            let factor = (base_factor + penalty_total).max(100) as u16;
            result.insert((project.clone(), bucket_key), (factor, penalties.clone()));
            bucket += config.bucket_millis;
        }
    }

    result
}

/// Cross-machine bucket cap enforcement
fn apply_cross_machine_cap<P>(
    buckets: &mut BTreeMap<(String, UnixMillis), CreditedBucket<P>>,
    cap_seconds: u32,
) {
    let mut bucket_totals: HashMap<(String, String), u64> = HashMap::new();

    for bucket in buckets.values() {
        let key = (
            bucket.bucket_start.0.to_string(),
            bucket.bucket_end.0.to_string(),
        );
        *bucket_totals.entry(key).or_default() += bucket.credited_seconds;
    }

    for ((start_str, end_str), total) in bucket_totals {
        if total > cap_seconds as u64 {
            let scale = (cap_seconds as u64 * 1_000) / total;
            for bucket in buckets.values_mut() {
                if bucket.bucket_start.0.to_string() == start_str
                    && bucket.bucket_end.0.to_string() == end_str
                {
                    bucket.credited_seconds = (bucket.credited_seconds * scale / 1_000) as u64;
                }
            }
        }
    }
}

/// Main entry: compute credited buckets and episodes
pub fn productive_episodes<
    P: Clone + Eq + std::hash::Hash + std::fmt::Debug + ToString + Ord,
    I: Clone + ToString,
>(
    events: &[NormalizedEvent<P, I>],
    config: &FormulaConfig,
) -> Vec<ProductiveEpisode<P, String>> {
    if events.is_empty() {
        return Vec::new();
    }

    let timeline = build_human_timeline(events, config);
    let present = compute_present_seconds(&timeline, config);
    let stall = compute_stall_factor(events, &present, config);
    let dark_flow = compute_dark_flow_factor(events, config);

    let mut buckets: BTreeMap<(String, UnixMillis), CreditedBucket<P>> = BTreeMap::new();

    for ((project, bucket_time), present_seconds) in present {
        let bucket_start_key = bucket_time;
        let bucket_end_key = UnixMillis(bucket_time.0 + config.bucket_millis);

        let bucket_events: Vec<_> = events
            .iter()
            .filter(|e| {
                e.project.to_string() == project
                    && e.occurred_at.0 >= bucket_start_key.0
                    && e.occurred_at.0 < bucket_end_key.0
            })
            .collect();

        let (evidence_class, evidence_ids) = classify_evidence(&bucket_events, config);
        let evidence_weight = evidence_class.weight(&config.weights);

        let stall_factor = stall
            .get(&(project.clone(), bucket_start_key))
            .cloned()
            .unwrap_or(1000);
        let (dark_flow_factor, penalties) = dark_flow
            .get(&(project.clone(), bucket_start_key))
            .cloned()
            .unwrap_or((1000, Vec::new()));

        let effective_stall = match evidence_class {
            EvidenceClass::Driven | EvidenceClass::Steered => 1000,
            _ => stall_factor,
        };

        let credited = (present_seconds as u128
            * evidence_weight as u128
            * effective_stall as u128
            * dark_flow_factor as u128
            / 1_000_000_000) as u64;

        let mut error_signatures = Vec::new();
        let mut files_touched = Vec::new();
        let mut human_edit_seconds: u64 = 0;
        let mut agent_edit_seconds: u64 = 0;

        for event in &bucket_events {
            if let Some(tool) = &event.tool {
                if let Some(sig) = &tool.error_signature {
                    error_signatures.push(sig.clone());
                }
            }
            if let Some(path) = &event.rel_path {
                files_touched.push(path.clone());
            }
            if event.kind == EventKind::HumanEdit {
                if let Some(change) = &event.change {
                    human_edit_seconds += u64::from(change.human_added + change.human_deleted);
                    agent_edit_seconds += u64::from(change.agent_added + change.agent_deleted);
                }
            }
        }

        error_signatures.sort_by(|a, b| a.0.cmp(&b.0));
        error_signatures.dedup();
        files_touched.sort();
        files_touched.dedup();

        buckets.insert(
            (project.clone(), bucket_start_key),
            CreditedBucket {
                bucket_start: bucket_start_key,
                bucket_end: bucket_end_key,
                present_seconds,
                evidence_class,
                evidence_weight,
                stall_factor: effective_stall,
                dark_flow_factor,
                credited_seconds: credited,
                evidence_ids,
                info_events: Vec::new(),
                penalties,
                error_signatures,
                files_touched,
                human_edit_seconds,
                agent_edit_seconds,
            },
        );
    }

    apply_cross_machine_cap(&mut buckets, config.cross_machine_bucket_cap_seconds);

    let mut episodes = Vec::new();
    let mut current_episode_buckets: Vec<CreditedBucket<P>> = Vec::new();
    let mut current_project: Option<String> = None;
    let mut last_bucket_time: Option<UnixMillis> = None;

    for ((project, bucket_time), bucket) in buckets {
        if current_project.as_ref() != Some(&project)
            || last_bucket_time.is_some_and(|last| bucket_time.0 > last.0 + config.bucket_millis)
        {
            if !current_episode_buckets.is_empty() {
                if let Some(project_name) = current_project.take() {
                    episodes.push(finalize_episode_from_buckets(
                        project_name,
                        &mut current_episode_buckets,
                        config,
                    ));
                }
            }
            current_project = Some(project);
            current_episode_buckets.clear();
        }

        current_episode_buckets.push(bucket);
        last_bucket_time = Some(bucket_time);
    }

    if !current_episode_buckets.is_empty() {
        if let Some(project_name) = current_project.take() {
            episodes.push(finalize_episode_from_buckets(
                project_name,
                &mut current_episode_buckets,
                config,
            ));
        }
    }

    episodes
}

fn finalize_episode_from_buckets<P: Clone + Eq + std::hash::Hash + std::fmt::Debug + Ord>(
    project: String,
    buckets: &mut Vec<CreditedBucket<P>>,
    config: &FormulaConfig,
) -> ProductiveEpisode<P, String> {
    buckets.sort_by_key(|b| b.bucket_start.0);

    let started_at = buckets[0].bucket_start;
    let ended_at = buckets[buckets.len() - 1].bucket_end;
    let credited_seconds: u64 = buckets.iter().map(|b| b.credited_seconds).sum();
    let valid = credited_seconds > 0;

    let mut all_error_signatures = Vec::new();
    let mut all_files_touched = Vec::new();
    let mut total_human_edit_seconds: u64 = 0;
    let mut total_agent_edit_seconds: u64 = 0;

    for bucket in buckets.iter() {
        all_error_signatures.extend(bucket.error_signatures.clone());
        all_files_touched.extend(bucket.files_touched.clone());
        total_human_edit_seconds += bucket.human_edit_seconds;
        total_agent_edit_seconds += bucket.agent_edit_seconds;
    }

    all_error_signatures.sort_by(|a, b| a.0.cmp(&b.0));
    all_error_signatures.dedup();
    all_files_touched.sort();
    all_files_touched.dedup();

    let attempts = all_error_signatures.len().max(1) as u32;
    let distinct_signatures = all_error_signatures;
    let files_touched = all_files_touched;
    let human_edit_seconds = total_human_edit_seconds;
    let agent_edit_seconds = total_agent_edit_seconds;

    let resolved = distinct_signatures.is_empty() || agent_edit_seconds > 0;

    let time_to_resolution_seconds = if resolved && !distinct_signatures.is_empty() {
        Some(((ended_at.0 - started_at.0) / 1000) as u64)
    } else {
        None
    };

    ProductiveEpisode {
        formula_version: FORMULA_VERSION,
        started_at,
        ended_at,
        project: project.clone(),
        session: "main".into(),
        attempts,
        distinct_signatures,
        files_touched,
        human_edit_seconds,
        agent_edit_seconds,
        resolved,
        time_to_resolution_seconds,
        credited_seconds,
        valid,
        buckets: buckets.clone(),
    }
}

/// Score a single window (for compatibility with old API)
pub fn score_events<
    P: Clone + Eq + std::hash::Hash + std::fmt::Debug + ToString + Ord,
    I: Clone + ToString,
>(
    events: &[NormalizedEvent<P, I>],
    end: UnixMillis,
    config: &FormulaConfig,
) -> ScoreExplanation {
    let episodes = productive_episodes(events, config);
    let total_credited: u64 = episodes.iter().map(|e| e.credited_seconds).sum();
    let start = events
        .iter()
        .map(|e| e.occurred_at.0)
        .min()
        .unwrap_or(end.0);
    let wall = end.0.saturating_sub(start).max(0) as u64 / 1_000;

    ScoreExplanation {
        formula_version: FORMULA_VERSION,
        capped_wall_seconds: wall,
        base_seconds: wall,
        agent_discount_seconds: wall.saturating_sub(total_credited),
        error_bonus_seconds: 0,
        rework_bonus_seconds: 0,
        annotation_discount_seconds: 0,
        credited_seconds: total_credited,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ScoreInput {
    pub wall_seconds: u64,
    pub agent_seconds: u64,
    pub error_loop_seconds: u64,
    pub human_rework_seconds: u64,
    pub annotation_discount_permille: u16,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ScoreExplanation {
    pub formula_version: u16,
    pub capped_wall_seconds: u64,
    pub base_seconds: u64,
    pub agent_discount_seconds: u64,
    pub error_bonus_seconds: u64,
    pub rework_bonus_seconds: u64,
    pub annotation_discount_seconds: u64,
    pub credited_seconds: u64,
}

pub fn score(input: ScoreInput) -> ScoreExplanation {
    const WINDOW_CAP_SECONDS: u64 = 25 * 60;
    const AGENT_WEIGHT_PERMILLE: u64 = 300;
    const ERROR_LOOP_PERMILLE: u64 = 1_150;
    const HUMAN_REWORK_PERMILLE: u64 = 1_100;
    const STEERED_PERMILLE: u16 = 600;

    let wall = input.wall_seconds.min(WINDOW_CAP_SECONDS);
    let agent = input.agent_seconds.min(wall);
    let agent_discount = agent.saturating_mul(1_000 - AGENT_WEIGHT_PERMILLE) / 1_000;
    let error_bonus = input
        .error_loop_seconds
        .min(wall)
        .saturating_mul(ERROR_LOOP_PERMILLE - 1_000)
        / 1_000;
    let rework_bonus = input
        .human_rework_seconds
        .min(wall)
        .saturating_mul(HUMAN_REWORK_PERMILLE - 1_000)
        / 1_000;
    let before_annotation = wall
        .saturating_sub(agent_discount)
        .saturating_add(error_bonus)
        .saturating_add(rework_bonus)
        .min(wall);
    let annotation_discount = before_annotation
        .saturating_mul(u64::from(input.annotation_discount_permille.min(1_000)))
        / 1_000;

    ScoreExplanation {
        formula_version: FORMULA_VERSION,
        capped_wall_seconds: wall,
        base_seconds: wall,
        agent_discount_seconds: agent_discount,
        error_bonus_seconds: error_bonus,
        rework_bonus_seconds: rework_bonus,
        annotation_discount_seconds: annotation_discount,
        credited_seconds: before_annotation.saturating_sub(annotation_discount),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use hackai_schema::{
        ActorRole, EventId, EventKind, EventSource, NormalizedEvent, ToolClass, ToolFeatures,
        UnixMillis,
    };

    fn dummy_event(time: i64, kind: EventKind) -> NormalizedEvent<String, String> {
        NormalizedEvent {
            schema_version: 1,
            id: EventId(format!("evt-{time}")),
            occurred_at: UnixMillis(time),
            kind,
            role: ActorRole::Human,
            source: EventSource::ClaudeCode,
            project: "test".into(),
            session: "test".into(),
            rel_path: None,
            prompt: None,
            tool: None,
            change: None,
            commit: None,
        }
    }

    #[test]
    fn test_bucket_alignment() {
        let config = FormulaConfig::default();
        assert_eq!(bucket_start(0, config.bucket_millis), 0);
        assert_eq!(bucket_start(299_999, config.bucket_millis), 0);
        assert_eq!(bucket_start(300_000, config.bucket_millis), 300_000);
        assert_eq!(bucket_start(300_001, config.bucket_millis), 300_000);
    }

    #[test]
    fn test_empty_events_returns_empty() {
        let config = FormulaConfig::default();
        let result = productive_episodes::<String, String>(&[], &config);
        assert!(result.is_empty());
    }

    #[test]
    fn test_driven_evidence_classification() {
        let config = FormulaConfig::default();
        let events = vec![
            NormalizedEvent {
                schema_version: 1,
                id: EventId("e1".into()),
                occurred_at: UnixMillis(0),
                kind: EventKind::PromptSubmitted,
                role: ActorRole::Human,
                source: EventSource::ClaudeCode,
                project: "test-project".to_string(),
                session: "s".into(),
                rel_path: None::<String>,
                prompt: Some(hackai_schema::PromptFeatures {
                    char_len: 100,
                    has_attachment: false,
                    has_code_fence: false,
                    is_approval: false,
                    is_hypothesis: true,
                    is_low_effort: false,
                }),
                tool: None,
                change: None,
                commit: None,
            },
            NormalizedEvent {
                schema_version: 1,
                id: EventId("e2".into()),
                occurred_at: UnixMillis(60_000),
                kind: EventKind::HumanEdit,
                role: ActorRole::Human,
                source: EventSource::ClaudeCode,
                project: "test-project".to_string(),
                session: "s".into(),
                rel_path: Some("src/main.rs".to_string()),
                prompt: None,
                tool: None,
                change: Some(hackai_schema::ChangeFeatures {
                    human_added: 10,
                    human_deleted: 0,
                    agent_added: 0,
                    agent_deleted: 0,
                    hunk_hashes: vec![
                        hackai_schema::SaltedHmacHunkHash::parse("a".repeat(64)).unwrap(),
                    ],
                }),
                commit: None,
            },
            NormalizedEvent {
                schema_version: 1,
                id: EventId("e3".into()),
                occurred_at: UnixMillis(120_000),
                kind: EventKind::HumanEdit,
                role: ActorRole::Human,
                source: EventSource::ClaudeCode,
                project: "test-project".to_string(),
                session: "s".into(),
                rel_path: Some("src/main.rs".to_string()),
                prompt: None,
                tool: None,
                change: Some(hackai_schema::ChangeFeatures {
                    human_added: 5,
                    human_deleted: 0,
                    agent_added: 0,
                    agent_deleted: 0,
                    hunk_hashes: vec![
                        hackai_schema::SaltedHmacHunkHash::parse("b".repeat(64)).unwrap(),
                    ],
                }),
                commit: None,
            },
        ];
        let episodes = productive_episodes::<String, String>(&events, &config);
        assert!(!episodes.is_empty());
        assert!(episodes[0].credited_seconds > 0);
        assert!(episodes[0].human_edit_seconds > 0);
        assert!(!episodes[0].files_touched.is_empty());
    }

    #[test]
    fn test_legacy_score_agent_discount() {
        let v = score(ScoreInput {
            wall_seconds: 100,
            agent_seconds: 100,
            error_loop_seconds: 0,
            human_rework_seconds: 0,
            annotation_discount_permille: 0,
        });
        assert_eq!(v.credited_seconds, 30);
    }

    #[test]
    fn test_legacy_annotation_never_raises() {
        let a = score(ScoreInput {
            wall_seconds: 100,
            agent_seconds: 0,
            error_loop_seconds: 0,
            human_rework_seconds: 0,
            annotation_discount_permille: 1000,
        });
        assert_eq!(a.credited_seconds, 0);
    }
}
