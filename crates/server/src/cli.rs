#![forbid(unsafe_code)]

//! CLI browser pairing flow (device authorization grant).
//!
//! POST /api/v1/cli/pairings        — CLI creates a short-lived pairing, no auth.
//! POST /api/v1/cli/pairings/complete — browser session approves a user_code and
//!                                        an installation token (api_keys) is created.
//! POST /api/v1/cli/pairings/status — CLI polls with its device secret in the body.

use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    response::Json,
    routing::{get, post},
};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as base64_engine;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::Row;
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

use crate::AppState;

const PAIRING_TTL_SECS: i64 = 1800;
const USER_CODE_LEN: usize = 8;
const ALPHABET: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";

pub fn routes() -> axum::Router<AppState> {
    axum::Router::new()
        .route("/api/v1/cli/pairings", post(create_pairing))
        .route("/api/v1/cli/pairings/complete", post(complete_pairing))
        .route("/api/v1/cli/pairings/status", post(pairing_status))
        .route("/api/v1/me", get(me))
        .route("/api/v1/me/api-key", get(api_key).post(rotate_api_key))
}

#[derive(Deserialize)]
pub struct CompleteBody {
    pub code: String,
}

#[derive(Deserialize)]
pub struct PairingStatusBody {
    pub device_code: String,
}

/// Public profile of the currently authenticated user. Fields mirror what the
/// Next.js frontend expects from GET /api/v1/me.
#[derive(Clone, Debug, Serialize)]
pub struct Me {
    pub id: String,
    pub username: String,
    pub display_name: Option<String>,
    pub email: Option<String>,
    pub slack_id: Option<String>,
    pub avatar_url: Option<String>,
    pub country_code: Option<String>,
    pub profile_public: bool,
    pub leaderboard_opt_out: bool,
    pub is_admin: bool,
}

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or_default()
}

