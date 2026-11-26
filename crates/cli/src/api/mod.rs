// API client functions
use crate::git::{git_head_sha, git_remote_url};
use crate::state::{
    read_local_annotations, read_local_checkpoints, read_local_episodes, read_user_feedback,
};
use reqwest::blocking::Client;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::time::Duration;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SyncCommitClaim {
    pub sha: String,
    pub tree_hash: String,
    pub committed_at_millis: i64,
    pub files_changed: u32,
    pub lines_added: u32,
    pub lines_deleted: u32,
}

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
    /// Local folder name only; never an absolute path.
    pub project_name: Option<String>,
    pub remote_url: Option<String>,
    pub head_sha: Option<String>,
    /// Recent commit claims for server-side GitHub cross-checking. No commit
    /// messages, author identities, paths, or source content are included.
    #[serde(default)]
    pub commits: Vec<SyncCommitClaim>,
    pub annotations: Vec<Value>,
    pub checkpoints: Vec<Value>,
    pub episodes: Vec<Value>,
    /// Daily aggregate prompt metrics only; raw prompt text never syncs.
    pub prompt_metrics: Vec<Value>,
    /// Stripped harness events; never includes prompt text.
    pub prompt_events: Vec<Value>,
    /// Stripped tool/lifecycle hook records. Advisory telemetry, not time credit.
    #[serde(default)]
    pub harness_events: Vec<Value>,
    /// Explicit opt-in agent coaching notes, capped at 50 words each.
    #[serde(default)]
    pub user_feedback: Vec<Value>,
    /// SHA-256 digests only. Raw hardware identifiers never leave the device.
    #[serde(default)]
    pub hardware_hashes: BTreeMap<String, String>,
    #[serde(default = "environment_class")]
    pub environment_class: String,
}

#[derive(Debug, Default)]
pub struct LocalConfig {
    pub server_url: Option<String>,
    pub api_key: Option<String>,
}

/// Read the deliberately small, line-oriented `.hackai.cfg` format. Values
/// are never logged; users should keep this file out of version control.
pub fn read_local_config(root: &Path) -> io::Result<LocalConfig> {
    let path = root.join(".hackai.cfg");
    let Ok(contents) = fs::read_to_string(path) else {
        return Ok(LocalConfig::default());
    };
    let mut config = LocalConfig::default();
    for line in contents.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let value = value.trim().trim_matches(['"', '\'']);
        match key.trim() {
            "api_key" | "token" => config.api_key = Some(value.to_owned()),
            "server_url" | "server" => config.server_url = Some(value.to_owned()),
            _ => {}
        }
    }
    Ok(config)
}

pub fn write_local_api_key(root: &Path, api_key: &str) -> io::Result<()> {
    let existing = read_local_config(root)?;
    let mut lines = vec![format!("api_key={api_key}")];
    if let Some(server_url) = existing.server_url {
        lines.push(format!("server_url={server_url}"));
    }
    fs::write(root.join(".hackai.cfg"), format!("{}\n", lines.join("\n")))
}

pub fn clear_local_api_key(root: &Path) -> io::Result<()> {
    let existing = read_local_config(root)?;
    if let Some(server_url) = existing.server_url {
        fs::write(
            root.join(".hackai.cfg"),
            format!("server_url={server_url}\n"),
        )
    } else {
        match fs::remove_file(root.join(".hackai.cfg")) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error),
        }
    }
}

pub fn api_request(
    server_url: &str,
    token: &str,
    method: &str,
    path: &str,
    body: Option<Value>,
) -> io::Result<Value> {
    let normalized = if path.starts_with("/api/v1/") || path.starts_with("/api/v2/") {
        path.to_owned()
    } else if path.starts_with("/v1/") {
        format!("/api{path}")
    } else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "API path must begin with /api/v1/, /api/v2/, or /v1/",
        ));
    };
    let client = Client::new();
    let method = reqwest::Method::from_bytes(method.as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid HTTP method"))?;
    let mut request = client
        .request(method, format!("{server_url}{normalized}"))
        .bearer_auth(token)
        .timeout(Duration::from_secs(30));
    if let Some(body) = body {
        request = request.json(&body);
    }
    let response = request
        .send()
        .map_err(|error| io::Error::other(format!("API request failed: {error}")))?;
    let status = response.status();
    const MAX_RESPONSE_BYTES: u64 = 1_048_576;
    if response
        .content_length()
        .is_some_and(|length| length > MAX_RESPONSE_BYTES)
    {
        return Err(io::Error::other("API response exceeds size limit"));
    }
    let mut text = String::new();
    response
        .take(MAX_RESPONSE_BYTES + 1)
        .read_to_string(&mut text)
        .map_err(|error| io::Error::other(format!("invalid API response: {error}")))?;
    if text.len() as u64 > MAX_RESPONSE_BYTES {
        return Err(io::Error::other("API response exceeds size limit"));
    }
    let value: Value = serde_json::from_str(&text).unwrap_or(Value::String(text));
    if !status.is_success() {
        return Err(io::Error::other(format!(
            "API request failed: HTTP {status}"
        )));
    }
    Ok(value)
}

