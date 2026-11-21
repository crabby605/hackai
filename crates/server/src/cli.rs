#![forbid(unsafe_code)]

//! CLI browser pairing flow (device authorization grant).
//!
//! POST /api/v1/cli/pairings        — CLI creates a short-lived pairing, no auth.
//! POST /api/v1/cli/pairings/complete — browser session approves a user_code and
//!                                        an installation token (api_keys) is created.
//! GET /api/v1/cli/pairings/:code   — CLI polls; the plaintext token is returned once.

use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::Json,
    routing::{get, post},
};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as base64_engine;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use sqlx::Row;
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

use crate::AppState;

const PAIRING_TTL_SECS: i64 = 600;
const USER_CODE_LEN: usize = 8;
const ALPHABET: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";

pub fn routes() -> axum::Router<AppState> {
    axum::Router::new()
        .route("/api/v1/cli/pairings", post(create_pairing))
        .route("/api/v1/cli/pairings/complete", post(complete_pairing))
        .route("/api/v1/cli/pairings/{code}", get(pairing_status))
}

#[derive(Deserialize)]
pub struct CompleteBody {
    pub code: String,
}

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or_default()
}

fn issue_token() -> (String, Vec<u8>) {
    let mut hasher = Sha256::new();
    hasher.update(Uuid::new_v4().to_string().as_bytes());
    hasher.update(now_secs().to_string().as_bytes());
    let digest = hasher.finalize();
    let token = format!("hak_{}", base64_engine.encode(digest.as_slice()));
    (token, digest.as_slice().to_vec())
}

/// Short, unambiguous user code derived from the device code (no rand dependency).
fn user_code(device_code: &str) -> String {
    let digest = Sha256::digest(device_code.as_bytes());
    (0..USER_CODE_LEN)
        .map(|index| ALPHABET[(digest.as_slice()[index] % ALPHABET.len() as u8) as usize] as char)
        .collect()
}

async fn create_pairing(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let device_code = Uuid::new_v4().to_string();
    let code = user_code(&device_code);
    let expires_at = (now_secs() + PAIRING_TTL_SECS) as f64;

    sqlx::query(
        "INSERT INTO cli_pairings (device_code, user_code, expires_at) \
         VALUES ($1, $2, to_timestamp($3))",
    )
    .bind(&device_code)
    .bind(&code)
    .bind(expires_at)
    .execute(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(serde_json::json!({
        "device_code": device_code,
        "user_code": code,
        "verification_uri": format!("/cli/pair?code={code}"),
        "expires_in": PAIRING_TTL_SECS,
    })))
}

async fn complete_pairing(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<CompleteBody>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let sub = crate::auth::extract_current_user(&state.auth, &state.db, &headers)
        .await
        .ok_or(StatusCode::UNAUTHORIZED)?;
    let user_row = sqlx::query("SELECT id FROM users WHERE auth_subject = $1")
        .bind(&sub)
        .fetch_optional(&state.db)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::UNAUTHORIZED)?;
    let user_id: i64 = user_row
        .try_get("id")
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let code = body.code.trim();
    if code.is_empty() {
        return Err(StatusCode::UNPROCESSABLE_ENTITY);
    }

    let (token, token_hash) = issue_token();
    // ponytail: plaintext installation token sits in cli_pairings.token until the
    // CLI's first poll; upgrade to encrypted-at-rest or delivery via the approval page.

    let approved = sqlx::query(
        "UPDATE cli_pairings \
             SET user_id = $1, status = 'approved', completed_at = now(), token = $2 \
           WHERE user_code = $3 AND status = 'pending' AND expires_at > now() \
           RETURNING id",
    )
    .bind(user_id)
    .bind(&token)
    .bind(code)
    .fetch_optional(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    .is_some();

    if !approved {
        return Err(StatusCode::CONFLICT);
    }

    // Persist only the hash; the raw token is delivered once via the poll endpoint.
    let _ = sqlx::query("INSERT INTO api_keys (user_id, token_hash) VALUES ($1, $2)")
        .bind(user_id)
        .bind(token_hash.as_slice())
        .execute(&state.db)
        .await;

    Ok(Json(serde_json::json!({ "status": "approved" })))
}

async fn pairing_status(
    State(state): State<AppState>,
    Path(code): Path<String>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    // Deliver the plaintext installation token exactly once, atomically.
    if let Some(row) = sqlx::query(
        "UPDATE cli_pairings SET token = NULL, consumed_at = now(), status = 'consumed' \
         WHERE device_code = $1 AND token IS NOT NULL \
         RETURNING token",
    )
    .bind(&code)
    .fetch_optional(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    {
        let token: String = row
            .try_get("token")
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        return Ok(Json(
            serde_json::json!({ "status": "approved", "token": token }),
        ));
    }

    let row = sqlx::query(
        "SELECT status, EXTRACT(EPOCH FROM expires_at) AS expires_at \
         FROM cli_pairings WHERE device_code = $1",
    )
    .bind(&code)
    .fetch_optional(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    .ok_or(StatusCode::NOT_FOUND)?;

    let status: String = row
        .try_get("status")
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let expires_at: f64 = row
        .try_get("expires_at")
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let status = if status == "pending" && expires_at < now_secs() as f64 {
        "expired"
    } else {
        status.as_str()
    };

    Ok(Json(serde_json::json!({ "status": status })))
}
