#![forbid(unsafe_code)]

mod admin;
mod annotations;
mod auth;
mod cli;
mod coach;
mod evidence;
mod ingest;
mod modules;
mod openapi;
mod policy;
mod risk;
mod rollups;
mod sse;
mod sync;

use axum::{
    BoxError, Router,
    http::{HeaderValue, StatusCode},
    routing::get,
};
use sqlx::{PgPool, Row};
use std::time::Duration;
use tower_http::cors::{Any, CorsLayer};
use tower_http::limit::RequestBodyLimitLayer;
use tower_http::timeout::TimeoutLayer;

#[derive(Clone)]
struct AppState {
    db: PgPool,
    auth: crate::auth::AuthState,
    sse_hub: crate::sse::SseHub,
}

async fn get_user_id(db: &PgPool, sub: &str) -> Result<i64, sqlx::Error> {
    let row = sqlx::query("SELECT id FROM users WHERE auth_subject = $1")
        .bind(sub)
        .fetch_optional(db)
        .await?;
    row.ok_or(sqlx::Error::RowNotFound)?.try_get("id")
}

#[tokio::main]
async fn main() -> Result<(), BoxError> {
    let database_url = std::env::var("DATABASE_URL").map_err(|_| "DATABASE_URL must be set")?;
    let db = PgPool::connect(&database_url).await?;
    sqlx::migrate!("./migrations").run(&db).await?;

    let auth_state = crate::auth::AuthState::new().await.unwrap_or_else(|e| {
        eprintln!("Auth init failed (running without OIDC): {e}");
        crate::auth::AuthState::disabled()
    });

    let state = AppState {
        db,
        auth: auth_state,
        sse_hub: crate::sse::SseHub::new(),
    };

    let cors = CorsLayer::new()
        .allow_origin([
            HeaderValue::from_static("http://localhost:3001"),
            HeaderValue::from_static("https://hackclub.app"),
        ])
        .allow_credentials(true)
        .allow_methods(Any)
        .allow_headers(Any);

    let app = Router::new()
        .route("/healthz", get(|| async { "ok" }))
        .merge(auth::auth_routes())
        .merge(cli::routes())
        .merge(sync::routes())
        .merge(ingest::routes())
        .merge(evidence::routes())
        .merge(policy::routes())
        .merge(admin::routes())
        .merge(coach::routes())
        .merge(risk::routes())
        .merge(rollups::routes())
        .merge(sse::routes())
        .merge(annotations::routes())
        .merge(modules::projects::routes())
        .merge(modules::community::routes())
        .merge(modules::dashboard::routes())
        .merge(modules::episodes::routes())
        .merge(modules::struggles::routes())
        .merge(openapi::routes())
        .layer(cors)
        .layer(TimeoutLayer::with_status_code(
            StatusCode::REQUEST_TIMEOUT,
            Duration::from_secs(10),
        ))
        .layer(RequestBodyLimitLayer::new(1_048_576))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await?;
    axum::serve(listener, app).await?;
    Ok(())
}