pub fn build_sync_payload(repo_root: &Path) -> io::Result<SyncPayload> {
    let remote_url = git_remote_url(repo_root).unwrap_or_default();
    let head_sha = git_head_sha(repo_root).unwrap_or_default();
    let annotations = read_local_annotations(repo_root);
    let checkpoints = read_local_checkpoints(repo_root);
    let episodes = read_local_episodes(repo_root);
    let prompt_metrics = read_prompt_metrics(repo_root);
    let prompt_events = read_event_log(repo_root, "prompt-events.jsonl")?;
    let harness_events = read_event_log(repo_root, "harness-events.jsonl")?;
    let user_feedback = read_user_feedback(repo_root);
    let week_ago_millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(io::Error::other)?
        .as_millis()
        .try_into()
        .unwrap_or(i64::MAX)
        .saturating_sub(7 * 24 * 60 * 60 * 1000);
    let commits = hackai_gitx::commits_since(repo_root, week_ago_millis)
        .unwrap_or_default()
        .into_iter()
        .take(100)
        .map(|commit| SyncCommitClaim {
            sha: commit.sha.0,
            tree_hash: commit.tree,
            committed_at_millis: commit.committed_at_millis,
            files_changed: commit.files_changed,
            lines_added: commit.lines_added,
            lines_deleted: commit.lines_deleted,
        })
        .collect();

    Ok(SyncPayload {
        batch_id: Some(uuid::Uuid::new_v4().to_string()),
        annotation_policy_sha256: annotation_policy_sha256()?,
        cli_release_sha256: cli_release_sha256()?,
        adapter_sha256: adapter_sha256(repo_root),
        project_name: repo_root
            .file_name()
            .and_then(|name| name.to_str())
            .map(str::to_owned),
        remote_url: Some(remote_url),
        head_sha: Some(head_sha),
        commits,
        annotations,
        checkpoints,
        episodes,
        prompt_metrics,
        prompt_events,
        harness_events,
        user_feedback,
        hardware_hashes: hardware_hashes(),
        environment_class: environment_class(),
    })
}

