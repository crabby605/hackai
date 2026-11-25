#![forbid(unsafe_code)]

use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    response::Json,
};
use serde::Serialize;
use sqlx::Row;

use crate::{AppState, get_user_id};

const SECONDS_PER_DAY: i64 = 86_400;

#[derive(Serialize)]
pub struct DashboardStats {
    pub hours_per_day: u64,
    pub human_ratio_permille: u16,
    pub agent_ratio_permille: u16,
    pub struggle_episodes: u32,
    pub mean_pesdo: Option<u16>,
    pub habit_breakdown: Option<HabitBreakdown>,
}

#[derive(Serialize)]
pub struct HabitBreakdown {
    pub contextless_yelling_permille: u16,
    pub oneshotting_permille: u16,
    pub side_by_side_permille: u16,
    pub productive_permille: u16,
    pub total_episodes: u32,
}

#[derive(Serialize)]
pub struct DashboardInsights {
    pub most_used_harness: Option<String>,
    pub primary_language: Option<String>,
    pub most_active_day: Option<String>,
    pub pesdo_score_permille: Option<i64>,
    pub outcome_factor_permille: Option<i64>,
}

/// One privacy-safe, nullable dashboard payload. Values are aggregates or
/// explicit opt-in feedback; raw prompts, transcript content, paths, and tool
/// output are intentionally absent.
#[derive(Serialize)]
pub struct DashboardSnapshot {
    pub last_sync_at: Option<String>,
    pub sync_status: String,
    pub total_human_seconds: i64,
    pub total_agent_seconds: i64,
    pub presence_seconds: Option<i64>,
    pub total_inactive_seconds: Option<i64>,
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub feedback: Vec<UserFeedback>,
    pub projects: Vec<ProjectSnapshot>,
}

#[derive(Serialize)]
pub struct UserFeedback {
    pub recorded_at: String,
    pub feedback: String,
}

#[derive(Serialize)]
pub struct ProjectSnapshot {
    pub id: i64,
    pub name: String,
    pub episode_count: i64,
    pub credited_seconds: i64,
    pub human_seconds: i64,
    pub agent_seconds: i64,
    pub last_active: Option<String>,
}

