#![forbid(unsafe_code)]
//! Claude Code JSONL feature extractor. Input text never appears in output.
use hackai_schema::{
    ActorRole, EventKind, EventSource, NormalizedEvent, PromptFeatures, ToolClass, ToolFeatures,
    UnixMillis,
};
use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::path::Path;

/// Hard input bounds prevent a hostile or corrupted transcript from consuming
/// unbounded memory before features are extracted.
pub const MAX_JSONL_RECORDS: usize = 10_000;
pub const MAX_JSONL_LINE_BYTES: usize = 1_048_576;

#[derive(Debug, thiserror::Error)]
pub enum ParseError {
    #[error("invalid JSONL record at line {line}: {source}")]
    Json {
        line: usize,
        #[source]
        source: serde_json::Error,
    },
    #[error("record at line {0} has no timestamp")]
    MissingTime(usize),
    #[error("transcript exceeds the {MAX_JSONL_RECORDS}-record limit")]
    TooManyRecords,
    #[error("record at line {line} exceeds the {MAX_JSONL_LINE_BYTES}-byte limit")]
    LineTooLong { line: usize },
    #[error("could not read OpenCode database: {0}")]
    OpenCodeDatabase(String),
}
#[derive(Deserialize)]
struct ClaudeLine {
    #[serde(rename = "type")]
    entry_type: Option<String>,
    timestamp: Option<Value>,
    role: Option<String>,
    #[serde(default)]
    message: Option<ClaudeMessage>,
}
#[derive(Deserialize)]
struct ClaudeMessage {
    role: Option<String>,
    #[serde(default)]
    content: Option<Value>,
}

fn parse_timestamp(value: Option<Value>) -> Option<i64> {
    match value? {
        Value::Number(number) => number.as_i64(),
        Value::String(value) => value
            .parse::<i64>()
            .ok()
            .or_else(|| parse_rfc3339_millis(&value)),
        _ => None,
    }
}

/// Parse the RFC3339 timestamps emitted by Claude Code without retaining the
/// original value. Claude currently emits UTC timestamps, but offsets are
/// accepted so imported transcripts remain usable.
fn parse_rfc3339_millis(value: &str) -> Option<i64> {
    let bytes = value.as_bytes();
    if bytes.len() < 20 || bytes.get(4) != Some(&b'-') || bytes.get(7) != Some(&b'-') {
        return None;
    }
    let year = value.get(0..4)?.parse::<i64>().ok()?;
    let month = value.get(5..7)?.parse::<u32>().ok()?;
    let day = value.get(8..10)?.parse::<u32>().ok()?;
    let separator = *bytes.get(10)?;
    if separator != b'T' && separator != b't' && separator != b' ' {
        return None;
    }
    let hour = value.get(11..13)?.parse::<i64>().ok()?;
    let minute = value.get(14..16)?.parse::<i64>().ok()?;
    let second = value.get(17..19)?.parse::<i64>().ok()?;
    let leap_year =
        year.rem_euclid(4) == 0 && (year.rem_euclid(100) != 0 || year.rem_euclid(400) == 0);
    let days_in_month = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap_year => 29,
        2 => 28,
        _ => return None,
    };
    if bytes.get(13) != Some(&b':')
        || bytes.get(16) != Some(&b':')
        || !(1..=12).contains(&month)
        || day == 0
        || day > days_in_month
        || hour > 23
        || minute > 59
        || second > 60
    {
        return None;
    }

    let mut index = 19;
    let mut millis = 0_i64;
    if bytes.get(index) == Some(&b'.') {
        index += 1;
        let start = index;
        while bytes.get(index).is_some_and(u8::is_ascii_digit) {
            index += 1;
        }
        let fraction = value.get(start..index)?;
        if fraction.is_empty() {
            return None;
        }
        let digits = fraction.as_bytes();
        for position in 0..3 {
            millis = millis.saturating_mul(10)
                + digits
                    .get(position)
                    .map_or(0, |digit| i64::from(digit - b'0'));
        }
    }
    let offset_seconds = match bytes.get(index).copied()? {
        b'Z' | b'z' if index + 1 == bytes.len() => 0,
        sign @ (b'+' | b'-') if index + 6 == bytes.len() && bytes.get(index + 3) == Some(&b':') => {
            let offset_hour = value.get(index + 1..index + 3)?.parse::<i64>().ok()?;
            let offset_minute = value.get(index + 4..index + 6)?.parse::<i64>().ok()?;
            if offset_hour > 23 || offset_minute > 59 {
                return None;
            }
            let seconds = offset_hour * 3_600 + offset_minute * 60;
            if sign == b'+' { seconds } else { -seconds }
        }
        _ => return None,
    };
    let adjusted_year = year - i64::from(month <= 2);
    let era = adjusted_year.div_euclid(400);
    let year_of_era = adjusted_year - era * 400;
    let shifted_month = i64::from(month) + if month > 2 { -3 } else { 9 };
    let day_of_year = (153 * shifted_month + 2) / 5 + i64::from(day) - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    let days_since_epoch = era * 146_097 + day_of_era - 719_468;
    days_since_epoch
        .checked_mul(86_400_000)?
        .checked_add((hour * 3_600 + minute * 60 + second - offset_seconds) * 1_000)?
        .checked_add(millis)
}

