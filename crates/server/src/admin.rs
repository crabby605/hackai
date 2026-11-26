#![forbid(unsafe_code)]

//! Admin session integrity review endpoint.
//! Shows real session time (deterministic validated-event elapsed) vs reported
//! session time plus their delta. Fraud-review signal only: never exposed on
//! public pages, never merged with scoring, never auto-punish.

use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::Json,
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use sqlx::Row;

use crate::AppState;

#[derive(Clone, Debug, Serialize)]
pub struct SessionReview {
    pub session_key: String,
    pub user_id: i64,
    pub username: String,
    pub day: String,
    pub real_seconds: f64,
    pub reported_seconds: Option<f64>,
    pub delta_seconds: Option<f64>,
    pub event_count: i64,
    pub status: &'static str,
    pub evidence: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct AdminUser {
    pub id: i64,
    pub username: String,
    pub is_admin: bool,
    pub display_name: Option<String>,
    pub email: Option<String>,
    pub slack_id: Option<String>,
    pub avatar_url: Option<String>,
    pub country_code: Option<String>,
    pub profile_public: bool,
    pub leaderboard_opt_out: bool,
    pub projects: i64,
    pub episodes: i64,
    pub credited_seconds: i64,
}

#[derive(Clone, Debug, Serialize)]
pub struct AdminProject {
    pub id: i64,
    pub name: String,
    pub total_seconds: i64,
    pub human_seconds: i64,
    pub agent_seconds: i64,
    pub last_active: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct AdminActivity {
    pub id: String,
    pub username: String,
    pub kind: String,
    pub occurred_at: String,
    pub payload: serde_json::Value,
    /// Attributed authorship of the event: human, agent, or unknown.
    pub authorship: Option<String>,
}

/// Advisory-only local-hook integrity evidence. It does not alter PESDO,
/// eligibility, or any user-facing score.
#[derive(Clone, Debug, Serialize)]
pub struct HarnessIntegrityReview {
    pub username: String,
    pub project_id: i64,
    pub harness: String,
    pub session_id: String,
    pub hook_events: i64,
    pub cadence_every: i64,
    pub cadence_remainder: i64,
    pub cadence_anomaly: bool,
    pub tampered_events: i64,
}

#[derive(Deserialize)]
pub struct AdminSessionsQuery {
    #[serde(default)]
    pub limit: Option<u32>,
}

#[derive(Deserialize)]
pub struct AdminUsersQuery {
    #[serde(default)]
    pub q: Option<String>,
    #[serde(default)]
    pub limit: Option<u32>,
}

#[derive(Deserialize)]
pub struct AdminActivityQuery {
    #[serde(default)]
    pub limit: Option<u32>,
}

#[derive(Deserialize)]
pub struct BanBody {
    #[serde(default)]
    pub until: Option<String>, // ISO-8601 timestamp; absent = indefinite ban
}

#[derive(Deserialize)]
pub struct AdminBody {
    pub is_admin: bool,
}

#[derive(Deserialize)]
pub struct HardwareBanBody {
    pub sha256_hash: String,
}

#[derive(Serialize)]
pub struct AdminInsights {
    pub users: i64,
    pub public_profiles: i64,
    pub projects: i64,
    pub episodes: i64,
    pub credited_seconds: i64,
    pub active_installations: i64,
    pub active_users_7d: i64,
    pub resolution_rate_permille: i64,
    pub failed_batches: i64,
    pub harnesses: Vec<serde_json::Value>,
    pub projects_list: Vec<serde_json::Value>,
    pub recently_active: Vec<serde_json::Value>,
    pub risky_cases: Vec<serde_json::Value>,
}

pub fn routes() -> axum::Router<AppState> {
    axum::Router::new()
        .route("/api/v1/admin/sessions", get(list_sessions))
        .route("/api/v1/admin/users", get(list_users))
        .route("/api/v1/admin/users/{id}", get(get_user_review))
        .route(
            "/api/v1/admin/users/{id}/understanding",
            get(user_understanding),
        )
        .route("/api/v1/admin/users/{id}/admin", post(set_admin))
        .route("/api/v1/admin/users/{id}/projects", get(user_projects))
        .route("/api/v1/admin/activity", get(list_activity))
        .route("/api/v1/admin/insights", get(insights))
        .route(
            "/api/v1/admin/harness-integrity",
            get(list_harness_integrity),
        )
        .route("/api/v1/admin/users/{id}/ban", post(ban_user))
        .route("/api/v1/admin/users/{id}/unban", post(unban_user))
        .route("/api/v1/admin/users/{id}/shadow-ban", post(shadow_ban_user))
        .route("/api/v1/admin/users/{id}/unshadow", post(unshadow_user))
        .route("/api/v1/admin/users/{id}/cooldown", post(set_cooldown))
        .route("/api/v1/admin/hardware-ban", post(ban_hardware))
}

/// Detailed derived-only collaboration profile for staff review. This is never
/// routed through owner-facing APIs: users receive coaching language from the
/// session-improvement endpoint instead of this internal classification view.
async fn user_understanding(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let _ = require_admin(&state, &headers).await?;
    let rows = sqlx::query(
        "SELECT t.episode_id, t.turn_index, t.event_id, t.user_act, \
                t.act_confidence_permille, t.new_information, t.prompt_length_bucket, \
                t.fullcaps, t.answers_question, t.agent_state, t.verification, \
                t.files_changed_bucket, t.tool_errors, t.reason_codes::TEXT AS reason_codes \
         FROM derived_session_transcript_turns t \
         JOIN productive_episodes e ON e.id = t.episode_id \
         WHERE e.user_id = $1 ORDER BY t.episode_id DESC, t.turn_index ASC LIMIT 1000",
    )
    .bind(id)
    .fetch_all(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let mut high_information = 0_i64;
    let mut verified_green = 0_i64;
    let mut verification_red = 0_i64;
    let mut recovery_loops = 0_i64;
    let turns: Vec<serde_json::Value> = rows
        .into_iter()
        .map(|row| {
            let information = row.try_get::<String, _>("new_information").unwrap_or_default();
            let agent_state = row.try_get::<String, _>("agent_state").unwrap_or_default();
            let verification = row.try_get::<String, _>("verification").unwrap_or_default();
            high_information += i64::from(information == "high");
            verified_green += i64::from(verification == "green");
            verification_red += i64::from(verification == "red");
            recovery_loops += i64::from(agent_state == "looping" || agent_state == "tool_error");
            serde_json::json!({
                "episode_id": row.try_get::<i64, _>("episode_id").unwrap_or_default().to_string(),
                "turn": row.try_get::<i32, _>("turn_index").unwrap_or_default() + 1,
                "event_id": row.try_get::<String, _>("event_id").unwrap_or_default(),
                "user_act": row.try_get::<String, _>("user_act").unwrap_or_default(),
                "act_confidence_permille": row.try_get::<i32, _>("act_confidence_permille").unwrap_or_default(),
                "new_information": information,
                "prompt_length_bucket": row.try_get::<i16, _>("prompt_length_bucket").unwrap_or_default(),
                "fullcaps": row.try_get::<bool, _>("fullcaps").unwrap_or(false),
                "answers_question": row.try_get::<bool, _>("answers_question").unwrap_or(false),
                "agent_state": agent_state,
                "verification": verification,
                "files_changed_bucket": row.try_get::<i16, _>("files_changed_bucket").unwrap_or_default(),
                "tool_errors": row.try_get::<i16, _>("tool_errors").unwrap_or_default(),
                "reason_codes": serde_json::from_str::<serde_json::Value>(&row.try_get::<String, _>("reason_codes").unwrap_or_else(|_| "[]".into())).unwrap_or_else(|_| serde_json::json!([])),
            })
        })
        .collect();
    Ok(Json(serde_json::json!({
        "user_id": id.to_string(),
        "understanding": {
            "turns_observed": turns.len(),
            "high_information_turns": high_information,
            "verified_green_turns": verified_green,
            "red_verification_turns": verification_red,
            "recovery_loop_or_tool_error_turns": recovery_loops,
            "scope": "derived classifications only; never raw prompt text, replies, code, paths, or tool output"
        },
        "turns": turns,
    })))
}

async fn insights(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<AdminInsights>, StatusCode> {
    let _ = require_admin(&state, &headers).await?;
    let users: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users")
        .fetch_one(&state.db)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let public_profiles: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM users WHERE profile_public = TRUE")
            .fetch_one(&state.db)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let projects: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM projects WHERE deleted_at IS NULL")
            .fetch_one(&state.db)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let episodes: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM productive_episodes")
        .fetch_one(&state.db)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let credited_seconds: i64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(credited_seconds * outcome_factor_permille / 1000), 0)::BIGINT FROM productive_episodes",
    )
    .fetch_one(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let active_installations: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM api_keys WHERE revoked_at IS NULL")
            .fetch_one(&state.db)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let active_users_7d: i64 = sqlx::query_scalar("SELECT COUNT(DISTINCT user_id) FROM productive_episodes WHERE started_at >= now() - interval '7 days'")
        .fetch_one(&state.db).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let resolved: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM productive_episodes WHERE resolved")
            .fetch_one(&state.db)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let resolution_rate_permille = if episodes > 0 {
        resolved * 1000 / episodes
    } else {
        0
    };
    let failed_batches: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM tampered_sync_requests")
        .fetch_one(&state.db)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let harness_rows = sqlx::query("SELECT harness, COUNT(*)::BIGINT AS sessions FROM prompt_events GROUP BY harness ORDER BY sessions DESC")
        .fetch_all(&state.db).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let harnesses = harness_rows
        .into_iter()
        .map(|row| {
            serde_json::json!({
                "harness": row.try_get::<String, _>("harness").unwrap_or_else(|_| "unknown".into()),
                "sessions": row.try_get::<i64, _>("sessions").unwrap_or(0),
            })
        })
        .collect();
    let project_rows = sqlx::query("SELECT p.id, p.name, p.owner_id, u.username, u.display_name, COUNT(e.id)::BIGINT AS episodes, COALESCE(SUM(e.credited_seconds * e.outcome_factor_permille / 1000), 0)::BIGINT AS credited_seconds, EXTRACT(EPOCH FROM MAX(e.ended_at))::BIGINT * 1000 AS last_active FROM projects p JOIN users u ON u.id = p.owner_id LEFT JOIN productive_episodes e ON e.project_id = p.id WHERE p.deleted_at IS NULL GROUP BY p.id, u.username, u.display_name ORDER BY MAX(e.ended_at) DESC NULLS LAST LIMIT 100")
        .fetch_all(&state.db).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let projects_list = project_rows.into_iter().map(|row| serde_json::json!({
        "id": row.try_get::<i64, _>("id").unwrap_or_default().to_string(),
        "name": row.try_get::<String, _>("name").unwrap_or_else(|_| "local-project".into()),
        "owner_id": row.try_get::<i64, _>("owner_id").unwrap_or_default().to_string(),
        "username": row.try_get::<String, _>("username").unwrap_or_else(|_| "unknown".into()),
        "display_name": row.try_get::<Option<String>, _>("display_name").unwrap_or(None),
        "episodes": row.try_get::<i64, _>("episodes").unwrap_or(0),
        "credited_seconds": row.try_get::<i64, _>("credited_seconds").unwrap_or(0),
        "last_active": row.try_get::<Option<i64>, _>("last_active").unwrap_or(None),
    })).collect();
    let active_rows = sqlx::query("SELECT u.id, u.username, u.display_name, EXTRACT(EPOCH FROM MAX(e.ended_at))::BIGINT * 1000 AS last_active, COALESCE(SUM(e.credited_seconds * e.outcome_factor_permille / 1000), 0)::BIGINT AS credited_seconds FROM users u JOIN productive_episodes e ON e.user_id = u.id GROUP BY u.id, u.username, u.display_name ORDER BY MAX(e.ended_at) DESC LIMIT 10")
        .fetch_all(&state.db).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let recently_active = active_rows
        .into_iter()
        .map(|row| {
            serde_json::json!({
                "id": row.try_get::<i64, _>("id").unwrap_or_default().to_string(),
                "username": row.try_get::<String, _>("username").unwrap_or_default(),
                "display_name": row.try_get::<Option<String>, _>("display_name").unwrap_or(None),
                "last_active": row.try_get::<Option<i64>, _>("last_active").unwrap_or(None),
                "credited_seconds": row.try_get::<i64, _>("credited_seconds").unwrap_or(0),
            })
        })
        .collect();
    let case_rows = sqlx::query("SELECT c.id, c.subject_user_id, u.username, u.display_name, c.priority, c.status, c.summary, EXTRACT(EPOCH FROM c.opened_at)::BIGINT * 1000 AS opened_at FROM review_cases c JOIN users u ON u.id = c.subject_user_id WHERE c.status IN ('open', 'hold', 'sample') ORDER BY c.priority DESC, c.opened_at ASC LIMIT 10")
        .fetch_all(&state.db).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let risky_cases = case_rows
        .into_iter()
        .map(|row| {
            serde_json::json!({
                "id": row.try_get::<i64, _>("id").unwrap_or_default().to_string(),
                "user_id": row.try_get::<i64, _>("subject_user_id").unwrap_or_default().to_string(),
                "username": row.try_get::<String, _>("username").unwrap_or_default(),
                "display_name": row.try_get::<Option<String>, _>("display_name").unwrap_or(None),
                "priority": row.try_get::<i64, _>("priority").unwrap_or(0),
                "status": row.try_get::<String, _>("status").unwrap_or_default(),
                "summary": row.try_get::<String, _>("summary").unwrap_or_default(),
                "opened_at": row.try_get::<Option<i64>, _>("opened_at").unwrap_or(None),
            })
        })
        .collect();
    Ok(Json(AdminInsights {
        users,
        public_profiles,
        projects,
        episodes,
        credited_seconds,
        active_installations,
        active_users_7d,
        resolution_rate_permille,
        failed_batches,
        harnesses,
        projects_list,
        recently_active,
        risky_cases,
    }))
}

async fn list_harness_integrity(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<AdminSessionsQuery>,
) -> Result<Json<Vec<HarnessIntegrityReview>>, StatusCode> {
    let _ = require_admin(&state, &headers).await?;
    let limit = query.limit.unwrap_or(100).clamp(1, 500) as i32;
    // A native hook heartbeat is expected on a five-turn cadence. A remainder
    // only raises an admin review signal: sessions can end mid-window.
    let rows = sqlx::query(
        "SELECT u.username, p.project_id, p.harness, p.session_id,
                COUNT(*) AS hook_events,
                COUNT(*) FILTER (WHERE p.tampered_request) AS tampered_events
         FROM prompt_events p
         JOIN users u ON u.id = p.user_id
         GROUP BY u.username, p.project_id, p.harness, p.session_id
         ORDER BY MAX(p.occurred_at) DESC
         LIMIT $1",
    )
    .bind(limit)
    .fetch_all(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let mut reviews = Vec::with_capacity(rows.len());
    for row in rows {
        let hook_events: i64 = row
            .try_get("hook_events")
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        reviews.push(HarnessIntegrityReview {
            username: row
                .try_get("username")
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
            project_id: row
                .try_get("project_id")
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
            harness: row
                .try_get("harness")
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
            session_id: row
                .try_get("session_id")
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
            hook_events,
            cadence_every: 5,
            cadence_remainder: hook_events.rem_euclid(5),
            cadence_anomaly: hook_events.rem_euclid(5) != 0,
            tampered_events: row
                .try_get("tampered_events")
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
        });
    }
    Ok(Json(reviews))
}

// ---- Admin moderation: ban / unban / cooldown ----

async fn require_admin(state: &AppState, headers: &HeaderMap) -> Result<i64, StatusCode> {
    let sub = crate::auth::extract_current_user(&state.auth, &state.db, headers)
        .await
        .ok_or(StatusCode::UNAUTHORIZED)?;

    let row = sqlx::query("SELECT id, is_admin FROM users WHERE auth_subject = $1")
        .bind(&sub)
        .fetch_optional(&state.db)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::UNAUTHORIZED)?;

    let is_admin: bool = row
        .try_get("is_admin")
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if !is_admin {
        return Err(StatusCode::FORBIDDEN);
    }
    row.try_get("id")
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

async fn audit_mutation(state: &AppState, admin_user_id: i64, action: &str, target_user_id: i64) {
    let _ = sqlx::query(
        "INSERT INTO admin_audit_log (admin_user_id, action, target_user_id) VALUES ($1, $2, $3)",
    )
    .bind(admin_user_id)
    .bind(action)
    .bind(target_user_id)
    .execute(&state.db)
    .await;
}

async fn set_admin(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Json(body): Json<AdminBody>,
) -> Result<Json<AdminUser>, StatusCode> {
    let admin_user_id = require_admin(&state, &headers).await?;
    let row = sqlx::query(
        "UPDATE users SET is_admin = $2 WHERE id = $1
         AND ($2 = TRUE OR is_admin = FALSE OR (SELECT COUNT(*) FROM users WHERE is_admin = TRUE) > 1)
         RETURNING id, username, is_admin, display_name, email, slack_id, avatar_url, country_code, profile_public, leaderboard_opt_out",
    )
    .bind(id)
    .bind(body.is_admin)
    .fetch_optional(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    .ok_or(StatusCode::NOT_FOUND)?;
    audit_mutation(
        &state,
        admin_user_id,
        if body.is_admin {
            "grant_admin"
        } else {
            "revoke_admin"
        },
        id,
    )
    .await;
    Ok(Json(AdminUser {
        id: row
            .try_get("id")
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
        username: row
            .try_get("username")
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
        is_admin: row
            .try_get("is_admin")
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
        display_name: row.try_get("display_name").ok(),
        email: row.try_get("email").ok(),
        slack_id: row.try_get("slack_id").ok(),
        avatar_url: row.try_get("avatar_url").ok(),
        country_code: row.try_get("country_code").ok(),
        profile_public: row.try_get("profile_public").unwrap_or(true),
        leaderboard_opt_out: row.try_get("leaderboard_opt_out").unwrap_or(false),
        projects: 0,
        episodes: 0,
        credited_seconds: 0,
    }))
}

async fn ban_user(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Json(body): Json<BanBody>,
) -> Result<StatusCode, StatusCode> {
    let admin_user_id = require_admin(&state, &headers).await?;

    let until = body
        .until
        .as_deref()
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());

    let row = sqlx::query(
        "UPDATE users \
         SET banned = TRUE, banned_until = $2 \
         WHERE id = $1 AND is_admin = FALSE \
         RETURNING id",
    )
    .bind(id)
    .bind(until)
    .fetch_optional(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    if row.is_none() {
        return Err(StatusCode::CONFLICT);
    }

    audit_mutation(&state, admin_user_id, "ban", id).await;

    Ok(StatusCode::NO_CONTENT)
}

async fn unban_user(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> Result<StatusCode, StatusCode> {
    let admin_user_id = require_admin(&state, &headers).await?;

    let row = sqlx::query(
        "UPDATE users \
         SET banned = FALSE, banned_until = NULL \
         WHERE id = $1 AND is_admin = FALSE \
         RETURNING id",
    )
    .bind(id)
    .fetch_optional(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    if row.is_none() {
        return Err(StatusCode::CONFLICT);
    }

    audit_mutation(&state, admin_user_id, "unban", id).await;

    Ok(StatusCode::NO_CONTENT)
}

async fn shadow_ban_user(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> Result<StatusCode, StatusCode> {
    let admin_user_id = require_admin(&state, &headers).await?;
    let row = sqlx::query(
        "UPDATE users SET shadow_banned = TRUE WHERE id = $1 AND is_admin = FALSE RETURNING id",
    )
    .bind(id)
    .fetch_optional(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if row.is_none() {
        return Err(StatusCode::CONFLICT);
    }
    audit_mutation(&state, admin_user_id, "shadow_ban", id).await;
    Ok(StatusCode::NO_CONTENT)
}

async fn unshadow_user(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> Result<StatusCode, StatusCode> {
    let admin_user_id = require_admin(&state, &headers).await?;
    let row = sqlx::query(
        "UPDATE users SET shadow_banned = FALSE WHERE id = $1 AND is_admin = FALSE RETURNING id",
    )
    .bind(id)
    .fetch_optional(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if row.is_none() {
        return Err(StatusCode::CONFLICT);
    }
    audit_mutation(&state, admin_user_id, "unshadow", id).await;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
pub struct CooldownBody {
    until: String, // ISO-8601 timestamp
}

async fn set_cooldown(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Json(body): Json<CooldownBody>,
) -> Result<StatusCode, StatusCode> {
    let admin_user_id = require_admin(&state, &headers).await?;

    let row = sqlx::query(
        "UPDATE users \
         SET cooldown_until = $2 \
         WHERE id = $1 AND is_admin = FALSE \
         RETURNING id",
    )
    .bind(id)
    .bind(body.until)
    .fetch_optional(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    if row.is_none() {
        return Err(StatusCode::CONFLICT);
    }

    audit_mutation(&state, admin_user_id, "set_cooldown", id).await;

    Ok(StatusCode::NO_CONTENT)
}

async fn ban_hardware(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<HardwareBanBody>,
) -> Result<StatusCode, StatusCode> {
    let admin_user_id = require_admin(&state, &headers).await?;
    let hash = body.sha256_hash.to_ascii_lowercase();
    if hash.len() != 64 || !hash.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(StatusCode::UNPROCESSABLE_ENTITY);
    }
    sqlx::query("INSERT INTO hardware_bans (sha256_hash, banned_by) VALUES ($1, $2) ON CONFLICT (sha256_hash) DO NOTHING")
        .bind(&hash)
        .bind(admin_user_id)
        .execute(&state.db)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    // Never hardware-ban an admin; lockouts must be reversible by another admin.
    sqlx::query(
        "UPDATE users SET banned = TRUE, banned_until = NULL \
             WHERE id IN (SELECT user_id FROM hardware_fingerprints WHERE sha256_hash = $1) \
               AND is_admin = FALSE",
    )
    .bind(&hash)
    .execute(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    audit_mutation(&state, admin_user_id, "ban_hardware", admin_user_id).await;
    Ok(StatusCode::NO_CONTENT)
}

async fn list_sessions(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<AdminSessionsQuery>,
) -> Result<Json<Vec<SessionReview>>, StatusCode> {
    let sub = crate::auth::extract_current_user(&state.auth, &state.db, &headers)
        .await
        .ok_or(StatusCode::UNAUTHORIZED)?;

    // Verify admin role server-side — never trust client flags.
    let row = sqlx::query("SELECT is_admin FROM users WHERE auth_subject = $1")
        .bind(&sub)
        .fetch_optional(&state.db)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::UNAUTHORIZED)?;

    let is_admin: bool = row
        .try_get("is_admin")
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if !is_admin {
        return Err(StatusCode::FORBIDDEN);
    }

    let limit = query.limit.unwrap_or(50).clamp(1, 500) as i32;

    // Real session time = elapsed between first and last validated event per
    // user per day. Reported time = sum of heartbeat `time` values. Delta is a
    // fraud-review signal only.
    let rows = sqlx::query(
        "WITH event_sessions AS (
            SELECT
                user_id,
                to_char(DATE(occurred_at), 'YYYY-MM-DD') AS day,
                MIN(occurred_at) AS start_time,
                MAX(occurred_at) AS end_time,
                COUNT(*) AS event_count
            FROM events
            GROUP BY user_id, DATE(occurred_at)
        ),
        reported AS (
            SELECT
                user_id,
                to_char(DATE(to_timestamp(time)), 'YYYY-MM-DD') AS day,
                MAX(time) - MIN(time) AS reported_seconds
            FROM heartbeats
            GROUP BY user_id, DATE(to_timestamp(time))
        )
        SELECT
            e.user_id,
            u.username,
            e.day,
            EXTRACT(EPOCH FROM (e.end_time - e.start_time)) AS real_seconds,
            r.reported_seconds,
            (EXTRACT(EPOCH FROM (e.end_time - e.start_time)) - COALESCE(r.reported_seconds, 0)) AS delta_seconds,
            e.event_count
        FROM event_sessions e
        LEFT JOIN reported r
            ON e.user_id = r.user_id AND e.day = r.day
        JOIN users u ON u.id = e.user_id
        ORDER BY e.day DESC, e.user_id
        LIMIT $1",
    )
    .bind(limit)
    .fetch_all(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let mut entries = Vec::with_capacity(rows.len());
    for row in rows {
        entries.push(SessionReview {
            session_key: format!(
                "{}:{}",
                row.try_get::<i64, _>("user_id")
                    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
                row.try_get::<String, _>("day")
                    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
            ),
            user_id: row
                .try_get("user_id")
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
            username: row
                .try_get("username")
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
            day: row
                .try_get("day")
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
            real_seconds: row
                .try_get("real_seconds")
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
            reported_seconds: row.try_get("reported_seconds").ok(),
            delta_seconds: row.try_get("delta_seconds").ok(),
            event_count: row
                .try_get("event_count")
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
            status: "unreviewed",
            evidence: format!(
                "/profile/{}",
                row.try_get::<String, _>("username")
                    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
            ),
        });
    }

    Ok(Json(entries))
}

// ---- Admin user search ----

async fn list_users(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<AdminUsersQuery>,
) -> Result<Json<Vec<AdminUser>>, StatusCode> {
    let sub = crate::auth::extract_current_user(&state.auth, &state.db, &headers)
        .await
        .ok_or(StatusCode::UNAUTHORIZED)?;

    // Verify admin role server-side — never trust client flags.
    let row = sqlx::query("SELECT is_admin FROM users WHERE auth_subject = $1")
        .bind(&sub)
        .fetch_optional(&state.db)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::UNAUTHORIZED)?;

    let is_admin: bool = row
        .try_get("is_admin")
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if !is_admin {
        return Err(StatusCode::FORBIDDEN);
    }

    let limit = query.limit.unwrap_or(50).clamp(1, 500) as i32;
    let q = query
        .q
        .map(|v| format!("%{}%", v))
        .unwrap_or("%".to_string());

    // Projects and episodes are aggregated in separate CTEs to avoid the
    // project x episode fanout that joining both to users in one query would
    // cause (COUNT/SUM over the Cartesian product would over-count).
    let rows = sqlx::query(
        "WITH pc AS (
             SELECT owner_id, COUNT(*)::BIGINT AS projects
             FROM projects WHERE deleted_at IS NULL GROUP BY owner_id
         ),
         ep AS (
             SELECT user_id,
                    COUNT(*)::BIGINT AS episodes,
                    COALESCE(SUM(credited_seconds * outcome_factor_permille / 1000), 0)::BIGINT AS credited_seconds
             FROM productive_episodes GROUP BY user_id
         )
         SELECT u.id, u.username, u.is_admin, u.display_name, u.email, u.slack_id, u.avatar_url, \
                 u.country_code, u.profile_public, u.leaderboard_opt_out, \
                 COALESCE(pc.projects, 0) AS projects, \
                 COALESCE(ep.episodes, 0) AS episodes, \
                 COALESCE(ep.credited_seconds, 0) AS credited_seconds \
         FROM users u \
         LEFT JOIN pc ON pc.owner_id = u.id \
         LEFT JOIN ep ON ep.user_id = u.id \
         WHERE u.username ILIKE $1 OR u.display_name ILIKE $1 \
         ORDER BY u.username ASC \
         LIMIT $2",
    )
    .bind(q)
    .bind(limit)
    .fetch_all(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        out.push(AdminUser {
            id: row
                .try_get("id")
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
            username: row
                .try_get("username")
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
            is_admin: row
                .try_get("is_admin")
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
            display_name: row.try_get("display_name").ok(),
            email: row.try_get("email").ok(),
            slack_id: row.try_get("slack_id").ok(),
            avatar_url: row.try_get("avatar_url").ok(),
            country_code: row.try_get("country_code").ok(),
            profile_public: row.try_get("profile_public").unwrap_or(true),
            leaderboard_opt_out: row.try_get("leaderboard_opt_out").unwrap_or(false),
            projects: row.try_get("projects").unwrap_or(0),
            episodes: row.try_get("episodes").unwrap_or(0),
            credited_seconds: row.try_get("credited_seconds").unwrap_or(0),
        });
    }
    Ok(Json(out))
}

async fn get_user_review(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let _ = require_admin(&state, &headers).await?;

    let user_row = sqlx::query(
        "SELECT username, display_name, avatar_url, country_code, profile_public \
         FROM users WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    .ok_or(StatusCode::NOT_FOUND)?;

    let username: String = user_row
        .try_get("username")
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let display_name: Option<String> = user_row.try_get("display_name").ok();
    let avatar_url: Option<String> = user_row.try_get("avatar_url").ok();
    let country_code: Option<String> = user_row.try_get("country_code").ok();
    let profile_public: bool = user_row.try_get("profile_public").unwrap_or(true);

    let project_rows = sqlx::query(
        "SELECT p.id, p.name, r.remote_url, r.public, r.head_sha, \
                COUNT(e.id)::BIGINT AS episodes, \
                COALESCE(SUM(e.credited_seconds * e.outcome_factor_permille / 1000), 0)::BIGINT AS credited_seconds \
         FROM projects p \
         LEFT JOIN repositories r ON r.project_id = p.id \
         LEFT JOIN productive_episodes e ON e.project_id = p.id \
         WHERE p.owner_id = $1 AND p.deleted_at IS NULL \
         GROUP BY p.id, p.name, r.remote_url, r.public, r.head_sha \
         ORDER BY credited_seconds DESC, p.name ASC",
    )
    .bind(id)
    .fetch_all(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let projects: Vec<serde_json::Value> = project_rows
        .into_iter()
        .map(|r| {
            serde_json::json!({
                "id": r.try_get::<i64, _>("id").unwrap_or_default().to_string(),
                "name": r.try_get::<String, _>("name").unwrap_or_default(),
                "remote_url": r.try_get::<Option<String>, _>("remote_url").unwrap_or(None),
                "github_public": r.try_get::<Option<bool>, _>("public").unwrap_or(None),
                "head_sha": r.try_get::<Option<String>, _>("head_sha").unwrap_or(None),
                "episodes": r.try_get::<i64, _>("episodes").unwrap_or(0),
                "credited_seconds": r.try_get::<i64, _>("credited_seconds").unwrap_or(0),
            })
        })
        .collect();

    let episode_rows = sqlx::query(
        "SELECT id, COALESCE(batch_id, '') AS batch_id, project_id, session_key, harness, \
                EXTRACT(EPOCH FROM started_at)::BIGINT * 1000 AS started_at, \
                EXTRACT(EPOCH FROM ended_at)::BIGINT * 1000 AS ended_at, \
                attempts, CASE WHEN resolved THEN 1 ELSE 0 END AS resolved, \
                credited_seconds, outcome_factor_permille, human_edit_seconds, agent_edit_seconds, \
                signatures::TEXT AS signatures_json, \
                jsonb_array_length(files_touched)::BIGINT AS files_touched_count, \
                evidence_ids::TEXT AS evidence_ids_json, \
                formula_version, \
                'archived' AS batch_status, \
                0::BIGINT AS batch_bytes, \
                EXTRACT(EPOCH FROM created_at)::BIGINT * 1000 AS batch_received_at \
         FROM productive_episodes \
         WHERE user_id = $1 \
         ORDER BY started_at DESC \
         LIMIT 100",
    )
    .bind(id)
    .fetch_all(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let episodes: Vec<serde_json::Value> = episode_rows
        .into_iter()
        .map(|r| {
            serde_json::json!({
                "id": r.try_get::<i64, _>("id").unwrap_or_default().to_string(),
                "batch_id": r.try_get::<String, _>("batch_id").unwrap_or_default(),
                "project_id": r.try_get::<i64, _>("project_id").unwrap_or_default().to_string(),
                "session_key": r.try_get::<String, _>("session_key").unwrap_or_default(),
                "harness": r.try_get::<Option<String>, _>("harness").unwrap_or(None),
                "started_at": r.try_get::<i64, _>("started_at").unwrap_or(0),
                "ended_at": r.try_get::<i64, _>("ended_at").unwrap_or(0),
                "attempts": r.try_get::<i32, _>("attempts").unwrap_or(1),
                "resolved": r.try_get::<i32, _>("resolved").unwrap_or(0),
                "claimed_seconds": r.try_get::<i64, _>("credited_seconds").unwrap_or(0),
                "credited_seconds": r.try_get::<i64, _>("credited_seconds").unwrap_or(0)
                    * r.try_get::<i32, _>("outcome_factor_permille").unwrap_or(1000) as i64 / 1000,
                "outcome_factor_permille": r.try_get::<i32, _>("outcome_factor_permille").unwrap_or(1000),
                "human_edit_seconds": r.try_get::<i64, _>("human_edit_seconds").unwrap_or(0),
                "agent_edit_seconds": r.try_get::<i64, _>("agent_edit_seconds").unwrap_or(0),
                "signatures_json": r.try_get::<String, _>("signatures_json").unwrap_or_else(|_| "[]".into()),
                "files_touched_count": r
                    .try_get::<i64, _>("files_touched_count")
                    .unwrap_or(0),
                "evidence_ids_json": r.try_get::<String, _>("evidence_ids_json").unwrap_or_else(|_| "[]".into()),
                "formula_version": r.try_get::<i32, _>("formula_version").unwrap_or(1),
                "batch_status": r.try_get::<String, _>("batch_status").unwrap_or_else(|_| "archived".into()),
                "batch_bytes": r.try_get::<i64, _>("batch_bytes").unwrap_or(0),
                "batch_received_at": r.try_get::<i64, _>("batch_received_at").unwrap_or(0),
            })
        })
        .collect();

    let commit_rows = sqlx::query(
        "SELECT c.sha, c.tree_hash, c.files_changed, c.lines_added, c.lines_deleted, \
                EXTRACT(EPOCH FROM c.committed_at)::BIGINT * 1000 AS committed_at, \
                EXTRACT(EPOCH FROM c.github_observed_at)::BIGINT * 1000 AS github_observed_at, \
                r.remote_owner, r.remote_name, r.public, \
                s.outcome_factor_permille, s.github_gate_reason, \
                EXTRACT(EPOCH FROM s.submitted_at)::BIGINT * 1000 AS submitted_at \
         FROM commits c \
         JOIN repositories r ON r.id = c.repository_id \
         LEFT JOIN submissions s ON s.commit_sha = c.sha AND s.user_id = c.user_id AND s.project_id = r.project_id \
         WHERE c.user_id = $1 \
         ORDER BY c.committed_at DESC NULLS LAST, c.sha DESC \
         LIMIT 100",
    )
    .bind(id)
    .fetch_all(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let commits: Vec<serde_json::Value> = commit_rows
        .into_iter()
        .map(|row| {
            let owner = row.try_get::<String, _>("remote_owner").unwrap_or_default();
            let name = row.try_get::<String, _>("remote_name").unwrap_or_default();
            let sha = row.try_get::<String, _>("sha").unwrap_or_default();
            serde_json::json!({
                "sha": sha,
                "tree_hash": row.try_get::<String, _>("tree_hash").unwrap_or_default(),
                "repository": format!("{owner}/{name}"),
                "github_url": format!("https://github.com/{owner}/{name}/commit/{sha}"),
                "github_public": row.try_get::<Option<bool>, _>("public").unwrap_or(None),
                "claimed_committed_at": row.try_get::<Option<i64>, _>("committed_at").unwrap_or(None),
                "github_observed_at": row.try_get::<Option<i64>, _>("github_observed_at").unwrap_or(None),
                "submitted_at": row.try_get::<Option<i64>, _>("submitted_at").unwrap_or(None),
                "files_changed": row.try_get::<i32, _>("files_changed").unwrap_or(0),
                "lines_added": row.try_get::<i32, _>("lines_added").unwrap_or(0),
                "lines_deleted": row.try_get::<i32, _>("lines_deleted").unwrap_or(0),
                "outcome_factor_permille": row.try_get::<Option<i32>, _>("outcome_factor_permille").unwrap_or(None),
                "github_gate_reason": row.try_get::<Option<String>, _>("github_gate_reason").unwrap_or(None),
            })
        })
        .collect();

    let installation_rows =
        sqlx::query("SELECT id FROM api_keys WHERE user_id = $1 AND revoked_at IS NULL")
            .bind(id)
            .fetch_all(&state.db)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let installations: Vec<serde_json::Value> = installation_rows
        .into_iter()
        .map(|r| {
            serde_json::json!({
                "id": r.try_get::<i64, _>("id").unwrap_or_default().to_string(),
            })
        })
        .collect();

    let hardware_rows = sqlx::query(
        "SELECT kind, sha256_hash FROM hardware_fingerprints WHERE user_id = $1 ORDER BY kind, last_seen_at DESC",
    )
    .bind(id)
    .fetch_all(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let hardware_hashes: Vec<serde_json::Value> = hardware_rows
        .into_iter()
        .map(|row| {
            serde_json::json!({
                "kind": row.try_get::<String, _>("kind").unwrap_or_default(),
            "sha256_hash": row.try_get::<String, _>("sha256_hash").unwrap_or_default(),
            })
        })
        .collect();

    Ok(Json(serde_json::json!({
        "username": username,
        "display_name": display_name,
        "avatar_url": avatar_url,
        "country_code": country_code,
        "profile_public": profile_public,
        "projects": projects,
        "episodes": episodes,
        "commits": commits,
        "installations": installations,
        "hardware_hashes": hardware_hashes,
    })))
}

// ---- Admin: projects for a specific user ----

async fn user_projects(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> Result<Json<Vec<AdminProject>>, StatusCode> {
    let sub = crate::auth::extract_current_user(&state.auth, &state.db, &headers)
        .await
        .ok_or(StatusCode::UNAUTHORIZED)?;

    let row = sqlx::query("SELECT is_admin FROM users WHERE auth_subject = $1")
        .bind(&sub)
        .fetch_optional(&state.db)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::UNAUTHORIZED)?;

    let is_admin: bool = row
        .try_get("is_admin")
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if !is_admin {
        return Err(StatusCode::FORBIDDEN);
    }

    let rows = sqlx::query(
        "SELECT p.id, p.name, \
                 COALESCE(SUM(r.credited_seconds), 0)::BIGINT AS total_seconds, \
                 COALESCE(SUM(r.human_seconds), 0)::BIGINT AS human_seconds, \
                 COALESCE(SUM(r.agent_seconds), 0)::BIGINT AS agent_seconds, \
                 to_char(MAX(r.day), 'YYYY-MM-DD') AS last_active \
          FROM projects p \
          LEFT JOIN rollups r ON r.project_id = p.id AND r.user_id = $1 \
          WHERE p.owner_id = $1 \
          GROUP BY p.id, p.name \
          ORDER BY total_seconds DESC, p.name ASC",
    )
    .bind(id)
    .fetch_all(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        out.push(AdminProject {
            id: row
                .try_get("id")
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
            name: row
                .try_get("name")
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
            total_seconds: row
                .try_get::<i64, _>("total_seconds")
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
            human_seconds: row
                .try_get::<i64, _>("human_seconds")
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
            agent_seconds: row
                .try_get::<i64, _>("agent_seconds")
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
            last_active: row
                .try_get::<Option<String>, _>("last_active")
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
        });
    }
    Ok(Json(out))
}

// ---- Admin: activity log (recent events) ----

async fn list_activity(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<AdminActivityQuery>,
) -> Result<Json<Vec<AdminActivity>>, StatusCode> {
    let sub = crate::auth::extract_current_user(&state.auth, &state.db, &headers)
        .await
        .ok_or(StatusCode::UNAUTHORIZED)?;

    let row = sqlx::query("SELECT is_admin FROM users WHERE auth_subject = $1")
        .bind(&sub)
        .fetch_optional(&state.db)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::UNAUTHORIZED)?;

    let is_admin: bool = row
        .try_get("is_admin")
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if !is_admin {
        return Err(StatusCode::FORBIDDEN);
    }

    let limit = query.limit.unwrap_or(100).clamp(1, 1000) as i32;

    let rows = sqlx::query(
        "SELECT e.id, u.username, (e.payload->>'kind') AS kind, \
                 to_char(e.occurred_at, 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"') AS occurred_at, \
                 e.payload \
          FROM events e \
          JOIN users u ON u.id = e.user_id \
          ORDER BY e.occurred_at DESC \
          LIMIT $1",
    )
    .bind(limit)
    .fetch_all(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        let payload: serde_json::Value = row
            .try_get("payload")
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        let authorship = payload
            .get("role")
            .and_then(serde_json::Value::as_str)
            .map(|role| match role {
                "Human" => "human".to_string(),
                "Agent" => "agent".to_string(),
                _ => "unknown".to_string(),
            });
        out.push(AdminActivity {
            id: row
                .try_get("id")
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
            username: row
                .try_get("username")
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
            kind: row
                .try_get("kind")
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
            occurred_at: row
                .try_get("occurred_at")
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
            payload,
            authorship,
        });
    }
    Ok(Json(out))
}
