#![forbid(unsafe_code)]

//! Hack Club Auth (OIDC) integration.
//!
//! Discovers the OIDC config from https://hackclub.com/.well-known/openid-configuration,
//! caches the JWKS, and verifies RS256 ID tokens. The user is always derived from the
//! `sub` claim of a verified ID token — never from the request body.

use axum::{
    Router,
    extract::{Query, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Redirect},
    routing::get,
};
use base64::Engine;
use jsonwebtoken::{
    Algorithm, DecodingKey, EncodingKey, Header, Validation, decode, encode, jwk::JwkSet,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::Row;
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::AppState;

#[derive(Clone, Debug, Deserialize)]
pub struct OidcDiscovery {
    #[allow(dead_code)]
    pub issuer: String,
    pub authorization_endpoint: String,
    pub token_endpoint: String,
    pub jwks_uri: String,
    #[serde(default)]
    pub id_token_signing_alg_values_supported: Vec<String>,
    #[allow(dead_code)]
    pub scopes_supported: Vec<String>,
}

/// Discovered from https://hackclub.com/.well-known/openid-configuration.
#[derive(Debug, Clone)]
pub struct AuthState {
    pub client: reqwest::Client,
    #[allow(dead_code)]
    pub issuer: String,
    pub client_id: String,
    pub client_secret: String,
    /// Dedicated key for signing session cookies (HS256).
    /// Separate from OIDC client_secret to limit blast radius if leaked.
    pub session_signing_key: String,
    pub redirect_uri: String,
    pub authorization_endpoint: String,
    pub token_endpoint: String,
    pub jwks_uri: String,
    pub signing_algorithms: Vec<Algorithm>,
    #[allow(clippy::type_complexity)]
    pub jwks: std::sync::Arc<tokio::sync::RwLock<Option<JwkSet>>>,
}

#[derive(Debug, Serialize, Deserialize)]
struct HackaiSessionClaims {
    sub: String,
    iat: usize,
    exp: usize,
    iss: String,
}

const SESSION_TTL_SECONDS: u64 = 30 * 24 * 60 * 60;

impl AuthState {
    pub fn is_enabled(&self) -> bool {
        !self.client_id.is_empty()
    }

    fn issue_session_token(&self, subject: &str) -> Result<String, AuthError> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|error| AuthError::InvalidToken(error.to_string()))?
            .as_secs() as usize;
        encode(
            &Header::new(Algorithm::HS256),
            &HackaiSessionClaims {
                sub: subject.to_owned(),
                iat: now,
                exp: now.saturating_add(SESSION_TTL_SECONDS as usize),
                iss: "hackai".to_owned(),
            },
            &EncodingKey::from_secret(self.session_signing_key.as_bytes()),
        )
        .map_err(|error| AuthError::InvalidToken(format!("session issue: {error}")))
    }

    fn verify_session_token(&self, token: &str) -> Option<String> {
        let mut validation = Validation::new(Algorithm::HS256);
        validation.set_issuer(&["hackai"]);
        decode::<HackaiSessionClaims>(
            token,
            &DecodingKey::from_secret(self.session_signing_key.as_bytes()),
            &validation,
        )
        .ok()
        .map(|data| data.claims.sub)
    }

    /// Disabled auth state — used when client credentials are not configured.
    /// All auth routes return a clear config error.
    pub fn disabled() -> Self {
        Self {
            client: self_client(),
            issuer: String::new(),
            client_id: String::new(),
            client_secret: String::new(),
            session_signing_key: String::new(),
            redirect_uri: String::new(),
            authorization_endpoint: String::new(),
            token_endpoint: String::new(),
            jwks_uri: String::new(),
            signing_algorithms: Vec::new(),
            jwks: Arc::new(RwLock::new(None)),
        }
    }

    pub async fn new() -> Result<Self, AuthError> {
        let issuer = std::env::var("HC_AUTH_ISSUER")
            .unwrap_or_else(|_| "https://auth.hackclub.com".to_string());
        let client_id = std::env::var("HC_AUTH_CLIENT_ID")
            .map_err(|_| AuthError::Config("HC_AUTH_CLIENT_ID not set".into()))?;
        let client_secret = std::env::var("HC_AUTH_CLIENT_SECRET")
            .map_err(|_| AuthError::Config("HC_AUTH_CLIENT_SECRET not set".into()))?;
        let redirect_uri = std::env::var("HC_AUTH_REDIRECT_URI")
            .unwrap_or_else(|_| "http://localhost:3000/callback".to_string());
        // Dedicated session signing key; separate from OIDC client_secret to limit
        // blast radius if either is leaked. Must be 32+ random bytes (base64 or hex).
        let session_signing_key = std::env::var("SESSION_SIGNING_KEY")
            .map_err(|_| AuthError::Config("SESSION_SIGNING_KEY not set".into()))?;

        let discovery: OidcDiscovery = self_client()
            .get(format!("{issuer}/.well-known/openid-configuration"))
            .send()
            .await
            .map_err(|e| AuthError::Oidc(format!("discovery fetch failed: {e}")))?
            .json()
            .await
            .map_err(|e| AuthError::Oidc(format!("discovery parse failed: {e}")))?;

        let signing_algorithms = discovery
            .id_token_signing_alg_values_supported
            .iter()
            .filter_map(|algorithm| match algorithm.as_str() {
                // A public-key algorithm is required. Never accept a symmetric
                // HMAC token algorithm from discovery or an untrusted header.
                "RS256" => Some(Algorithm::RS256),
                "RS384" => Some(Algorithm::RS384),
                "RS512" => Some(Algorithm::RS512),
                "PS256" => Some(Algorithm::PS256),
                "PS384" => Some(Algorithm::PS384),
                "PS512" => Some(Algorithm::PS512),
                "ES256" => Some(Algorithm::ES256),
                "ES384" => Some(Algorithm::ES384),
                "EdDSA" => Some(Algorithm::EdDSA),
                _ => None,
            })
            .collect::<Vec<_>>();
        if signing_algorithms.is_empty() {
            return Err(AuthError::Oidc(
                "issuer discovery advertises no supported public ID-token signing algorithm".into(),
            ));
        }

        Ok(Self {
            client: self_client(),
            issuer,
            client_id,
            client_secret,
            session_signing_key,
            redirect_uri,
            authorization_endpoint: discovery.authorization_endpoint,
            token_endpoint: discovery.token_endpoint,
            jwks_uri: discovery.jwks_uri,
            signing_algorithms,
            jwks: Arc::new(RwLock::new(None)),
        })
    }

    pub async fn ensure_jwks(&self) -> Result<(), AuthError> {
        {
            let cached = self.jwks.read().await;
            if cached.is_some() {
                return Ok(());
            }
        }
        let jwks: JwkSet = self
            .client
            .get(&self.jwks_uri)
            .send()
            .await
            .map_err(|e| AuthError::Oidc(format!("jwks fetch failed: {e}")))?
            .json()
            .await
            .map_err(|e| AuthError::Oidc(format!("jwks parse failed: {e}")))?;
        *self.jwks.write().await = Some(jwks);
        Ok(())
    }

    /// Verify an issuer-advertised public-key ID token and return its claims.
    pub async fn verify_id_token(&self, token: &str) -> Result<IdClaims, AuthError> {
        self.ensure_jwks().await?;
        let header = jsonwebtoken::decode_header(token)
            .map_err(|e| AuthError::InvalidToken(format!("decode header: {e}")))?;
        let kid = header
            .kid
            .ok_or(AuthError::InvalidToken("missing kid".into()))?;
        if !self.signing_algorithms.contains(&header.alg) {
            return Err(AuthError::InvalidToken(
                "unadvertised signing algorithm".into(),
            ));
        }

        let jwks = self.jwks.read().await;
        let jwks = jwks
            .as_ref()
            .ok_or(AuthError::Oidc("jwks not loaded".into()))?;
        let jwk = jwks
            .find(&kid)
            .ok_or(AuthError::InvalidToken("unknown kid".into()))?;

        let key = DecodingKey::from_jwk(jwk)
            .map_err(|e| AuthError::InvalidToken(format!("build key: {e}")))?;

        let mut validation = Validation::new(header.alg);
        validation.set_issuer(&[self.issuer.as_str()]);
        validation.set_audience(&[self.client_id.as_str()]);
        validation.validate_exp = true;

        let data = jsonwebtoken::decode::<IdClaims>(token, &key, &validation)
            .map_err(|e| AuthError::InvalidToken(format!("verify: {e}")))?;
        Ok(data.claims)
    }
}