fn content_text(content: Option<&Value>) -> String {
    match content {
        Some(Value::String(value)) => value.clone(),
        Some(Value::Array(blocks)) => blocks
            .iter()
            .filter_map(|block| block.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

fn role(record: &ClaudeLine) -> Option<&str> {
    record
        .role
        .as_deref()
        .or_else(|| record.message.as_ref()?.role.as_deref())
}

fn normalized_role(value: Option<&str>) -> Option<ActorRole> {
    match value?.to_ascii_lowercase().as_str() {
        "assistant" | "agent" => Some(ActorRole::Agent),
        "user" | "human" => Some(ActorRole::Human),
        _ => None,
    }
}

/// Claude's `user` envelope is also used to deliver a tool result back to the
/// model.  It is not a human prompt.  Looking at the content-block type avoids
/// incorrectly granting presence credit for those harness-generated records.
fn has_content_block_type(content: Option<&Value>, expected: &str) -> bool {
    match content {
        Some(Value::Array(blocks)) => blocks.iter().any(|block| {
            block
                .get("type")
                .and_then(Value::as_str)
                .is_some_and(|kind| kind.eq_ignore_ascii_case(expected))
        }),
        Some(Value::Object(block)) => block
            .get("type")
            .and_then(Value::as_str)
            .is_some_and(|kind| kind.eq_ignore_ascii_case(expected)),
        _ => false,
    }
}

fn content_blocks<'a>(content: Option<&'a Value>, expected: &str) -> Vec<&'a Value> {
    match content {
        Some(Value::Array(blocks)) => blocks
            .iter()
            .filter(|block| {
                block
                    .get("type")
                    .and_then(Value::as_str)
                    .is_some_and(|kind| kind.eq_ignore_ascii_case(expected))
            })
            .collect(),
        Some(value @ Value::Object(_))
            if value
                .get("type")
                .and_then(Value::as_str)
                .is_some_and(|kind| kind.eq_ignore_ascii_case(expected)) =>
        {
            vec![value]
        }
        _ => Vec::new(),
    }
}

fn contains_word(haystack: &str, needle: &str) -> bool {
    haystack
        .split(|c: char| !c.is_ascii_alphanumeric())
        .any(|word| word.eq_ignore_ascii_case(needle))
}

/// Removes values that are unstable or identifying while retaining the error's
/// type and message shape. Raw tool output never leaves this crate.
pub fn normalize_error_signature(output: &str) -> Option<String> {
    let lower = output.to_ascii_lowercase();
    let signals = [
        "error",
        "failed",
        "failure",
        "exception",
        "panic",
        "cannot",
        "undefined",
    ];
    if !signals.iter().any(|signal| contains_word(&lower, signal)) {
        return None;
    }
    let mut normalized = String::with_capacity(lower.len().min(256));
    let mut number = false;
    let mut skipped_path = false;
    for ch in lower.chars().take(4096) {
        if ch.is_ascii_digit() {
            if !number {
                normalized.push('#');
                number = true;
            }
            continue;
        }
        number = false;
        if ch == '/' || ch == '\\' {
            if !skipped_path {
                normalized.push_str("<path>");
                skipped_path = true;
            }
            continue;
        }
        if skipped_path {
            if ch.is_whitespace() {
                skipped_path = false;
                normalized.push(' ');
            }
            continue;
        }
        if ch.is_ascii_alphabetic() || ch.is_ascii_punctuation() || ch.is_whitespace() {
            normalized.push(ch);
        }
    }
    let normalized = normalized.split_whitespace().collect::<Vec<_>>().join(" ");
    (!normalized.is_empty()).then_some(normalized)
}

fn signature_hash(output: &str) -> Option<hackai_schema::ErrorSignatureHash> {
    let normalized = normalize_error_signature(output)?;
    let digest = Sha256::digest(normalized.as_bytes());
    let value = format!("{digest:x}");
    hackai_schema::ErrorSignatureHash::parse(value).ok()
}

fn classify_tool(content: &str) -> ToolClass {
    let value = content.to_ascii_lowercase();
    if ["test", "jest", "vitest", "cargo test", "pytest"]
        .iter()
        .any(|needle| value.contains(needle))
    {
        ToolClass::Test
    } else if ["lint", "clippy", "eslint"]
        .iter()
        .any(|needle| value.contains(needle))
    {
        ToolClass::Lint
    } else if ["build", "compile", "tsc"]
        .iter()
        .any(|needle| value.contains(needle))
    {
        ToolClass::Build
    } else if [
        "debug", "gdb", "lldb", "profile", "bisect", "--help", " man ",
    ]
    .iter()
    .any(|needle| value.contains(needle))
    {
        ToolClass::Debug
    } else if ["run", "start", "serve"]
        .iter()
        .any(|needle| value.contains(needle))
    {
        ToolClass::Run
    } else {
        ToolClass::Other
    }
}

fn low_entropy(output: &str) -> bool {
    let compact = output.split_whitespace().collect::<String>();
    compact.len() < 12
        || compact
            .chars()
            .collect::<std::collections::BTreeSet<_>>()
            .len()
            < 4
}

fn is_hypothesis_prompt(content: &str) -> bool {
    let value = content.to_ascii_lowercase();
    ([
        "because",
        "maybe",
        "likely",
        "hypothesis",
        "repro",
        "line ",
        "file ",
        "src/",
    ]
    .iter()
    .any(|needle| value.contains(needle)))
        && !matches!(value.trim(), "continue" | "fix it" | "try again")
}

pub fn parse_claude_jsonl<P: Clone, I: Clone>(
    jsonl: &str,
    project: I,
    rel_path: Option<P>,
    session: String,
) -> Result<Vec<NormalizedEvent<P, I>>, ParseError> {
    let mut parsed = Vec::new();
    let line_count = jsonl.lines().count();
    let mut record_count = 0_usize;
    for (index, line) in jsonl.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        if line.len() > MAX_JSONL_LINE_BYTES {
            return Err(ParseError::LineTooLong { line: index + 1 });
        }
        record_count = record_count.saturating_add(1);
        if record_count > MAX_JSONL_RECORDS {
            return Err(ParseError::TooManyRecords);
        }
        let record: ClaudeLine = match serde_json::from_str(line) {
            Ok(record) => record,
            Err(_) if index + 1 == line_count && !jsonl.ends_with('\n') => continue,
            Err(source) => {
                return Err(ParseError::Json {
                    line: index + 1,
                    source,
                });
            }
        };
        // Claude has several non-conversation envelope types (summary,
        // snapshots, queue operations, and system records).  They do not map
        // to a normalized event without inventing a human or agent action.
        // Unknown future types take this same safe path.
        let envelope_role = normalized_role(role(&record));
        let known_message_type = record
            .entry_type
            .as_deref()
            .is_none_or(|kind| matches!(kind.to_ascii_lowercase().as_str(), "user" | "assistant"));
        let Some(role) = envelope_role.filter(|_| known_message_type) else {
            continue;
        };
        let time = parse_timestamp(record.timestamp).ok_or(ParseError::MissingTime(index + 1))?;
        let content_value = record
            .message
            .as_ref()
            .and_then(|message| message.content.as_ref());
        let content = content_text(content_value);
        let base_id = format!("claude-{session}-{index}");

        if role == ActorRole::Human && !has_content_block_type(content_value, "tool_result") {
            parsed.push(NormalizedEvent {
                schema_version: 1,
                id: hackai_schema::EventId(base_id),
                occurred_at: UnixMillis(time),
                kind: EventKind::PromptSubmitted,
                role,
                source: EventSource::ClaudeCode,
                project: project.clone(),
                session: session.clone(),
                rel_path: rel_path.clone(),
                prompt: Some(PromptFeatures {
                    char_len: content.chars().count().try_into().unwrap_or(u32::MAX),
                    has_attachment: false,
                    has_code_fence: content.contains("```"),
                    is_approval: matches!(
                        content.trim().to_ascii_lowercase().as_str(),
                        "yes" | "y" | "approve" | "approved"
                    ),
                    is_hypothesis: is_hypothesis_prompt(&content),
                    is_low_effort: matches!(
                        content.trim().to_ascii_lowercase().as_str(),
                        "continue" | "fix it" | "try again" | "go on"
                    ),
                }),
                tool: None,
                change: None,
                commit: None,
            });
            continue;
        }

        let tool_results = content_blocks(content_value, "tool_result");
        if !tool_results.is_empty() {
            for (tool_index, block) in tool_results.iter().enumerate() {
                let output = value_text(block);
                let is_error = block
                    .get("is_error")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                parsed.push(NormalizedEvent {
                    schema_version: 1,
                    id: hackai_schema::EventId(format!("{base_id}-result-{tool_index}")),
                    occurred_at: UnixMillis(time),
                    kind: EventKind::ToolFinished,
                    role: ActorRole::Agent,
                    source: EventSource::ClaudeCode,
                    project: project.clone(),
                    session: session.clone(),
                    rel_path: rel_path.clone(),
                    prompt: None,
                    tool: Some(ToolFeatures {
                        class: ToolClass::Other,
                        started_at: UnixMillis(time),
                        finished_at: Some(UnixMillis(time)),
                        exit_code: None,
                        error_signature: if is_error {
                            signature_hash(&output).or_else(|| signature_hash("tool error"))
                        } else {
                            signature_hash(&output)
                        },
                        is_steering: false,
                        output_low_entropy: low_entropy(&output),
                    }),
                    change: None,
                    commit: None,
                });
            }
            continue;
        }

        let tool_uses = content_blocks(content_value, "tool_use");
        if !tool_uses.is_empty() {
            for (tool_index, block) in tool_uses.iter().enumerate() {
                let name = block
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                let class = classify_tool(name);
                parsed.push(NormalizedEvent {
                    schema_version: 1,
                    id: hackai_schema::EventId(format!("{base_id}-call-{tool_index}")),
                    occurred_at: UnixMillis(time),
                    kind: EventKind::ToolStarted,
                    role: ActorRole::Agent,
                    source: EventSource::ClaudeCode,
                    project: project.clone(),
                    session: session.clone(),
                    rel_path: rel_path.clone(),
                    prompt: None,
                    tool: Some(ToolFeatures {
                        class,
                        started_at: UnixMillis(time),
                        finished_at: None,
                        exit_code: None,
                        error_signature: None,
                        is_steering: matches!(class, ToolClass::Debug),
                        output_low_entropy: false,
                    }),
                    change: None,
                    commit: None,
                });
            }
            continue;
        }

        // A plain assistant reply is useful for dialogue ordering, but its
        // prose is not objective proof that a tool ran.  Keep only an
        // annotation rather than fabricating a successful tool completion.
        parsed.push(NormalizedEvent {
            schema_version: 1,
            id: hackai_schema::EventId(base_id),
            occurred_at: UnixMillis(time),
            kind: EventKind::Annotation,
            role: ActorRole::Agent,
            source: EventSource::ClaudeCode,
            project: project.clone(),
            session: session.clone(),
            rel_path: rel_path.clone(),
            prompt: None,
            tool: None,
            change: None,
            commit: None,
        });
    }
    Ok(parsed)
}

