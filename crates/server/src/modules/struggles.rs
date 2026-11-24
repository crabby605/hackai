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
pub struct StruggleDay {
    pub day: String,
    pub struggle_episodes: i64,
    pub credited_seconds: i64,
    pub human_seconds: i64,
    pub agent_seconds: i64,
}

pub async fn struggles_list(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Vec<StruggleDay>>, StatusCode> {
    let sub = crate::auth::extract_current_user(&state.auth, &state.db, &headers)
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
                .try_get("struggle_episodes")
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
        });
    }
    Ok(Json(out))
}

pub fn routes() -> axum::Router<AppState> {
    axum::Router::new().route("/api/v1/struggles", axum::routing::get(struggles_list))
}
