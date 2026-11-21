// Local state management: paths, config, queue, audit
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

pub fn root(input: &Path) -> io::Result<PathBuf> {
    let start = input.canonicalize()?;
    start
        .ancestors()
        .find(|p| p.join(".git").exists())
        .map(Path::to_path_buf)
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "not inside a git repository"))
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
    dot_git.join("hackai")
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

#[derive(Debug, Deserialize, Serialize)]
pub struct OfflineQueueItem {
    pub id: String,
    pub repo_root: String,
    pub project_name: String,
    pub payload: crate::api::SyncPayload,
    pub created_at: u64,
    pub attempts: u32,
}

fn queue_path(root: &Path) -> PathBuf {
    state_dir(root).join("sync_queue.json")
}

pub fn read_queue(root: &Path) -> Vec<OfflineQueueItem> {
    let path = queue_path(root);
    let Ok(content) = fs::read_to_string(&path) else {
        return Vec::new();
    };
    serde_json::from_str(&content).unwrap_or_default()
}

pub fn write_queue(root: &Path, queue: &[OfflineQueueItem]) -> io::Result<()> {
    let directory = state_dir(root);
    fs::create_dir_all(&directory)?;
    fs::write(
        directory.join("sync_queue.json"),
        serde_json::to_vec_pretty(queue).map_err(io::Error::other)?,
    )
}

pub fn enqueue_sync(
    root: &Path,
    repo_root: &Path,
    payload: crate::api::SyncPayload,
) -> io::Result<()> {
    let mut queue = read_queue(root);
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
            .unwrap()
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