/// Parse the JSONL formats used by Codex and Gemini. Their on-disk schemas
/// evolve independently, so this deliberately recognizes only user/assistant
/// message records and ignores metadata, compaction, and unknown record types.
/// As with the Claude adapter, text exists only while local features are made.
pub fn parse_harness_jsonl<P: Clone, I: Clone>(
    jsonl: &str,
    project: I,
    rel_path: Option<P>,
    session: String,
    source: EventSource,
) -> Result<Vec<NormalizedEvent<P, I>>, ParseError> {
    let mut parsed = Vec::new();
    let line_count = jsonl.lines().count();
    let mut record_count = 0_usize;
    for (index, line) in jsonl.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        if line.len() > MAX_JSONL_LINE_BYTES {
            return Err(ParseError::LineTooLong { line: index + 1 });
        }
        record_count = record_count.saturating_add(1);
        if record_count > MAX_JSONL_RECORDS {
            return Err(ParseError::TooManyRecords);
        }
        let record: Value = match serde_json::from_str(line) {
            Ok(record) => record,
            Err(_) if index + 1 == line_count && !jsonl.ends_with('\n') => continue,
            Err(source) => {
                return Err(ParseError::Json {
                    line: index + 1,
                    source,
                });
            }
        };
        let Some(role) = find_role(&record) else {
            continue;
        };
        let Some(time) = find_timestamp(&record) else {
            continue;
        };
        let content = value_text(&record);
        let is_assistant = role == "assistant" || role == "agent";
        let actor_role = if is_assistant {
            ActorRole::Agent
        } else if role == "user" || role == "human" {
            ActorRole::Human
        } else {
            continue;
        };
        parsed.push(NormalizedEvent {
            schema_version: 1,
            id: hackai_schema::EventId(format!("{source:?}-{session}-{index}")),
            occurred_at: UnixMillis(time),
            kind: if is_assistant {
                EventKind::ToolFinished
            } else {
                EventKind::PromptSubmitted
            },
            role: actor_role,
            source,
            project: project.clone(),
            session: session.clone(),
            rel_path: rel_path.clone(),
            prompt: (!is_assistant).then_some(PromptFeatures {
                char_len: content.chars().count().try_into().unwrap_or(u32::MAX),
                has_attachment: false,
                has_code_fence: content.contains("```"),
                is_approval: matches!(
                    content.trim().to_ascii_lowercase().as_str(),
                    "yes" | "y" | "approve" | "approved"
                ),
                is_hypothesis: is_hypothesis_prompt(&content),
                is_low_effort: matches!(
                    content.trim().to_ascii_lowercase().as_str(),
                    "continue" | "fix it" | "try again" | "go on"
                ),
            }),
            tool: is_assistant.then_some(ToolFeatures {
                class: classify_tool(&content),
                started_at: UnixMillis(time),
                finished_at: Some(UnixMillis(time)),
                exit_code: None,
                error_signature: signature_hash(&content),
                is_steering: matches!(classify_tool(&content), ToolClass::Debug),
                output_low_entropy: low_entropy(&content),
            }),
            change: None,
            commit: None,
        });
    }
    Ok(parsed)
}