pub async fn dashboard_stats(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<DashboardStats>, StatusCode> {
    let sub = crate::auth::extract_current_user(&state.auth, &state.db, &headers)
        .await
        .ok_or(StatusCode::UNAUTHORIZED)?;
    let user_id = get_user_id(&state.db, &sub)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let since = (std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64)
        .saturating_sub(SECONDS_PER_DAY);

    // All last-24h stats are derived from productive_episodes (not raw events).
    let row = sqlx::query(
        "SELECT \
             COALESCE(SUM(human_edit_seconds), 0)::BIGINT AS human_seconds, \
             COALESCE(SUM(agent_edit_seconds), 0)::BIGINT AS agent_seconds, \
             COALESCE(SUM((human_edit_seconds + agent_edit_seconds) * outcome_factor_permille) / 1000, 0)::BIGINT AS credited_seconds, \
             COUNT(*) FILTER (WHERE attempts > 1)::BIGINT AS struggle_episodes, \
             COUNT(*)::BIGINT AS total_episodes, \
             AVG(pesdo_score_permille)::FLOAT8 AS mean_pesdo \
         FROM productive_episodes \
         WHERE user_id = $1 AND started_at > to_timestamp($2)",
    )
    .bind(user_id)
    .bind(since as f64)
    .fetch_one(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let human_seconds: i64 = row.try_get("human_seconds").unwrap_or(0);
    let agent_seconds: i64 = row.try_get("agent_seconds").unwrap_or(0);
    let credited_seconds: i64 = row.try_get("credited_seconds").unwrap_or(0);
    let struggle_episodes: i64 = row.try_get("struggle_episodes").unwrap_or(0);

    let total = human_seconds.saturating_add(agent_seconds);
    let human_ratio = human_seconds
        .checked_mul(1000)
        .and_then(|value| value.checked_div(total))
        .unwrap_or(0) as u16;
    let agent_ratio = 1000u16.saturating_sub(human_ratio);

    let mean_pesdo: Option<f64> = row.try_get("mean_pesdo").ok().flatten();
    let mean_pesdo = mean_pesdo.map(|m| m as u16);

    // Habit labels are not stored on productive_episodes, so habit metrics are
    // reported as unavailable rather than fabricated from a nonexistent column.
    let habit_breakdown: Option<HabitBreakdown> = None;

    Ok(Json(DashboardStats {
        hours_per_day: credited_seconds.checked_div(3600).unwrap_or(0) as u64,
        human_ratio_permille: human_ratio,
        agent_ratio_permille: agent_ratio,
        struggle_episodes: struggle_episodes as u32,
        mean_pesdo,
        habit_breakdown,
    }))
}

async fn first_text(
    db: &sqlx::Pool<sqlx::Postgres>,
    user_id: i64,
    sql: &'static str,
) -> Result<Option<String>, StatusCode> {
    Ok(sqlx::query(sql)
        .bind(user_id)
        .fetch_optional(db)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .and_then(|r| r.try_get("value").ok()))
}

async fn first_i64(
    db: &sqlx::Pool<sqlx::Postgres>,
    user_id: i64,
    sql: &'static str,
) -> Result<Option<i64>, StatusCode> {
    Ok(sqlx::query(sql)
        .bind(user_id)
        .fetch_optional(db)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .and_then(|r| r.try_get::<i64, _>("value").ok()))
}

pub async fn dashboard_insights(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<DashboardInsights>, StatusCode> {
    let sub = crate::auth::extract_current_user(&state.auth, &state.db, &headers)
        .await
        .ok_or(StatusCode::UNAUTHORIZED)?;
    let user_id = get_user_id(&state.db, &sub)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let most_used_harness = first_text(&state.db, user_id,
        "SELECT harness AS value FROM productive_episodes WHERE user_id = $1 AND harness IS NOT NULL AND harness <> '' GROUP BY value ORDER BY COUNT(*) DESC, value ASC LIMIT 1")
        .await?;
    let primary_language = first_text(&state.db, user_id,
        "SELECT language AS value FROM heartbeats WHERE user_id = $1 AND language IS NOT NULL AND language <> '' GROUP BY value ORDER BY COUNT(*) DESC, value ASC LIMIT 1")
        .await?;
    let most_active_day = first_text(&state.db, user_id,
        "SELECT to_char(day, 'YYYY-MM-DD') AS value FROM rollups WHERE user_id = $1 GROUP BY day ORDER BY SUM(credited_seconds) DESC, day DESC LIMIT 1")
        .await?;
    let pesdo_score_permille = first_i64(&state.db, user_id,
        "SELECT pesdo_score_permille AS value FROM productive_episodes WHERE user_id = $1 AND pesdo_score_permille IS NOT NULL ORDER BY started_at DESC LIMIT 1")
        .await?;
    let outcome_factor_permille = first_i64(&state.db, user_id,
        "SELECT outcome_factor_permille AS value FROM productive_episodes WHERE user_id = $1 ORDER BY started_at DESC LIMIT 1")
        .await?;

    Ok(Json(DashboardInsights {
        most_used_harness,
        primary_language,
        most_active_day,
        pesdo_score_permille,
        outcome_factor_permille,
    }))
}

pub async fn dashboard_snapshot(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<DashboardSnapshot>, StatusCode> {
    let sub = crate::auth::extract_current_user(&state.auth, &state.db, &headers)
        .await
        .ok_or(StatusCode::UNAUTHORIZED)?;
    let user_id = get_user_id(&state.db, &sub)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let totals = sqlx::query(
        "SELECT COALESCE(SUM(human_edit_seconds), 0)::BIGINT AS human_seconds, \
                COALESCE(SUM(agent_edit_seconds), 0)::BIGINT AS agent_seconds, \
                NULLIF(COALESCE(SUM(present_seconds), 0), 0)::BIGINT AS presence_seconds \
         FROM productive_episodes WHERE user_id = $1",
    )
    .bind(user_id)
    .fetch_one(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let human_seconds = totals.try_get("human_seconds").unwrap_or(0);
    let agent_seconds = totals.try_get("agent_seconds").unwrap_or(0);
    let presence_seconds = totals.try_get("presence_seconds").ok().flatten();

    let last_sync_at = sqlx::query_scalar::<_, Option<String>>(
        "SELECT to_char(MAX(created_at), 'YYYY-MM-DD\"T\"HH24:MI:SSOF') FROM processed_batches WHERE user_id = $1",
    )
    .bind(user_id)
    .fetch_one(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let feedback_rows = sqlx::query(
        "SELECT to_char(recorded_at, 'YYYY-MM-DD\"T\"HH24:MI:SSOF') AS recorded_at, feedback \
         FROM user_feedback WHERE user_id = $1 ORDER BY recorded_at DESC LIMIT 3",
    )
    .bind(user_id)
    .fetch_all(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let feedback = feedback_rows
        .into_iter()
        .filter_map(|row| {
            Some(UserFeedback {
                recorded_at: row.try_get("recorded_at").ok()?,
                feedback: row.try_get("feedback").ok()?,
            })
        })
        .collect();

    let project_rows = sqlx::query(
        "SELECT p.id, p.name, COUNT(e.id)::BIGINT AS episode_count, \
                COALESCE(SUM(e.credited_seconds * e.outcome_factor_permille / 1000), 0)::BIGINT AS credited_seconds, \
                COALESCE(SUM(e.human_edit_seconds), 0)::BIGINT AS human_seconds, \
                COALESCE(SUM(e.agent_edit_seconds), 0)::BIGINT AS agent_seconds, \
                to_char(MAX(e.ended_at), 'YYYY-MM-DD\"T\"HH24:MI:SSOF') AS last_active \
         FROM projects p LEFT JOIN productive_episodes e ON e.project_id = p.id \
         WHERE p.owner_id = $1 AND p.deleted_at IS NULL \
         GROUP BY p.id, p.name ORDER BY MAX(e.ended_at) DESC NULLS LAST, p.name ASC",
    )
    .bind(user_id)
    .fetch_all(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let projects = project_rows
        .into_iter()
        .filter_map(|row| {
            Some(ProjectSnapshot {
                id: row.try_get("id").ok()?,
                name: row.try_get("name").ok()?,
                episode_count: row.try_get("episode_count").ok()?,
                credited_seconds: row.try_get("credited_seconds").ok()?,
                human_seconds: row.try_get("human_seconds").ok()?,
                agent_seconds: row.try_get("agent_seconds").ok()?,
                last_active: row.try_get("last_active").ok().flatten(),
            })
        })
        .collect();

    Ok(Json(DashboardSnapshot {
        sync_status: if last_sync_at.is_some() {
            "synced"
        } else {
            "waiting_for_first_sync"
        }
        .to_owned(),
        last_sync_at,
        total_human_seconds: human_seconds,
        total_agent_seconds: agent_seconds,
        presence_seconds,
        total_inactive_seconds: None,
        input_tokens: None,
        output_tokens: None,
        feedback,
        projects,
    }))
}

pub fn routes() -> axum::Router<AppState> {
    axum::Router::new()
        .route(
            "/api/v1/dashboard/stats",
            axum::routing::get(dashboard_stats),
        )
        .route(
            "/api/v1/dashboard/insights",
            axum::routing::get(dashboard_insights),
        )
        .route(
            "/api/v1/dashboard/snapshot",
            axum::routing::get(dashboard_snapshot),
        )
}
