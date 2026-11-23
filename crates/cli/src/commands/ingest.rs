use crate::{TranscriptHarness, processor, state};
use hackai_parser::{
    parse_aider_markdown, parse_claude_jsonl, parse_codex_jsonl, parse_copilot_jsonl,
    parse_gemini_jsonl, parse_kimi_code_jsonl, parse_opencode_jsonl, parse_opencode_sqlite,
    parse_qwen_code_jsonl, parse_z_code_jsonl,
};
use hackai_schema::UnixMillis;
use hackai_scorer::TimeAccountingConfig;
use std::{fs, io, path::Path};

pub fn cmd_ingest(
    repo: &Path,
    harness: TranscriptHarness,
    source: &Path,
    session: String,
    session_started_ms: Option<i64>,
) -> io::Result<()> {
    let root = state::root(repo)?;
    let project = root
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("project")
        .to_owned();
    let events = match harness {
        TranscriptHarness::Claude => parse_claude_jsonl(
            &fs::read_to_string(source)?,
            project,
            None::<String>,
            session,
        ),
        TranscriptHarness::Codex => parse_codex_jsonl(
            &fs::read_to_string(source)?,
            project,
            None::<String>,
            session,
        ),
        TranscriptHarness::Gemini => parse_gemini_jsonl(
            &fs::read_to_string(source)?,
            project,
            None::<String>,
            session,
        ),
        TranscriptHarness::Opencode if source.extension().is_some_and(|ext| ext == "db") => {
            parse_opencode_sqlite(source, project, None::<String>, session)
        }
        TranscriptHarness::Opencode => parse_opencode_jsonl(
            &fs::read_to_string(source)?,
            project,
            None::<String>,
            session,
        ),
        TranscriptHarness::Aider => {
            let started_at = session_started_ms.ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "--session-started-ms is required for Aider history",
                )
            })?;
            Ok(parse_aider_markdown(
                &fs::read_to_string(source)?,
                UnixMillis(started_at),
                project,
                None::<String>,
                session,
            ))
        }
        TranscriptHarness::QwenCode => parse_qwen_code_jsonl(
            &fs::read_to_string(source)?,
            project,
            None::<String>,
            session,
        ),
        TranscriptHarness::KimiCode => parse_kimi_code_jsonl(
            &fs::read_to_string(source)?,
            project,
            None::<String>,
            session,
        ),
        TranscriptHarness::ZCode => parse_z_code_jsonl(
            &fs::read_to_string(source)?,
            project,
            None::<String>,
            session,
        ),
        TranscriptHarness::Copilot => parse_copilot_jsonl(
            &fs::read_to_string(source)?,
            project,
            None::<String>,
            session,
        ),
    }
    .map_err(io::Error::other)?;
    let values = events
        .iter()
        .map(serde_json::to_value)
        .collect::<Result<Vec<_>, _>>()
        .map_err(io::Error::other)?;
    let stored = state::append_normalized_events(&root, &values)?;
    let accounting = hackai_scorer::account_time(&events, TimeAccountingConfig::default());
    let processed = processor::process_and_write(&root)?;
    state::audit(&root, "ingest")?;
    println!(
        "Stored {stored} normalized event(s). Credited union: {} ms (human {} ms, attended agent {} ms); derived {} dialogue turn(s) across {} episode(s).",
        accounting.union_millis,
        accounting.human_active_millis,
        accounting.agent_attended_millis,
        processed.dialogue_turns.len(),
        processed.episodes.len(),
    );
    Ok(())
}

pub fn cmd_watch(
    repo: &Path,
    harness: TranscriptHarness,
    source: &Path,
    session: String,
    session_started_ms: Option<i64>,
    interval_seconds: u64,
) -> io::Result<()> {
    println!(
        "Watching {} every {interval_seconds}s. Press Ctrl-C to stop.",
        source.display()
    );
    loop {
        match cmd_ingest(repo, harness, source, session.clone(), session_started_ms) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                eprintln!("Waiting for {}", source.display());
            }
            Err(error) => eprintln!("Ingest failed: {error}"),
        }
        std::thread::sleep(std::time::Duration::from_secs(interval_seconds));
    }
}