fn value_type(value: &Value) -> Option<&str> {
    value
        .get("type")
        .and_then(Value::as_str)
        .or_else(|| value.get("payload").and_then(value_type))
}

fn is_codex_message_type(record_type: &str, payload_type: Option<&str>) -> bool {
    record_type == "response_item"
        || (record_type == "event_msg"
            && payload_type.is_some_and(|kind| {
                matches!(
                    kind,
                    "user_message" | "agent_message" | "assistant_message" | "message"
                )
            }))
}

fn codex_role(record: &Value, record_type: &str, payload_type: Option<&str>) -> Option<ActorRole> {
    normalized_role(find_role(record)).or_else(|| {
        (record_type == "event_msg").then(|| match payload_type? {
            "user_message" => Some(ActorRole::Human),
            "agent_message" | "assistant_message" => Some(ActorRole::Agent),
            _ => None,
        })?
    })
}

fn codex_tool_kind(payload_type: Option<&str>) -> Option<EventKind> {
    match payload_type? {
        "function_call" | "tool_call" | "command_execution" => Some(EventKind::ToolStarted),
        "function_call_output" | "tool_result" | "command_execution_output" => {
            Some(EventKind::ToolFinished)
        }
        _ => None,
    }
}

fn find_named_string<'a>(value: &'a Value, keys: &[&str]) -> Option<&'a str> {
    match value {
        Value::Object(values) => keys
            .iter()
            .find_map(|key| values.get(*key).and_then(Value::as_str))
            .or_else(|| {
                values
                    .values()
                    .find_map(|value| find_named_string(value, keys))
            }),
        Value::Array(values) => values
            .iter()
            .find_map(|value| find_named_string(value, keys)),
        _ => None,
    }
}

fn find_exit_code(value: &Value) -> Option<i32> {
    match value {
        Value::Object(values) => values
            .get("exit_code")
            .or_else(|| values.get("exitCode"))
            .and_then(Value::as_i64)
            .and_then(|code| i32::try_from(code).ok())
            .or_else(|| values.values().find_map(find_exit_code)),
        Value::Array(values) => values.iter().find_map(find_exit_code),
        _ => None,
    }
}