fn self_client() -> reqwest::Client {
    reqwest::Client::new()
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct IdClaims {
    pub sub: String,
    pub aud: String,
    pub iss: String,
    pub exp: i64,
    pub iat: i64,
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default)]
    pub email_verified: Option<bool>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub nickname: Option<String>,
    #[serde(default)]
    pub picture: Option<String>,
    #[serde(default)]
    pub slack_id: Option<String>,
    #[serde(default)]
    pub ysws_eligible: Option<bool>,
    #[serde(default)]
    pub groups: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CallbackQuery {
    pub code: String,
    #[allow(dead_code)]
    pub state: String,
}

#[derive(Debug, Deserialize)]
pub struct TokenResponse {
    #[allow(dead_code)]
    pub access_token: Option<String>,
    pub id_token: Option<String>,
    #[allow(dead_code)]
    pub refresh_token: Option<String>,
}

const HOST_SESSION_COOKIE: &str = "__Host-hackai_session";
const PLAIN_SESSION_COOKIE: &str = "hackai_session";
const OIDC_STATE_COOKIE: &str = "hackai_oidc_state";

/// `__Host-` prefixed Secure cookie over HTTPS, plain `hackai_session` (non-Secure)
/// over local HTTP. `__Host-` requires Path=/, no Domain, Secure; plain cookie omits
/// Secure and is host-only (no Domain attribute) so it stays on localhost.
fn session_cookie_name(is_https: bool) -> &'static str {
    if is_https {
        HOST_SESSION_COOKIE
    } else {
        PLAIN_SESSION_COOKIE
    }
}

