// Local state management: paths, config, queue, audit
use keyring::Entry;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

pub fn root(input: &Path) -> io::Result<PathBuf> {
    let start = input.canonicalize()?;
    let start = if start.is_file() {
        start.parent().map(Path::to_path_buf).ok_or_else(|| {
            io::Error::new(io::ErrorKind::NotFound, "input has no parent directory")
        })?
    } else {
        start
    };
    Ok(start
        .ancestors()
        .find(|p| p.join(".git").exists())
        .map(Path::to_path_buf)
        // Non-Git directories are first-class projects too. Git contributes
        // optional evidence and submission-gate data when it is available.
        .unwrap_or(start))
}

pub fn state_dir(root: &Path) -> PathBuf {
    let dot_git = root.join(".git");
    if let Ok(content) = fs::read_to_string(&dot_git)
        && let Some(gitdir) = content.trim().strip_prefix("gitdir:")
    {
        let gitdir_path = Path::new(gitdir.trim());
        let resolved = if gitdir_path.is_absolute() {
            gitdir_path.to_path_buf()
        } else {
            root.join(gitdir_path)
        };
        return resolved.join("hackai");
    }
    if dot_git.is_dir() {
        dot_git.join("hackai")
    } else {
        root.join(".hackai")
    }
}

#[derive(Debug, Deserialize, Serialize)]
pub struct TrackerConfig {
    pub checkpoint_turn_interval: u16,
}
impl Default for TrackerConfig {
    fn default() -> Self {
        Self {
            checkpoint_turn_interval: 10,
        }
    }
}

pub fn tracker_config(root: &Path) -> io::Result<TrackerConfig> {
    let path = state_dir(root).join("config.json");
    let config = match fs::read(&path) {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(io::Error::other)?,
        Err(error) if error.kind() == io::ErrorKind::NotFound => TrackerConfig::default(),
        Err(error) => return Err(error),
    };
    if !(5..=100).contains(&config.checkpoint_turn_interval) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "checkpoint_turn_interval must be between 5 and 100",
        ));
    }
    Ok(config)
}

pub fn write_config(root: &Path, config: &TrackerConfig) -> io::Result<()> {
    let directory = state_dir(root);
    fs::create_dir_all(&directory)?;
    fs::write(
        directory.join("config.json"),
        serde_json::to_vec_pretty(config).map_err(io::Error::other)?,
    )
}

#[derive(Debug, Deserialize, Serialize, Default)]
pub struct CheckpointState {
    pub turns_since_nudge: u16,
    pub last_nudge_unix: u64,
}

pub fn checkpoint_state_path(root: &Path) -> PathBuf {
    state_dir(root).join("checkpoint-state.json")
}

pub fn read_checkpoint_state(root: &Path) -> io::Result<CheckpointState> {
    let path = checkpoint_state_path(root);
    let state = match fs::read(&path) {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(io::Error::other)?,
        Err(error) if error.kind() == io::ErrorKind::NotFound => CheckpointState::default(),
        Err(error) => return Err(error),
    };
    Ok(state)
}

pub fn write_checkpoint_state(root: &Path, state: &CheckpointState) -> io::Result<()> {
    let directory = state_dir(root);
    fs::create_dir_all(&directory)?;
    fs::write(
        directory.join("checkpoint-state.json"),
        serde_json::to_vec(state).map_err(io::Error::other)?,
    )
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct OfflineQueueItem {
    pub id: String,
    pub repo_root: String,
    pub project_name: String,
    pub payload: crate::api::SyncPayload,
    pub created_at: u64,
    pub attempts: u32,
}

#[derive(Deserialize, Serialize)]
struct SignedQueue {
    items: Vec<OfflineQueueItem>,
    mac: String,
}

fn queue_path(root: &Path) -> PathBuf {
    state_dir(root).join("sync_queue.json")
}

pub fn read_queue(root: &Path) -> io::Result<Vec<OfflineQueueItem>> {
    let path = queue_path(root);
    let content = match fs::read_to_string(&path) {
        Ok(content) => content,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };
    let queue: SignedQueue = serde_json::from_str(&content).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("sync queue is corrupt or uses the legacy unsigned format: {error}"),
        )
    })?;
    let payload = serde_json::to_vec(&queue.items).map_err(io::Error::other)?;
    let expected = queue_mac(&queue_key(root)?, &payload);
    if !constant_time_eq(queue.mac.as_bytes(), expected.as_bytes()) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "sync queue integrity check failed",
        ));
    }
    Ok(queue.items)
}

pub fn write_queue(root: &Path, queue: &[OfflineQueueItem]) -> io::Result<()> {
    let directory = state_dir(root);
    fs::create_dir_all(&directory)?;
    let payload = serde_json::to_vec(queue).map_err(io::Error::other)?;
    let signed = SignedQueue {
        items: queue.to_vec(),
        mac: queue_mac(&queue_key(root)?, &payload),
    };
    fs::write(
        directory.join("sync_queue.json"),
        serde_json::to_vec_pretty(&signed).map_err(io::Error::other)?,
    )
}

