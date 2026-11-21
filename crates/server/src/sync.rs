#![forbid(unsafe_code)]

//! Sync endpoint — pushes local tracking state to the server.
//! Registers the project from the git remote origin and stores annotations,
//! events, and checkpoints from local `.git/hackai/` state.

use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    response::Json,
    routing::post,
};
use serde::{Deserialize, Serialize};
use sqlx::Row;

use crate::AppState;

// This value is deployed with the server, not accepted from the client.
const ANNOTATION_POLICY_SHA256: &str =
    "1b5e6a4325f6fd9da7d6cc2071bb06086970f14cf72d5c630a319422e92485c6";

#[derive(Deserialize)]
pub struct SyncRequest {
    /// SHA-256 only; policy text is never uploaded.
    pub annotation_policy_sha256: String,
    #[serde(default)]
    pub cli_release_sha256: String,
    #[serde(default)]
    pub adapter_sha256: std::collections::BTreeMap<String, String>,
    /// Git remote origin URL, e.g. "https://github.com/owner/name.git"
    pub remote_url: Option<String>,
    /// Current HEAD commit SHA
    pub head_sha: Option<String>,
    /// Hash of this sync batch; re-sending the same batch is a no-op.
    #[serde(default)]
    pub batch_id: Option<String>,
    /// Local annotations to sync
    #[serde(default)]
    pub annotations: Vec<LocalAnnotation>,
    /// Local checkpoint state to sync
    #[serde(default)]
    pub checkpoints: Vec<LocalCheckpoint>,
    /// Privacy-preserving, locally derived productive-struggle episodes.
    #[serde(default)]
    pub episodes: Vec<LocalEpisode>,
    /// Privacy-preserving prompt classifications; raw prompt text is rejected.
    #[serde(default)]
    pub prompt_events: Vec<LocalPromptEvent>,
    /// Daily aggregate prompt metrics; raw prompts are never accepted.
    #[serde(default)]
    pub prompt_metrics: Vec<LocalPromptMetric>,
}

#[derive(Deserialize, Clone)]
#[allow(dead_code)]
pub struct LocalAnnotation {
    pub session_key: String,
    pub label: String,
    pub evidence: String,
    #[serde(default)]
    pub applied: Option<bool>,
    #[serde(default)]
    pub discount_permille: Option<i32>,
}

#[derive(Deserialize, Clone)]
pub struct LocalCheckpoint {
    pub cursor: String,
}

#[derive(Deserialize, Clone)]
pub struct LocalEpisode {
    pub formula_version: u16,
    pub started_at: LocalTime,
    pub ended_at: LocalTime,
    pub session: String,
    pub attempts: u32,
    #[serde(default)]
    pub distinct_signatures: Vec<String>,
    #[serde(default)]
    pub files_touched: Vec<String>,
    pub human_edit_seconds: u64,
    pub agent_edit_seconds: u64,
    pub resolved: bool,
    pub credited_seconds: u64,
    #[serde(default)]
    pub buckets: Vec<LocalBucket>,
}

#[derive(Deserialize, Clone)]
pub struct LocalTime(pub i64);

#[derive(Deserialize, Clone)]
pub struct LocalBucket {
    #[serde(default)]
    pub evidence_ids: Vec<LocalEventId>,
}

#[derive(Deserialize, Clone)]
pub struct LocalPromptEvent {
    #[serde(default = "unknown_harness")]
    pub harness: String,
    pub session_id: String,
    pub occurred_at_unix_ms: i64,
    #[serde(default)]
    pub pasted_environment_hint: bool,
    pub signals: serde_json::Value,
}

#[derive(Deserialize, Clone)]
pub struct LocalPromptMetric {
    pub day_start_unix: i64,
    pub prompt_count: i64,
    pub proficiency_sum_permille: i64,
    pub slop_sum_permille: i64,
}

fn unknown_harness() -> String {
    "unknown".to_owned()
}

#[derive(Deserialize, Clone)]
pub struct LocalEventId(pub String);

#[derive(Serialize)]
pub struct SyncResponse {
    pub status: &'static str,
    pub project_id: i64,
    pub repository_id: Option<i64>,
    pub annotations_synced: i64,
    pub checkpoints_synced: i64,
    pub episodes_synced: i64,
    pub prompt_events_synced: i64,
    pub prompt_metrics_synced: i64,
    pub tampered_request: bool,
    pub github_url: Option<String>,
}

