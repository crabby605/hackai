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
// Can be overridden via ANNOTATION_POLICY_SHA256 env var.
fn annotation_policy_sha256() -> &'static str {
    static SHA: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    SHA.get_or_init(|| {
        std::env::var("ANNOTATION_POLICY_SHA256").unwrap_or_else(|_| {
            "ac36dc69ffb032a0a86c0c2123bfdb20df1dcdbe8e7080980f03116e6c4c169d".to_string()
        })
    })
}

#[derive(Deserialize, Clone)]
pub struct SyncCommitClaim {
    pub sha: String,
    pub tree_hash: String,
    pub committed_at_millis: i64,
    pub files_changed: u32,
    pub lines_added: u32,
    pub lines_deleted: u32,
}

#[derive(Deserialize)]
pub struct SyncRequest {
    /// SHA-256 only; policy text is never uploaded.
    pub annotation_policy_sha256: String,
    #[serde(default)]
    pub cli_release_sha256: String,
    #[serde(default)]
    pub adapter_sha256: std::collections::BTreeMap<String, String>,
    #[serde(default)]
    pub project_name: Option<String>,
    /// Git remote origin URL, e.g. "https://github.com/owner/name.git"
    pub remote_url: Option<String>,
    /// Current HEAD commit SHA
    pub head_sha: Option<String>,
    /// Bounded recent commit claims; messages, authors, paths and code are not accepted.
    #[serde(default)]
    pub commits: Vec<SyncCommitClaim>,
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
    /// User-opted-in, bounded coaching notes written by the agent locally.
    #[serde(default)]
    pub user_feedback: Vec<LocalUserFeedback>,
    /// SHA-256 digests only; raw hardware identifiers are never accepted.
    #[serde(default)]
    pub hardware_hashes: std::collections::BTreeMap<String, String>,
    #[serde(default = "unknown_environment_class")]
    pub environment_class: String,
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

#[derive(Deserialize, Clone)]
pub struct LocalUserFeedback {
    pub recorded_at_unix_ms: i64,
    pub feedback: String,
}

fn unknown_harness() -> String {
    "unknown".to_owned()
}

fn unknown_environment_class() -> String {
    "unknown".to_owned()
}

fn valid_environment_class(value: &str) -> bool {
    matches!(
        value,
        "physical" | "vm" | "container" | "wsl" | "cloud_dev_env" | "unknown"
    )
}

fn safe_metadata(value: &str, max_len: usize) -> bool {
    !value.is_empty()
        && value.len() <= max_len
        && !value.chars().any(|ch| {
            ch.is_control() || matches!(ch, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
        })
}

fn contains_prompt_text(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::Object(map) => map.iter().any(|(key, value)| {
            matches!(key.as_str(), "prompt" | "prompt_text" | "content" | "text")
                || contains_prompt_text(value)
        }),
        serde_json::Value::Array(values) => values.iter().any(contains_prompt_text),
        _ => false,
    }
}

fn valid_hardware_hashes(values: &std::collections::BTreeMap<String, String>) -> bool {
    values.len() <= 3
        && values.iter().all(|(kind, hash)| {
            matches!(kind.as_str(), "fingerprint" | "asset_tag" | "serial_number")
                && hash.len() == 64
                && hash
                    .bytes()
                    .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        })
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
    pub user_feedback_synced: i64,
    pub tampered_request: bool,
    pub github_url: Option<String>,
    pub commits_synced: usize,
    pub outcome_factor_permille: Option<i32>,
    pub github_gate_reason: Option<&'static str>,
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
    let tampered_request = req.annotation_policy_sha256 != annotation_policy_sha256()
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

    if !valid_hardware_hashes(&req.hardware_hashes)
        || !valid_environment_class(&req.environment_class)
    {
        return Err(StatusCode::UNPROCESSABLE_ENTITY);
    }
    for hash in req.hardware_hashes.values() {
        let banned: bool =
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM hardware_bans WHERE sha256_hash = $1)")
                .bind(hash)
                .fetch_one(&state.db)
                .await
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        if banned {
            // Flag for review instead of instant ban - create a review case
            let cluster_key = format!("hardware-ban:{hash}");
            let cluster_id: i64 = sqlx::query_scalar(
                "INSERT INTO review_clusters (cluster_key) VALUES ($1) ON CONFLICT (cluster_key) DO UPDATE SET updated_at = now() RETURNING id"
            )
                .bind(&cluster_key)
                .fetch_one(&state.db)
                .await
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
            let dedupe_key = format!("hardware-ban:{user_id}:{hash}");
            let _ = sqlx::query(
                "INSERT INTO review_cases (cluster_id, subject_user_id, dedupe_key, priority, summary) VALUES ($1, $2, $3, 5, 'User hardware hash matches banned hash') ON CONFLICT (dedupe_key) DO UPDATE SET updated_at = now()"
            )
                .bind(cluster_id)
                .bind(user_id)
                .bind(&dedupe_key)
                .execute(&state.db)
                .await;
            // Don't auto-ban, just flag for review
        }
    }
    for (kind, hash) in &req.hardware_hashes {
        sqlx::query(
            "INSERT INTO hardware_fingerprints (user_id, kind, sha256_hash, environment_class) VALUES ($1, $2, $3, $4) \
             ON CONFLICT (user_id, kind, sha256_hash) DO UPDATE SET last_seen_at = now(), environment_class = EXCLUDED.environment_class",
        )
        .bind(user_id)
        .bind(kind)
        .bind(hash)
        .bind(&req.environment_class)
        .execute(&state.db)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        if req.environment_class == "physical" {
            let linked_users = sqlx::query_scalar::<_, i64>("SELECT user_id FROM hardware_fingerprints WHERE sha256_hash = $1 AND user_id <> $2")
                .bind(hash)
                .bind(user_id)
                .fetch_all(&state.db)
                .await
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
            for related_user_id in linked_users {
                let cluster_key = format!("hardware:{hash}");
                let cluster_id: i64 = sqlx::query_scalar("INSERT INTO review_clusters (cluster_key) VALUES ($1) ON CONFLICT (cluster_key) DO UPDATE SET updated_at = now() RETURNING id")
                    .bind(&cluster_key).fetch_one(&state.db).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
                let dedupe_key = format!("hardware-link:{user_id}:{hash}");
                let case_id: i64 = sqlx::query_scalar("INSERT INTO review_cases (cluster_id, subject_user_id, dedupe_key, priority, summary) VALUES ($1, $2, $3, 0, 'Shared physical-device hash requires review') ON CONFLICT (dedupe_key) DO UPDATE SET updated_at = now() RETURNING id")
                    .bind(cluster_id).bind(user_id).bind(&dedupe_key).fetch_one(&state.db).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
                let _ = sqlx::query("INSERT INTO review_case_edges (case_id, edge_type, strength, subject_value, related_user_id) VALUES ($1, 'shared_device_hash', 'strong', $2, $3) ON CONFLICT DO NOTHING")
                    .bind(case_id).bind(hash).bind(related_user_id).execute(&state.db).await;
            }
        }
    }

