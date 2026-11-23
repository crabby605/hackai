//! Derived-only evidence-bundle construction.
//!
//! This module deliberately has no serializer for raw transcript payloads. It
//! reads the local normalized event store and emits only the closed server
//! schema used by `/api/v1/evidence-bundles`.

use crate::state;
use rusqlite::{Connection, OpenFlags};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::io;
use std::path::Path;

const PROCESSOR_VERSION: &str = "derived-event-export-v1";
const FORMULA_VERSION: &str = "pesdo-v1";

pub struct EvidenceBundle {
    pub value: Value,
    pub event_count: usize,
    pub source_counts: Vec<(String, usize)>,
}

/// Build a stable, redacted export from the repository-local event database.
pub fn build(root: &Path) -> io::Result<EvidenceBundle> {
    let database_path = state::event_store_path(root);
    if !database_path.exists() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "no local derived events found; run `hackai collect` or `hackai ingest` first",
        ));
    }
    let database = Connection::open_with_flags(database_path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|error| io::Error::other(format!("open event store: {error}")))?;
    let mut query = database
        .prepare(
            "SELECT event_id, occurred_at_ms, source, session_id, payload_json
             FROM normalized_events ORDER BY occurred_at_ms, event_id LIMIT 1000",
        )
        .map_err(|error| io::Error::other(format!("read event store: {error}")))?;
    let rows = query
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
            ))
        })
        .map_err(|error| io::Error::other(format!("query event store: {error}")))?;

    let mut events = Vec::new();
    let mut source_counts = std::collections::BTreeMap::new();
    for (sequence, row) in rows.enumerate() {
        let (id, occurred_at, source, session, payload) =
            row.map_err(|error| io::Error::other(format!("read event row: {error}")))?;
        let payload: Value = serde_json::from_str(&payload)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        if let Some(event) = derive_event(&id, occurred_at, &source, &session, &payload, sequence) {
            *source_counts.entry(source).or_insert(0_usize) += 1;
            events.push(event);
        }
    }
    if events.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "the local event store has no exportable derived events",
        ));
    }

    let canonical_events = serde_json::to_vec(&events).map_err(io::Error::other)?;
    let content_hash = hash_bytes(&canonical_events);
    let generated_at = events
        .iter()
        .filter_map(|event| event.get("ts_utc_ms").and_then(Value::as_i64))
        .max()
        .unwrap_or(0);
    let bundle = json!({
        "bundle_id": format!("bundle-{}", &content_hash[..32]),
        "schema_version": 1,
        "processor_version": PROCESSOR_VERSION,
        "processor_hash": processor_hash(),
        "formula_version": FORMULA_VERSION,
        "client_version": env!("CARGO_PKG_VERSION"),
        "generated_at_unix_ms": generated_at,
        "preview_approved": true,
        "events": events,
    });
    Ok(EvidenceBundle {
        event_count: bundle["events"].as_array().map_or(0, Vec::len),
        value: bundle,
        source_counts: source_counts.into_iter().collect(),
    })
}