fn sha256_hex(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn command_output(program: &str, args: &[&str]) -> Option<String> {
    let output = std::process::Command::new(program)
        .args(args)
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
        .filter(|value| !value.is_empty())
}

/// Collect machine identifiers locally and immediately hash each value. The
/// returned map contains no raw serial number, asset tag, or fingerprint.
fn hardware_hashes() -> BTreeMap<String, String> {
    let mut raw = BTreeMap::new();
    #[cfg(target_os = "macos")]
    {
        if let Some(value) = command_output("ioreg", &["-rd1", "-c", "IOPlatformExpertDevice"]) {
            for (label, key) in [
                ("fingerprint", "IOPlatformUUID"),
                ("serial_number", "IOPlatformSerialNumber"),
            ] {
                if let Some(value) = value
                    .lines()
                    .find(|line| line.contains(key))
                    .and_then(|line| line.split('"').nth(3))
                {
                    raw.insert(label, value.to_owned());
                }
            }
        }
    }
    #[cfg(target_os = "linux")]
    {
        for (label, path) in [
            ("fingerprint", "/etc/machine-id"),
            ("serial_number", "/sys/class/dmi/id/product_serial"),
            ("asset_tag", "/sys/class/dmi/id/chassis_asset_tag"),
        ] {
            if let Ok(value) = fs::read_to_string(path) {
                let value = value.trim();
                if !value.is_empty() {
                    raw.insert(label, value.to_owned());
                }
            }
        }
    }
    #[cfg(target_os = "windows")]
    {
        if let Some(value) = command_output("wmic", &["csproduct", "get", "uuid", "/value"]) {
            if let Some(value) = value.lines().find_map(|line| line.strip_prefix("UUID=")) {
                raw.insert("fingerprint", value.to_owned());
            }
        }
    }
    raw.into_iter()
        .map(|(label, value)| (label.to_owned(), sha256_hex(&value)))
        .collect()
}

fn environment_class() -> String {
    #[cfg(target_os = "linux")]
    {
        if std::env::var_os("WSL_INTEROP").is_some()
            || fs::read_to_string("/proc/version")
                .is_ok_and(|v| v.to_ascii_lowercase().contains("microsoft"))
        {
            return "wsl".to_owned();
        }
        if std::path::Path::new("/.dockerenv").exists()
            || fs::read_to_string("/proc/1/cgroup")
                .is_ok_and(|v| v.contains("docker") || v.contains("kubepods"))
        {
            return "container".to_owned();
        }
        if fs::read_to_string("/sys/class/dmi/id/sys_vendor").is_ok_and(|v| {
            let v = v.to_ascii_lowercase();
            v.contains("vmware")
                || v.contains("virtualbox")
                || v.contains("qemu")
                || v.contains("microsoft")
        }) {
            return "vm".to_owned();
        }
    }
    #[cfg(target_os = "macos")]
    if std::env::var_os("CODESPACES").is_some() {
        return "cloud_dev_env".to_owned();
    }
    "physical".to_owned()
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

fn read_event_log(repo_root: &Path, name: &str) -> io::Result<Vec<Value>> {
    let path = crate::state::state_dir(repo_root).join(name);
    let content = match fs::read_to_string(path) {
        Ok(content) => content,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };
    parse_event_log(&content)
}

fn parse_event_log(content: &str) -> io::Result<Vec<Value>> {
    let lines: Vec<_> = content.lines().collect();
    let mut events = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        match serde_json::from_str(line) {
            Ok(event) => events.push(event),
            // An async hook may still be appending its final record.
            Err(_) if index + 1 == lines.len() && !content.ends_with('\n') => {}
            Err(error) => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("invalid hook event on line {}: {error}", index + 1),
                ));
            }
        }
    }
    Ok(events)
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
        .or_else(|| {
            let project = PathBuf::from("annotations/SKILL.md");
            project.exists().then_some(project)
        })
        .or_else(|| dirs::home_dir().map(|home| home.join(".config/hackai/SKILL.md")))
        .unwrap_or_else(|| "annotations/SKILL.md".into());
    let bytes = fs::read(path)?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

const SYNC_PAYLOAD_LIMIT: usize = 4_718_592; // 4.5 MiB, leaves headroom under server 5 MiB limit
const EVIDENCE_BUNDLE_LIMIT: usize = 4_718_592; // 4.5 MiB

pub fn sync_to_server(server_url: &str, token: &str, payload: SyncPayload) -> io::Result<Value> {
    let batches = split_sync_payload(payload, SYNC_PAYLOAD_LIMIT)?;
    let mut result = Value::Null;
    let mut counts = BTreeMap::<String, u64>::new();
    for batch in batches {
        let response = send_sync_batch(server_url, token, &batch)?;
        if let Some(fields) = response.as_object() {
            for (name, value) in fields {
                if name.ends_with("_synced")
                    && let Some(count) = value.as_u64()
                {
                    let total = counts.entry(name.clone()).or_default();
                    *total = total.saturating_add(count);
                }
            }
        }
        result = response;
    }
    for (name, count) in counts {
        result[name] = json!(count);
    }
    Ok(result)
}

/// Partition the immutable snapshot below the server's 1 MiB request limit.
/// Child IDs are deterministic, so a partially acknowledged upload is safe
/// to retry without reapplying its successful children.
fn split_sync_payload(payload: SyncPayload, limit: usize) -> io::Result<Vec<SyncPayload>> {
    let mut template = serde_json::to_value(payload).map_err(io::Error::other)?;
    let mut records = Vec::new();
    for key in [
        "annotations",
        "checkpoints",
        "episodes",
        "prompt_metrics",
        "prompt_events",
        "harness_events",
        "user_feedback",
    ] {
        if let Some(items) = template[key].as_array_mut() {
            records.extend(std::mem::take(items).into_iter().map(|value| (key, value)));
        }
    }
    let overhead = serde_json::to_vec(&template)
        .map_err(io::Error::other)?
        .len()
        + 128;
    if overhead >= limit {
        return Err(io::Error::other("sync metadata exceeds upload limit"));
    }
    let mut chunk = template.clone();
    let mut size = overhead;
    let mut chunks = Vec::new();
    for (key, value) in records {
        let bytes = serde_json::to_vec(&value).map_err(io::Error::other)?.len() + 1;
        if overhead + bytes > limit {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("one {key} record exceeds the upload limit; retained locally"),
            ));
        }
        if size + bytes > limit {
            chunks.push(chunk);
            chunk = template.clone();
            size = overhead;
        }
        if let Some(items) = chunk[key].as_array_mut() {
            items.push(value);
        }
        size += bytes;
    }
    chunks.push(chunk);
    let multipart = chunks.len() > 1;
    chunks
        .into_iter()
        .map(|mut chunk| {
            if multipart || chunk["batch_id"].is_null() {
                let bytes = serde_json::to_vec(&chunk).map_err(io::Error::other)?;
                chunk["batch_id"] = json!(format!("{:x}", Sha256::digest(bytes)));
            }
            serde_json::from_value(chunk).map_err(io::Error::other)
        })
        .collect()
}

