#![forbid(unsafe_code)]

mod admin;
mod annotations;
mod archives;
mod auth;
mod cli;
mod ingest;
mod rollups;
mod sse;
mod sync;

use axum::{
    BoxError, Json, Router,
    error_handling::HandleErrorLayer,
    extract::{DefaultBodyLimit, Path, State},
    http::{HeaderMap, HeaderName, StatusCode},
    routing::{delete, get, post},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Row};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tower::{
    ServiceBuilder, limit::ConcurrencyLimitLayer, load_shed::LoadShedLayer, timeout::TimeoutLayer,
};
use tower_http::{
    catch_panic::CatchPanicLayer,
    cors::{AllowOrigin, CorsLayer},
    request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer},
    trace::TraceLayer,
};

#[derive(Clone)]
pub struct AppState {
    pub auth: Arc<auth::AuthState>,
    pub db: PgPool,
    pub sse_hub: sse::SseHub,
    pub username_bloom: Arc<RwLock<UsernameBloom>>,
}

const USERNAME_BLOOM_BITS: usize = 32_768;
const USERNAME_BLOOM_HASHES: usize = 5;

#[derive(Default)]
pub struct UsernameBloom {
    bits: Vec<u8>,
}

impl UsernameBloom {
    fn with_capacity() -> Self {
        Self {
            bits: vec![0; USERNAME_BLOOM_BITS / 8],
        }
    }
    fn indexes(value: &str) -> [usize; USERNAME_BLOOM_HASHES] {
        let digest = Sha256::digest(value.as_bytes());
        let mut indexes = [0; USERNAME_BLOOM_HASHES];
        for (index, output) in indexes.iter_mut().enumerate() {
            let offset = index * 4;
            *output = (u32::from_be_bytes([
                digest[offset],
                digest[offset + 1],
                digest[offset + 2],
                digest[offset + 3],
            ]) as usize)
                % USERNAME_BLOOM_BITS;
        }
        indexes
    }
    fn insert(&mut self, value: &str) {
        for index in Self::indexes(value) {
            self.bits[index / 8] |= 1 << (index % 8);
        }
    }
    fn might_contain(&self, value: &str) -> bool {
        Self::indexes(value)
            .into_iter()
            .all(|index| self.bits[index / 8] & (1 << (index % 8)) != 0)
    }
}

async fn health() -> &'static str {
    "ok"
}

#[derive(Serialize)]
pub struct MeResponse {
    pub sub: String,
    pub is_admin: bool,
    pub display_name: Option<String>,
    pub username: String,
    pub email: Option<String>,
    pub slack_id: Option<String>,
    pub avatar_url: Option<String>,
    pub profile_public: bool,
    pub country_code: Option<String>,
}