fn cookie_value<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers
        .get(header::COOKIE)?
        .to_str()
        .ok()?
        .split(';')
        .find_map(|part| {
            let part = part.trim();
            part.strip_prefix(&format!("{name}="))
        })
        .map(|value| value.trim())
}

pub fn session_cookie(id_token: &str, is_https: bool, _domain: Option<&str>) -> HeaderMap {
    let name = session_cookie_name(is_https);
    let secure_flag = if is_https { "; Secure" } else { "" };
    let mut headers = HeaderMap::new();
    let value = format!(
        "{name}={id_token}; Path=/; HttpOnly{secure_flag}; SameSite=Lax; Max-Age={SESSION_TTL_SECONDS}"
    );
    headers.insert(
        header::SET_COOKIE,
        HeaderValue::from_str(&value).unwrap_or_else(|_| HeaderValue::from_static("")),
    );
    headers
}

#[allow(dead_code)]
pub fn clear_session_cookie(is_https: bool, _domain: Option<&str>) -> HeaderMap {
    let name = session_cookie_name(is_https);
    let secure_flag = if is_https { "; Secure" } else { "" };
    let mut headers = HeaderMap::new();
    let value = format!("{name}=; Path=/; HttpOnly{secure_flag}; SameSite=Lax; Max-Age=0");
    headers.insert(
        header::SET_COOKIE,
        HeaderValue::from_str(&value).unwrap_or_else(|_| HeaderValue::from_static("")),
    );
    headers
}

fn clear_oidc_state_cookie(is_https: bool) -> HeaderValue {
    let secure_flag = if is_https { "; Secure" } else { "" };
    HeaderValue::from_str(&format!(
        "{OIDC_STATE_COOKIE}=; Path=/; HttpOnly{secure_flag}; SameSite=Lax; Max-Age=0"
    ))
    .unwrap_or_else(|_| HeaderValue::from_static(""))
}

/// Extract and verify current user `sub` from the session cookie.
/// When OIDC is configured, verifies the RS256 ID token and extracts `sub`.
/// Returns None for banned users.
pub async fn extract_current_user(
    auth: &AuthState,
    db: &sqlx::Pool<sqlx::Postgres>,
    headers: &HeaderMap,
) -> Option<String> {
    if let Some(value) = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        && let Some(token) = value.strip_prefix("Bearer ").map(str::trim)
    {
        let hash = Sha256::digest(token.as_bytes());
        let row = sqlx::query(
            "SELECT u.auth_subject, u.banned FROM api_keys k JOIN users u ON u.id = k.user_id
             WHERE k.token_hash = $1 AND k.revoked_at IS NULL",
        )
        .bind(hash.as_slice())
        .fetch_optional(db)
        .await
        .ok()
        .flatten()?;
        let banned: bool = row.try_get("banned").ok()?;
        if banned {
            return None;
        }
        return row.try_get("auth_subject").ok();
    }
    let raw_val = cookie_value(headers, HOST_SESSION_COOKIE)
        .or_else(|| cookie_value(headers, PLAIN_SESSION_COOKIE))
        .filter(|value| !value.is_empty());
    let raw_val = match raw_val {
        Some(value) => value,
        None => return None,
    };

    if auth.is_enabled() {
        let sub = if let Some(subject) = auth.verify_session_token(raw_val) {
            subject
        } else {
            match auth.verify_id_token(raw_val).await {
                Ok(claims) => claims.sub,
                Err(e) => {
                    eprintln!("ID token verification failed: {e}");
                    return None;
                }
            }
        };
        // Reject banned users.
        if is_user_banned(db, &sub).await {
            return None;
        }
        Some(sub)
    } else {
        // A missing OIDC configuration must never turn an arbitrary cookie into
        // an authenticated identity, even in a local development server.
        None
    }
}