fn queue_key(root: &Path) -> io::Result<Vec<u8>> {
    let account = format!(
        "repo-{:x}",
        Sha256::digest(root.as_os_str().as_encoded_bytes())
    );
    let entry = Entry::new("hackai-queue-integrity", &account).map_err(io::Error::other)?;
    match entry.get_password() {
        Ok(value) => Ok(value.into_bytes()),
        Err(keyring::Error::NoEntry) => {
            let key = Uuid::new_v4().to_string();
            entry.set_password(&key).map_err(io::Error::other)?;
            Ok(key.into_bytes())
        }
        Err(error) => Err(io::Error::other(error)),
    }
}

fn queue_mac(key: &[u8], payload: &[u8]) -> String {
    const BLOCK: usize = 64;
    let mut padded_key = [0_u8; BLOCK];
    let source = if key.len() > BLOCK {
        Sha256::digest(key).to_vec()
    } else {
        key.to_vec()
    };
    padded_key[..source.len()].copy_from_slice(&source);
    let inner_key: Vec<u8> = padded_key.iter().map(|byte| byte ^ 0x36).collect();
    let outer_key: Vec<u8> = padded_key.iter().map(|byte| byte ^ 0x5c).collect();
    let mut inner = Sha256::new();
    inner.update(&inner_key);
    inner.update(payload);
    let mut outer = Sha256::new();
    outer.update(&outer_key);
    outer.update(inner.finalize());
    format!("{:x}", outer.finalize())
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    left.len() == right.len()
        && left
            .iter()
            .zip(right)
            .fold(0_u8, |diff, (a, b)| diff | (a ^ b))
            == 0
}

pub fn enqueue_sync(
    root: &Path,
    repo_root: &Path,
    payload: crate::api::SyncPayload,
) -> io::Result<()> {
    let mut queue = read_queue(root)?;
    let project_name = repo_root
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown")
        .to_string();
    queue.push(OfflineQueueItem {
        id: uuid::Uuid::new_v4().to_string(),
        repo_root: repo_root.display().to_string(),
        project_name,
        payload,
        created_at: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(io::Error::other)?
            .as_secs(),
        attempts: 0,
    });
    write_queue(root, &queue)
}

pub fn audit(root: &Path, action: &str) -> io::Result<()> {
    let directory = state_dir(root);
    fs::create_dir_all(&directory)?;
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(io::Error::other)?
        .as_secs();
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(directory.join("audit.jsonl"))?;
    writeln!(file, "{{\"time\":{seconds},\"action\":\"{action}\"}}")
}

pub fn safe_terminal(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            if let Some(&next) = chars.peek() {
                if next == '[' {
                    chars.next();
                    for inner in chars.by_ref() {
                        if ('@'..='~').contains(&inner) {
                            break;
                        }
                    }
                } else if next == ']' {
                    chars.next();
                    let mut prev = ' ';
                    for inner in chars.by_ref() {
                        if inner == '\u{07}' || (prev == '\u{1b}' && inner == '\\') {
                            break;
                        }
                        prev = inner;
                    }
                } else {
                    chars.next();
                }
            }
        } else if !c.is_control() {
            output.push(c);
        }
    }
    output
}

#[derive(Deserialize)]
struct LocalAnnotationFile {
    session_key: String,
    label: String,
    evidence: String,
    #[serde(default)]
    applied: bool,
    #[serde(default)]
    discount_permille: i32,
}

pub fn read_local_annotations(repo_root: &Path) -> Vec<serde_json::Value> {
    let s = state_dir(repo_root);
    let path = s.join("annotations.jsonl");
    let Ok(content) = fs::read_to_string(&path) else {
        return Vec::new();
    };
    content
        .lines()
        .filter_map(|line| serde_json::from_str::<LocalAnnotationFile>(line).ok())
        .map(|a| {
            serde_json::json!({
                "session_key": a.session_key,
                "label": a.label,
                "evidence": a.evidence,
                "applied": a.applied,
                "discount_permille": a.discount_permille,
            })
        })
        .collect()
}

pub fn read_local_checkpoints(repo_root: &Path) -> Vec<serde_json::Value> {
    let s = state_dir(repo_root);
    let path = s.join("checkpoint-state.json");
    let Ok(content) = fs::read_to_string(&path) else {
        return Vec::new();
    };
    let Ok(state_val) = serde_json::from_str::<serde_json::Value>(&content) else {
        return Vec::new();
    };
    let cursor = state_val
        .get("cursor")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    vec![serde_json::json!({ "cursor": cursor })]
}