pub fn routes() -> axum::Router<AppState> {
    // One canonical upload contract. `/sync` remains a compatibility alias
    // for older CLIs already installed on users' machines.
    axum::Router::new()
        .route("/api/v1/upload", post(sync))
        .route("/api/v1/sync", post(sync))
}

async fn sync(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<SyncRequest>,
) -> Result<Json<SyncResponse>, StatusCode> {
    let sub = crate::auth::extract_current_user(&state.auth, &state.db, &headers)
        .await
        .ok_or(StatusCode::UNAUTHORIZED)?;

    // Server-derived only: an upload is kept but visibly marked when the
    // source-of-truth SKILL.md hash differs from deployed policy.
    let trusted_release =
        sqlx::query("SELECT 1 FROM trusted_cli_releases WHERE sha256 = $1 AND active = TRUE")
            .bind(&req.cli_release_sha256)
            .fetch_optional(&state.db)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
            .is_some();
    let adapter_hashes_valid = req
        .adapter_sha256
        .values()
        .all(|hash| hash.len() == 64 && hash.bytes().all(|byte| byte.is_ascii_hexdigit()));
    let tampered_request = req.annotation_policy_sha256 != ANNOTATION_POLICY_SHA256
        || !trusted_release
        || !adapter_hashes_valid;

    // Find or create the user
    let user_row = sqlx::query("SELECT id FROM users WHERE auth_subject = $1")
        .bind(&sub)
        .fetch_optional(&state.db)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;
    let user_id: i64 = user_row
        .try_get("id")
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Parse GitHub URL from remote
    let github_url = req.remote_url.as_deref().and_then(parse_github_url);
    let (remote_owner, remote_name) = github_url
        .as_ref()
        .map(|(o, n)| (o.as_str(), n.as_str()))
        .unzip();

    // Find or create project
    let project_name = remote_name.unwrap_or("local-project");
    let project_row = sqlx::query(
        "INSERT INTO projects (name, owner_id, private) \
         VALUES ($1, $2, true) \
         ON CONFLICT (owner_id, name) DO UPDATE SET name = projects.name \
         RETURNING id",
    )
    .bind(project_name)
    .bind(user_id)
    .fetch_one(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let project_id: i64 = project_row
        .try_get("id")
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    if tampered_request {
        let _ = sqlx::query(
            "INSERT INTO tampered_sync_requests (user_id, project_id, observed_policy_sha256) VALUES ($1, $2, $3)",
        )
        .bind(user_id)
        .bind(project_id)
        .bind(&req.annotation_policy_sha256)
        .execute(&state.db)
        .await;
    }

    // Find or create repository record
    let repository_id = if let (Some(owner), Some(name)) = (remote_owner, remote_name) {
        let repo_row = sqlx::query(
            "INSERT INTO repositories (project_id, remote_owner, remote_name, public) \
             VALUES ($1, $2, $3, false) \
             ON CONFLICT (project_id) DO UPDATE SET \
               remote_owner = EXCLUDED.remote_owner, \
               remote_name = EXCLUDED.remote_name \
             RETURNING id",
        )
        .bind(project_id)
        .bind(owner)
        .bind(name)
        .fetch_one(&state.db)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        Some(
            repo_row
                .try_get::<i64, _>("id")
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
        )
    } else {
        None
    };

    // Idempotent on batch hash: a batch already applied for this user is skipped
    // wholesale so retries never duplicate episodes or double-count credit.
    // ponytail: check-then-insert race under concurrent identical-batch retries
    // (no unique constraint on batch_id); upgrade with a processed_batches table.
    if let Some(batch_id) = &req.batch_id {
        let already: Option<(i64,)> = sqlx::query_as(
            "SELECT id FROM productive_episodes WHERE user_id = $1 AND batch_id = $2 LIMIT 1",
        )
        .bind(user_id)
        .bind(batch_id)
        .fetch_optional(&state.db)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        if already.is_some() {
            return Ok(Json(SyncResponse {
                status: "synced",
                project_id,
                repository_id,
                annotations_synced: 0,
                checkpoints_synced: 0,
                episodes_synced: 0,
                prompt_events_synced: 0,
                prompt_metrics_synced: 0,
                tampered_request,
                github_url: github_url.map(|(o, n)| format!("https://github.com/{}/{}", o, n)),
            }));
        }
    }

    // Store commit SHA if provided
    if let Some(sha) = &req.head_sha {
        if let Some(repo_id) = repository_id {
            let _ = sqlx::query(
                "INSERT INTO commits (sha, repository_id, user_id, tree_hash) \
                 VALUES ($1, $2, $3, $4) \
                 ON CONFLICT (sha) DO NOTHING",
            )
            .bind(sha)
            .bind(repo_id)
            .bind(user_id)
            .bind("0000000000000000000000000000000000000000")
            .execute(&state.db)
            .await;
        }
    }

    // Sync annotations
    let mut annotations_synced: i64 = 0;
    for ann in &req.annotations {
        if !["debugging", "guided", "delegated", "reading", "exploring"]
            .contains(&ann.label.as_str())
        {
            continue;
        }
        let _ = sqlx::query(
            "INSERT INTO annotations (user_id, session_key, label, evidence, applied, discount_permille) \
             VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(user_id)
        .bind(&ann.session_key)
        .bind(&ann.label)
        .bind(&ann.evidence)
        .bind(false)    // applied is always server-forced; client value ignored
        .bind(0)        // discount_permille is always server-forced; client value ignored
        .execute(&state.db)
        .await;
        annotations_synced += 1;
    }

    // Sync checkpoints
    let mut checkpoints_synced: i64 = 0;
    for cp in &req.checkpoints {
        let _ = sqlx::query("INSERT INTO checkpoints (user_id, cursor) VALUES ($1, $2)")
            .bind(user_id)
            .bind(&cp.cursor)
            .execute(&state.db)
            .await;
        checkpoints_synced += 1;
    }

    let mut episodes_synced: i64 = 0;
    for episode in &req.episodes {
        if episode.session.is_empty()
            || episode.session.len() > 128
            || episode.attempts == 0
            || episode.ended_at.0 < episode.started_at.0
            || episode.distinct_signatures.len() > 64
            || episode.files_touched.len() > 256
            || episode.buckets.len() > 512
            || episode
                .distinct_signatures
                .iter()
                .any(|value| value.len() != 64)
            || episode
                .files_touched
                .iter()
                .any(|value| value.is_empty() || value.len() > 1_024)
        {
            continue;
        }
        let evidence: Vec<String> = episode
            .buckets
            .iter()
            .flat_map(|bucket| bucket.evidence_ids.iter())
            .map(|id| id.0.clone())
            .filter(|id| !id.is_empty() && id.len() <= 256)
            .collect();
        let started_seconds = episode.started_at.0 / 1_000;
        let ended_seconds = episode.ended_at.0 / 1_000;
        let stored = sqlx::query(
            "INSERT INTO productive_episodes \
             (user_id, project_id, session_key, started_at, ended_at, attempts, signatures, files_touched, \
              human_edit_seconds, agent_edit_seconds, resolved, credited_seconds, evidence_ids, formula_version, batch_id) \
             VALUES ($1, $2, $3, to_timestamp($4), to_timestamp($5), $6, $7, $8, $9, $10, $11, $12, $13, $14, $15) \
             ON CONFLICT (user_id, project_id, session_key, started_at) DO UPDATE SET \
               ended_at = EXCLUDED.ended_at, attempts = EXCLUDED.attempts, signatures = EXCLUDED.signatures, \
               files_touched = EXCLUDED.files_touched, human_edit_seconds = EXCLUDED.human_edit_seconds, \
               agent_edit_seconds = EXCLUDED.agent_edit_seconds, resolved = EXCLUDED.resolved, \
               credited_seconds = EXCLUDED.credited_seconds, evidence_ids = EXCLUDED.evidence_ids, \
               formula_version = EXCLUDED.formula_version",
        )
        .bind(user_id)
        .bind(project_id)
        .bind(&episode.session)
        .bind(started_seconds as f64)
        .bind(ended_seconds as f64)
        .bind(i64::from(episode.attempts))
        .bind(serde_json::json!(episode.distinct_signatures))
        .bind(serde_json::json!(episode.files_touched))
        .bind(i64::try_from(episode.human_edit_seconds).unwrap_or(i64::MAX))
        .bind(i64::try_from(episode.agent_edit_seconds).unwrap_or(i64::MAX))
        .bind(episode.resolved)
        .bind(i64::try_from(episode.credited_seconds).unwrap_or(i64::MAX))
        .bind(serde_json::json!(evidence))
        .bind(i32::from(episode.formula_version))
        .bind(req.batch_id.as_deref())
        .execute(&state.db)
        .await;
        if stored.is_ok() {
            episodes_synced += 1;
        }
    }

    let mut prompt_events_synced = 0;
    for event in &req.prompt_events {
        if event.session_id.is_empty()
            || event.session_id.len() > 128
            || event.occurred_at_unix_ms < 0
            || event.signals.get("prompt").is_some()
        {
            continue;
        }
        let stored = sqlx::query("INSERT INTO prompt_events (user_id, project_id, harness, session_id, occurred_at, pasted_environment_hint, tampered_request, signals) VALUES ($1, $2, $3, $4, to_timestamp($5::double precision / 1000), $6, $7, $8) ON CONFLICT (user_id, project_id, session_id, occurred_at) DO UPDATE SET harness = EXCLUDED.harness, pasted_environment_hint = EXCLUDED.pasted_environment_hint, tampered_request = EXCLUDED.tampered_request, signals = EXCLUDED.signals")
            .bind(user_id).bind(project_id).bind(&event.harness).bind(&event.session_id).bind(event.occurred_at_unix_ms).bind(event.pasted_environment_hint).bind(tampered_request).bind(&event.signals).execute(&state.db).await;
        if stored.is_ok() {
            prompt_events_synced += 1;
        }
    }

    let mut prompt_metrics_synced = 0;
    for metric in &req.prompt_metrics {
        if metric.day_start_unix < 0
            || metric.prompt_count <= 0
            || metric.proficiency_sum_permille < 0
            || metric.slop_sum_permille < 0
            || metric.proficiency_sum_permille > metric.prompt_count * 1000
            || metric.slop_sum_permille > metric.prompt_count * 1000
        {
            continue;
        }
        let stored = sqlx::query(
            "INSERT INTO prompt_metrics (user_id, project_id, day_start, prompt_count, proficiency_sum_permille, slop_sum_permille) VALUES ($1, $2, to_timestamp($3), $4, $5, $6) ON CONFLICT (user_id, project_id, day_start) DO UPDATE SET prompt_count = EXCLUDED.prompt_count, proficiency_sum_permille = EXCLUDED.proficiency_sum_permille, slop_sum_permille = EXCLUDED.slop_sum_permille",
        )
        .bind(user_id).bind(project_id).bind(metric.day_start_unix)
        .bind(metric.prompt_count).bind(metric.proficiency_sum_permille).bind(metric.slop_sum_permille)
        .execute(&state.db).await;
        if stored.is_ok() {
            prompt_metrics_synced += 1;
        }
    }

    Ok(Json(SyncResponse {
        status: "synced",
        project_id,
        repository_id,
        annotations_synced,
        checkpoints_synced,
        episodes_synced,
        prompt_events_synced,
        prompt_metrics_synced,
        tampered_request,
        github_url: github_url.map(|(o, n)| format!("https://github.com/{}/{}", o, n)),
    }))
}

/// Parse a GitHub remote URL into (owner, name).
/// Supports: https://github.com/owner/name, https://github.com/owner/name.git, git@github.com:owner/name.git
fn parse_github_url(url: &str) -> Option<(String, String)> {
    let s = url.trim();
    // HTTPS: https://github.com/owner/name(.git)?
    if let Some(rest) = s.strip_prefix("https://github.com/") {
        let parts: Vec<&str> = rest.split('/').collect();
        if parts.len() >= 2 {
            let owner = parts[0].to_string();
            let name = parts[1].trim_end_matches(".git").to_string();
            if !owner.is_empty() && !name.is_empty() {
                return Some((owner, name));
            }
        }
    }
    // SSH: git@github.com:owner/name(.git)?
    if let Some(rest) = s.strip_prefix("git@github.com:") {
        let parts: Vec<&str> = rest.split('/').collect();
        if parts.len() >= 2 {
            let owner = parts[0].to_string();
            let name = parts[1].trim_end_matches(".git").to_string();
            if !owner.is_empty() && !name.is_empty() {
                return Some((owner, name));
            }
        }
    }
    None
}