/// Returns true if the user is banned (banned = TRUE and (banned_until IS NULL OR
/// banned_until > now)).
pub async fn is_user_banned(db: &sqlx::Pool<sqlx::Postgres>, sub: &str) -> bool {
    let row: Option<(bool,)> = sqlx::query_as(
        "SELECT banned FROM users \
         WHERE auth_subject = $1 \
           AND banned = TRUE \
           AND (banned_until IS NULL OR banned_until > NOW())",
    )
    .bind(sub)
    .fetch_optional(db)
    .await
    .ok()
    .flatten();

    row.is_some()
}

/// Returns true if the user is in cooldown (cooldown_until > now).
pub async fn is_user_in_cooldown(db: &sqlx::Pool<sqlx::Postgres>, sub: &str) -> bool {
    let row: Option<(bool,)> = sqlx::query_as(
        "SELECT true FROM users \
         WHERE auth_subject = $1 \
           AND cooldown_until IS NOT NULL \
           AND cooldown_until > NOW()",
    )
    .bind(sub)
    .fetch_optional(db)
    .await
    .ok()
    .flatten();

    row.is_some()
}

pub fn auth_routes() -> Router<AppState> {
    Router::new()
        .route("/auth/login", get(login))
        .route("/callback", get(callback))
        .route("/auth/callback", get(callback))
}

fn safe_continue_path(path: &str) -> Option<&str> {
    (path.starts_with('/')
        && !path.starts_with("//")
        && !path
            .bytes()
            .any(|byte| byte == b'\\' || byte.is_ascii_control()))
    .then_some(path)
}

async fn login(
    State(app): State<AppState>,
    Query(params): Query<std::collections::HashMap<String, String>>,
) -> Result<axum::response::Response, AuthError> {
    let state = &app.auth;
    if state.client_id.is_empty() {
        return Ok((
            StatusCode::SERVICE_UNAVAILABLE,
            "Hack Club Auth not configured: set HC_AUTH_CLIENT_ID and HC_AUTH_CLIENT_SECRET",
        )
            .into_response());
    }
    let state_nonce = Uuid::new_v4().to_string();
    let continue_path = params
        .get("continue")
        .and_then(|path| safe_continue_path(path))
        .unwrap_or("/dashboard");
    let verifier = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
    let challenge = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .encode(Sha256::digest(verifier.as_bytes()));
    let url = url::Url::parse_with_params(
        &state.authorization_endpoint,
        [
            ("response_type", "code"),
            ("client_id", &state.client_id),
            ("redirect_uri", &state.redirect_uri),
            ("scope", "openid profile slack_id"),
            ("state", &state_nonce),
            ("code_challenge", &challenge),
            ("code_challenge_method", "S256"),
        ],
    )
    .map_err(|e| AuthError::InvalidToken(format!("authorize URL: {e}")))?;
    let mut response = Redirect::to(url.as_str()).into_response();
    let is_https = state.redirect_uri.starts_with("https://");
    let secure_flag = if is_https { "; Secure" } else { "" };
    // Store state, continue path, and PKCE verifier in cookie: state|continue_path|verifier
    let cookie_value = format!("{state_nonce}|{continue_path}|{verifier}");
    let cookie = format!(
        "{OIDC_STATE_COOKIE}={cookie_value}; Path=/; HttpOnly{secure_flag}; SameSite=Lax; Max-Age=600"
    );
    let value = HeaderValue::from_str(&cookie)
        .map_err(|e| AuthError::InvalidToken(format!("state cookie: {e}")))?;
    response.headers_mut().insert(header::SET_COOKIE, value);
    // Starting a fresh OIDC flow must discard any stale provider or HackAI
    // session cookie; otherwise the browser can keep retrying the same expired
    // identity while the callback is trying to establish a new session.
    response.headers_mut().append(
        header::SET_COOKIE,
        clear_session_cookie(is_https, None)
            .get(header::SET_COOKIE)
            .cloned()
            .ok_or_else(|| AuthError::InvalidToken("clear session cookie".into()))?,
    );
    Ok(response)
}