fn send_sync_batch(server_url: &str, token: &str, payload: &SyncPayload) -> io::Result<Value> {
    let client = Client::new();
    let response = client
        .post(format!("{server_url}/api/v2/upload"))
        .header("Content-Type", "application/json")
        .bearer_auth(token)
        .json(&payload)
        .timeout(Duration::from_secs(30))
        .send();

    match response {
        Ok(resp) if resp.status().is_success() => {
            let body: Value = resp.json().map_err(|error| {
                io::Error::other(format!("invalid sync acknowledgement: {error}"))
            })?;
            if body.get("status").and_then(Value::as_str) != Some("synced")
                || body.get("project_id").and_then(Value::as_i64).is_none()
            {
                return Err(io::Error::other(
                    "server did not acknowledge sync; retained for retry",
                ));
            }
            if !payload.harness_events.is_empty() && body.get("harness_events_synced").is_none() {
                return Err(io::Error::other(
                    "server needs the harness-event migration; retained for retry",
                ));
            }
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

/// Upload an evidence bundle with automatic batching under the server's 5 MiB limit.
pub fn upload_evidence_bundle(server_url: &str, token: &str, bundle: Value) -> io::Result<Value> {
    let batches = split_evidence_bundle(bundle, EVIDENCE_BUNDLE_LIMIT)?;
    let mut total_events = 0_u64;
    let mut last_response = Value::Null;
    for batch in batches {
        let response = api_request(
            server_url,
            token,
            "POST",
            "/api/v1/evidence-bundles",
            Some(batch),
        )?;
        if let Some(events_stored) = response.get("events_stored").and_then(Value::as_u64) {
            total_events = total_events.saturating_add(events_stored);
        }
        last_response = response;
    }
    last_response["events_stored"] = json!(total_events);
    Ok(last_response)
}

/// Partition an evidence bundle's events array into chunks under the size limit.
fn split_evidence_bundle(bundle: Value, limit: usize) -> io::Result<Vec<Value>> {
    let events = bundle
        .get("events")
        .and_then(Value::as_array)
        .ok_or_else(|| io::Error::other("evidence bundle missing events array"))?;
    if events.is_empty() {
        return Ok(vec![bundle]);
    }
    let mut template = bundle.clone();
    template["events"] = json!([]);
    let overhead = serde_json::to_vec(&template)
        .map_err(io::Error::other)?
        .len()
        + 64;
    if overhead >= limit {
        return Err(io::Error::other(
            "evidence bundle metadata exceeds upload limit",
        ));
    }
    let mut chunks = Vec::new();
    let mut chunk = template.clone();
    let mut size = overhead;
    for event in events {
        let bytes = serde_json::to_vec(event).map_err(io::Error::other)?.len() + 1;
        if overhead + bytes > limit {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "one evidence event exceeds the upload limit",
            ));
        }
        if size + bytes > limit {
            chunks.push(chunk);
            chunk = template.clone();
            size = overhead;
        }
        if let Some(items) = chunk["events"].as_array_mut() {
            items.push(event.clone());
        }
        size += bytes;
    }
    chunks.push(chunk);
    Ok(chunks)
}

/// Fetch the server's canonical annotation policy. The policy is public, but
/// it is always downloaded from the configured server rather than trusted
/// from a mutable local checkout.
pub fn fetch_annotation_policy(server_url: &str) -> io::Result<String> {
    let response = Client::new()
        .get(format!("{server_url}/api/v1/policy/skill"))
        .timeout(Duration::from_secs(15))
        .send()
        .map_err(|error| io::Error::other(format!("policy request failed: {error}")))?;
    if !response.status().is_success() {
        return Err(io::Error::other(format!(
            "policy request failed: HTTP {}",
            response.status()
        )));
    }
    response
        .text()
        .map_err(|error| io::Error::other(format!("invalid policy response: {error}")))
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
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(io::Error::other)?;
    let resp = client
        .post(format!("{server_url}/api/v1/cli/pairings/status"))
        .json(&json!({ "device_code": device_code }))
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