/// Codex CLI rollout adapter.  Rollouts include metadata, compaction, state,
/// and token-count records.  Only documented response/event message and tool
/// records become normalized events; unknown future records are ignored.
pub fn parse_codex_jsonl<P: Clone, I: Clone>(
    jsonl: &str,
    project: I,
    rel_path: Option<P>,
    session: String,
) -> Result<Vec<NormalizedEvent<P, I>>, ParseError> {
    let mut parsed = Vec::new();
    let line_count = jsonl.lines().count();
    let mut record_count = 0_usize;
    for (index, line) in jsonl.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        if line.len() > MAX_JSONL_LINE_BYTES {
            return Err(ParseError::LineTooLong { line: index + 1 });
        }
        record_count = record_count.saturating_add(1);
        if record_count > MAX_JSONL_RECORDS {
            return Err(ParseError::TooManyRecords);
        }
        let record: Value = match serde_json::from_str(line) {
            Ok(record) => record,
            Err(_) if index + 1 == line_count && !jsonl.ends_with('\n') => continue,
            Err(source) => {
                return Err(ParseError::Json {
                    line: index + 1,
                    source,
                });
            }
        };
        let Some(record_type) = record.get("type").and_then(Value::as_str) else {
            continue;
        };
        let Some(payload) = record.get("payload").or_else(|| record.get("item")) else {
            continue;
        };
        let payload_type = value_type(payload);
        let message_role = codex_role(&record, record_type, payload_type);
        let tool_kind = codex_tool_kind(payload_type);
        if !is_codex_message_type(record_type, payload_type)
            && !(record_type == "response_item" && tool_kind.is_some())
        {
            continue;
        }
        let Some(time) = find_timestamp(&record) else {
            // A partially flushed rollout can contain a valid JSON object that
            // has not received its timestamp yet.  It is safe to revisit it.
            continue;
        };
        let id = hackai_schema::EventId(format!("codex-{session}-{index}"));
        if let Some(role) = message_role {
            let content = value_text(payload);
            parsed.push(NormalizedEvent {
                schema_version: 1,
                id,
                occurred_at: UnixMillis(time),
                kind: if role == ActorRole::Human {
                    EventKind::PromptSubmitted
                } else {
                    EventKind::Annotation
                },
                role,
                source: EventSource::Codex,
                project: project.clone(),
                session: session.clone(),
                rel_path: rel_path.clone(),
                prompt: (role == ActorRole::Human).then_some(PromptFeatures {
                    char_len: content.chars().count().try_into().unwrap_or(u32::MAX),
                    has_attachment: false,
                    has_code_fence: content.contains("```"),
                    is_approval: matches!(
                        content.trim().to_ascii_lowercase().as_str(),
                        "yes" | "y" | "approve" | "approved"
                    ),
                    is_hypothesis: is_hypothesis_prompt(&content),
                    is_low_effort: matches!(
                        content.trim().to_ascii_lowercase().as_str(),
                        "continue" | "fix it" | "try again" | "go on"
                    ),
                }),
                tool: None,
                change: None,
                commit: None,
            });
        } else if let Some(kind) = tool_kind {
            let name =
                find_named_string(payload, &["name", "tool_name", "command"]).unwrap_or_default();
            let output = value_text(payload);
            let class = classify_tool(name);
            parsed.push(NormalizedEvent {
                schema_version: 1,
                id,
                occurred_at: UnixMillis(time),
                kind,
                role: ActorRole::Agent,
                source: EventSource::Codex,
                project: project.clone(),
                session: session.clone(),
                rel_path: rel_path.clone(),
                prompt: None,
                tool: Some(ToolFeatures {
                    class,
                    started_at: UnixMillis(time),
                    finished_at: (kind == EventKind::ToolFinished).then_some(UnixMillis(time)),
                    exit_code: find_exit_code(payload),
                    error_signature: (kind == EventKind::ToolFinished)
                        .then(|| signature_hash(&output))
                        .flatten(),
                    is_steering: matches!(class, ToolClass::Debug),
                    output_low_entropy: kind == EventKind::ToolFinished && low_entropy(&output),
                }),
                change: None,
                commit: None,
            });
        }
    }
    Ok(parsed)
}

/// Gemini CLI session adapter.
pub fn parse_gemini_jsonl<P: Clone, I: Clone>(
    jsonl: &str,
    project: I,
    rel_path: Option<P>,
    session: String,
) -> Result<Vec<NormalizedEvent<P, I>>, ParseError> {
    parse_harness_jsonl(jsonl, project, rel_path, session, EventSource::Gemini)
}

/// OpenCode's older storage format is JSON/JSONL. Its record schema is close
/// enough to the common adapter that unknown records remain safely ignored.
pub fn parse_opencode_jsonl<P: Clone, I: Clone>(
    jsonl: &str,
    project: I,
    rel_path: Option<P>,
    session: String,
) -> Result<Vec<NormalizedEvent<P, I>>, ParseError> {
    parse_harness_jsonl(jsonl, project, rel_path, session, EventSource::OpenCode)
}

/// Qwen Code CLI session adapter.
/// Expected JSONL format with role, content, timestamp fields.
pub fn parse_qwen_code_jsonl<P: Clone, I: Clone>(
    jsonl: &str,
    project: I,
    rel_path: Option<P>,
    session: String,
) -> Result<Vec<NormalizedEvent<P, I>>, ParseError> {
    parse_harness_jsonl(jsonl, project, rel_path, session, EventSource::QwenCode)
}

/// Kimi Code CLI session adapter.
/// Expected JSONL format with role, content, timestamp fields.
pub fn parse_kimi_code_jsonl<P: Clone, I: Clone>(
    jsonl: &str,
    project: I,
    rel_path: Option<P>,
    session: String,
) -> Result<Vec<NormalizedEvent<P, I>>, ParseError> {
    parse_harness_jsonl(jsonl, project, rel_path, session, EventSource::KimiCode)
}