async fn callback(
    State(app): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<CallbackQuery>,
) -> Result<axum::response::Response, AuthError> {
    let cookie_val = headers
        .get(header::COOKIE)
        .and_then(|value| value.to_str().ok())
        .and_then(|cookies| {
            cookies
                .split(';')
                .find_map(|part| part.trim().strip_prefix(&format!("{OIDC_STATE_COOKIE}=")))
        });
    let (expected_state, continue_path, code_verifier) = match cookie_val {
        Some(v) => {
            let parts: Vec<&str> = v.splitn(3, '|').collect();
            match parts.as_slice() {
                [s, p, cv] => (*s, *p, Some(*cv)),
                [s, p] => (*s, *p, None),
                [s] => (*s, "/dashboard", None),
                _ => ("", "/dashboard", None),
            }
        }
        None => ("", "/dashboard", None),
    };
    let continue_path = safe_continue_path(continue_path).unwrap_or("/dashboard");
    if expected_state.is_empty() || expected_state != query.state {
        return Err(AuthError::InvalidToken("OAuth state mismatch".into()));
    }
    let client = &app.auth.client;
    let token_endpoint = &app.auth.token_endpoint;
    let redirect_uri = &app.auth.redirect_uri;
    let client_id = &app.auth.client_id;
    let client_secret = &app.auth.client_secret;

    let mut form_fields = vec![
        ("grant_type", "authorization_code"),
        ("code", query.code.as_str()),
        ("redirect_uri", redirect_uri.as_str()),
        ("client_id", client_id.as_str()),
        ("client_secret", client_secret.as_str()),
    ];
    if let Some(cv) = code_verifier {
        form_fields.push(("code_verifier", cv));
    }

    let response = client
        .post(token_endpoint)
        .form(&form_fields)
        .send()
        .await
        .map_err(|e| AuthError::Upstream(e.to_string()))?;
    let status = response.status();
    let tokens: TokenResponse = response
        .json()
        .await
        .map_err(|e| AuthError::Upstream(format!("token response parse: {e}")))?;
    if !status.is_success() {
        return Err(AuthError::Upstream(format!(
            "token exchange returned {status}"
        )));
    }
    let id_token = tokens
        .id_token
        .ok_or_else(|| AuthError::InvalidToken("missing id_token".into()))?;
    let claims = app.auth.verify_id_token(&id_token).await?;

    if is_user_banned(&app.db, &claims.sub).await {
        return Ok(Redirect::to("/banned").into_response());
    }

    // Provision by stable subject. Provider presentation fields remain private
    // and are not used as authorization inputs.
    // Hack Club Auth deliberately does not issue OIDC `picture` claims. When a
    // user grants `slack_id`, Cachet supplies the corresponding Slack profile
    // image without this service needing Slack credentials.
    let avatar_url = claims.picture.clone().or_else(|| {
        claims
            .slack_id
            .as_ref()
            .map(|slack_id| format!("https://cachet.dunkirk.sh/users/{slack_id}/r"))
    });

    sqlx::query(
        "INSERT INTO users (username, auth_subject, display_name, email, slack_id, avatar_url) \
         VALUES ($1, $2, $3, $4, $5, $6) \
         ON CONFLICT (auth_subject) DO UPDATE SET \
           display_name = EXCLUDED.display_name, email = EXCLUDED.email, \
           slack_id = EXCLUDED.slack_id, avatar_url = EXCLUDED.avatar_url",
    )
    .bind(claims.sub.clone())
    .bind(claims.sub.clone())
    .bind(claims.name.or(claims.nickname))
    .bind(claims.email)
    .bind(claims.slack_id)
    .bind(avatar_url)
    .execute(&app.db)
    .await
    .map_err(|e| AuthError::Upstream(format!("user provision: {e}")))?;

    let session_token = app.auth.issue_session_token(&claims.sub)?;
    let is_https = app.auth.redirect_uri.starts_with("https://");
    let frontend_url = std::env::var("FRONTEND_URL")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| {
            if app.auth.redirect_uri.contains("localhost:3000")
                || app.auth.redirect_uri.contains("127.0.0.1:3000")
            {
                "http://localhost:3001".to_string()
            } else {
                String::new()
            }
        });
    let redirect_target = if !frontend_url.is_empty() && continue_path.starts_with('/') {
        format!("{}{}", frontend_url.trim_end_matches('/'), continue_path)
    } else {
        continue_path.to_string()
    };
    let mut response = Redirect::to(&redirect_target).into_response();
    response
        .headers_mut()
        .extend(session_cookie(&session_token, is_https, None));
    response
        .headers_mut()
        .append(header::SET_COOKIE, clear_oidc_state_cookie(is_https));
    Ok(response)
}