fn derive_event(
    id: &str,
    occurred_at: i64,
    source: &str,
    session: &str,
    payload: &Value,
    sequence: usize,
) -> Option<Value> {
    if occurred_at < 0 {
        return None;
    }
    let kind = payload.get("kind").and_then(Value::as_str)?;
    let event_kind = match kind {
        "PromptSubmitted" => "prompt_submit",
        "ToolStarted" => "tool_call",
        "ToolFinished" => tool_event_kind(payload),
        "HumanEdit" => "human_edit",
        "GitCommit" => "commit",
        "SessionEnded" => "session_end",
        // Advisory annotations are not trace evidence and never leave the device.
        "Annotation" => return None,
        _ => return None,
    };
    let harness = match source {
        "ClaudeCode" => "claude_code",
        "Codex" => "codex",
        "OpenCode" => "opencode",
        "Gemini" => "gemini",
        "Aider" => "aider",
        "QwenCode" => "qwen_code",
        "KimiCode" => "kimi_code",
        "ZCode" => "z_code",
        "Copilot" => "copilot",
        _ => "unknown",
    };
    let source_kind = match kind {
        "GitCommit" => "git",
        "HumanEdit" => "fs_watch",
        _ => "transcript",
    };
    let mut event = json!({
        "event_id": format!("evt-{}", &hash_text(id)[..32]),
        "ts_utc_ms": occurred_at,
        "ts_seq_idx": u32::try_from(sequence).unwrap_or(u32::MAX),
        "source_kind": source_kind,
        "harness": harness,
        "event_kind": event_kind,
        "session_id_hash": hash_text(session),
        "reason_codes": reason_codes(kind, payload),
    });
    if let Some(project) = payload.get("project").and_then(Value::as_str) {
        event["project_id_hash"] = Value::String(hash_text(project));
    }
    if kind == "PromptSubmitted" {
        event["prompt_features"] = prompt_features(payload);
    }
    if matches!(event_kind, "test_run" | "build_run")
        && let Some(exit_code) = payload
            .pointer("/tool/exit_code")
            .and_then(Value::as_i64)
            .and_then(|value| i32::try_from(value).ok())
    {
        event["test_exit_code"] = Value::from(exit_code);
    }
    Some(event)
}

fn tool_event_kind(payload: &Value) -> &'static str {
    match payload.pointer("/tool/class").and_then(Value::as_str) {
        Some("Test") => "test_run",
        Some("Build") | Some("Lint") => "build_run",
        _ => "tool_result",
    }
}

fn prompt_features(payload: &Value) -> Value {
    let length = payload
        .pointer("/prompt/char_len")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let length_bucket = match length {
        0 => "empty",
        1..=40 => "short",
        41..=200 => "medium",
        201..=800 => "long",
        _ => "very_long",
    };
    json!({
        "length_bucket": length_bucket,
        "anchor_count_bucket": "zero",
        "fullcaps": false,
    })
}

fn reason_codes(kind: &str, payload: &Value) -> Vec<&'static str> {
    match kind {
        "HumanEdit" => vec!["HUMAN_EDIT_OBSERVED"],
        "GitCommit" => vec!["COMMIT_OBSERVED"],
        "ToolFinished" if payload.pointer("/tool/exit_code").and_then(Value::as_i64) == Some(0) => {
            vec!["TEST_PASSED"]
        }
        "ToolFinished"
            if payload
                .pointer("/tool/exit_code")
                .and_then(Value::as_i64)
                .is_some() =>
        {
            vec!["TEST_FAILED"]
        }
        _ => Vec::new(),
    }
}

fn hash_text(value: &str) -> String {
    hash_bytes(value.as_bytes())
}

fn hash_bytes(value: &[u8]) -> String {
    format!("{:x}", Sha256::digest(value))
}

fn processor_hash() -> String {
    hash_text(&format!(
        "{}:{}:{PROCESSOR_VERSION}",
        env!("CARGO_PKG_NAME"),
        env!("CARGO_PKG_VERSION")
    ))
}

#[cfg(test)]
mod tests {
    use super::derive_event;
    use serde_json::json;

    #[test]
    fn derived_event_contains_no_raw_text_or_path() -> Result<(), Box<dyn std::error::Error>> {
        let event = derive_event(
            "raw-event-id",
            1,
            "ClaudeCode",
            "session-1",
            &json!({"kind":"PromptSubmitted", "project":"private-project", "prompt":{"char_len":44}, "rel_path":"src/secret.rs", "text":"private"}),
            0,
        )
        .ok_or("prompt event should map")?;
        let encoded = serde_json::to_string(&event)?;
        assert!(!encoded.contains("private"));
        assert!(!encoded.contains("secret.rs"));
        assert_eq!(event["prompt_features"]["length_bucket"], "medium");
        Ok(())
    }

    #[test]
    fn annotations_are_local_only() {
        assert!(
            derive_event(
                "id",
                1,
                "Codex",
                "session",
                &json!({"kind":"Annotation"}),
                0
            )
            .is_none()
        );
    }
}
