// API client functions
use crate::git::{git_head_sha, git_remote_url};
use crate::state::{read_local_annotations, read_local_checkpoints, read_local_episodes};
use reqwest::blocking::Client;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SyncPayload {
    /// Stable id for this queued batch; retries are idempotent on the server.
    #[serde(default)]
    pub batch_id: Option<String>,
    /// SHA-256 of the locally installed, agent-readable annotation policy.
    pub annotation_policy_sha256: String,
    /// SHA-256 of the running HackAI release, verified against the server's
    /// admin-managed trusted-release registry.
    pub cli_release_sha256: String,
    /// Hashes of the installed first-party harness adapters, keyed by harness.
    pub adapter_sha256: BTreeMap<String, String>,
    pub remote_url: Option<String>,
    pub head_sha: Option<String>,
    pub annotations: Vec<Value>,
    pub checkpoints: Vec<Value>,
    pub episodes: Vec<Value>,
    /// Daily aggregate prompt metrics only; raw prompt text never syncs.
    pub prompt_metrics: Vec<Value>,
    /// Stripped harness events; never includes prompt text.
    pub prompt_events: Vec<Value>,
}

pub fn build_sync_payload(repo_root: &Path) -> io::Result<SyncPayload> {
    let remote_url = git_remote_url(repo_root).unwrap_or_default();
    let head_sha = git_head_sha(repo_root).unwrap_or_default();
    let annotations = read_local_annotations(repo_root);
    let checkpoints = read_local_checkpoints(repo_root);
    let episodes = read_local_episodes(repo_root);
    let prompt_metrics = read_prompt_metrics(repo_root);
    let prompt_events = read_prompt_events(repo_root);

    Ok(SyncPayload {
        batch_id: Some(uuid::Uuid::new_v4().to_string()),
        annotation_policy_sha256: annotation_policy_sha256()?,
        cli_release_sha256: cli_release_sha256()?,
        adapter_sha256: adapter_sha256(repo_root),
        remote_url: Some(remote_url),
        head_sha: Some(head_sha),
        annotations,
        checkpoints,
        episodes,
        prompt_metrics,
        prompt_events,
    })
}

fn cli_release_sha256() -> io::Result<String> {
    if let Some(value) = std::env::var_os("HACKAI_RELEASE_SHA256") {
        return Ok(value.to_string_lossy().to_ascii_lowercase());
    }
    let executable = std::env::current_exe()?;
    Ok(format!("{:x}", Sha256::digest(fs::read(executable)?)))
}

fn adapter_sha256(repo_root: &Path) -> BTreeMap<String, String> {
    [
        ("codex", ".codex/hooks.json"),
        ("claude-code", ".claude/settings.local.json"),
        ("antigravity", ".agents/hooks.json"),
        ("opencode", ".opencode/plugins/hackai.ts"),
    ]
    .into_iter()
    .filter_map(|(harness, relative)| {
        fs::read(repo_root.join(relative))
            .ok()
            .map(|bytes| (harness.to_owned(), format!("{:x}", Sha256::digest(bytes))))
    })
    .collect()
}

fn read_prompt_events(repo_root: &Path) -> Vec<Value> {
    let path = crate::state::state_dir(repo_root).join("prompt-events.jsonl");
    fs::read_to_string(path)
        .ok()
        .into_iter()
        .flat_map(|content| {
            content
                .lines()
                .filter_map(|line| serde_json::from_str::<Value>(line).ok())
                .take(500)
                .collect::<Vec<_>>()
        })
        .collect()
}

fn read_prompt_metrics(repo_root: &Path) -> Vec<Value> {
    let path = repo_root.join("ml/prompt-lab.local.jsonl");
    let Ok(content) = fs::read_to_string(path) else {
        return Vec::new();
    };
    let mut days: BTreeMap<u64, (u64, u64, u64)> = BTreeMap::new();
    for line in content.lines() {
        let Ok(record) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        let Some(recorded_at) = record.get("recorded_at_unix").and_then(Value::as_u64) else {
            continue;
        };
        let level = record
            .pointer("/classification/ai_coding_prompt_proficiency/level")
            .and_then(Value::as_str);
        let proficiency = match level {
            Some("developing") => 500,
            Some("proficient") => 1000,
            Some("new") => 0,
            _ => continue,
        };
        let Some(slop) = record
            .pointer("/classification/slop_likelihood/likelihood_permille")
            .and_then(Value::as_u64)
            .filter(|value| *value <= 1000)
        else {
            continue;
        };
        let day = recorded_at / 86_400 * 86_400;
        let metric = days.entry(day).or_default();
        metric.0 += 1;
        metric.1 += proficiency;
        metric.2 += slop;
    }
    days.into_iter().map(|(day_start_unix, (prompt_count, proficiency_sum_permille, slop_sum_permille))| {
        json!({"day_start_unix": day_start_unix, "prompt_count": prompt_count, "proficiency_sum_permille": proficiency_sum_permille, "slop_sum_permille": slop_sum_permille})
    }).collect()
}