#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    #[error("config error: {0}")]
    Config(String),
    #[error("oidc error: {0}")]
    Oidc(String),
    #[error("invalid token: {0}")]
    InvalidToken(String),
    #[error("upstream error: {0}")]
    Upstream(String),
}

impl IntoResponse for AuthError {
    fn into_response(self) -> axum::response::Response {
        let status = match self {
            AuthError::Config(_) => StatusCode::INTERNAL_SERVER_ERROR,
            AuthError::Oidc(_) | AuthError::InvalidToken(_) => StatusCode::BAD_REQUEST,
            AuthError::Upstream(_) => StatusCode::BAD_GATEWAY,
        };
        (status, self.to_string()).into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::{
        clear_session_cookie, cookie_value, safe_continue_path, session_cookie, session_cookie_name,
    };
    use axum::http::{HeaderMap, header};

    fn header_text(headers: &HeaderMap) -> &str {
        assert!(headers.contains_key(header::SET_COOKIE));
        headers
            .get(header::SET_COOKIE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or("")
    }

    #[test]
    fn continue_paths_stay_on_the_frontend() {
        assert_eq!(
            safe_continue_path("/dashboard?tab=week"),
            Some("/dashboard?tab=week")
        );
        for path in [
            "https://evil.example",
            "//evil.example",
            "/\\evil.example",
            "/bad\r\nLocation: https://evil.example",
        ] {
            assert_eq!(safe_continue_path(path), None);
        }
    }

    #[test]
    fn session_cookie_name_picks_host_over_https() {
        assert_eq!(session_cookie_name(true), super::HOST_SESSION_COOKIE);
        assert_eq!(session_cookie_name(false), super::PLAIN_SESSION_COOKIE);
    }

    #[test]
    fn session_cookie_sets_secure_only_over_https() {
        let https = session_cookie("token", true, None);
        let val = header_text(&https);
        assert!(val.starts_with("__Host-hackai_session="));
        assert!(val.contains("; Secure"));
        assert!(val.contains(" HttpOnly"));
        assert!(val.contains(" SameSite=Lax"));

        let http = session_cookie("token", false, None);
        let val = header_text(&http);
        assert!(val.starts_with("hackai_session="));
        assert!(!val.contains("Secure"));
        assert!(val.contains(" HttpOnly"));
        assert!(!val.contains("__Host-"));
    }

    #[test]
    fn clear_session_cookie_matches_name_and_max_age_zero() {
        let https = clear_session_cookie(true, None);
        let val = header_text(&https);
        assert!(val.starts_with("__Host-hackai_session="));
        assert!(val.contains("Max-Age=0"));

        let http = clear_session_cookie(false, None);
        let val = header_text(&http);
        assert!(val.starts_with("hackai_session="));
        assert!(!val.contains("Secure"));
        assert!(val.contains("Max-Age=0"));
    }

    #[test]
    fn cookie_value_reads_both_names() {
        let set = |pairs: &str| {
            let mut h = HeaderMap::new();
            if let Ok(value) = pairs.parse() {
                h.insert(header::COOKIE, value);
            }
            h
        };
        assert_eq!(
            cookie_value(
                &set("hackai_session=plain_value; other=1"),
                "hackai_session"
            ),
            Some("plain_value")
        );
        assert_eq!(
            cookie_value(
                &set("__Host-hackai_session=host_value"),
                "__Host-hackai_session"
            ),
            Some("host_value")
        );
        assert_eq!(cookie_value(&set("other=1"), "hackai_session"), None);
    }
}
