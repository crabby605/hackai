#![forbid(unsafe_code)]

//! Server-Sent Events endpoint for real-time dashboard updates.
//! Clients subscribe to `/api/v1/sse` and receive reconnect-safe event messages.
//! Auth is via the session cookie (same as all other authenticated endpoints).

use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    response::sse::{Event, Sse},
    routing::get,
};
use sqlx::Row;
use std::pin::Pin;
use tokio::sync::broadcast;
use tokio_stream::wrappers::errors::BroadcastStreamRecvError;
use tokio_stream::{Stream, StreamExt, wrappers::BroadcastStream};

use crate::AppState;

/// A message broadcast to SSE subscribers.
#[derive(Clone, Debug, serde::Serialize)]
pub struct SseMessage {
    pub user_id: i64,
    pub event: String,
    pub data: serde_json::Value,
}

/// Shared broadcast hub. One channel for all subscribers; each subscriber
/// filters messages by its own user_id.
#[derive(Clone)]
pub struct SseHub {
    tx: broadcast::Sender<SseMessage>,
}

impl SseHub {
    pub fn new() -> (Self, broadcast::Receiver<SseMessage>) {
        let (tx, rx) = broadcast::channel(1024);
        (Self { tx }, rx)
    }

    /// Returns the sender so other modules (e.g. ingest, rollups) can publish.
    pub fn sender(&self) -> broadcast::Sender<SseMessage> {
        self.tx.clone()
    }

    pub async fn publish(&self, user_id: i64, event: &str, data: serde_json::Value) {
        let _ = self.tx.send(SseMessage {
            user_id,
            event: event.to_string(),
            data,
        });
    }
}

pub fn routes() -> axum::Router<AppState> {
    axum::Router::new().route("/api/v1/sse", get(sse_handler))
}

/// SSE handler. Authenticates via session cookie, subscribes to the broadcast
/// channel, and streams matching events as text/event-stream.
async fn sse_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Sse<Pin<Box<dyn Stream<Item = Result<Event, axum::Error>> + Send>>>, StatusCode> {
    let sub = crate::auth::extract_current_user(&state.auth, &state.db, &headers)
        .await
        .ok_or(StatusCode::UNAUTHORIZED)?;

    // Target the authenticated user's own id — never the client-supplied user_id.
    let target_user_id = sqlx::query("SELECT id FROM users WHERE auth_subject = $1")
        .bind(&sub)
        .fetch_optional(&state.db)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .and_then(|row| row.try_get::<i64, _>("id").ok())
        .ok_or(StatusCode::UNAUTHORIZED)?;

    let rx = state.sse_hub.tx.subscribe();
    let target = Some(target_user_id);
    let stream = BroadcastStream::new(rx).filter_map(move |item| match item {
        Ok(msg) => {
            if let Some(uid) = target
                && msg.user_id != uid
            {
                return None;
            }
            let data = serde_json::to_string(&msg.data).unwrap_or_default();
            Some(Ok(Event::default().event(msg.event.as_str()).data(data)))
        }
        Err(BroadcastStreamRecvError::Lagged(n)) => {
            eprintln!("SSE subscriber lagged by {n} messages");
            Some(Ok(Event::default().event("lag").data(n.to_string())))
        }
    });

    let stream = Box::pin(stream);
    Ok(Sse::new(stream))
}
