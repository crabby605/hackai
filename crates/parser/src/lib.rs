#![forbid(unsafe_code)]
//! Claude Code JSONL feature extractor. Input text never appears in output.
use hackai_schema::{
    ActorRole, EventKind, EventSource, NormalizedEvent, PromptFeatures, ToolClass, ToolFeatures,
    UnixMillis,
};
use serde::Deserialize;
use sha2::{Digest, Sha256};

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
}
#[derive(Deserialize)]
struct ClaudeLine {
    timestamp: Option<i64>,
    role: Option<String>,
    #[serde(default)]
    message: Option<ClaudeMessage>,
}
#[derive(Deserialize)]
struct ClaudeMessage {
    #[serde(default)]
    content: Option<String>,
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
        let record: ClaudeLine = serde_json::from_str(line).map_err(|source| ParseError::Json {
            line: index + 1,
            source,
        })?;
        let time = record.timestamp.ok_or(ParseError::MissingTime(index + 1))?;
        let content = record.message.and_then(|m| m.content).unwrap_or_default();
        let role = if record.role.as_deref() == Some("assistant") {
            ActorRole::Agent
        } else {
            ActorRole::Human
        };
        let kind = if role == ActorRole::Human {
            EventKind::PromptSubmitted
        } else {
            EventKind::ToolFinished
        };
        parsed.push(NormalizedEvent {
            schema_version: 1,
            id: hackai_schema::EventId(format!("claude-{session}-{index}")),
            occurred_at: UnixMillis(time),
            kind,
            role,
            source: EventSource::ClaudeCode,
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
            tool: (role == ActorRole::Agent).then_some(ToolFeatures {
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
        let record: ClaudeLine = serde_json::from_str(line).map_err(|source| ParseError::Json {
            line: index + 1,
            source,
        })?;
        if record.role.as_deref() != Some("assistant") {
            let time = record.timestamp.ok_or(ParseError::MissingTime(index + 1))?;
            visitor(
                UnixMillis(time),
                record
                    .message
                    .and_then(|message| message.content)
                    .as_deref()
                    .unwrap_or(""),
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
