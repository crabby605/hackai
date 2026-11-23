#![forbid(unsafe_code)]

use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct UnixMillis(pub i64);

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct EventId(pub String);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum EventKind {
    PromptSubmitted,
    ToolStarted,
    ToolFinished,
    HumanEdit,
    GitCommit,
    SessionEnded,
    Annotation,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum ActorRole {
    Human,
    Agent,
    Harness,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum EventSource {
    ClaudeCode,
    Codex,
    OpenCode,
    Gemini,
    Aider,
    QwenCode,
    KimiCode,
    ZCode,
    Copilot,
    ManualCli,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub enum ToolClass {
    Shell,
    FileRead,
    FileWrite,
    FilePatch,
    Search,
    Test,
    Build,
    Lint,
    Run,
    Debug,
    Git,
    Network,
    Other,
}

macro_rules! hex_newtype {
    ($name:ident) => {
        #[derive(Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(pub String);
        impl $name {
            pub fn parse(value: String) -> Result<Self, ValueError> {
                if value.len() == 64
                    && value
                        .bytes()
                        .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
                {
                    Ok(Self(value))
                } else {
                    Err(ValueError::InvalidHex(stringify!($name)))
                }
            }
        }
    };
}
hex_newtype!(ErrorSignatureHash);
hex_newtype!(SaltedHmacHunkHash);
hex_newtype!(GitTreeHash);
#[derive(Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct GitSha(pub String);
impl GitSha {
    pub fn parse(value: String) -> Result<Self, ValueError> {
        if value.len() >= 40
            && value.len() <= 64
            && value
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        {
            Ok(Self(value))
        } else {
            Err(ValueError::InvalidHex("GitSha"))
        }
    }
}

/// Case-insensitive unique identifier, restricted charset, reserved-name list.
/// Stored lowercase; comparisons are case-insensitive.
fn validate_name(value: &str, field: &'static str) -> Result<(), ValueError> {
    if value.is_empty() {
        return Err(ValueError::InvalidName(format!("{field} is empty")));
    }
    let len = value.chars().count();
    if !(2..=64).contains(&len) {
        return Err(ValueError::InvalidName(format!(
            "{field} length must be 2..=64"
        )));
    }
    let mut chars = value.chars();
    let first = match chars.next() {
        // value is non-empty (checked above), so this arm is unreachable.
        Some(c) => c,
        None => return Err(ValueError::InvalidName(format!("{field} is empty"))),
    };
    if !first.is_ascii_alphanumeric() {
        return Err(ValueError::InvalidName(format!(
            "{field} must start with alphanumeric"
        )));
    }
    for c in value.chars() {
        match c {
            'a'..='z' | '0'..='9' | '-' | '_' => {}
            _ => {
                return Err(ValueError::InvalidName(format!(
                    "{field} may only contain lowercase letters, digits, hyphens, underscores"
                )));
            }
        }
    }
    let last = match value.chars().last() {
        // value is non-empty (checked above), so this arm is unreachable.
        Some(c) => c,
        None => return Err(ValueError::InvalidName(format!("{field} is empty"))),
    };
    if last == '-' || last == '_' {
        return Err(ValueError::InvalidName(format!(
            "{field} must end with alphanumeric"
        )));
    }
    Ok(())
}

/// Reserved names that may not be used as a username or project name.
const RESERVED_NAMES: &[&str] = &[
    "www",
    "api",
    "admin",
    "root",
    "git",
    "hackai",
    "hackatime",
    "me",
    "settings",
    "new",
    "login",
    "logout",
    "auth",
    "healthz",
];

#[derive(Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ProjectName(pub String);
impl ProjectName {
    pub fn parse(value: String) -> Result<Self, ValueError> {
        let normalized = value.to_ascii_lowercase();
        validate_name(&normalized, "project name")?;
        if RESERVED_NAMES.contains(&normalized.as_str()) {
            return Err(ValueError::InvalidName(format!(
                "project name '{normalized}' is reserved"
            )));
        }
        Ok(Self(normalized))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Username(pub String);
impl Username {
    pub fn parse(value: String) -> Result<Self, ValueError> {
        let normalized = value.to_ascii_lowercase();
        validate_name(&normalized, "username")?;
        if RESERVED_NAMES.contains(&normalized.as_str()) {
            return Err(ValueError::InvalidName(format!(
                "username '{normalized}' is reserved"
            )));
        }
        Ok(Self(normalized))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ValueError {
    #[error("invalid {0}")]
    InvalidHex(&'static str),
    #[error("invalid name: {0}")]
    InvalidName(String),
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PromptFeatures {
    pub char_len: u32,
    pub has_attachment: bool,
    pub has_code_fence: bool,
    pub is_approval: bool,
    #[serde(default)]
    pub is_hypothesis: bool,
    #[serde(default)]
    pub is_low_effort: bool,
}

/// Privacy-safe, closed vocabulary for a human turn.  The raw prompt belongs
/// only in the local evidence store; exports carry this value and its derived
/// features instead.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UserAct {
    Specify,
    Nudge,
    ReportFailure,
    Steer,
    Decide,
    Approve,
    RejectOrRevert,
    Interrupt,
    AskToUnderstand,
    Meta,
}

/// Grounded context attached to a failure report.  `false, false, false` is
/// deliberately the only definition of a contextless report.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct FailureContext {
    pub evidence: bool,
    pub expectation: bool,
    pub location: bool,
}

impl FailureContext {
    pub const fn is_contextless(self) -> bool {
        !self.evidence && !self.expectation && !self.location
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VerificationState {
    Green,
    Red,
    Unverified,
    NotApplicable,
}

/// State of the preceding agent turn.  Implementations should derive these
/// from tool traces and verification before considering the agent's text.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentState {
    Asked,
    Planned,
    Explained,
    EditedGreen,
    EditedRed,
    EditedUnverified,
    ClaimedDoneUnverified,
    ToolError,
    Blocked,
    Looping,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InformationBucket {
    None,
    Low,
    Medium,
    High,
}

/// A redaction-safe record of a dialogue transition.  IDs point to local
/// evidence events; no prompt or reply text is present here.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DialogueTurn {
    pub event_id: EventId,
    pub reply_to: Option<EventId>,
    pub episode_id: String,
    pub rule_version: u16,
    pub user_act: UserAct,
    /// Confidence in permille, never a score.
    pub act_confidence_permille: u16,
    pub context: FailureContext,
    pub new_information: InformationBucket,
    pub prompt_length_bucket: u8,
    pub fullcaps: bool,
    pub repeat_distance_bucket: u8,
    pub answers_question: bool,
    pub latency_bucket: u8,
    pub agent_state: AgentState,
    pub verification: VerificationState,
    pub files_changed_bucket: u8,
    pub tool_errors: u16,
    pub reason_codes: Vec<String>,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ToolFeatures {
    pub class: ToolClass,
    pub started_at: UnixMillis,
    pub finished_at: Option<UnixMillis>,
    pub exit_code: Option<i32>,
    pub error_signature: Option<ErrorSignatureHash>,
    #[serde(default)]
    pub is_steering: bool,
    #[serde(default)]
    pub output_low_entropy: bool,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ChangeFeatures {
    pub human_added: u32,
    pub human_deleted: u32,
    pub agent_added: u32,
    pub agent_deleted: u32,
    pub hunk_hashes: Vec<SaltedHmacHunkHash>,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CommitEvidence {
    pub sha: GitSha,
    pub tree: GitTreeHash,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct NormalizedEvent<P, I> {
    pub schema_version: u16,
    pub id: EventId,
    pub occurred_at: UnixMillis,
    pub kind: EventKind,
    pub role: ActorRole,
    pub source: EventSource,
    pub project: I,
    pub session: String,
    pub rel_path: Option<P>,
    pub prompt: Option<PromptFeatures>,
    pub tool: Option<ToolFeatures>,
    pub change: Option<ChangeFeatures>,
    pub commit: Option<CommitEvidence>,
}

impl fmt::Display for EventId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
