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

pub fn routes() -> axum::Router<AppState> {
    axum::Router::new()
        .route("/api/v1/admin/sessions", get(list_sessions))
        .route("/api/v1/admin/users", get(list_users))
        .route("/api/v1/admin/users/{id}/admin", post(set_admin))
        .route("/api/v1/admin/users/{id}/projects", get(user_projects))
        .route("/api/v1/admin/activity", get(list_activity))
        .route(
            "/api/v1/admin/harness-integrity",
            get(list_harness_integrity),
        )
        .route("/api/v1/admin/users/{id}/ban", post(ban_user))
        .route("/api/v1/admin/users/{id}/unban", post(unban_user))
        .route("/api/v1/admin/users/{id}/cooldown", post(set_cooldown))
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
    Ok(0) // placeholder; caller doesn't need the value
}

async fn set_admin(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Json(body): Json<AdminBody>,
) -> Result<Json<AdminUser>, StatusCode> {
    let _ = require_admin(&state, &headers).await?;
    let row = sqlx::query(
        "UPDATE users SET is_admin = $2 WHERE id = $1
         RETURNING id, username, is_admin, display_name, email, slack_id, avatar_url",
    )
    .bind(id)
    .bind(body.is_admin)
    .fetch_optional(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    .ok_or(StatusCode::NOT_FOUND)?;
    Ok(Json(AdminUser {
        id: row.try_get("id").map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
        username: row.try_get("username").map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
        is_admin: row.try_get("is_admin").map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
        display_name: row.try_get("display_name").ok(),
        email: row.try_get("email").ok(),
        slack_id: row.try_get("slack_id").ok(),
        avatar_url: row.try_get("avatar_url").ok(),
    }))
}

async fn ban_user(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Json(body): Json<BanBody>,
) -> Result<StatusCode, StatusCode> {
    let _ = require_admin(&state, &headers).await?;

    let until = body
        .until
        .as_deref()
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());

    let row = sqlx::query(
        "UPDATE users \
         SET banned = TRUE, banned_until = $2 \
         WHERE id = $1 \
         RETURNING id",
    )
    .bind(id)
    .bind(until)
    .fetch_optional(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    if row.is_none() {
        return Err(StatusCode::NOT_FOUND);
    }

    Ok(StatusCode::NO_CONTENT)
}

async fn unban_user(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> Result<StatusCode, StatusCode> {
    let _ = require_admin(&state, &headers).await?;

    let row = sqlx::query(
        "UPDATE users \
         SET banned = FALSE, banned_until = NULL \
         WHERE id = $1 \
         RETURNING id",
    )
    .bind(id)
    .fetch_optional(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    if row.is_none() {
        return Err(StatusCode::NOT_FOUND);
    }

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
    let _ = require_admin(&state, &headers).await?;

    let row = sqlx::query(
        "UPDATE users \
         SET cooldown_until = $2 \
         WHERE id = $1 \
         RETURNING id",
    )
    .bind(id)
    .bind(body.until)
    .fetch_optional(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    if row.is_none() {
        return Err(StatusCode::NOT_FOUND);
    }

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

    let rows = sqlx::query(
        "SELECT id, username, is_admin, display_name, email, slack_id, avatar_url \
         FROM users \
         WHERE username ILIKE $1 OR display_name ILIKE $1 \
         ORDER BY username ASC \
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
            display_name: row
                .try_get("display_name")
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
            email: row.try_get("email").ok(),
            slack_id: row.try_get("slack_id").ok(),
            avatar_url: row.try_get("avatar_url").ok(),
        });
    }
    Ok(Json(out))
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
                 COALESCE(SUM(r.credited_seconds), 0) AS total_seconds, \
                 COALESCE(SUM(r.human_seconds), 0) AS human_seconds, \
                 COALESCE(SUM(r.agent_seconds), 0) AS agent_seconds, \
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
            payload: row
                .try_get("payload")
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
        });
    }
    Ok(Json(out))
}
