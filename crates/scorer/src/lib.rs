#![forbid(unsafe_code)]
//! Deterministic integer-only scoring formula, version 1.
//! All constants live in `FormulaConfig` so they can be calibrated without code changes.
use hackai_schema::{
    ActorRole, AgentState, ErrorSignatureHash, EventId, EventKind, FailureContext, NormalizedEvent,
    SaltedHmacHunkHash, ToolClass, UnixMillis, UserAct,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};

pub const FORMULA_VERSION: u16 = 1;

/// Whether outcome verification is a submission gate (v1) or a future soft
/// factor.  Keeping both modes in the serialized formula makes a mode change
/// explicit and replayable.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutcomeMode {
    #[default]
    Gate,
    SoftFactor,
}

/// Factors are permille.  `outcome` is intentionally separate: in gate mode
/// it does not participate in bucket arithmetic.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PesdoFactors {
    pub presence: u16,
    pub evidence: u16,
    pub specification: u16,
    pub diligence: u16,
}

impl Default for PesdoFactors {
    fn default() -> Self {
        Self {
            presence: 1_000,
            evidence: 1_000,
            specification: 1_000,
            diligence: 1_000,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct OutcomeConfig {
    pub mode: OutcomeMode,
    /// Used only in `SoftFactor` mode.  Valid values are 0..=1000.
    pub soft_factor_permille: u16,
}

impl Default for OutcomeConfig {
    fn default() -> Self {
        Self {
            mode: OutcomeMode::Gate,
            soft_factor_permille: 1_000,
        }
    }
}

/// A PESDO-compatible formula section suitable for a versioned
/// `formula.toml`.  Defaults are neutral on purpose: factor expansion is a
/// policy decision, while the arithmetic is deterministic today.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PesdoConfig {
    pub outcome: OutcomeConfig,
    /// Limits signed dialogue process deltas; it is not an extra score.
    pub process_delta_limit: i32,
}

impl Default for PesdoConfig {
    fn default() -> Self {
        Self {
            outcome: OutcomeConfig::default(),
            process_delta_limit: 20,
        }
    }
}

/// A configured, privacy-safe transition rule.  Rules are evaluated in order
/// so a formula can put a context-specific rule before a broad rule.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TransitionRule {
    pub agent_state: AgentState,
    pub user_act: UserAct,
    pub require_contextless: Option<bool>,
    pub delta: i16,
}

/// Apply PESD with one `u128` division at the end.  A failed outcome gate
/// always returns zero; a soft outcome is multiplied only in soft mode.
pub fn score_pesdo(
    base_units: u64,
    factors: PesdoFactors,
    outcome_verified: bool,
    config: PesdoConfig,
) -> u64 {
    if config.outcome.mode == OutcomeMode::Gate && !outcome_verified {
        return 0;
    }
    let mut product = u128::from(base_units)
        .saturating_mul(u128::from(factors.presence))
        .saturating_mul(u128::from(factors.evidence))
        .saturating_mul(u128::from(factors.specification))
        .saturating_mul(u128::from(factors.diligence));
    let mut divisor = 1_000_u128.pow(4);
    if config.outcome.mode == OutcomeMode::SoftFactor {
        product =
            product.saturating_mul(u128::from(config.outcome.soft_factor_permille.min(1_000)));
        divisor = divisor.saturating_mul(1_000);
    }
    u64::try_from(product / divisor).unwrap_or(u64::MAX)
}

/// Sum per-turn, user-attributable process deltas and bound the episode total.
/// No transition can raise time credit; callers map the outcome only into S/D.
pub fn process_delta(
    turns: &[(AgentState, UserAct, FailureContext)],
    rules: &[TransitionRule],
    limit: i32,
) -> i32 {
    let total = turns.iter().fold(0_i32, |total, (state, act, context)| {
        let contextless = context.is_contextless();
        let delta = rules
            .iter()
            .find(|rule| {
                rule.agent_state == *state
                    && rule.user_act == *act
                    && rule
                        .require_contextless
                        .is_none_or(|required| required == contextless)
            })
            .map_or(0, |rule| i32::from(rule.delta));
        total.saturating_add(delta)
    });
    total.clamp(-limit.abs(), limit.abs())
}

/// Deterministic, harness-agnostic time accounting. This intentionally sits
/// below PESDO: it measures time from server-recomputable event timestamps and
/// makes no claim about the still-unresolved factor meanings.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TimeAccountingConfig {
    pub human_gap_cap_millis: i64,
    pub agent_wait_cap_millis: i64,
    pub follow_up_idle_limit_millis: i64,
}