/// Hash the policy without including its contents in any payload.
/// The installer/harness may set `HACKAI_POLICY_PATH`; the repository path
/// keeps local development usable.
fn annotation_policy_sha256() -> io::Result<String> {
    let path: PathBuf = std::env::var_os("HACKAI_POLICY_PATH")
        .map(Into::into)
        .unwrap_or_else(|| "annotations/SKILL.md".into());
    let bytes = fs::read(path)?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

pub fn sync_to_server(server_url: &str, token: &str, payload: SyncPayload) -> io::Result<Value> {
    let client = Client::new();
    let response = client
        .post(format!("{server_url}/api/v1/upload"))
        .header("Content-Type", "application/json")
        .bearer_auth(token)
        .json(&payload)
        .timeout(Duration::from_secs(30))
        .send();

    match response {
        Ok(resp) if resp.status().is_success() => {
            let body: Value = resp.json().unwrap_or_default();
            Ok(body)
        }
        Ok(resp) => {
            let status = resp.status();
            let body: Value = resp.json().unwrap_or_default();
            let reason = body
                .get("error")
                .and_then(Value::as_str)
                .unwrap_or("sync_failed");
            Err(io::Error::other(format!(
                "sync failed: HTTP {status} ({reason})"
            )))
        }
        Err(error) => Err(io::Error::other(format!("sync request failed: {error}"))),
    }
}

pub async fn api_get_projects(server_url: &str, token: &str) -> io::Result<Vec<Value>> {
    let client = reqwest::Client::new();
    let resp = client
        .get(format!("{server_url}/api/v1/projects"))
        .bearer_auth(token)
        .send()
        .await
        .map_err(|e| io::Error::other(format!("request failed: {e}")))?;
    if !resp.status().is_success() {
        return Err(io::Error::other(format!("HTTP {}", resp.status())));
    }
    let body: Value = resp
        .json()
        .await
        .map_err(|e| io::Error::other(format!("invalid JSON: {e}")))?;
    Ok(body.as_array().cloned().unwrap_or_default())
}

pub async fn api_delete_project(server_url: &str, token: &str, project_id: &str) -> io::Result<()> {
    let client = reqwest::Client::new();
    let resp = client
        .delete(format!("{server_url}/api/v1/projects/{project_id}"))
        .bearer_auth(token)
        .send()
        .await
        .map_err(|e| io::Error::other(format!("request failed: {e}")))?;
    if !resp.status().is_success() {
        return Err(io::Error::other(format!("HTTP {}", resp.status())));
    }
    Ok(())
}

pub async fn api_merge_projects(
    server_url: &str,
    token: &str,
    source_id: &str,
    target_id: &str,
) -> io::Result<Value> {
    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{server_url}/api/v1/projects/merge"))
        .bearer_auth(token)
        .json(&json!({ "source_project_id": source_id, "target_project_id": target_id }))
        .send()
        .await
        .map_err(|e| io::Error::other(format!("request failed: {e}")))?;
    if !resp.status().is_success() {
        return Err(io::Error::other(format!("HTTP {}", resp.status())));
    }
    resp.json()
        .await
        .map_err(|e| io::Error::other(format!("invalid JSON: {e}")))
}

pub async fn api_cli_pair(server_url: &str) -> io::Result<Value> {
    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{server_url}/api/v1/cli/pairings"))
        .send()
        .await
        .map_err(|e| io::Error::other(format!("request failed: {e}")))?;
    if !resp.status().is_success() {
        return Err(io::Error::other(format!("HTTP {}", resp.status())));
    }
    resp.json()
        .await
        .map_err(|e| io::Error::other(format!("invalid JSON: {e}")))
}

pub async fn api_cli_pair_status(server_url: &str, device_code: &str) -> io::Result<Value> {
    let client = reqwest::Client::new();
    let resp = client
        .get(format!("{server_url}/api/v1/cli/pairings/{device_code}"))
        .send()
        .await
        .map_err(|e| io::Error::other(format!("request failed: {e}")))?;
    if !resp.status().is_success() {
        return Err(io::Error::other(format!("HTTP {}", resp.status())));
    }
    resp.json()
        .await
        .map_err(|e| io::Error::other(format!("invalid JSON: {e}")))
}

pub async fn api_get_me(server_url: &str, token: &str) -> io::Result<Value> {
    let client = reqwest::Client::new();
    let resp = client
        .get(format!("{server_url}/api/v1/me"))
        .bearer_auth(token)
        .send()
        .await
        .map_err(|e| io::Error::other(format!("request failed: {e}")))?;
    if !resp.status().is_success() {
        return Err(io::Error::other(format!("HTTP {}", resp.status())));
    }
    resp.json()
        .await
        .map_err(|e| io::Error::other(format!("invalid JSON: {e}")))
}