async fn me(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<MeResponse>, StatusCode> {
    let sub = auth::extract_current_user(&state.auth, &state.db, &headers)
        .await
        .ok_or(StatusCode::UNAUTHORIZED)?;
    let row = sqlx::query(
        "SELECT is_admin, username, display_name, email, slack_id, avatar_url, profile_public, country_code \
         FROM users WHERE auth_subject = $1",
    )
    .bind(&sub)
    .fetch_optional(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    .ok_or(StatusCode::UNAUTHORIZED)?;
    let is_admin: bool = row
        .try_get("is_admin")
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let username = row
        .try_get("username")
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let display_name = row
        .try_get("display_name")
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let email = row
        .try_get("email")
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let slack_id = row
        .try_get("slack_id")
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let avatar_url = row
        .try_get("avatar_url")
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let profile_public = row
        .try_get("profile_public")
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let country_code = row
        .try_get("country_code")
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(MeResponse {
        sub,
        is_admin,
        display_name,
        username,
        email,
        slack_id,
        avatar_url,
        profile_public,
        country_code,
    }))
}

#[derive(Deserialize)]
struct UpdateProfileRequest {
    username: String,
    profile_public: bool,
    #[serde(default)]
    country_code: Option<String>,
}

fn valid_username(username: &str) -> bool {
    (3..=30).contains(&username.len())
        && username
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        && !username.starts_with('-')
        && !username.ends_with('-')
}

#[derive(Serialize)]
struct UsernameAvailability {
    may_be_available: bool,
}

async fn username_availability(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(username): Path<String>,
) -> Result<Json<UsernameAvailability>, StatusCode> {
    auth::extract_current_user(&state.auth, &state.db, &headers)
        .await
        .ok_or(StatusCode::UNAUTHORIZED)?;
    let username = username.trim().to_ascii_lowercase();
    if !valid_username(&username) {
        return Err(StatusCode::UNPROCESSABLE_ENTITY);
    }
    let bloom = state.username_bloom.read().await;
    Ok(Json(UsernameAvailability {
        may_be_available: !bloom.might_contain(&username),
    }))
}

async fn update_profile(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<UpdateProfileRequest>,
) -> Result<Json<MeResponse>, StatusCode> {
    let sub = auth::extract_current_user(&state.auth, &state.db, &headers)
        .await
        .ok_or(StatusCode::UNAUTHORIZED)?;
    let username = request.username.trim().to_ascii_lowercase();
    if !valid_username(&username) {
        return Err(StatusCode::UNPROCESSABLE_ENTITY);
    }

    let country_code = request
        .country_code
        .map(|code| code.trim().to_ascii_uppercase())
        .filter(|code| !code.is_empty());
    if country_code
        .as_ref()
        .is_some_and(|code| code.len() != 2 || !code.bytes().all(|byte| byte.is_ascii_uppercase()))
    {
        return Err(StatusCode::UNPROCESSABLE_ENTITY);
    }
    let row = sqlx::query(
        "UPDATE users SET username = $1, profile_public = $2, country_code = $3 \
         WHERE auth_subject = $4 \
         RETURNING is_admin, username, display_name, email, slack_id, avatar_url, profile_public, country_code",
    )
    .bind(&username)
    .bind(request.profile_public)
    .bind(country_code)
    .bind(&sub)
    .fetch_one(&state.db)
    .await
    .map_err(|error| match error {
        sqlx::Error::Database(database_error) if database_error.is_unique_violation() => {
            StatusCode::CONFLICT
        }
        sqlx::Error::RowNotFound => StatusCode::UNAUTHORIZED,
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    })?;
    state.username_bloom.write().await.insert(&username);

    Ok(Json(MeResponse {
        sub,
        is_admin: row
            .try_get("is_admin")
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
        username: row
            .try_get("username")
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
        display_name: row
            .try_get("display_name")
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
        email: row
            .try_get("email")
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
        slack_id: row
            .try_get("slack_id")
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
        avatar_url: row
            .try_get("avatar_url")
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
        profile_public: row
            .try_get("profile_public")
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
        country_code: row
            .try_get("country_code")
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
    }))
}

#[derive(serde::Serialize)]
pub struct DashboardStats {
    pub hours_per_day: u64,
    pub human_ratio_permille: u16,
    pub agent_ratio_permille: u16,
    pub struggle_episodes: u32,
}

#[derive(serde::Serialize)]
pub struct ProjectSummary {
    pub id: i64,
    pub name: String,
    pub total_seconds: i64,
    pub human_seconds: i64,
    pub agent_seconds: i64,
    pub last_active: Option<String>,
}

#[allow(clippy::manual_checked_ops)]
async fn dashboard_stats(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<DashboardStats>, StatusCode> {
    let sub = auth::extract_current_user(&state.auth, &state.db, &headers)
        .await
        .ok_or(StatusCode::UNAUTHORIZED)?;
    let user_id = get_user_id(&state.db, &sub)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let since = (std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64)
        .saturating_sub(86_400);

    let events = sqlx::query(
        "SELECT (payload->>'kind') AS kind, EXTRACT(EPOCH FROM occurred_at)::float8 AS occurred_at \
         FROM events \
         WHERE user_id = $1 AND occurred_at > to_timestamp($2) \
         ORDER BY occurred_at ASC",
    )
    .bind(user_id)
    .bind(since as f64)
    .fetch_all(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let mut human_seconds: u64 = 0;
    let mut agent_seconds: u64 = 0;
    let struggle_episodes: u32 = 0;

    let mut last_time: Option<f64> = None;
    for ev in &events {
        let kind: Option<String> = ev
            .try_get("kind")
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        let occurred_at: f64 = ev
            .try_get("occurred_at")
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

        let is_human =
            kind.as_deref() == Some("PromptSubmitted") || kind.as_deref() == Some("HumanEdit");
        let duration = match last_time {
            Some(prev) if occurred_at >= prev => {
                let diff = (occurred_at - prev) as u64;
                if diff <= 120 { diff } else { 30 }
            }
            _ => 30,
        };
        last_time = Some(occurred_at);

        if is_human {
            human_seconds = human_seconds.saturating_add(duration);
        } else {
            agent_seconds = agent_seconds.saturating_add(duration);
        }
    }

    let total = human_seconds.saturating_add(agent_seconds);
    let human_ratio = if total > 0 {
        let ratio = human_seconds * 1000 / total;
        ratio as u16
    } else {
        0
    };
    let agent_ratio = 1000u16.saturating_sub(human_ratio);

    Ok(Json(DashboardStats {
        hours_per_day: human_seconds.saturating_add(agent_seconds) / 3600,
        human_ratio_permille: human_ratio,
        agent_ratio_permille: agent_ratio,
        struggle_episodes,
    }))
}

async fn projects_list(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Vec<ProjectSummary>>, StatusCode> {
    let sub = auth::extract_current_user(&state.auth, &state.db, &headers)
        .await
        .ok_or(StatusCode::UNAUTHORIZED)?;
    let user_id = get_user_id(&state.db, &sub)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let rows = sqlx::query(
        "SELECT p.id, p.name, \
                 COALESCE(SUM(r.credited_seconds), 0) AS total_seconds, \
                 COALESCE(SUM(r.human_seconds), 0) AS human_seconds, \
                 COALESCE(SUM(r.agent_seconds), 0) AS agent_seconds, \
                 to_char(MAX(r.day), 'YYYY-MM-DD') AS last_active \
          FROM projects p \
          LEFT JOIN rollups r ON r.project_id = p.id AND r.user_id = $1 \
          WHERE p.owner_id = $1 AND p.deleted_at IS NULL \
          GROUP BY p.id, p.name \
          ORDER BY total_seconds DESC, p.name ASC",
    )
    .bind(user_id)
    .fetch_all(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        out.push(ProjectSummary {
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

#[derive(serde::Serialize)]
pub struct StruggleDay {
    pub day: String,
    pub struggle_episodes: i64,
    pub credited_seconds: i64,
    pub human_seconds: i64,
    pub agent_seconds: i64,
}

async fn struggles_list(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Vec<StruggleDay>>, StatusCode> {
    let sub = auth::extract_current_user(&state.auth, &state.db, &headers)
        .await
        .ok_or(StatusCode::UNAUTHORIZED)?;
    let user_id = get_user_id(&state.db, &sub)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let rows = sqlx::query(
        "SELECT to_char(day, 'YYYY-MM-DD') AS day, \
                 struggle_episodes, credited_seconds, human_seconds, agent_seconds \
          FROM rollups \
          WHERE user_id = $1 AND struggle_episodes > 0 \
          ORDER BY struggle_episodes DESC, day DESC \
          LIMIT 30",
    )
    .bind(user_id)
    .fetch_all(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        out.push(StruggleDay {
            day: row
                .try_get("day")
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
            struggle_episodes: row
                .try_get::<i64, _>("struggle_episodes")
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
            credited_seconds: row
                .try_get::<i64, _>("credited_seconds")
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
            human_seconds: row
                .try_get::<i64, _>("human_seconds")
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
            agent_seconds: row
                .try_get::<i64, _>("agent_seconds")
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
        });
    }
    Ok(Json(out))
}

#[derive(Serialize)]
struct EpisodeSummary {
    started_at: String,
    ended_at: String,
    attempts: i32,
    resolved: bool,
    credited_seconds: i64,
    human_edit_seconds: i64,
    agent_edit_seconds: i64,
}

async fn episodes_list(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Vec<EpisodeSummary>>, StatusCode> {
    let sub = auth::extract_current_user(&state.auth, &state.db, &headers)
        .await
        .ok_or(StatusCode::UNAUTHORIZED)?;
    let user_id = get_user_id(&state.db, &sub)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let rows = sqlx::query(
        "SELECT to_char(started_at, 'YYYY-MM-DD HH24:MI') AS started_at, \
                to_char(ended_at, 'YYYY-MM-DD HH24:MI') AS ended_at, attempts, resolved, \
                credited_seconds, human_edit_seconds, agent_edit_seconds \
         FROM productive_episodes WHERE user_id = $1 \
         ORDER BY started_at DESC LIMIT 50",
    )
    .bind(user_id)
    .fetch_all(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let mut episodes = Vec::with_capacity(rows.len());
    for row in rows {
        episodes.push(EpisodeSummary {
            started_at: row
                .try_get("started_at")
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
            ended_at: row
                .try_get("ended_at")
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
            attempts: row
                .try_get("attempts")
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
            resolved: row
                .try_get("resolved")
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
            credited_seconds: row
                .try_get("credited_seconds")
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
            human_edit_seconds: row
                .try_get("human_edit_seconds")
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
            agent_edit_seconds: row
                .try_get("agent_edit_seconds")
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
        });
    }
    Ok(Json(episodes))
}

#[derive(Serialize)]
pub struct DashboardInsights {
    pub most_used_harness: Option<String>,
    pub primary_language: Option<String>,
    pub most_active_day: Option<String>,
    pub pesdo_score_permille: Option<i64>,
    pub outcome_factor_permille: Option<i64>,
}

async fn first_text(
    db: &sqlx::Pool<sqlx::Postgres>,
    user_id: i64,
    sql: &str,
) -> Result<Option<String>, StatusCode> {
    Ok(sqlx::query(sql)
        .bind(user_id)
        .fetch_optional(db)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .and_then(|row| row.try_get::<Option<String>, _>("value").ok().flatten()))
}

async fn first_i64(
    db: &sqlx::Pool<sqlx::Postgres>,
    user_id: i64,
    sql: &str,
) -> Result<Option<i64>, StatusCode> {
    Ok(sqlx::query(sql)
        .bind(user_id)
        .fetch_optional(db)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .and_then(|row| row.try_get::<Option<i64>, _>("value").ok().flatten()))
}

async fn dashboard_insights(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<DashboardInsights>, StatusCode> {
    let sub = crate::auth::extract_current_user(&state.auth, &state.db, &headers)
        .await
        .ok_or(StatusCode::UNAUTHORIZED)?;
    let user_id = get_user_id(&state.db, &sub)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(DashboardInsights {
        most_used_harness: first_text(
            &state.db,
            user_id,
            "SELECT harness AS value FROM productive_episodes WHERE user_id = $1 AND harness IS NOT NULL AND harness <> '' GROUP BY value ORDER BY COUNT(*) DESC, value ASC LIMIT 1",
        )
        .await?,
        primary_language: first_text(
            &state.db,
            user_id,
            "SELECT language AS value FROM heartbeats WHERE user_id = $1 AND language IS NOT NULL AND language <> '' GROUP BY value ORDER BY COUNT(*) DESC, value ASC LIMIT 1",
        )
        .await?,
        most_active_day: first_text(
            &state.db,
            user_id,
            "SELECT to_char(day, 'YYYY-MM-DD') AS value FROM rollups WHERE user_id = $1 GROUP BY day ORDER BY SUM(credited_seconds) DESC, day DESC LIMIT 1",
        )
        .await?,
        pesdo_score_permille: first_i64(
            &state.db,
            user_id,
            "SELECT pesdo_score_permille AS value FROM productive_episodes WHERE user_id = $1 AND pesdo_score_permille IS NOT NULL ORDER BY started_at DESC LIMIT 1",
        )
        .await?,
        outcome_factor_permille: first_i64(
            &state.db,
            user_id,
            "SELECT outcome_factor_permille AS value FROM productive_episodes WHERE user_id = $1 ORDER BY started_at DESC LIMIT 1",
        )
        .await?,
    }))
}

async fn delete_project(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> Result<StatusCode, StatusCode> {
    let sub = crate::auth::extract_current_user(&state.auth, &state.db, &headers)
        .await
        .ok_or(StatusCode::UNAUTHORIZED)?;
    let user_id = get_user_id(&state.db, &sub)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let row = sqlx::query(
        "UPDATE projects SET deleted_at = now() WHERE id = $1 AND owner_id = $2 AND deleted_at IS NULL RETURNING id",
    )
    .bind(id)
    .bind(user_id)
    .fetch_optional(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    match row {
        Some(_) => Ok(StatusCode::NO_CONTENT),
        None => Err(StatusCode::NOT_FOUND),
    }
}

#[derive(Deserialize)]
pub struct MergeBody {
    pub source_project_id: i64,
    pub target_project_id: i64,
}

async fn merge_projects(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<MergeBody>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let sub = crate::auth::extract_current_user(&state.auth, &state.db, &headers)
        .await
        .ok_or(StatusCode::UNAUTHORIZED)?;
    let user_id = get_user_id(&state.db, &sub)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if body.source_project_id == body.target_project_id {
        return Err(StatusCode::UNPROCESSABLE_ENTITY);
    }
    let owner = sqlx::query("SELECT 1 FROM projects WHERE id = $1 AND owner_id = $2")
        .bind(body.source_project_id)
        .bind(user_id)
        .fetch_optional(&state.db)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if owner.is_none() {
        return Err(StatusCode::NOT_FOUND);
    }
    let target = sqlx::query("SELECT 1 FROM projects WHERE id = $1 AND owner_id = $2")
        .bind(body.target_project_id)
        .bind(user_id)
        .fetch_optional(&state.db)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if target.is_none() {
        return Err(StatusCode::NOT_FOUND);
    }

    let mut tx = state
        .db
        .begin()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let result = sqlx::query(
        "UPDATE productive_episodes SET project_id = $1 WHERE project_id = $2 AND user_id = $3",
    )
    .bind(body.target_project_id)
    .bind(body.source_project_id)
    .bind(user_id)
    .execute(&mut *tx)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let _ = sqlx::query(
        "UPDATE projects SET deleted_at = now() WHERE id = $1 AND owner_id = $2 AND deleted_at IS NULL",
    )
    .bind(body.source_project_id)
    .bind(user_id)
    .execute(&mut *tx)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    tx.commit()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(serde_json::json!({
        "status": "merged",
        "moved_episodes": result.rows_affected(),
    })))
}

async fn get_user_id(db: &sqlx::Pool<sqlx::Postgres>, sub: &str) -> Result<i64, sqlx::Error> {
    let row = sqlx::query("SELECT id FROM users WHERE auth_subject = $1")
        .bind(sub)
        .fetch_optional(db)
        .await?
        .ok_or(sqlx::Error::RowNotFound)?;
    row.try_get::<i64, _>("id")
}

#[derive(Serialize)]
pub struct ProfileResponse {
    pub username: String,
    pub is_admin: bool,
}

fn is_valid_username(username: &str) -> bool {
    !username.is_empty()
        && username.len() <= 30
        && username
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-')
        && !username.starts_with('-')
        && !username.ends_with('-')
}

async fn user_profile(
    State(state): State<AppState>,
    axum::extract::Path(username): axum::extract::Path<String>,
) -> Result<Json<ProfileResponse>, StatusCode> {
    if !is_valid_username(&username) {
        return Err(StatusCode::NOT_FOUND);
    }
    let row = sqlx::query(
        "SELECT username, is_admin FROM users WHERE username = $1 AND profile_public = TRUE",
    )
    .bind(&username)
    .fetch_optional(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    .ok_or(StatusCode::NOT_FOUND)?;
    let username: String = row
        .try_get("username")
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let is_admin: bool = row
        .try_get("is_admin")
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(ProfileResponse { username, is_admin }))
}

#[derive(Serialize)]
struct LeaderboardEntry {
    rank: u32,
    username: String,
    display_name: Option<String>,
    country_code: Option<String>,
    credited_seconds: i64,
    projects: i64,
}

async fn leaderboard(
    State(state): State<AppState>,
) -> Result<Json<Vec<LeaderboardEntry>>, StatusCode> {
    let rows = sqlx::query(
        "SELECT u.username, u.display_name, u.country_code, COALESCE(SUM(r.credited_seconds), 0) AS credited_seconds, \
                COUNT(DISTINCT p.id) AS projects \
         FROM users u \
         LEFT JOIN projects p ON p.owner_id = u.id \
         LEFT JOIN rollups r ON r.user_id = u.id AND r.project_id = p.id \
         WHERE u.profile_public = TRUE \
         GROUP BY u.id, u.username, u.display_name, u.country_code \
         HAVING COALESCE(SUM(r.credited_seconds), 0) > 0 \
         ORDER BY credited_seconds DESC, u.username ASC \
         LIMIT 100",
    )
    .fetch_all(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let mut entries = Vec::with_capacity(rows.len());
    for (index, row) in rows.into_iter().enumerate() {
        entries.push(LeaderboardEntry {
            rank: u32::try_from(index.saturating_add(1)).unwrap_or(u32::MAX),
            username: row
                .try_get("username")
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
            display_name: row
                .try_get("display_name")
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
            country_code: row
                .try_get("country_code")
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
            credited_seconds: row
                .try_get("credited_seconds")
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
            projects: row
                .try_get("projects")
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
        });
    }
    Ok(Json(entries))
}

#[tokio::main]
async fn main() {
    let _ = dotenvy::dotenv();
    let auth = match auth::AuthState::new().await {
        Ok(a) => Arc::new(a),
        Err(e) => {
            eprintln!(
                "auth setup failed: {e} — auth routes will return 503 until HC_AUTH_CLIENT_ID/SECRET are set"
            );
            Arc::new(auth::AuthState::disabled())
        }
    };

    let database_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://hackai:hackai@localhost:5432/hackai".to_string());
    let db = match PgPool::connect(&database_url).await {
        Ok(p) => p,
        Err(e) => {
            eprintln!("db connect failed: {e}");
            return;
        }
    };

    let (sse_hub, _sse_rx) = sse::SseHub::new();
    let mut username_bloom = UsernameBloom::with_capacity();
    for row in sqlx::query("SELECT username FROM users")
        .fetch_all(&db)
        .await
        .unwrap_or_default()
    {
        if let Ok(username) = row.try_get::<String, _>("username") {
            username_bloom.insert(&username);
        }
    }

    let state = AppState {
        auth,
        db,
        sse_hub,
        username_bloom: Arc::new(RwLock::new(username_bloom)),
    };

    let request_id = HeaderName::from_static("x-request-id");
    let cors = CorsLayer::new()
        .allow_origin(AllowOrigin::predicate(
            |origin: &axum::http::HeaderValue, _| {
                if let Ok(s) = origin.to_str() {
                    s == "http://localhost:3001" || s == "http://127.0.0.1:3001"
                } else {
                    false
                }
            },
        ))
        .allow_methods([
            axum::http::Method::DELETE,
            axum::http::Method::GET,
            axum::http::Method::POST,
            axum::http::Method::OPTIONS,
        ])
        .allow_headers([
            axum::http::header::CONTENT_TYPE,
            axum::http::header::AUTHORIZATION,
            axum::http::header::COOKIE,
        ])
        .allow_credentials(true);

    let app = Router::new()
        .route("/healthz", get(health))
        .route("/api/v1/me", get(me))
        .route("/api/v1/me/profile", post(update_profile))
        .route(
            "/api/v1/me/username-availability/{username}",
            get(username_availability),
        )
        .route("/api/v1/dashboard/stats", get(dashboard_stats))
        .route("/api/v1/dashboard/insights", get(dashboard_insights))
        .route("/api/v1/projects", get(projects_list))
        .route("/api/v1/projects/{id}", delete(delete_project))
        .route("/api/v1/projects/merge", post(merge_projects))
        .route("/api/v1/struggles", get(struggles_list))
        .route("/api/v1/episodes", get(episodes_list))
        .route("/api/v1/leaderboard", get(leaderboard))
        .route("/api/v1/users/{username}", get(user_profile))
        .merge(admin::routes())
        .merge(archives::routes())
        .merge(cli::routes())
        .merge(annotations::routes())
        .merge(rollups::routes())
        .merge(sse::routes())
        .merge(sync::routes())
        .merge(auth::auth_routes())
        .merge(ingest::routes())
        .layer(cors)
        .layer(
            ServiceBuilder::new()
                .layer(HandleErrorLayer::new(|_: BoxError| async {
                    StatusCode::SERVICE_UNAVAILABLE
                }))
                .layer(CatchPanicLayer::new())
                .layer(LoadShedLayer::new())
                .layer(ConcurrencyLimitLayer::new(256))
                .layer(TimeoutLayer::new(Duration::from_secs(10)))
                .layer(DefaultBodyLimit::max(256 * 1024))
                .layer(SetRequestIdLayer::new(request_id.clone(), MakeRequestUuid))
                .layer(PropagateRequestIdLayer::new(request_id))
                .layer(TraceLayer::new_for_http()),
        )
        .with_state(state);

    let listener = match tokio::net::TcpListener::bind("0.0.0.0:3000").await {
        Ok(v) => v,
        Err(error) => {
            eprintln!("bind failed: {error}");
            return;
        }
    };
    if let Err(error) = axum::serve(listener, app).await {
        eprintln!("server failed: {error}");
    }
}