impl Default for TimeAccountingConfig {
    fn default() -> Self {
        Self {
            human_gap_cap_millis: 120_000,
            agent_wait_cap_millis: 300_000,
            follow_up_idle_limit_millis: 900_000,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, Default)]
pub struct TimeAccounting {
    pub human_active_millis: u64,
    pub agent_attended_millis: u64,
    pub union_millis: u64,
    pub sum_interval_millis: u64,
    /// `sum / union` in permille, useful for detecting concurrent-session
    /// inflation while keeping credited time a union rather than a sum.
    pub concurrency_ratio_permille: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct FormulaConfig {
    pub version: u16,
    /// Versioned PESDO arithmetic and outcome-gate policy.  This is kept
    /// alongside legacy bucket knobs until consumers migrate together.
    #[serde(default)]
    pub pesdo: PesdoConfig,
    #[serde(default)]
    pub transition_rules: Vec<TransitionRule>,
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
            pesdo: PesdoConfig::default(),
            transition_rules: default_transition_rules(),
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

fn default_transition_rules() -> Vec<TransitionRule> {
    vec![
        TransitionRule {
            agent_state: AgentState::EditedRed,
            user_act: UserAct::ReportFailure,
            require_contextless: Some(true),
            delta: -2,
        },
        TransitionRule {
            agent_state: AgentState::EditedRed,
            user_act: UserAct::ReportFailure,
            require_contextless: Some(false),
            delta: 2,
        },
        TransitionRule {
            agent_state: AgentState::EditedGreen,
            user_act: UserAct::Approve,
            require_contextless: None,
            delta: 1,
        },
        TransitionRule {
            agent_state: AgentState::EditedRed,
            user_act: UserAct::Approve,
            require_contextless: None,
            delta: -2,
        },
    ]
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

/// Derived full-caps signal.  It is deliberately a small feature, never a
/// dialogue label by itself.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct FullCapsAssessment {
    pub is_fullcaps: bool,
    pub cased_letters: u16,
    pub lowercase_letters: u16,
}

const TECH_TERMS: &[&str] = &[
    "API", "ASCII", "CLI", "CPU", "CSS", "DNS", "ENOENT", "HTTP", "HTTPS", "JSON", "JWT", "README",
    "RFC", "SIGSEGV", "SQL", "TODO", "URL", "UTF", "UUID", "XML", "YAML",
];

fn is_technical_token(token: &str, dynamic_terms: &HashSet<String>) -> bool {
    let trimmed = token.trim_matches(|ch: char| !ch.is_alphanumeric() && ch != '_' && ch != '/');
    if trimmed.is_empty()
        || trimmed.contains('/')
        || trimmed.contains('\\')
        || trimmed.contains('_')
        || trimmed.chars().any(|ch| ch.is_ascii_digit())
        || trimmed.starts_with("http://")
        || trimmed.starts_with("https://")
    {
        return true;
    }
    let upper = trimmed.to_uppercase();
    TECH_TERMS.contains(&upper.as_str())
        || dynamic_terms.contains(&upper)
        || [".rs", ".ts", ".tsx", ".js", ".json", ".toml", ".md", ".py"]
            .iter()
            .any(|extension| trimmed.ends_with(extension))
}

/// Detect an all-caps *whole prompt* after removing likely pasted or technical
/// material. `dynamic_terms` should contain uppercase tokens observed in the
/// repository, prior tool output, and prior assistant text.
pub fn assess_fullcaps(
    prompt: &str,
    dynamic_terms: impl IntoIterator<Item = impl AsRef<str>>,
) -> FullCapsAssessment {
    let dynamic_terms: HashSet<String> = dynamic_terms
        .into_iter()
        .map(|term| term.as_ref().to_uppercase())
        .collect();
    let mut in_fence = false;
    let mut cased_letters = 0_u16;
    let mut lowercase_letters = 0_u16;
    for line in prompt.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence || trimmed.starts_with('>') {
            continue;
        }
        let mut in_inline_code = false;
        for token in line.split_whitespace() {
            let ticks = token.chars().filter(|ch| *ch == '`').count();
            if in_inline_code || ticks > 0 {
                if ticks % 2 == 1 {
                    in_inline_code = !in_inline_code;
                }
                continue;
            }
            if is_technical_token(token, &dynamic_terms) {
                continue;
            }
            for character in token.chars() {
                if character.is_uppercase() || character.is_lowercase() {
                    cased_letters = cased_letters.saturating_add(1);
                    if character.is_lowercase() {
                        lowercase_letters = lowercase_letters.saturating_add(1);
                    }
                }
            }
        }
    }
    FullCapsAssessment {
        is_fullcaps: cased_letters >= 8 && lowercase_letters == 0,
        cased_letters,
        lowercase_letters,
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

/// Check if an event represents a human action that establishes presence
fn is_human_presence_event<P, I>(event: &NormalizedEvent<P, I>) -> Option<HumanActionKind> {
    match event.kind {
        EventKind::PromptSubmitted => {
            let prompt = event.prompt.as_ref()?;
            if prompt.is_approval {
                Some(HumanActionKind::Approval)
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
        EventKind::HumanEdit | EventKind::GitCommit => {
            // Only a positively-human edit or commit establishes human
            // presence.  Agent and unknown attributions create no presence.
            if event.role == ActorRole::Human {
                Some(HumanActionKind::HumanEdit)
            } else {
                None
            }
        }
        _ => None,
    }
}

/// Classify the evidence strength for a bucket based on events in it
fn classify_evidence<P, I>(
    events: &[&NormalizedEvent<P, I>],
    _config: &FormulaConfig,
) -> (EvidenceClass, Vec<EventId>) {
    let mut evidence_ids = Vec::new();
    let mut has_driven = false;
    let mut has_steered = false;
    let mut has_directed = false;

    for event in events {
        match event.kind {
            EventKind::HumanEdit => {
                if event.role == ActorRole::Human {
                    let change = event.change.as_ref();
                    if change.is_some_and(|c| !c.hunk_hashes.is_empty()) {
                        has_driven = true;
                        evidence_ids.push(event.id.clone());
                    }
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
                if tool.is_some_and(|t| {
                    t.is_steering
                        || matches!(
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
            if let Some(prompt) = &event.prompt
                && prompt.is_hypothesis
                && !prompt.is_low_effort
            {
                return true;
            }
        }
        _ => {}
    }
    if let Some(path) = &event.rel_path
        && !seen_files.contains(path)
    {
        return true;
    }
    false
}

/// Build human action timeline per user across all sessions/projects
fn build_human_timeline<P: Clone + Eq + std::hash::Hash, I: Clone + ToString>(
    events: &[NormalizedEvent<P, I>],
    _config: &FormulaConfig,
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
        } else if event.kind == EventKind::ToolFinished
            && event.role == ActorRole::Agent
            && let Some(tool) = &event.tool
            && let Some(fin) = tool.finished_at
        {
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
        let _bucket_key = UnixMillis(bucket);

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

/// Prompt text is deliberately not retained, so no dark-flow penalty can be
/// computed without guessing from unrelated metadata such as event IDs.
fn compute_dark_flow_factor<P, I>(
    _events: &[NormalizedEvent<P, I>],
    _config: &FormulaConfig,
) -> BTreeMap<(String, UnixMillis), (u16, Vec<DarkFlowPenalty>)> {
    BTreeMap::new()
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
                    bucket.credited_seconds = bucket.credited_seconds * scale / 1_000;
                }
            }
        }
    }
}

type Interval = (i64, i64);

fn merge_intervals(mut intervals: Vec<Interval>) -> Vec<Interval> {
    intervals.retain(|(start, end)| end > start);
    intervals.sort_unstable_by_key(|(start, end)| (*start, *end));
    let mut merged: Vec<Interval> = Vec::new();
    for (start, end) in intervals {
        if let Some((_, previous_end)) = merged.last_mut()
            && start <= *previous_end
        {
            *previous_end = (*previous_end).max(end);
        } else {
            merged.push((start, end));
        }
    }
    merged
}

fn interval_measure(intervals: &[Interval]) -> u64 {
    intervals
        .iter()
        .map(|(start, end)| u64::try_from(end.saturating_sub(*start)).unwrap_or(0))
        .sum()
}

/// Derive credited time as a union of intervals, never as a sum of concurrent
/// harness sessions. Agent output by itself creates no credit: a run counts
/// only when a subsequent human-presence event arrives within the idle limit.
pub fn account_time<P, I>(
    events: &[NormalizedEvent<P, I>],
    config: TimeAccountingConfig,
) -> TimeAccounting {
    let mut timeline: Vec<_> = events.iter().collect();
    timeline.sort_unstable_by_key(|event| event.occurred_at.0);
    let is_presence = |event: &&NormalizedEvent<P, I>| {
        matches!(
            event.kind,
            EventKind::PromptSubmitted | EventKind::HumanEdit | EventKind::GitCommit
        )
    };
    let presence: Vec<_> = timeline.iter().filter(|event| is_presence(event)).collect();
    let mut human = Vec::new();
    for pair in presence.windows(2) {
        let start = pair[0].occurred_at.0;
        let end = pair[1]
            .occurred_at
            .0
            .min(start.saturating_add(config.human_gap_cap_millis));
        human.push((start, end));
    }

    let mut agent = Vec::new();
    for (index, prompt) in timeline.iter().enumerate() {
        if prompt.kind != EventKind::PromptSubmitted {
            continue;
        }
        let next_prompt = timeline[index + 1..]
            .iter()
            .position(|event| event.kind == EventKind::PromptSubmitted)
            .map(|offset| index + 1 + offset)
            .unwrap_or(timeline.len());
        let last_agent = timeline[index + 1..next_prompt]
            .iter()
            .rev()
            .find(|event| event.role == ActorRole::Agent);
        let next_presence = timeline[next_prompt..]
            .iter()
            .find(|event| is_presence(event));
        if let (Some(last_agent), Some(follow_up)) = (last_agent, next_presence)
            && follow_up
                .occurred_at
                .0
                .saturating_sub(last_agent.occurred_at.0)
                <= config.follow_up_idle_limit_millis
        {
            agent.push((
                prompt.occurred_at.0,
                last_agent.occurred_at.0.min(
                    prompt
                        .occurred_at
                        .0
                        .saturating_add(config.agent_wait_cap_millis),
                ),
            ));
        }
    }
    let human = merge_intervals(human);
    let agent = merge_intervals(agent);
    let human_active_millis = interval_measure(&human);
    let combined = merge_intervals(human.iter().chain(&agent).copied().collect::<Vec<_>>());
    let union_millis = interval_measure(&combined);
    let agent_attended_millis = union_millis.saturating_sub(human_active_millis);
    let sum_interval_millis = interval_measure(&human).saturating_add(interval_measure(&agent));
    TimeAccounting {
        human_active_millis,
        agent_attended_millis,
        union_millis,
        sum_interval_millis,
        concurrency_ratio_permille: sum_interval_millis
            .saturating_mul(1_000)
            .checked_div(union_millis)
            .unwrap_or(0),
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
            if let Some(tool) = &event.tool
                && let Some(sig) = &tool.error_signature
            {
                error_signatures.push(sig.clone());
            }
            if let Some(path) = &event.rel_path {
                files_touched.push(path.clone());
            }
            if event.kind == EventKind::HumanEdit
                && event.role == ActorRole::Human
                && let Some(change) = &event.change
            {
                human_edit_seconds += u64::from(change.human_added + change.human_deleted);
                agent_edit_seconds += u64::from(change.agent_added + change.agent_deleted);
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
            if !current_episode_buckets.is_empty()
                && let Some(project_name) = current_project.take()
            {
                episodes.push(finalize_episode_from_buckets(
                    project_name,
                    &mut current_episode_buckets,
                    config,
                ));
            }
            current_project = Some(project);
            current_episode_buckets.clear();
        }

        current_episode_buckets.push(bucket);
        last_bucket_time = Some(bucket_time);
    }

    if !current_episode_buckets.is_empty()
        && let Some(project_name) = current_project.take()
    {
        episodes.push(finalize_episode_from_buckets(
            project_name,
            &mut current_episode_buckets,
            config,
        ));
    }

    episodes
}

fn finalize_episode_from_buckets<P: Clone + Eq + std::hash::Hash + std::fmt::Debug + Ord>(
    project: String,
    buckets: &mut [CreditedBucket<P>],
    _config: &FormulaConfig,
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
        buckets: buckets.to_vec(),
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
    use hackai_schema::{ActorRole, EventId, EventKind, EventSource, NormalizedEvent, UnixMillis};

    fn event(
        id: &str,
        at: i64,
        kind: EventKind,
        role: ActorRole,
    ) -> NormalizedEvent<String, String> {
        NormalizedEvent {
            schema_version: 1,
            id: EventId(id.into()),
            occurred_at: UnixMillis(at),
            kind,
            role,
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
    fn time_accounting_caps_gaps_and_unions_concurrent_activity() {
        let events = vec![
            event("prompt-1", 0, EventKind::PromptSubmitted, ActorRole::Human),
            event("agent-1", 60_000, EventKind::ToolFinished, ActorRole::Agent),
            event(
                "prompt-2",
                180_000,
                EventKind::PromptSubmitted,
                ActorRole::Human,
            ),
            event("edit", 190_000, EventKind::HumanEdit, ActorRole::Human),
        ];
        let accounting = account_time(&events, TimeAccountingConfig::default());
        // 0..120s plus 180..190s for human presence; the attended agent
        // interval is wholly inside the first human interval.
        assert_eq!(accounting.human_active_millis, 130_000);
        assert_eq!(accounting.agent_attended_millis, 0);
        assert_eq!(accounting.union_millis, 130_000);
        assert!(accounting.concurrency_ratio_permille > 1_000);
    }

    #[test]
    fn time_accounting_does_not_credit_unattended_agent_work() {
        let events = vec![
            event("prompt", 0, EventKind::PromptSubmitted, ActorRole::Human),
            event("agent", 60_000, EventKind::ToolFinished, ActorRole::Agent),
            event("later", 1_000_000, EventKind::HumanEdit, ActorRole::Human),
        ];
        let accounting = account_time(&events, TimeAccountingConfig::default());
        assert_eq!(accounting.agent_attended_millis, 0);
        assert_eq!(accounting.human_active_millis, 120_000);
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
    fn event_ids_do_not_trigger_duplicate_prompt_penalties() {
        let mut events = vec![
            event(
                "prompt-aaa",
                0,
                EventKind::PromptSubmitted,
                ActorRole::Human,
            ),
            event(
                "prompt-aab",
                1,
                EventKind::PromptSubmitted,
                ActorRole::Human,
            ),
        ];
        for event in &mut events {
            event.prompt = Some(hackai_schema::PromptFeatures {
                char_len: 20,
                has_attachment: false,
                has_code_fence: false,
                is_approval: false,
                is_hypothesis: true,
                is_low_effort: false,
            });
        }
        assert!(compute_dark_flow_factor(&events, &FormulaConfig::default()).is_empty());
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
                        hackai_schema::SaltedHmacHunkHash::parse("a".repeat(64))
                            .expect("valid hash"),
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
                        hackai_schema::SaltedHmacHunkHash::parse("b".repeat(64))
                            .expect("valid hash"),
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

    #[test]
    fn pesdo_gate_and_soft_factor_have_one_deterministic_result() {
        let factors = PesdoFactors {
            presence: 1_000,
            evidence: 500,
            specification: 1_000,
            diligence: 1_000,
        };
        let gate = PesdoConfig::default();
        assert_eq!(score_pesdo(100, factors, false, gate), 0);
        assert_eq!(score_pesdo(100, factors, true, gate), 50);
        let soft = PesdoConfig {
            outcome: OutcomeConfig {
                mode: OutcomeMode::SoftFactor,
                soft_factor_permille: 500,
            },
            ..PesdoConfig::default()
        };
        assert_eq!(score_pesdo(100, factors, false, soft), 25);
    }

    #[test]
    fn process_delta_is_context_sensitive_and_bounded() {
        let config = FormulaConfig::default();
        let turns = [
            (
                AgentState::EditedRed,
                UserAct::ReportFailure,
                FailureContext::default(),
            ),
            (
                AgentState::EditedRed,
                UserAct::ReportFailure,
                FailureContext {
                    evidence: true,
                    expectation: false,
                    location: false,
                },
            ),
            (
                AgentState::EditedGreen,
                UserAct::Approve,
                FailureContext::default(),
            ),
        ];
        assert_eq!(
            process_delta(
                &turns,
                &config.transition_rules,
                config.pesdo.process_delta_limit
            ),
            1
        );
        assert_eq!(process_delta(&turns, &config.transition_rules, 1), 1);
    }

    #[test]
    fn fullcaps_skips_code_tech_terms_and_caseless_text() {
        assert!(
            assess_fullcaps("FIX THIS BROKEN THING NOW", std::iter::empty::<&str>()).is_fullcaps
        );
        assert!(
            !assess_fullcaps(
                "FIX `MAX_RETRIES` in ```rs\nHTTP\n```",
                std::iter::empty::<&str>()
            )
            .is_fullcaps
        );
        assert!(!assess_fullcaps("这是一个测试", std::iter::empty::<&str>()).is_fullcaps);
        assert!(assess_fullcaps("BROKEN WIDGET", std::iter::empty::<&str>()).is_fullcaps);
        assert!(!assess_fullcaps("BROKEN WIDGET", ["WIDGET"]).is_fullcaps);
    }

    fn commit_event(role: ActorRole) -> NormalizedEvent<String, String> {
        NormalizedEvent {
            schema_version: 1,
            id: EventId("commit".into()),
            occurred_at: UnixMillis(1),
            kind: EventKind::GitCommit,
            role,
            source: EventSource::Codex,
            project: "p".into(),
            session: "s".into(),
            rel_path: None,
            prompt: None,
            tool: None,
            change: None,
            commit: None,
        }
    }

    fn worktree_event(role: ActorRole) -> NormalizedEvent<String, String> {
        NormalizedEvent {
            schema_version: 1,
            id: EventId("worktree".into()),
            occurred_at: UnixMillis(1),
            kind: EventKind::HumanEdit,
            role,
            source: EventSource::Codex,
            project: "p".into(),
            session: "s".into(),
            rel_path: None,
            prompt: None,
            tool: None,
            change: None,
            commit: None,
        }
    }

    #[test]
    fn human_commit_establishes_human_presence() {
        assert!(is_human_presence_event(&commit_event(ActorRole::Human)).is_some());
    }

    #[test]
    fn agent_and_unknown_commits_create_no_human_presence() {
        for role in [ActorRole::Agent, ActorRole::Unknown, ActorRole::Harness] {
            assert!(
                is_human_presence_event(&commit_event(role)).is_none(),
                "role {role:?} must not create human presence"
            );
        }
    }

    #[test]
    fn unknown_worktree_candidate_creates_no_human_presence() {
        assert!(is_human_presence_event(&worktree_event(ActorRole::Unknown)).is_none());
        assert!(is_human_presence_event(&worktree_event(ActorRole::Agent)).is_none());
        assert!(is_human_presence_event(&worktree_event(ActorRole::Human)).is_some());
    }

    #[test]
    fn legacy_commit_defaults_to_unknown_without_human_credit() {
        // A legacy commit with no resolvable role is unknown and credited to no one.
        assert_eq!(commit_event(ActorRole::Unknown).role, ActorRole::Unknown);
        assert!(is_human_presence_event(&commit_event(ActorRole::Unknown)).is_none());
    }
}
