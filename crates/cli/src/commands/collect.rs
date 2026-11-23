use crate::{state, TranscriptHarness};
use hackai_gitx;
use sha2::{Digest, Sha256};
use std::{fs, io, path::Path, time::{SystemTime, UNIX_EPOCH}};

fn now_unix_millis() -> io::Result<i64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(io::Error::other)
        .and_then(|duration| i64::try_from(duration.as_millis()).map_err(io::Error::other))
}

fn git_cursor_path(root: &Path) -> std::path::PathBuf {
    state::state_dir(root).join("git-observer.cursor-ms")
}

fn read_git_cursor(root: &Path) -> io::Result<Option<i64>> {
    match fs::read_to_string(git_cursor_path(root)) {
        Ok(value) => Ok(value.trim().parse::<i64>().ok()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

fn write_git_cursor(root: &Path, value: i64) -> io::Result<()> {
    fs::create_dir_all(state::state_dir(root))?;
    fs::write(git_cursor_path(root), value.to_string())
}

fn discovered_session_name(source: &Path) -> String {
    source
        .file_stem()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or("discovered")
        .to_owned()
}

pub fn cmd_collect(repo: &Path) -> io::Result<()> {
    let root = state::root(repo)?;
    if let Err(error) = collect_git_evidence(&root, None) {
        eprintln!("Could not collect local git evidence: {error}");
    }
    let mut imported_sources = 0_usize;
    let mut failures = 0_usize;

    for source in state::discover_claude_transcripts(&root)? {
        let session = discovered_session_name(&source);
        match crate::commands::ingest::cmd_ingest(&root, TranscriptHarness::Claude, &source, session, None) {
            Ok(()) => imported_sources += 1,
            Err(error) => {
                failures += 1;
                eprintln!(
                    "Could not ingest Claude transcript {}: {error}",
                    source.display()
                );
            }
        }
    }

    let checkout = root.to_string_lossy();
    for source in state::discover_codex_rollouts()? {
        let transcript = fs::read_to_string(&source)?;
        if !transcript.contains(checkout.as_ref()) {
            continue;
        }
        let session = discovered_session_name(&source);
        match crate::commands::ingest::cmd_ingest(&root, TranscriptHarness::Codex, &source, session, None) {
            Ok(()) => imported_sources += 1,
            Err(error) => {
                failures += 1;
                eprintln!(
                    "Could not ingest Codex rollout {}: {error}",
                    source.display()
                );
            }
        }
    }

    if state::discover_opencode_database().is_some() {
        println!(
            "OpenCode database found but not auto-imported: session-to-repository attribution is not yet verified. Use `hackai ingest opencode <path>` for an explicitly selected source."
        );
    }
    println!("Collector imported {imported_sources} source(s); {failures} source(s) failed.");
    Ok(())
}

pub fn cmd_observe(
    repo: &Path,
    since_ms: Option<i64>,
    interval_seconds: Option<u64>,
) -> io::Result<()> {
    if let Some(interval) = interval_seconds {
        println!(
            "Observing derived git/worktree activity every {interval}s. Press Ctrl-C to stop."
        );
        loop {
            if let Err(error) = collect_git_evidence(repo, since_ms) {
                eprintln!("Git observation failed: {error}");
            }
            std::thread::sleep(std::time::Duration::from_secs(interval));
        }
    }
    collect_git_evidence(repo, since_ms)
}

fn collect_git_evidence(repo: &Path, since_ms: Option<i64>) -> io::Result<()> {
    let root = state::root(repo)?;
    let now_ms = now_unix_millis()?;
    let cursor = read_git_cursor(&root)?;
    // An overlap makes a crashed cursor update harmless; event ids are stable
    // and SQLite ignores the duplicate commit records.
    let default_since = now_ms.saturating_sub(7 * 24 * 60 * 60 * 1000);
    let since = since_ms
        .or(cursor)
        .unwrap_or(default_since)
        .saturating_sub(1_000);
    let commits = hackai_gitx::commits_since(&root, since).map_err(io::Error::other)?;
    let worktree = hackai_gitx::working_tree_summary(&root).map_err(io::Error::other)?;
    let project = root
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("project")
        .to_owned();

    let mut events = Vec::with_capacity(commits.len() + usize::from(!worktree.is_clean()));
    for commit in commits {
        let tree_digest = format!("{:x}", Sha256::digest(commit.tree.as_bytes()));
        let tree = hackai_schema::GitTreeHash::parse(tree_digest).map_err(io::Error::other)?;
        let event = hackai_schema::NormalizedEvent {
            schema_version: 1,
            id: hackai_schema::EventId(format!("git-commit-{}", commit.sha.0)),
            occurred_at: hackai_schema::UnixMillis(commit.committed_at_millis),
            kind: hackai_schema::EventKind::GitCommit,
            role: hackai_schema::ActorRole::Human,
            source: hackai_schema::EventSource::ManualCli,
            project: project.clone(),
            session: "git-local".to_owned(),
            rel_path: None::<String>,
            prompt: None,
            tool: None,
            change: Some(hackai_schema::ChangeFeatures {
                human_added: commit.lines_added,
                human_deleted: commit.lines_deleted,
                agent_added: 0,
                agent_deleted: 0,
                hunk_hashes: Vec::new(),
            }),
            commit: Some(hackai_schema::CommitEvidence {
                sha: commit.sha,
                tree,
            }),
        };
        // The file count is intentionally encoded only in an opaque, derived
        // event id suffix; consumers use line totals and never receive paths.
        let mut value = serde_json::to_value(event).map_err(io::Error::other)?;
        if let Some(change) = value.get_mut("change").and_then(serde_json::Value::as_object_mut) {
            change.insert(
                "files_changed".to_owned(),
                serde_json::json!(commit.files_changed),
            );
        }
        events.push(value);
    }

    if !worktree.is_clean() {
        let fingerprint_input = format!(
            "{}:{}:{}:{}:{}",
            worktree
                .head
                .as_ref()
                .map_or("none", |head| head.0.as_str()),
            worktree.files_changed,
            worktree.lines_added,
            worktree.lines_deleted,
            worktree.untracked_files
        );
        let fingerprint = format!("{:x}", Sha256::digest(fingerprint_input.as_bytes()));
        let event = hackai_schema::NormalizedEvent {
            schema_version: 1,
            id: hackai_schema::EventId(format!("worktree-{fingerprint}")),
            occurred_at: hackai_schema::UnixMillis(now_ms),
            kind: hackai_schema::EventKind::HumanEdit,
            // This is intentionally only a candidate: agent tool writes are
            // excluded by the processor before E receives human-edit credit.
            role: hackai_schema::ActorRole::Human,
            source: hackai_schema::EventSource::ManualCli,
            project,
            session: "worktree-poll".to_owned(),
            rel_path: None::<String>,
            prompt: None,
            tool: None,
            change: Some(hackai_schema::ChangeFeatures {
                human_added: worktree.lines_added,
                human_deleted: worktree.lines_deleted,
                agent_added: 0,
                agent_deleted: 0,
                hunk_hashes: Vec::new(),
            }),
            commit: None,
        };
        let mut value = serde_json::to_value(event).map_err(io::Error::other)?;
        if let Some(change) = value.get_mut("change").and_then(serde_json::Value::as_object_mut) {
            change.insert(
                "files_changed".to_owned(),
                serde_json::json!(worktree.files_changed),
            );
            change.insert(
                "untracked_files".to_owned(),
                serde_json::json!(worktree.untracked_files),
            );
            change.insert("candidate_only".to_owned(), serde_json::json!(true));
        }
        events.push(value);
    }

    let stored = state::append_normalized_events(&root, &events)?;
    write_git_cursor(&root, now_ms)?;
    state::audit(&root, "observe")?;
    println!(
        "Recorded {stored} derived git event(s): {} commit(s), worktree {}. No paths, code, messages, or author data were stored.",
        events
            .iter()
            .filter(|event| event.get("kind") == Some(&serde_json::json!("GitCommit")))
            .count(),
        if worktree.is_clean() {
            "clean"
        } else {
            "changed (candidate only)"
        }
    );
    Ok(())
}