pub fn read_local_episodes(repo_root: &Path) -> Vec<serde_json::Value> {
    let path = state_dir(repo_root).join("episodes.json");
    let Ok(content) = fs::read_to_string(path) else {
        return Vec::new();
    };
    serde_json::from_str(&content).unwrap_or_default()
}

/// Return the repository-local append-only event database.
pub fn event_store_path(root: &Path) -> PathBuf {
    state_dir(root).join("events.sqlite")
}

/// Find Claude Code transcripts for this checkout. Compaction artifacts are
/// skipped because they replay already-seen turns.
pub fn discover_claude_transcripts(repo_root: &Path) -> io::Result<Vec<PathBuf>> {
    let Some(home) = dirs::home_dir() else {
        return Ok(Vec::new());
    };
    let directory = home
        .join(".claude/projects")
        .join(claude_project_key(repo_root));
    discover_jsonl_files(&directory, true)
}

fn claude_project_key(repo_root: &Path) -> String {
    repo_root
        .to_string_lossy()
        .replace(['/', '\\'], "-")
        .trim_start_matches('-')
        .to_owned()
}

/// Locate Codex rollout files below `$CODEX_HOME/sessions`, falling back to
/// `~/.codex/sessions`.
pub fn discover_codex_rollouts() -> io::Result<Vec<PathBuf>> {
    let base = std::env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|home| home.join(".codex")));
    match base {
        Some(base) => discover_files_named(&base.join("sessions"), "rollout-", ".jsonl"),
        None => Ok(Vec::new()),
    }
}

/// Discover OpenCode's SQLite database. Prefer its own path resolver, then
/// use documented environment/XDG fallbacks when the executable is absent.
pub fn discover_opencode_database() -> Option<PathBuf> {
    if let Ok(output) = Command::new("opencode")
        .args(["debug", "paths", "db"])
        .output()
        && output.status.success()
        && let Ok(path) = String::from_utf8(output.stdout)
    {
        let candidate = PathBuf::from(path.trim());
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    let candidate = std::env::var_os("OPENCODE_DB")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("XDG_DATA_HOME")
                .map(PathBuf::from)
                .map(|base| base.join("opencode/opencode.db"))
        })
        .or_else(|| dirs::home_dir().map(|home| home.join(".local/share/opencode/opencode.db")));
    candidate.filter(|path| path.is_file())
}

fn discover_jsonl_files(directory: &Path, skip_compactions: bool) -> io::Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(files),
        Err(error) => return Err(error),
    };
    for entry in entries {
        let path = entry?.path();
        if path.is_file()
            && path
                .extension()
                .is_some_and(|extension| extension == "jsonl")
            && (!skip_compactions
                || !path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with("agent-acompact-")))
        {
            files.push(path);
        }
    }
    files.sort();
    Ok(files)
}

fn discover_files_named(
    directory: &Path,
    prefix: &str,
    extension: &str,
) -> io::Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(files),
        Err(error) => return Err(error),
    };
    for entry in entries {
        let path = entry?.path();
        if path.is_dir() {
            files.extend(discover_files_named(&path, prefix, extension)?);
        } else if path.is_file()
            && path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with(prefix) && name.ends_with(extension))
        {
            files.push(path);
        }
    }
    files.sort();
    Ok(files)
}

/// Append derived normalized events to the repository-local SQLite store.
/// Raw transcript records are deliberately rejected: callers may persist
/// feature objects, never prompt text, code, or tool output.
pub fn append_normalized_events(root: &Path, events: &[serde_json::Value]) -> io::Result<usize> {
    let directory = state_dir(root);
    fs::create_dir_all(&directory)?;
    let database = rusqlite::Connection::open(event_store_path(root))
        .map_err(|error| io::Error::other(format!("open event store: {error}")))?;
    database
        .execute_batch(
            "PRAGMA journal_mode = WAL;
            PRAGMA busy_timeout = 5000;
            CREATE TABLE IF NOT EXISTS event_store_meta (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );
            INSERT OR IGNORE INTO event_store_meta (key, value)
                VALUES ('schema_version', '1');
            CREATE TABLE IF NOT EXISTS normalized_events (
                event_id TEXT PRIMARY KEY,
                occurred_at_ms INTEGER NOT NULL,
                source TEXT NOT NULL,
                session_id TEXT NOT NULL,
                payload_json TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS normalized_events_time
                ON normalized_events (occurred_at_ms);",
        )
        .map_err(|error| io::Error::other(format!("initialize event store: {error}")))?;
    let transaction = database
        .unchecked_transaction()
        .map_err(|error| io::Error::other(format!("begin event store transaction: {error}")))?;
    let mut inserted = 0_usize;
    for event in events {
        if contains_raw_transcript_field(event) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "normalized event contains a raw transcript field",
            ));
        }
        let id = event
            .get("id")
            .and_then(|value| value.as_str())
            .ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidInput, "normalized event missing id")
            })?;
        let occurred_at = event
            .get("occurred_at")
            .and_then(|value| value.as_i64())
            .ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidInput, "normalized event missing time")
            })?;
        let source = event
            .get("source")
            .and_then(|value| value.as_str())
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "normalized event missing source",
                )
            })?;
        let session = event
            .get("session")
            .and_then(|value| value.as_str())
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "normalized event missing session",
                )
            })?;
        let payload = serde_json::to_string(event).map_err(io::Error::other)?;
        inserted += transaction
            .execute(
                "INSERT INTO normalized_events (event_id, occurred_at_ms, source, session_id, payload_json)
                 VALUES (?1, ?2, ?3, ?4, ?5) ON CONFLICT(event_id) DO NOTHING",
                rusqlite::params![id, occurred_at, source, session, payload],
            )
            .map_err(|error| io::Error::other(format!("write event store: {error}")))?;
    }
    transaction
        .commit()
        .map_err(|error| io::Error::other(format!("commit event store: {error}")))?;
    Ok(inserted)
}