/// Z.ai Code (Z Code) CLI session adapter.
/// Expected JSONL format with role, content, timestamp fields.
pub fn parse_z_code_jsonl<P: Clone, I: Clone>(
    jsonl: &str,
    project: I,
    rel_path: Option<P>,
    session: String,
) -> Result<Vec<NormalizedEvent<P, I>>, ParseError> {
    parse_harness_jsonl(jsonl, project, rel_path, session, EventSource::ZCode)
}

/// GitHub Copilot CLI session adapter.
/// Expected JSONL format with role, content, timestamp fields.
pub fn parse_copilot_jsonl<P: Clone, I: Clone>(
    jsonl: &str,
    project: I,
    rel_path: Option<P>,
    session: String,
) -> Result<Vec<NormalizedEvent<P, I>>, ParseError> {
    parse_harness_jsonl(jsonl, project, rel_path, session, EventSource::Copilot)
}

/// Read the current OpenCode SQLite store without taking a write lock. Only
/// message metadata is used; malformed JSON rows are ignored rather than
/// aborting tracking while OpenCode is writing its database.
pub fn parse_opencode_sqlite<P: Clone, I: Clone>(
    database: &Path,
    project: I,
    rel_path: Option<P>,
    session: String,
) -> Result<Vec<NormalizedEvent<P, I>>, ParseError> {
    let connection = rusqlite::Connection::open_with_flags(
        database,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|error| ParseError::OpenCodeDatabase(error.to_string()))?;
    let mut statement = connection
        .prepare("SELECT time_created, data FROM message ORDER BY time_created")
        .map_err(|error| ParseError::OpenCodeDatabase(error.to_string()))?;
    let rows = statement
        .query_map([], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(|error| ParseError::OpenCodeDatabase(error.to_string()))?;
    let mut jsonl = String::new();
    let mut record_count = 0_usize;
    for (index, row) in rows.enumerate() {
        let (timestamp, data) =
            row.map_err(|error| ParseError::OpenCodeDatabase(error.to_string()))?;
        if data.len() > MAX_JSONL_LINE_BYTES {
            return Err(ParseError::LineTooLong { line: index + 1 });
        }
        let Ok(data) = serde_json::from_str::<Value>(&data) else {
            continue;
        };
        record_count += 1;
        if record_count > MAX_JSONL_RECORDS {
            return Err(ParseError::TooManyRecords);
        }
        let timestamp = if timestamp.abs() < 10_000_000_000 {
            timestamp.saturating_mul(1_000)
        } else {
            timestamp
        };
        let record = serde_json::json!({"timestamp": timestamp, "payload": data}).to_string();
        if record.len() > MAX_JSONL_LINE_BYTES {
            return Err(ParseError::LineTooLong { line: index + 1 });
        }
        jsonl.push_str(&record);
        jsonl.push('\n');
    }
    parse_opencode_jsonl(&jsonl, project, rel_path, session)
}

/// Aider's default markdown history has no per-message timestamps. Callers
/// provide the session-header timestamp; prompts receive stable one-second
/// offsets solely to preserve their ordering and must be treated as
/// low-confidence timing evidence.
pub fn parse_aider_markdown<P: Clone, I: Clone>(
    markdown: &str,
    session_started_at: UnixMillis,
    project: I,
    rel_path: Option<P>,
    session: String,
) -> Vec<NormalizedEvent<P, I>> {
    markdown
        .lines()
        .enumerate()
        .filter_map(|(index, line)| {
            let prompt = line.strip_prefix("> ")?;
            let offset = i64::try_from(index)
                .unwrap_or(i64::MAX)
                .saturating_mul(1_000);
            Some(NormalizedEvent {
                schema_version: 1,
                id: hackai_schema::EventId(format!("aider-{session}-{index}")),
                occurred_at: UnixMillis(session_started_at.0.saturating_add(offset)),
                kind: EventKind::PromptSubmitted,
                role: ActorRole::Human,
                source: EventSource::Aider,
                project: project.clone(),
                session: session.clone(),
                rel_path: rel_path.clone(),
                prompt: Some(PromptFeatures {
                    char_len: prompt.chars().count().try_into().unwrap_or(u32::MAX),
                    has_attachment: false,
                    has_code_fence: prompt.contains("```"),
                    is_approval: matches!(
                        prompt.trim().to_ascii_lowercase().as_str(),
                        "yes" | "y" | "approve" | "approved"
                    ),
                    is_hypothesis: is_hypothesis_prompt(prompt),
                    is_low_effort: matches!(
                        prompt.trim().to_ascii_lowercase().as_str(),
                        "continue" | "fix it" | "try again" | "go on"
                    ),
                }),
                tool: None,
                change: None,
                commit: None,
            })
        })
        .collect()
}

fn find_timestamp(record: &Value) -> Option<i64> {
    ["timestamp", "ts", "time_created", "created_at"]
        .iter()
        .find_map(|key| {
            record
                .get(key)
                .cloned()
                .and_then(|value| parse_timestamp(Some(value)))
        })
        .or_else(|| record.get("payload").and_then(find_timestamp))
}

fn find_role(record: &Value) -> Option<&str> {
    record
        .get("role")
        .and_then(Value::as_str)
        .or_else(|| {
            record
                .get("message")
                .and_then(|value| value.get("role"))
                .and_then(Value::as_str)
        })
        .or_else(|| record.get("payload").and_then(find_role))
}

fn value_text(value: &Value) -> String {
    fn visit(value: &Value, output: &mut Vec<String>) {
        match value {
            Value::Object(map) => {
                for key in ["text", "content", "input", "output"] {
                    if let Some(value) = map.get(key) {
                        visit(value, output);
                    }
                }
            }
            Value::Array(values) => values.iter().for_each(|value| visit(value, output)),
            Value::String(value) => output.push(value.clone()),
            _ => {}
        }
    }
    let mut output = Vec::new();
    visit(value, &mut output);
    output.join("\n")
}

/// Visit human Claude Code prompts while their transcript text is in memory.
/// The callback must derive and retain only the data it needs; this crate does
/// not serialize or return prompt text.
pub fn visit_claude_prompts(
    jsonl: &str,
    mut visitor: impl FnMut(UnixMillis, &str),
) -> Result<(), ParseError> {
    let mut record_count = 0_usize;
    for (index, line) in jsonl.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        if line.len() > MAX_JSONL_LINE_BYTES {
            return Err(ParseError::LineTooLong { line: index + 1 });
        }
        record_count = record_count.saturating_add(1);
        if record_count > MAX_JSONL_RECORDS {
            return Err(ParseError::TooManyRecords);
        }
        let record: ClaudeLine = match serde_json::from_str(line) {
            Ok(record) => record,
            Err(_) if index + 1 == jsonl.lines().count() && !jsonl.ends_with('\n') => continue,
            Err(source) => {
                return Err(ParseError::Json {
                    line: index + 1,
                    source,
                });
            }
        };
        if role(&record) != Some("assistant") {
            let time =
                parse_timestamp(record.timestamp).ok_or(ParseError::MissingTime(index + 1))?;
            visitor(
                UnixMillis(time),
                &content_text(
                    record
                        .message
                        .as_ref()
                        .and_then(|message| message.content.as_ref()),
                ),
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn drops_prompt_text() {
        let events = parse_claude_jsonl(
            "{\"timestamp\":1,\"role\":\"user\",\"message\":{\"content\":\"secret\"}}",
            (),
            None::<()>,
            "s".into(),
        )
        .unwrap_or_else(|_| unreachable!());
        assert_eq!(events[0].prompt.as_ref().map(|p| p.char_len), Some(6));
        let encoded = serde_json::to_string(&events).unwrap_or_else(|_| unreachable!());
        assert!(!encoded.contains("secret"));
    }
    #[test]
    fn parses_claude_envelope_roles_content_blocks_and_rfc3339_time() {
        let events = parse_claude_jsonl(
            r#"{"timestamp":"2025-01-01T00:00:00.123Z","message":{"role":"user","content":[{"type":"text","text":"PRIVATE_PROMPT"}]}}
{"timestamp":"2025-01-01T00:00:01+00:00","message":{"role":"assistant","content":[{"type":"text","text":"ran cargo test"}]}}"#,
            (),
            None::<()>,
            "s".into(),
        )
        .unwrap_or_else(|_| unreachable!());
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].occurred_at, UnixMillis(1_735_689_600_123));
        assert_eq!(events[0].role, ActorRole::Human);
        assert_eq!(events[1].role, ActorRole::Agent);
        assert_eq!(events[1].kind, EventKind::Annotation);
        assert!(events[1].tool.is_none());
        let encoded = serde_json::to_string(&events).unwrap_or_else(|_| unreachable!());
        assert!(!encoded.contains("PRIVATE_PROMPT"));
    }
    #[test]
    fn claude_extracts_tool_blocks_without_treating_results_as_prompts() {
        let events = parse_claude_jsonl(
            r#"{"type":"assistant","timestamp":1,"message":{"role":"assistant","content":[{"type":"tool_use","name":"Bash"}]}}
{"type":"user","timestamp":2,"message":{"role":"user","content":[{"type":"tool_result","content":"error: PRIVATE_PATH /tmp/private"}]}}
{"type":"summary","timestamp":3,"message":{"role":"user","content":"PRIVATE_SUMMARY"}}"#,
            (),
            None::<()>,
            "s".into(),
        )
        .unwrap_or_else(|_| unreachable!());
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].kind, EventKind::ToolStarted);
        assert_eq!(events[1].kind, EventKind::ToolFinished);
        assert_eq!(events[1].role, ActorRole::Agent);
        assert!(
            events[1]
                .tool
                .as_ref()
                .and_then(|tool| tool.error_signature.as_ref())
                .is_some()
        );
        let encoded = serde_json::to_string(&events).unwrap_or_else(|_| unreachable!());
        assert!(!encoded.contains("PRIVATE_PATH"));
        assert!(!encoded.contains("PRIVATE_SUMMARY"));
    }
    #[test]
    fn codex_ignores_metadata_and_extracts_tool_records() {
        let events = parse_codex_jsonl(
            r#"{"type":"session_meta","timestamp":1,"payload":{"role":"user","content":"PRIVATE_META"}}
{"type":"response_item","timestamp":2,"payload":{"type":"message","role":"user","content":"PRIVATE_USER"}}
{"type":"response_item","timestamp":3,"payload":{"type":"function_call","name":"cargo test"}}
{"type":"response_item","timestamp":4,"payload":{"type":"function_call_output","exit_code":1,"output":"error: PRIVATE_OUTPUT"}}
{"type":"compacted","timestamp":5,"payload":{"role":"user","content":"PRIVATE_COMPACT"}}"#,
            (),
            None::<()>,
            "codex".into(),
        )
        .unwrap_or_else(|_| unreachable!());
        assert_eq!(events.len(), 3);
        assert_eq!(events[0].kind, EventKind::PromptSubmitted);
        assert_eq!(events[1].kind, EventKind::ToolStarted);
        assert_eq!(
            events[1].tool.as_ref().map(|tool| tool.class),
            Some(ToolClass::Test)
        );
        assert_eq!(events[2].kind, EventKind::ToolFinished);
        assert_eq!(
            events[2].tool.as_ref().and_then(|tool| tool.exit_code),
            Some(1)
        );
        let encoded = serde_json::to_string(&events).unwrap_or_else(|_| unreachable!());
        assert!(!encoded.contains("PRIVATE_"));
    }
    #[test]
    fn ignores_a_partial_final_jsonl_record() {
        let events = parse_claude_jsonl(
            "{\"timestamp\":1,\"role\":\"user\",\"message\":{\"content\":\"ok\"}}\n{\"timestamp\":2",
            (),
            None::<()>,
            "s".into(),
        )
        .unwrap_or_else(|_| unreachable!());
        assert_eq!(events.len(), 1);
    }
    #[test]
    fn parses_codex_and_gemini_message_envelopes_without_text_serialization() {
        let codex = parse_codex_jsonl(
            r#"{"timestamp":"2025-01-01T00:00:00Z","type":"response_item","payload":{"role":"user","content":[{"type":"input_text","text":"PRIVATE_CODEX"}]}}
{"timestamp":"2025-01-01T00:00:01Z","type":"response_item","payload":{"role":"assistant","content":[{"type":"output_text","text":"cargo test"}]}}"#,
            (),
            None::<()>,
            "codex".into(),
        )
        .unwrap_or_else(|_| unreachable!());
        let gemini = parse_gemini_jsonl(
            r#"{"timestamp":1000,"message":{"role":"user","content":"PRIVATE_GEMINI"}}"#,
            (),
            None::<()>,
            "gemini".into(),
        )
        .unwrap_or_else(|_| unreachable!());
        assert_eq!(codex.len(), 2);
        assert_eq!(codex[0].source, EventSource::Codex);
        assert_eq!(gemini[0].source, EventSource::Gemini);
        let encoded = serde_json::to_string(&(codex, gemini)).unwrap_or_else(|_| unreachable!());
        assert!(!encoded.contains("PRIVATE_CODEX"));
        assert!(!encoded.contains("PRIVATE_GEMINI"));
    }
    #[test]
    fn parses_aider_prompts_with_order_only_timestamps() {
        let events = parse_aider_markdown(
            "# aider chat\n> PRIVATE_AIDER\nassistant reply\n> fix it",
            UnixMillis(5_000),
            (),
            None::<()>,
            "aider".into(),
        );
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].occurred_at, UnixMillis(6_000));
        assert_eq!(events[0].source, EventSource::Aider);
        let encoded = serde_json::to_string(&events).unwrap_or_else(|_| unreachable!());
        assert!(!encoded.contains("PRIVATE_AIDER"));
    }
    #[test]
    fn reads_opencode_sqlite_read_only() {
        let path = std::env::temp_dir().join(format!("hackai-parser-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        {
            let connection = rusqlite::Connection::open(&path).unwrap_or_else(|_| unreachable!());
            connection
                .execute("CREATE TABLE message (time_created INTEGER, data TEXT)", [])
                .unwrap_or_else(|_| unreachable!());
            connection
                .execute(
                    "INSERT INTO message (time_created, data) VALUES (?1, ?2)",
                    rusqlite::params![
                        1_735_689_600_i64,
                        r#"{"role":"user","content":"PRIVATE_OPENCODE"}"#
                    ],
                )
                .unwrap_or_else(|_| unreachable!());
        }
        let events = parse_opencode_sqlite(&path, (), None::<()>, "opencode".into())
            .unwrap_or_else(|_| unreachable!());
        let _ = std::fs::remove_file(&path);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].source, EventSource::OpenCode);
        let encoded = serde_json::to_string(&events).unwrap_or_else(|_| unreachable!());
        assert!(!encoded.contains("PRIVATE_OPENCODE"));
    }
    #[test]
    fn rejects_oversized_opencode_sqlite_row() {
        let path =
            std::env::temp_dir().join(format!("hackai-parser-oversized-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        {
            let connection = rusqlite::Connection::open(&path).unwrap_or_else(|_| unreachable!());
            connection
                .execute("CREATE TABLE message (time_created INTEGER, data TEXT)", [])
                .unwrap_or_else(|_| unreachable!());
            connection
                .execute(
                    "INSERT INTO message (time_created, data) VALUES (1, ?1)",
                    [serde_json::json!({"role":"user","content":"x".repeat(MAX_JSONL_LINE_BYTES)}).to_string()],
                )
                .unwrap_or_else(|_| unreachable!());
        }
        let result = parse_opencode_sqlite(&path, (), None::<()>, "opencode".into());
        let _ = std::fs::remove_file(&path);
        assert!(matches!(result, Err(ParseError::LineTooLong { line: 1 })));
    }
    #[test]
    fn rejects_oversized_line() {
        let input = "x".repeat(MAX_JSONL_LINE_BYTES + 1);
        assert!(matches!(
            parse_claude_jsonl(&input, (), None::<()>, "s".into()),
            Err(ParseError::LineTooLong { line: 1 })
        ));
    }
    #[test]
    fn rejects_too_many_records() {
        let input = "{\"timestamp\":1}\n".repeat(MAX_JSONL_RECORDS + 1);
        assert!(matches!(
            parse_claude_jsonl(&input, (), None::<()>, "s".into()),
            Err(ParseError::TooManyRecords)
        ));
    }
}
