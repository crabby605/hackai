#![forbid(unsafe_code)]

//! Daily rollups: pre-computed credited time per user per day.
//! Computed from events and stored in the `rollups` table.
//! The dashboard consumes this endpoint instead of re-aggregating raw events.

use axum::{
    extract::{Query, State},
    http::{HeaderMap, StatusCode},
    response::Json,
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use sqlx::Row;

use crate::{AppState, get_user_id};

#[derive(Clone, Debug, Serialize)]
pub struct RollupEntry {
    pub day: String,
    pub credited_seconds: i64,
    pub human_seconds: i64,
    pub agent_seconds: i64,
    pub struggle_episodes: u32,
}

#[derive(Deserialize)]
pub struct RollupsQuery {
    #[serde(default)]
    pub days: Option<u32>,
}

pub fn routes() -> axum::Router<AppState> {
    axum::Router::new()
        .route("/api/v1/rollups", get(list_rollups))
        .route("/api/v1/rollups/compute", post(compute_rollups))
}

async fn list_rollups(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<RollupsQuery>,
) -> Result<Json<Vec<RollupEntry>>, StatusCode> {
    let sub = crate::auth::extract_current_user(&state.auth, &state.db, &headers)
        .await
        .ok_or(StatusCode::UNAUTHORIZED)?;
    let user_id = get_user_id(&state.db, &sub)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let days = query.days.unwrap_or(30).clamp(1, 365) as i32;

    let rows = sqlx::query(
        "SELECT to_char(day, 'YYYY-MM-DD') AS day, credited_seconds, human_seconds, \
                 agent_seconds, struggle_episodes \
         FROM rollups \
         WHERE user_id = $1 AND day >= (CURRENT_DATE - $2 * interval '1 day') \
         ORDER BY day ASC",
    )
    .bind(user_id)
    .bind(days)
    .fetch_all(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let mut entries = Vec::with_capacity(rows.len());
    for row in rows {
        entries.push(RollupEntry {
            day: row
                .try_get("day")
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
            credited_seconds: row
                .try_get("credited_seconds")
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
            human_seconds: row
                .try_get("human_seconds")
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
            agent_seconds: row
                .try_get("agent_seconds")
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
            struggle_episodes: row
                .try_get::<i64, _>("struggle_episodes")
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
                as u32,
        });
    }
    Ok(Json(entries))
}

async fn compute_rollups(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let sub = crate::auth::extract_current_user(&state.auth, &state.db, &headers)
        .await
        .ok_or(StatusCode::UNAUTHORIZED)?;
    let user_id = get_user_id(&state.db, &sub)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Compute rollups for the last 30 days directly in SQL.
    // Duration per event = gap to next event, capped at 120s, min 30s.
    // Human = PromptSubmitted / HumanEdit; Agent = everything else.
    // Struggle episodes = agent events with error_signature (red->green loops).
    sqlx::query(
        "WITH ordered AS (
            SELECT
                id,
                occurred_at,
                payload,
                (payload->>'kind') AS kind,
                LEAD(occurred_at) OVER (PARTITION BY user_id ORDER BY occurred_at) AS next_time
            FROM events
            WHERE user_id = $1
              AND occurred_at > to_timestamp(EXTRACT(EPOCH FROM (CURRENT_DATE - interval '30 days')))
        ),
        durations AS (
            SELECT
                id,
                occurred_at,
                kind,
                payload,
                CASE
                    WHEN next_time IS NOT NULL AND next_time > occurred_at
                    THEN LEAST(GREATEST(EXTRACT(EPOCH FROM (next_time - occurred_at)), 30), 120)
                    ELSE 30
                END AS duration_secs
            FROM ordered
        ),
        daily AS (
            SELECT
                DATE(occurred_at) AS day,
                SUM(CASE WHEN kind = 'PromptSubmitted' OR (kind = 'HumanEdit' AND payload->>'role' = 'Human') THEN duration_secs ELSE 0 END) AS human_seconds,
                SUM(CASE WHEN kind NOT IN ('PromptSubmitted','HumanEdit') THEN duration_secs ELSE 0 END) AS agent_seconds,
                COUNT(CASE WHEN kind = 'ToolFinished' AND (payload->>'error_signature') IS NOT NULL THEN 1 END) AS struggle_episodes
            FROM durations
            GROUP BY DATE(occurred_at)
        )
        INSERT INTO rollups (user_id, project_id, day, credited_seconds, human_seconds, agent_seconds, struggle_episodes)
        SELECT
            $1 AS user_id,
            NULL AS project_id,
            d.day,
            d.human_seconds + d.agent_seconds AS credited_seconds,
            d.human_seconds,
            d.agent_seconds,
            COALESCE(d.struggle_episodes, 0)
        FROM daily d
        ON CONFLICT (user_id, project_id, day) DO UPDATE SET
            credited_seconds = EXCLUDED.credited_seconds,
            human_seconds = EXCLUDED.human_seconds,
            agent_seconds = EXCLUDED.agent_seconds,
            struggle_episodes = EXCLUDED.struggle_episodes",
    )
    .bind(user_id)
    .execute(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(serde_json::json!({
        "status": "computed",
        "user_id": user_id,
        "days": 30
    })))
}