fn contains_raw_transcript_field(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::Object(object) => object.iter().any(|(key, value)| {
            matches!(
                key.as_str(),
                "text" | "content" | "output" | "prompt_text" | "raw"
            ) || contains_raw_transcript_field(value)
        }),
        serde_json::Value::Array(values) => values.iter().any(contains_raw_transcript_field),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::{discover_files_named, discover_jsonl_files};
    use std::fs;

    fn temporary_directory(label: &str) -> std::path::PathBuf {
        let directory =
            std::env::temp_dir().join(format!("hackai-state-{label}-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&directory).unwrap_or_else(|_| unreachable!());
        directory
    }

    #[test]
    fn claude_discovery_skips_compaction_artifacts() {
        let directory = temporary_directory("claude-discovery");
        fs::write(directory.join("session.jsonl"), "{}").unwrap_or_else(|_| unreachable!());
        fs::write(directory.join("agent-acompact-old.jsonl"), "{}")
            .unwrap_or_else(|_| unreachable!());
        fs::write(directory.join("notes.txt"), "ignored").unwrap_or_else(|_| unreachable!());

        let files = discover_jsonl_files(&directory, true).unwrap_or_else(|_| unreachable!());
        let _ = fs::remove_dir_all(&directory);
        assert_eq!(files.len(), 1);
        assert!(files[0].ends_with("session.jsonl"));
    }

    #[test]
    fn codex_discovery_recurses_and_filters_file_names() {
        let directory = temporary_directory("codex-discovery");
        let nested = directory.join("2026/11/23");
        fs::create_dir_all(&nested).unwrap_or_else(|_| unreachable!());
        fs::write(nested.join("rollout-one.jsonl"), "{}").unwrap_or_else(|_| unreachable!());
        fs::write(nested.join("session.jsonl"), "{}").unwrap_or_else(|_| unreachable!());

        let files = discover_files_named(&directory, "rollout-", ".jsonl")
            .unwrap_or_else(|_| unreachable!());
        let _ = fs::remove_dir_all(&directory);
        assert_eq!(files.len(), 1);
        assert!(files[0].ends_with("rollout-one.jsonl"));
    }
}

#[derive(Serialize)]
pub struct Annotation<'a> {
    pub window: &'a str,
    pub label: &'a str,
    pub user_role: &'a str,
    pub evidence: &'a [String],
    pub chapter_title: Option<&'a str>,
}

pub fn annotation_value<'a>(
    window: &'a str,
    label: &'a str,
    user_role: &'a str,
    evidence: &'a [String],
    chapter_title: Option<&'a str>,
) -> io::Result<Annotation<'a>> {
    if window.is_empty()
        || window.len() > 128
        || !matches!(
            label,
            "debugging" | "guided" | "delegated" | "reading" | "exploring"
        )
        || !matches!(user_role, "drove" | "steered" | "approved" | "absent")
        || evidence.is_empty()
        || evidence.len() > 16
        || evidence.iter().any(|value| {
            value.is_empty() || value.len() > 128 || value.chars().any(char::is_control)
        })
        || chapter_title
            .is_some_and(|title| title.len() > 120 || title.chars().any(char::is_control))
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid advisory annotation",
        ));
    }
    Ok(Annotation {
        window,
        label,
        user_role,
        evidence,
        chapter_title,
    })
}

pub fn write_annotation(root: &Path, annotation: &Annotation<'_>) -> io::Result<()> {
    let directory = state_dir(root);
    fs::create_dir_all(&directory)?;
    let encoded = serde_json::to_string(annotation).map_err(io::Error::other)?;
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(directory.join("annotations.jsonl"))?;
    writeln!(file, "{encoded}")?;
    audit(root, "annotate")
}
