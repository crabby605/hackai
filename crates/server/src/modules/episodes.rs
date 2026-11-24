#![forbid(unsafe_code)]

use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    response::Json,
};
use serde::Serialize;
use sqlx::Row;

use crate::{AppState, get_user_id};

#[derive(Serialize)]
pub struct EpisodeSummary {
    pub id: String,
    pub started_at: String,
    pub ended_at: String,
    pub duration: i64,
    pub human_seconds: i64,
    pub agent_seconds: i64,
    pub credited_seconds: i64,
    pub resolved: bool,
    pub attempts: i32,
}

pub async fn episodes_list(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Vec<EpisodeSummary>>, StatusCode> {
    let sub = crate::auth::extract_current_user(&state.auth, &state.db, &headers)
        .await
        .ok_or(StatusCode::UNAUTHORIZED)?;
    let user_id = get_user_id(&state.db, &sub)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let rows = sqlx::query(
        "SELECT id, \
                to_char(started_at, 'YYYY-MM-DD HH24:MI') AS started_at, \
                to_char(ended_at, 'YYYY-MM-DD HH24:MI') AS ended_at, \
                EXTRACT(EPOCH FROM (ended_at - started_at))::BIGINT AS duration, \
                attempts, resolved, \
                (credited_seconds * outcome_factor_permille / 1000)::BIGINT AS credited_seconds, \
                human_edit_seconds, agent_edit_seconds \
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
            id: row
                .try_get::<i64, _>("id")
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
                .to_string(),
            started_at: row
                .try_get("started_at")
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
            ended_at: row
                .try_get("ended_at")
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
            duration: row
                .try_get("duration")
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
            human_seconds: row
                .try_get("human_edit_seconds")
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
            agent_seconds: row
                .try_get("agent_edit_seconds")
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
            credited_seconds: row
                .try_get("credited_seconds")
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
            resolved: row
                .try_get("resolved")
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
            attempts: row
                .try_get("attempts")
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
        });
    }
    Ok(Json(episodes))
}

pub fn routes() -> axum::Router<AppState> {
    axum::Router::new().route("/api/v1/episodes", axum::routing::get(episodes_list))
}