pub(crate) fn issue_token() -> (String, Vec<u8>) {
    let mut hasher = Sha256::new();
    hasher.update(Uuid::new_v4().to_string().as_bytes());
    hasher.update(now_secs().to_string().as_bytes());
    let digest = hasher.finalize();
    let token = format!("hak_{}", base64_engine.encode(digest.as_slice()));
    let token_hash = Sha256::digest(token.as_bytes());
    (token, token_hash.as_slice().to_vec())
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
        "verification_uri": format!("{}/cli/pair?code={code}", std::env::var("HACKAI_PUBLIC_URL").unwrap_or_else(|_| "https://hackai.crabby605.hackclub.app".to_string())),
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

    // A refresh or double-click after approval is harmless. Never mint a second
    // installation key for the same device authorization.
    if let Some(existing) = sqlx::query(
        "SELECT status FROM cli_pairings WHERE user_code = $1 AND status IN ('approved', 'consumed')",
    )
    .bind(code)
    .fetch_optional(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    {
        let status: String = existing
            .try_get("status")
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        return Ok(Json(serde_json::json!({ "status": status })));
    }

    let (token, token_hash) = issue_token();

    // Keep the durable hash, plus a short-lived handoff value for the polling
    // CLI. pairing_status atomically clears the handoff on first consumption.
    let approved = sqlx::query(
        "UPDATE cli_pairings \
             SET user_id = $1, status = 'approved', completed_at = now(), token_hash = $2, pending_token = $3 \
           WHERE user_code = $4 AND status = 'pending' AND expires_at > now() \
           RETURNING id",
    )
    .bind(user_id)
    .bind(token_hash.as_slice())
    .bind(&token)
    .bind(code)
    .fetch_optional(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    .is_some();

    if !approved {
        return Err(StatusCode::CONFLICT);
    }

    // Keep one active credential per account. Persist only the hash.
    sqlx::query("UPDATE api_keys SET revoked_at = now() WHERE user_id = $1 AND revoked_at IS NULL")
        .bind(user_id)
        .execute(&state.db)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let _ = sqlx::query("INSERT INTO api_keys (user_id, token_hash) VALUES ($1, $2)")
        .bind(user_id)
        .bind(token_hash.as_slice())
        .execute(&state.db)
        .await;

    // Return plaintext token once to the authenticated browser session.
    Ok(Json(
        serde_json::json!({ "status": "approved", "token": token }),
    ))
}

async fn pairing_status(
    State(state): State<AppState>,
    Json(body): Json<PairingStatusBody>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let code = body.device_code.trim();
    if code.len() != 36
        || !code
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() || byte == b'-')
    {
        return Err(StatusCode::UNPROCESSABLE_ENTITY);
    }

    // Read and consume the handoff in one database transaction. A data
    // modifying CTE cannot safely update the same Postgres row twice, which
    // previously let the first update change the status to `consumed` before
    // the token could be returned to the CLI.
    let mut transaction = state
        .db
        .begin()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let claimed = sqlx::query(
        "SELECT id, pending_token FROM cli_pairings \
         WHERE device_code = $1 AND status = 'approved' FOR UPDATE",
    )
    .bind(code)
    .fetch_optional(&mut *transaction)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    if let Some(row) = claimed {
        let id: i64 = row
            .try_get("id")
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        let token: Option<String> = row
            .try_get("pending_token")
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        sqlx::query(
            "UPDATE cli_pairings \
             SET consumed_at = COALESCE(consumed_at, now()), status = 'consumed', pending_token = NULL \
             WHERE id = $1",
        )
        .bind(id)
        .execute(&mut *transaction)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        transaction
            .commit()
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        return Ok(Json(
            serde_json::json!({ "status": "approved", "token": token }),
        ));
    }
    transaction
        .commit()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let row = sqlx::query(
        "SELECT CASE WHEN status = 'pending' AND expires_at <= now() THEN 'expired' ELSE status END AS status FROM cli_pairings WHERE device_code = $1",
    )
    .bind(code)
    .fetch_optional(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    .ok_or(StatusCode::NOT_FOUND)?;

    let status: String = row
        .try_get("status")
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(serde_json::json!({ "status": status })))
}

fn mask_token(token: &str) -> String {
    let tail: String = token
        .chars()
        .rev()
        .take(10)
        .collect::<String>()
        .chars()
        .rev()
        .collect();
    format!("••••••••••{tail}")
}

/// Return only whether a key exists and its final ten characters. Plaintext is
/// never recoverable from the server.
async fn api_key(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let sub = crate::auth::extract_current_user(&state.auth, &state.db, &headers)
        .await
        .ok_or(StatusCode::UNAUTHORIZED)?;
    let row = sqlx::query(
        "SELECT k.created_at FROM api_keys k JOIN users u ON u.id = k.user_id
         WHERE u.auth_subject = $1 AND k.revoked_at IS NULL
         ORDER BY k.created_at DESC LIMIT 1",
    )
    .bind(&sub)
    .fetch_optional(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(serde_json::json!({
        "has_key": row.is_some(),
        "masked": if row.is_some() { "••••••••••" } else { "" },
    })))
}

/// Revoke all active keys and return one new plaintext key exactly once.
async fn rotate_api_key(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let sub = crate::auth::extract_current_user(&state.auth, &state.db, &headers)
        .await
        .ok_or(StatusCode::UNAUTHORIZED)?;
    let user_id: i64 = sqlx::query_scalar("SELECT id FROM users WHERE auth_subject = $1")
        .bind(&sub)
        .fetch_optional(&state.db)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::UNAUTHORIZED)?;
    let (token, token_hash) = issue_token();
    let mut tx = state
        .db
        .begin()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    sqlx::query("UPDATE api_keys SET revoked_at = now() WHERE user_id = $1 AND revoked_at IS NULL")
        .bind(user_id)
        .execute(&mut *tx)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    sqlx::query("INSERT INTO api_keys (user_id, token_hash) VALUES ($1, $2)")
        .bind(user_id)
        .bind(token_hash.as_slice())
        .execute(&mut *tx)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    tx.commit()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(
        serde_json::json!({ "api_key": token, "masked": mask_token(&token) }),
    ))
}

/// Profile of the currently authenticated user. Protected by the session cookie;
/// returns 401 when no valid session exists.
async fn me(State(state): State<AppState>, headers: HeaderMap) -> Result<Json<Me>, StatusCode> {
    let sub = crate::auth::extract_current_user(&state.auth, &state.db, &headers)
        .await
        .ok_or(StatusCode::UNAUTHORIZED)?;
    let row = sqlx::query(
        "SELECT id, username, display_name, email, slack_id, avatar_url, country_code, \
                 profile_public, leaderboard_opt_out, is_admin \
             FROM users WHERE auth_subject = $1",
    )
    .bind(&sub)
    .fetch_optional(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    .ok_or(StatusCode::NOT_FOUND)?;
    Ok(Json(Me {
        id: row
            .try_get::<i64, _>("id")
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
            .to_string(),
        username: row
            .try_get("username")
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
        display_name: row.try_get("display_name").ok(),
        email: row.try_get("email").ok(),
        slack_id: row.try_get("slack_id").ok(),
        avatar_url: row.try_get("avatar_url").ok(),
        country_code: row.try_get("country_code").ok(),
        profile_public: row.try_get("profile_public").unwrap_or(true),
        leaderboard_opt_out: row.try_get("leaderboard_opt_out").unwrap_or(false),
        is_admin: row.try_get("is_admin").unwrap_or(false),
    }))
}
