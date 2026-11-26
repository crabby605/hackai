#![forbid(unsafe_code)]

//! Public-facing routes: health, leaderboard, and public user profiles.
//! These require no session cookie.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::Json,
    routing::get,
};
use serde::Serialize;
use sqlx::Row;
use std::sync::OnceLock;
use std::time::Instant;

use crate::AppState;

static START_TIME: OnceLock<Instant> = OnceLock::new();

fn started() -> Instant {
    *START_TIME.get_or_init(Instant::now)
}

#[derive(Serialize)]
pub struct HealthStatus {
    pub status: String,
    pub version: String,
    pub uptime_seconds: u64,
    pub components: serde_json::Value,
}

pub async fn health(State(state): State<AppState>) -> Result<Json<HealthStatus>, StatusCode> {
    let db_ok = sqlx::query("SELECT 1").fetch_one(&state.db).await.is_ok();
    let storage = if db_ok { "operational" } else { "degraded" };
    let components = serde_json::json!({
        "web": { "status": "operational" },
        "auth": { "status": if db_ok { "operational" } else { "degraded" } },
        "storage": { "status": storage },
        "beats": { "status": "operational" },
    });
    Ok(Json(HealthStatus {
        status: if db_ok { "operational" } else { "degraded" }.into(),
        version: env!("CARGO_PKG_VERSION").into(),
        uptime_seconds: started().elapsed().as_secs(),
        components,
    }))
}

/// A deliberately unserious diagnostics endpoint.
pub async fn teapot() -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::IM_A_TEAPOT,
        Json(serde_json::json!({"message": "I'm a teapot"})),
    )
}

/// Public profile visible without auth. Only returned when profile_public = TRUE.
#[derive(Serialize)]
pub struct PublicProfile {
    pub username: String,
    pub is_admin: bool,
    pub display_name: Option<String>,
    pub avatar_url: Option<String>,
    pub country_code: Option<String>,
}

pub async fn public_profile(
    State(state): State<AppState>,
    Path(username): Path<String>,
) -> Result<Json<PublicProfile>, StatusCode> {
    let row = sqlx::query(
        "SELECT username, is_admin, display_name, avatar_url, country_code \
         FROM users WHERE username = $1 AND profile_public = TRUE",
    )
    .bind(username)
    .fetch_optional(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    .ok_or(StatusCode::NOT_FOUND)?;

    Ok(Json(PublicProfile {
        username: row
            .try_get("username")
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
        is_admin: row.try_get("is_admin").unwrap_or(false),
        display_name: row.try_get("display_name").ok(),
        avatar_url: row.try_get("avatar_url").ok(),
        country_code: row.try_get("country_code").ok(),
    }))
}

/// Leaderboard entry, public and opt-in only.
#[derive(Serialize)]
pub struct LeaderboardEntry {
    pub rank: i64,
    pub username: String,
    pub display_name: Option<String>,
    pub country_code: Option<String>,
    pub credited_seconds: i64,
    pub projects: i64,
}

pub async fn leaderboard(
    State(state): State<AppState>,
) -> Result<Json<Vec<LeaderboardEntry>>, StatusCode> {
    // Opt-in: public profile and not opted out of the leaderboard. Credited
    // seconds are gated (credited_seconds * outcome_factor_permille / 1000).
    // Projects and episodes are aggregated in separate CTEs to avoid the
    // project x episode fanout that multiplying joined rows would cause.
    let rows = sqlx::query(
        "WITH ep AS (
             SELECT user_id,
                    COALESCE(SUM(credited_seconds * outcome_factor_permille / 1000), 0)::BIGINT AS credited_seconds
             FROM productive_episodes GROUP BY user_id
         ),
         pc AS (
             SELECT owner_id, COUNT(*)::BIGINT AS projects
             FROM projects WHERE deleted_at IS NULL GROUP BY owner_id
         )
         SELECT ROW_NUMBER() OVER (ORDER BY COALESCE(ep.credited_seconds, 0) DESC, u.username ASC) AS rank,
                u.username, u.display_name, u.country_code,
                COALESCE(ep.credited_seconds, 0) AS credited_seconds,
                COALESCE(pc.projects, 0) AS projects
         FROM users u
         LEFT JOIN ep ON ep.user_id = u.id
         LEFT JOIN pc ON pc.owner_id = u.id
         WHERE u.profile_public = TRUE
           AND u.leaderboard_opt_out = FALSE
           AND u.shadow_banned = FALSE
         ORDER BY credited_seconds DESC, u.username ASC
         LIMIT 100",
    )
    .fetch_all(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        out.push(LeaderboardEntry {
            rank: row
                .try_get::<i64, _>("rank")
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
            username: row
                .try_get("username")
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
            display_name: row.try_get("display_name").ok(),
            country_code: row.try_get("country_code").ok(),
            credited_seconds: row
                .try_get::<i64, _>("credited_seconds")
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
            projects: row
                .try_get::<i64, _>("projects")
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
        });
    }
    Ok(Json(out))
}

pub fn routes() -> axum::Router<AppState> {
    axum::Router::new()
        .route("/api/v1/health", get(health))
        .route("/api/v2/teapot", get(teapot))
        .route("/api/v2/coffee", get(teapot))
        .route("/api/v2/kettle", get(teapot))
        .route("/api/v1/leaderboard", get(leaderboard))
        .route("/api/v1/users/{username}", get(public_profile))
}
