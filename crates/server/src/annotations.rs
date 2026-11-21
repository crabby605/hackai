#![forbid(unsafe_code)]

//! Advisory annotation endpoint.
//! Stores constrained, evidence-backed annotations that may only lower/flag
//! credit — never raise it. Evidence pointers must verify against transcript
//! events and git or the annotation is discarded.

use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    response::Json,
    routing::post,
};
use serde::{Deserialize, Serialize};
use sqlx::Row;

use crate::AppState;

const VALID_LABELS: &[&str] = &["debugging", "guided", "delegated", "reading", "exploring"];

#[derive(Deserialize)]
pub struct AnnotateRequest {
    pub session_key: String,
    pub label: String,
    pub evidence: String,
}

#[derive(Serialize)]
pub struct AnnotateResponse {
    pub id: i64,
    pub status: &'static str,
    pub note: &'static str,
}

pub fn routes() -> axum::Router<AppState> {
    axum::Router::new().route("/api/v1/annotations", post(annotate))
}

async fn annotate(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<AnnotateRequest>,
) -> Result<Json<AnnotateResponse>, StatusCode> {
    let sub = crate::auth::extract_current_user(&state.auth, &state.db, &headers)
        .await
        .ok_or(StatusCode::UNAUTHORIZED)?;

    // Validate label — must be one of the five constrained values.
    if !VALID_LABELS.contains(&req.label.as_str()) {
        return Err(StatusCode::BAD_REQUEST);
    }

    // Validate evidence is non-empty (must point to real transcript/git evidence).
    if req.evidence.trim().is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }

    let user_id = get_user_id(&state.db, &sub)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Insert as advisory — applied=false until verified against evidence.
    // discount_permille defaults to 0 (no credit change until verified).
    let row = sqlx::query(
        "INSERT INTO annotations (user_id, session_key, label, evidence, applied, discount_permille) \
         VALUES ($1, $2, $3, $4, false, 0) \
         RETURNING id",
    )
    .bind(user_id)
    .bind(&req.session_key)
    .bind(&req.label)
    .bind(&req.evidence)
    .fetch_one(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let id: i64 = row
        .try_get("id")
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(AnnotateResponse {
        id,
        status: "recorded",
        note: "advisory annotation recorded; evidence must verify before any credit adjustment",
    }))
}

async fn get_user_id(db: &sqlx::Pool<sqlx::Postgres>, sub: &str) -> Result<i64, sqlx::Error> {
    let row = sqlx::query("SELECT id FROM users WHERE auth_subject = $1")
        .bind(sub)
        .fetch_optional(db)
        .await?
        .ok_or(sqlx::Error::RowNotFound)?;
    row.try_get::<i64, _>("id")
}