    // Parse GitHub URL from remote
    let github_url = req.remote_url.as_deref().and_then(parse_github_url);
    let (remote_owner, remote_name) = github_url
        .as_ref()
        .map(|(o, n)| (o.as_str(), n.as_str()))
        .unzip();

    // Find or create project
    let local_project_name = req
        .project_name
        .as_deref()
        .map(str::trim)
        .filter(|name| !name.is_empty() && name.len() <= 120 && !name.contains(['/', '\\']))
        .unwrap_or("local-project");
    let project_name = remote_name.unwrap_or(local_project_name);
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
            "INSERT INTO repositories (project_id, remote_owner, remote_name, public, remote_url, head_sha) \
             VALUES ($1, $2, $3, false, $4, $5) \
             ON CONFLICT (project_id) DO UPDATE SET \
               remote_owner = EXCLUDED.remote_owner, \
               remote_name = EXCLUDED.remote_name, \
               remote_url = EXCLUDED.remote_url, \
               head_sha = EXCLUDED.head_sha \
             RETURNING id",
        )
        .bind(project_id)
        .bind(owner)
        .bind(name)
        .bind(req.remote_url.as_deref())
        .bind(req.head_sha.as_deref())
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

    if req.commits.len() > 100
        || req.commits.iter().any(|commit| {
            !valid_commit_hash(&commit.sha)
                || !valid_commit_hash(&commit.tree_hash)
                || commit.committed_at_millis < 0
                || commit.files_changed > 1_000_000
                || commit.lines_added > 100_000_000
                || commit.lines_deleted > 100_000_000
        })
        || req
            .head_sha
            .as_deref()
            .is_some_and(|sha| !sha.is_empty() && !valid_commit_hash(sha))
    {
        return Err(StatusCode::UNPROCESSABLE_ENTITY);
    }

    // Idempotent on batch hash: a batch already applied for this user is skipped
    // wholesale so retries never duplicate episodes or double-count credit.
    // Fast-path SELECT avoids the GitHub gate on retries; the batch is marked
    // atomically inside the write transaction below (race-safe).
    if let Some(batch_id) = &req.batch_id {
        let already: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM processed_batches WHERE user_id = $1 AND batch_id = $2)",
        )
        .bind(user_id)
        .bind(batch_id)
        .fetch_one(&state.db)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        if already {
            return Ok(Json(SyncResponse {
                status: "synced",
                project_id,
                repository_id,
                annotations_synced: 0,
                checkpoints_synced: 0,
                episodes_synced: 0,
                prompt_events_synced: 0,
                prompt_metrics_synced: 0,
                user_feedback_synced: 0,
                tampered_request,
                github_url: github_url.map(|(o, n)| format!("https://github.com/{}/{}", o, n)),
                commits_synced: 0,
                outcome_factor_permille: None,
                github_gate_reason: None,
            }));
        }
    }

    let mut gate = match (
        github_url.as_ref(),
        req.head_sha.as_deref().filter(|sha| !sha.is_empty()),
    ) {
        (Some((owner, name)), Some(sha)) => github_submission_gate(owner, name, sha).await,
        (Some(_), None) => GateDecision {
            outcome_factor_permille: 0,
            reason: "missing_head",
            repository_public: None,
            github_observed: false,
        },
        (None, _) => GateDecision {
            outcome_factor_permille: 0,
            reason: "not_github_origin",
            repository_public: None,
            github_observed: false,
        },
    };

    // A tampered client forfeits all outcome credit regardless of the GitHub gate.
    if tampered_request {
        gate.outcome_factor_permille = 0;
        gate.reason = "tampered_client";
    }

    // All writes from batch marking onward run in one transaction so a
    // mid-sync DB failure rolls back cleanly and can be retried without data
    // loss. The GitHub gate (external HTTP) deliberately stays outside it.
    let mut tx = state
        .db
        .begin()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    if let Some(batch_id) = &req.batch_id {
        let marked = sqlx::query(
            "INSERT INTO processed_batches (user_id, batch_id) VALUES ($1, $2) ON CONFLICT (user_id, batch_id) DO NOTHING RETURNING id",
        )
        .bind(user_id)
        .bind(batch_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        if marked.is_none() {
            // A concurrent sync already marked this batch; roll back and report synced.
            return Ok(Json(SyncResponse {
                status: "synced",
                project_id,
                repository_id,
                annotations_synced: 0,
                checkpoints_synced: 0,
                episodes_synced: 0,
                prompt_events_synced: 0,
                prompt_metrics_synced: 0,
                user_feedback_synced: 0,
                tampered_request,
                github_url: github_url.map(|(o, n)| format!("https://github.com/{}/{}", o, n)),
                commits_synced: 0,
                outcome_factor_permille: None,
                github_gate_reason: None,
            }));
        }
    }

    if let (Some(repo_id), Some(public)) = (repository_id, gate.repository_public) {
        sqlx::query("UPDATE repositories SET public = $1 WHERE id = $2")
            .bind(public)
            .bind(repo_id)
            .execute(&mut *tx)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    }

    let mut commits_synced = 0;
    if let Some(repo_id) = repository_id {
        for commit in &req.commits {
            sqlx::query(
                "INSERT INTO commits (sha, repository_id, user_id, tree_hash, committed_at, files_changed, lines_added, lines_deleted) \
                 VALUES ($1, $2, $3, $4, to_timestamp($5::double precision / 1000), $6, $7, $8) \
                 ON CONFLICT (sha) DO NOTHING",
            )
            .bind(&commit.sha)
            .bind(repo_id)
            .bind(user_id)
            .bind(&commit.tree_hash)
            .bind(commit.committed_at_millis as f64)
            .bind(i64::from(commit.files_changed))
            .bind(i64::from(commit.lines_added))
            .bind(i64::from(commit.lines_deleted))
            .execute(&mut *tx)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
            commits_synced += 1;
        }
    }

    // A globally reused SHA never transfers verification or ownership between users.
    let head_tree = req
        .commits
        .iter()
        .find(|commit| Some(commit.sha.as_str()) == req.head_sha.as_deref())
        .map(|commit| commit.tree_hash.clone())
        .unwrap_or_else(|| "0000000000000000000000000000000000000000".to_owned());
    if let (Some(sha), Some(repo_id)) = (&req.head_sha, repository_id)
        && !sha.is_empty()
    {
        sqlx::query(
            "INSERT INTO commits (sha, repository_id, user_id, tree_hash) \
             VALUES ($1, $2, $3, $4) ON CONFLICT (sha) DO NOTHING",
        )
        .bind(sha)
        .bind(repo_id)
        .bind(user_id)
        .bind(&head_tree)
        .execute(&mut *tx)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        let owned = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM commits WHERE sha = $1 AND repository_id = $2 AND user_id = $3)",
        )
        .bind(sha)
        .bind(repo_id)
        .bind(user_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        if !owned {
            gate.outcome_factor_permille = 0;
            gate.reason = "commit_claim_conflict";
            gate.github_observed = false;
        } else {
            sqlx::query(
                "INSERT INTO submissions (user_id, project_id, commit_sha, tree_hash, outcome_factor_permille, github_gate_reason, github_observed_at) \
                 VALUES ($1, $2, $3, $4, $5, $6, CASE WHEN $7 THEN now() ELSE NULL END) \
                 ON CONFLICT (user_id, project_id, commit_sha, tree_hash) DO UPDATE SET \
                 outcome_factor_permille = EXCLUDED.outcome_factor_permille, \
                 github_gate_reason = EXCLUDED.github_gate_reason, \
                 github_observed_at = EXCLUDED.github_observed_at",
            )
            .bind(user_id)
            .bind(project_id)
            .bind(sha)
            .bind(&head_tree)
            .bind(gate.outcome_factor_permille)
            .bind(gate.reason)
            .bind(gate.github_observed)
            .execute(&mut *tx)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
            if gate.github_observed {
                sqlx::query("UPDATE commits SET github_observed_at = now() WHERE sha = $1")
                    .bind(sha)
                    .execute(&mut *tx)
                    .await
                    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
            }
        }
    }

    // Sync annotations
    let mut annotations_synced: i64 = 0;
    for ann in &req.annotations {
        if !crate::annotations::VALID_LABELS.contains(&ann.label.as_str())
            || !safe_metadata(&ann.session_key, 128)
            || !safe_metadata(&ann.evidence, 1_024)
        {
            continue;
        }
        sqlx::query(
            "INSERT INTO annotations (user_id, session_key, label, evidence, applied, discount_permille) \
             VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(user_id)
        .bind(&ann.session_key)
        .bind(&ann.label)
        .bind(&ann.evidence)
        .bind(false)    // applied is always server-forced; client value ignored
        .bind(0)        // discount_permille is always server-forced; client value ignored
        .execute(&mut *tx)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        annotations_synced += 1;
    }

    // Sync checkpoints
    let mut checkpoints_synced: i64 = 0;
    for cp in &req.checkpoints {
        if !safe_metadata(&cp.cursor, 1_024) {
            continue;
        }
        sqlx::query("INSERT INTO checkpoints (user_id, cursor) VALUES ($1, $2)")
            .bind(user_id)
            .bind(&cp.cursor)
            .execute(&mut *tx)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        checkpoints_synced += 1;
    }

    let mut episodes_synced: i64 = 0;
    for episode in &req.episodes {
        if episode.session.is_empty()
            || episode.session.len() > 128
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
        let started_seconds = episode.started_at.0 / 1_000;
        let ended_seconds = episode.ended_at.0 / 1_000;
        if !episode_claims_within_bounds(episode) {
            continue;
        }
        let evidence: Vec<String> = episode
            .buckets
            .iter()
            .flat_map(|bucket| bucket.evidence_ids.iter())
            .map(|id| id.0.clone())
            .filter(|id| !id.is_empty() && id.len() <= 256)
            .collect();
        // Sanitize raw file paths: store only HMAC-SHA256 digests keyed by a
        // server secret, so no raw paths are ever persisted (admin sees
        // count/hash only because raw paths are no longer stored).
        let sanitized_files: Vec<String> = episode
            .files_touched
            .iter()
            .map(|path| hex::encode(hmac_sha256(files_hmac_secret(&state), path.as_bytes())))
            .collect();
        sqlx::query(
            "INSERT INTO productive_episodes \
             (user_id, project_id, session_key, started_at, ended_at, attempts, signatures, files_touched, \
              human_edit_seconds, agent_edit_seconds, resolved, credited_seconds, evidence_ids, formula_version, batch_id, outcome_factor_permille) \
             VALUES ($1, $2, $3, to_timestamp($4), to_timestamp($5), $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16) \
             ON CONFLICT (user_id, project_id, session_key, started_at) DO UPDATE SET \
               ended_at = EXCLUDED.ended_at, attempts = EXCLUDED.attempts, signatures = EXCLUDED.signatures, \
               files_touched = EXCLUDED.files_touched, human_edit_seconds = EXCLUDED.human_edit_seconds, \
               agent_edit_seconds = EXCLUDED.agent_edit_seconds, resolved = EXCLUDED.resolved, \
               credited_seconds = EXCLUDED.credited_seconds, evidence_ids = EXCLUDED.evidence_ids, \
               formula_version = EXCLUDED.formula_version, outcome_factor_permille = EXCLUDED.outcome_factor_permille",
        )
        .bind(user_id)
        .bind(project_id)
        .bind(&episode.session)
        .bind(started_seconds as f64)
        .bind(ended_seconds as f64)
        .bind(i64::from(episode.attempts))
        .bind(serde_json::json!(episode.distinct_signatures))
        .bind(serde_json::json!(sanitized_files))
        .bind(i64::try_from(episode.human_edit_seconds).ok().unwrap_or(0))
        .bind(i64::try_from(episode.agent_edit_seconds).ok().unwrap_or(0))
        .bind(episode.resolved)
        .bind(i64::try_from(episode.credited_seconds).ok().unwrap_or(0))
        .bind(serde_json::json!(evidence))
        .bind(i32::from(episode.formula_version))
        .bind(req.batch_id.as_deref())
        .bind(gate.outcome_factor_permille)
        .execute(&mut *tx)
        .await
        .map_err(|error| {
            eprintln!("episode sync insert failed: {error}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
        episodes_synced += 1;
    }

    let mut prompt_events_synced = 0;
    for event in &req.prompt_events {
        if event.session_id.is_empty()
            || event.session_id.len() > 128
            || event.occurred_at_unix_ms < 0
            || contains_prompt_text(&event.signals)
        {
            continue;
        }
        sqlx::query("INSERT INTO prompt_events (user_id, project_id, harness, session_id, occurred_at, pasted_environment_hint, tampered_request, signals) VALUES ($1, $2, $3, $4, to_timestamp($5::double precision / 1000), $6, $7, $8) ON CONFLICT (user_id, project_id, session_id, occurred_at) DO UPDATE SET harness = EXCLUDED.harness, pasted_environment_hint = EXCLUDED.pasted_environment_hint, tampered_request = EXCLUDED.tampered_request, signals = EXCLUDED.signals")
            .bind(user_id).bind(project_id).bind(&event.harness).bind(&event.session_id).bind(event.occurred_at_unix_ms).bind(event.pasted_environment_hint).bind(tampered_request).bind(&event.signals).execute(&mut *tx).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        prompt_events_synced += 1;
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
        sqlx::query(
            "INSERT INTO prompt_metrics (user_id, project_id, day_start, prompt_count, proficiency_sum_permille, slop_sum_permille) VALUES ($1, $2, to_timestamp($3), $4, $5, $6) ON CONFLICT (user_id, project_id, day_start) DO UPDATE SET prompt_count = EXCLUDED.prompt_count, proficiency_sum_permille = EXCLUDED.proficiency_sum_permille, slop_sum_permille = EXCLUDED.slop_sum_permille",
        )
        .bind(user_id).bind(project_id).bind(metric.day_start_unix)
        .bind(metric.prompt_count).bind(metric.proficiency_sum_permille).bind(metric.slop_sum_permille)
        .execute(&mut *tx)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        prompt_metrics_synced += 1;
    }

    let mut user_feedback_synced = 0;
    for feedback in &req.user_feedback {
        let word_count = feedback.feedback.split_whitespace().count();
        if feedback.recorded_at_unix_ms < 0
            || !(1..=50).contains(&word_count)
            || feedback.feedback.len() > 600
            || feedback.feedback.chars().any(char::is_control)
        {
            continue;
        }
        sqlx::query(
            "INSERT INTO user_feedback (user_id, project_id, recorded_at, feedback) \
             VALUES ($1, $2, to_timestamp($3::double precision / 1000), $4) \
             ON CONFLICT (user_id, project_id, recorded_at) DO NOTHING",
        )
        .bind(user_id)
        .bind(project_id)
        .bind(feedback.recorded_at_unix_ms)
        .bind(&feedback.feedback)
        .execute(&mut *tx)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        user_feedback_synced += 1;
    }

    tx.commit()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    state
        .sse_hub
        .publish(
            user_id,
            "sync",
            serde_json::json!({
                "project_id": project_id,
                "episodes_synced": episodes_synced,
                "commits_synced": commits_synced,
                "outcome_factor_permille": gate.outcome_factor_permille,
            }),
        )
        .await;

    Ok(Json(SyncResponse {
        status: "synced",
        project_id,
        repository_id,
        annotations_synced,
        checkpoints_synced,
        episodes_synced,
        prompt_events_synced,
        prompt_metrics_synced,
        user_feedback_synced,
        tampered_request,
        github_url: github_url.map(|(o, n)| format!("https://github.com/{}/{}", o, n)),
        commits_synced,
        outcome_factor_permille: Some(gate.outcome_factor_permille),
        github_gate_reason: Some(gate.reason),
    }))
}

#[derive(Debug, PartialEq, Eq)]
struct GateDecision {
    outcome_factor_permille: i32,
    reason: &'static str,
    repository_public: Option<bool>,
    github_observed: bool,
}

fn gate_from_facts(repository_public: Option<bool>, head_exists: Option<bool>) -> GateDecision {
    let (outcome_factor_permille, reason) = match (repository_public, head_exists) {
        (Some(true), Some(true)) => (1000, "verified"),
        (Some(false), _) => (0, "private_or_nonpublic_repository"),
        (None, _) => (0, "github_unavailable"),
        (_, Some(false)) => (0, "head_not_found_in_repository"),
        (_, None) => (0, "github_unavailable"),
    };
    GateDecision {
        outcome_factor_permille,
        reason,
        repository_public,
        github_observed: head_exists.is_some(),
    }
}

async fn github_submission_gate(owner: &str, name: &str, sha: &str) -> GateDecision {
    let Ok(client) = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(4))
        .user_agent("hackai-submission-gate")
        .build()
    else {
        return gate_from_facts(None, None);
    };
    let token = std::env::var("GITHUB_TOKEN").ok();
    let repository = client
        .get(format!("https://api.github.com/repos/{owner}/{name}"))
        .header("Accept", "application/vnd.github+json");
    let repository = match &token {
        Some(token) if !token.is_empty() => repository.bearer_auth(token),
        _ => repository,
    };
    let Ok(repository) = repository.send().await else {
        return gate_from_facts(None, None);
    };
    if !repository.status().is_success() {
        return gate_from_facts(None, None);
    }
    let Ok(repository) = repository.json::<serde_json::Value>().await else {
        return gate_from_facts(None, None);
    };
    let public = repository
        .get("private")
        .and_then(serde_json::Value::as_bool)
        .map(|private| !private);
    if public != Some(true) {
        return gate_from_facts(public, None);
    }

    let commit = client
        .get(format!(
            "https://api.github.com/repos/{owner}/{name}/commits/{sha}"
        ))
        .header("Accept", "application/vnd.github+json");
    let commit = match &token {
        Some(token) if !token.is_empty() => commit.bearer_auth(token),
        _ => commit,
    };
    match commit.send().await {
        Ok(response) if response.status().is_success() => gate_from_facts(public, Some(true)),
        Ok(response) if response.status() == reqwest::StatusCode::NOT_FOUND => {
            gate_from_facts(public, Some(false))
        }
        Ok(_) | Err(_) => gate_from_facts(public, None),
    }
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
            if valid_github_component(&owner) && valid_github_component(&name) {
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
            if valid_github_component(&owner) && valid_github_component(&name) {
                return Some((owner, name));
            }
        }
    }
    None
}

fn valid_commit_hash(value: &str) -> bool {
    (value.len() == 40 || value.len() == 64)
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn valid_github_component(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 100
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

/// Server secret used to key the HMAC that sanitizes file paths before storage.
/// Configure via FILES_TOUCHED_HMAC_SECRET; a fallback keeps dev/tests working.
fn files_hmac_secret(state: &AppState) -> &'static [u8] {
    static SECRET: std::sync::OnceLock<Vec<u8>> = std::sync::OnceLock::new();
    SECRET.get_or_init(|| {
        std::env::var("FILES_TOUCHED_HMAC_SECRET")
            .ok()
            .filter(|secret| !secret.is_empty())
            .map(String::into_bytes)
            .unwrap_or_else(|| {
                if state.auth.session_signing_key.is_empty() {
                    uuid::Uuid::new_v4().to_string().into_bytes()
                } else {
                    state.auth.session_signing_key.clone().into_bytes()
                }
            })
    })
}

/// HMAC-SHA256 built from the already-dependency `sha2` crate (no new dependency).
/// Returns the raw 32-byte digest.
fn hmac_sha256(key: &[u8], message: &[u8]) -> [u8; 32] {
    use sha2::Digest;
    const BLOCK: usize = 64;
    let mut k = [0u8; BLOCK];
    if key.len() > BLOCK {
        k.copy_from_slice(&sha2::Sha256::digest(key));
    } else {
        k[..key.len()].copy_from_slice(key);
    }
    let mut inner_key = k;
    let mut outer_key = k;
    for i in 0..BLOCK {
        inner_key[i] ^= 0x36;
        outer_key[i] ^= 0x5c;
    }
    let mut hasher = sha2::Sha256::new();
    hasher.update(inner_key);
    hasher.update(message);
    let inner = hasher.finalize();
    let mut hasher = sha2::Sha256::new();
    hasher.update(outer_key);
    hasher.update(inner);
    let mut out = [0u8; 32];
    out.copy_from_slice(&hasher.finalize());
    out
}

/// Pure bounds check for an episode's credit claims. Returns false (skip) when
/// any claim exceeds the i64 range or the elapsed session duration. Never
/// clamps: out-of-range claims are rejected outright.
fn episode_claims_within_bounds(episode: &LocalEpisode) -> bool {
    let started_seconds = episode.started_at.0 / 1_000;
    let ended_seconds = episode.ended_at.0 / 1_000;
    let elapsed = ended_seconds - started_seconds;
    if elapsed < 0 {
        return false;
    }
    let max_credit: u64 = u64::try_from(i64::MAX).ok().unwrap_or(0);
    let elapsed_u64: u64 = u64::try_from(elapsed).ok().unwrap_or(0);
    episode.human_edit_seconds <= max_credit
        && episode.agent_edit_seconds <= max_credit
        && episode.credited_seconds <= max_credit
        && episode.credited_seconds <= elapsed_u64
        && episode
            .human_edit_seconds
            .saturating_add(episode.agent_edit_seconds)
            <= elapsed_u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn github_gate_requires_public_repo_and_observed_head() {
        assert_eq!(
            gate_from_facts(Some(true), Some(true)).outcome_factor_permille,
            1000
        );
        for decision in [
            gate_from_facts(Some(false), Some(true)),
            gate_from_facts(Some(true), Some(false)),
            gate_from_facts(Some(true), None),
            gate_from_facts(None, None),
        ] {
            assert_eq!(decision.outcome_factor_permille, 0);
        }
    }

    #[test]
    fn commit_hashes_are_bounded_lowercase_hex() {
        assert!(valid_commit_hash(&"a".repeat(40)));
        assert!(valid_commit_hash(&"0123456789abcdef".repeat(4)));
        assert!(!valid_commit_hash(&"A".repeat(40)));
        assert!(!valid_commit_hash("not-a-sha"));
    }

    #[test]
    fn episode_claims_are_rejected_when_out_of_bounds() {
        let ep =
            |human: u64, agent: u64, credited: u64, started_ms: i64, ended_ms: i64| LocalEpisode {
                formula_version: 1,
                started_at: LocalTime(started_ms),
                ended_at: LocalTime(ended_ms),
                session: "s".into(),
                attempts: 1,
                distinct_signatures: vec![],
                files_touched: vec![],
                human_edit_seconds: human,
                agent_edit_seconds: agent,
                resolved: false,
                credited_seconds: credited,
                buckets: vec![],
            };
        let one_hour = 3600;
        // Within bounds: accepted.
        assert!(episode_claims_within_bounds(&ep(
            100,
            100,
            200,
            0,
            one_hour * 1000
        )));
        // Credited exceeds elapsed: rejected (never clamped).
        assert!(!episode_claims_within_bounds(&ep(
            100,
            100,
            4000,
            0,
            one_hour * 1000
        )));
        // Human + agent exceeds elapsed: rejected.
        assert!(!episode_claims_within_bounds(&ep(
            2000,
            2000,
            100,
            0,
            one_hour * 1000
        )));
        // i64 range overflow on human or credited: rejected.
        assert!(!episode_claims_within_bounds(&ep(
            u64::MAX,
            0,
            0,
            0,
            one_hour * 1000
        )));
        assert!(!episode_claims_within_bounds(&ep(
            0,
            0,
            u64::MAX,
            0,
            one_hour * 1000
        )));
        // Negative elapsed: rejected.
        assert!(!episode_claims_within_bounds(&ep(
            0,
            0,
            0,
            one_hour * 1000,
            0
        )));
    }
}
