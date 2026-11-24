#![forbid(unsafe_code)]

//! Ingest endpoints: native `/v1/events` and WakaTime-compatible
//! `/api/hackatime/v1/users/current/heartbeats.bulk`.
//!
//! Real Postgres persistence against the hackatime-derived schema.
//! User identity comes from the Hack Club Auth session cookie (`sub`); never from the body.

use crate::AppState;
use axum::{
    Json,
    extract::State,
    http::{HeaderMap, StatusCode},
    routing::post,
};
use serde::Deserialize;
use sqlx::Row;
use sqlx::types::Json as SqlxJson;

const VALID_EVENT_KINDS: &[&str] = &[
    "PromptSubmitted",
    "ToolStarted",
    "ToolFinished",
    "HumanEdit",
    "GitCommit",
    "SessionEnded",
    "Annotation",
    "QueueOperation",
    "AgentTurnEnd",
    "ToolCall",
    "ToolResult",
    "PermissionRequest",
    "PermissionDecision",
    "Interrupt",
    "TestRun",
    "BuildRun",
    "Commit",
    "Compaction",
    "Clear",
    "Resume",
    "SessionStart",
    "SessionEnd",
];

const MAX_EVENTS_PER_BATCH: usize = 100;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
#[allow(dead_code)]
pub struct NativeEvent {
    pub id: String,
    pub occurred_at: i64,
    pub kind: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
#[allow(dead_code)]
pub struct NativeEventsBatch {
    pub events: Vec<NativeEvent>,
}

#[derive(Deserialize)]
#[allow(dead_code)]
pub struct Heartbeat {
    pub id: String,
    pub time: f64,
    pub created_at: Option<String>,
    pub category: Option<String>,
    pub language: Option<String>,
    pub editor: Option<String>,
    pub is_write: Option<bool>,
    pub path_id: Option<String>,
    pub line_additions: Option<i32>,
    pub line_deletions: Option<i32>,
    pub ai_model: Option<String>,
    pub ai_session: Option<String>,
    pub ai_prompt_length: Option<i32>,
    pub ai_line_changes: Option<i32>,
    pub human_line_changes: Option<i32>,
    pub source: Option<String>,
    pub source_type: Option<String>,
    pub machine_hash: Option<String>,
    pub operating_system: Option<String>,
    pub dedupe_key: String,
}

#[derive(Deserialize)]
#[allow(dead_code)]
pub struct HeartbeatsBulk {
    #[serde(default)]
    #[allow(dead_code)]
    pub __ignore_unknown: Option<()>,
}

pub fn routes() -> axum::Router<AppState> {
    axum::Router::new()
        .route("/v1/events", post(native_events))
        .route(
            "/api/hackatime/v1/users/current/heartbeats.bulk",
            post(waka_heartbeats_bulk),
        )
}

async fn native_events(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(batch): Json<NativeEventsBatch>,
) -> Result<StatusCode, StatusCode> {
    if batch.events.is_empty() || batch.events.len() > MAX_EVENTS_PER_BATCH {
        return Err(StatusCode::BAD_REQUEST);
    }

    let sub = crate::auth::extract_current_user(&state.auth, &state.db, &headers)
        .await
        .ok_or(StatusCode::UNAUTHORIZED)?;

    // Cooldown: reject ingest while the user is in cooldown.
    if crate::auth::is_user_in_cooldown(&state.db, &sub).await {
        return Err(StatusCode::TOO_MANY_REQUESTS);
    }

    let user_id = get_or_create_user(&state.db, &sub)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let mut tx = state
        .db
        .begin()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    for event in &batch.events {
        if event.id.is_empty() {
            return Err(StatusCode::BAD_REQUEST);
        }
        if !VALID_EVENT_KINDS.contains(&event.kind.as_str()) {
            return Err(StatusCode::BAD_REQUEST);
        }
        sqlx::query(
            "INSERT INTO events (id, user_id, occurred_at, payload) \
             VALUES ($1, $2, to_timestamp($3), $4) \
             ON CONFLICT (id) DO NOTHING",
        )
        .bind(&event.id)
        .bind(user_id)
        .bind(event.occurred_at as f64 / 1000.0)
        .bind(SqlxJson(&serde_json::json!({"kind": event.kind})))
        .execute(&mut *tx)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    }

    tx.commit()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(StatusCode::CREATED)
}

async fn waka_heartbeats_bulk(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(batch): Json<serde_json::Value>,
) -> Result<StatusCode, StatusCode> {
    let records: Vec<serde_json::Value> = batch.as_array().ok_or(StatusCode::BAD_REQUEST)?.to_vec();

    if records.is_empty() || records.len() > MAX_EVENTS_PER_BATCH {
        return Err(StatusCode::BAD_REQUEST);
    }

    let sub = crate::auth::extract_current_user(&state.auth, &state.db, &headers)
        .await
        .ok_or(StatusCode::UNAUTHORIZED)?;

    // Cooldown: reject ingest while the user is in cooldown.
    if crate::auth::is_user_in_cooldown(&state.db, &sub).await {
        return Err(StatusCode::TOO_MANY_REQUESTS);
    }

    let user_id = get_or_create_user(&state.db, &sub)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let mut tx = state
        .db
        .begin()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    for record in &records {
        let time = record
            .get("time")
            .and_then(|v| v.as_f64())
            .ok_or(StatusCode::BAD_REQUEST)?;
        let dedupe_key = record
            .get("dedupe_key")
            .and_then(|v| v.as_str())
            .ok_or(StatusCode::BAD_REQUEST)?
            .to_string();

        sqlx::query(
            "INSERT INTO heartbeats (user_id, time, dedupe_key, category, language, editor, \
             is_write, line_additions, line_deletions, ai_model, ai_session, \
             ai_prompt_length, ai_line_changes, human_line_changes, source, source_type, \
             machine_hash, operating_system) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18) \
             ON CONFLICT (user_id, dedupe_key) DO NOTHING",
        )
        .bind(user_id)
        .bind(time)
        .bind(dedupe_key)
        .bind(record.get("category").and_then(|v| v.as_str()))
        .bind(record.get("language").and_then(|v| v.as_str()))
        .bind(record.get("editor").and_then(|v| v.as_str()))
        .bind(record.get("is_write").and_then(|v| v.as_bool()))
        .bind(record.get("line_additions").and_then(|v| v.as_i64()).map(|v| v as i32))
        .bind(record.get("line_deletions").and_then(|v| v.as_i64()).map(|v| v as i32))
        .bind(record.get("ai_model").and_then(|v| v.as_str()))
        .bind(record.get("ai_session").and_then(|v| v.as_str()))
        .bind(record.get("ai_prompt_length").and_then(|v| v.as_i64()).map(|v| v as i32))
        .bind(record.get("ai_line_changes").and_then(|v| v.as_i64()).map(|v| v as i32))
        .bind(record.get("human_line_changes").and_then(|v| v.as_i64()).map(|v| v as i32))
        .bind(record.get("source").and_then(|v| v.as_str()).unwrap_or("hackai"))
        .bind(record.get("source_type").and_then(|v| v.as_str()).unwrap_or("manual"))
        .bind(record.get("machine_hash").and_then(|v| v.as_str()))
        .bind(record.get("operating_system").and_then(|v| v.as_str()))
        .execute(&mut *tx)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    }

    tx.commit()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(StatusCode::CREATED)
}

async fn get_or_create_user(
    db: &sqlx::Pool<sqlx::Postgres>,
    sub: &str,
) -> Result<i64, sqlx::Error> {
    // Find existing user by auth_subject, otherwise create one with username = sub.
    let row = sqlx::query("SELECT id FROM users WHERE auth_subject = $1")
        .bind(sub)
        .fetch_optional(db)
        .await?;

    match row {
        Some(r) => Ok(r.try_get::<i64, _>("id")?),
        None => {
            let row = sqlx::query(
                "INSERT INTO users (username, auth_subject) VALUES ($1, $2) RETURNING id",
            )
            .bind(sub)
            .bind(sub)
            .fetch_one(db)
            .await?;
            Ok(row.try_get::<i64, _>("id")?)
        }
    }
}
