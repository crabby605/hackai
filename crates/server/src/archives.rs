#![forbid(unsafe_code)]

//! Archived sync batch manifests.
//! GET /api/v1/archives        — the caller's own archives (session auth).
//! GET /api/v1/admin/archives  — any account's archives (admin session, ?user_id=).

use axum::{
    extract::{Query, State},
    http::{HeaderMap, StatusCode},
    response::Json,
    routing::get,
};
use serde::{Deserialize, Serialize};
use sqlx::Row;

use crate::AppState;

#[derive(Deserialize)]
pub struct ArchivesQuery {
    #[serde(default)]
    pub project_id: Option<i64>,
    #[serde(default)]
    pub from: Option<String>,
    #[serde(default)]
    pub to: Option<String>,
    #[serde(default)]
    pub user_id: Option<i64>,
}

#[derive(Serialize)]
pub struct ArchiveManifest {
    pub id: i64,
    pub project_id: i64,
    pub project_name: String,
    pub object_key: String,
    pub bytes: i64,
    pub status: String,
    pub received_at: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub archived_at: Option<i64>,
}

#[derive(Serialize)]
pub struct ProjectOption {
    pub id: i64,
    pub name: String,
}

#[derive(Serialize)]
pub struct ArchivesResponse {
    pub manifests: Vec<ArchiveManifest>,
    pub projects: Vec<ProjectOption>,
}

pub fn routes() -> axum::Router<AppState> {
    axum::Router::new()
        .route("/api/v1/archives", get(list_archives))
        .route("/api/v1/admin/archives", get(list_archives_admin))
}

async fn current_user_id(state: &AppState, headers: &HeaderMap) -> Result<i64, StatusCode> {
    let sub = crate::auth::extract_current_user(&state.auth, &state.db, headers)
        .await
        .ok_or(StatusCode::UNAUTHORIZED)?;
    sqlx::query("SELECT id FROM users WHERE auth_subject = $1")
        .bind(&sub)
        .fetch_optional(&state.db)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::UNAUTHORIZED)?
        .try_get::<i64, _>("id")
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

async fn fetch_manifests(
    db: &sqlx::Pool<sqlx::Postgres>,
    user_id: i64,
    query: &ArchivesQuery,
) -> Result<Vec<ArchiveManifest>, StatusCode> {
    let mut sql = String::from(
        "SELECT a.id, a.project_id, p.name AS project_name, a.object_key, a.bytes, \
                 a.status, \
                 EXTRACT(EPOCH FROM a.received_at)::bigint * 1000 AS received_at, \
                 EXTRACT(EPOCH FROM a.archived_at)::bigint * 1000 AS archived_at \
          FROM archives a \
          JOIN projects p ON p.id = a.project_id \
          WHERE a.user_id = $1",
    );
    // Only introduce the parameter counter when at least one filter is present.
    if query.project_id.is_some() || query.from.is_some() || query.to.is_some() {
        let mut param = 2i32;
        if query.project_id.is_some() {
            param += 1;
            sql.push_str(&format!(" AND a.project_id = ${param}"));
        }
        if query.from.is_some() {
            param += 1;
            sql.push_str(&format!(" AND DATE(a.received_at) >= ${param}::date"));
        }
        if query.to.is_some() {
            param += 1;
            sql.push_str(&format!(" AND DATE(a.received_at) <= ${param}::date"));
        }
    }
    sql.push_str(" ORDER BY a.received_at DESC LIMIT 100");

    let mut statement = sqlx::query(&sql).bind(user_id);
    if let Some(value) = query.project_id {
        statement = statement.bind(value);
    }
    if let Some(value) = &query.from {
        statement = statement.bind(value);
    }
    if let Some(value) = &query.to {
        statement = statement.bind(value);
    }

    let rows = statement
        .fetch_all(db)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let mut manifests = Vec::with_capacity(rows.len());
    for row in rows {
        manifests.push(ArchiveManifest {
            id: row
                .try_get("id")
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
            project_id: row
                .try_get("project_id")
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
            project_name: row
                .try_get("project_name")
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
            object_key: row
                .try_get("object_key")
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
            bytes: row
                .try_get("bytes")
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
            status: row
                .try_get("status")
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
            received_at: row
                .try_get("received_at")
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
            archived_at: row.try_get("archived_at").ok(),
        });
    }
    Ok(manifests)
}

async fn projects_dropdown(
    db: &sqlx::Pool<sqlx::Postgres>,
    owner_id: i64,
) -> Result<Vec<ProjectOption>, StatusCode> {
    let rows = sqlx::query(
        "SELECT id, name FROM projects WHERE owner_id = $1 AND deleted_at IS NULL \
         ORDER BY name ASC LIMIT 100",
    )
    .bind(owner_id)
    .fetch_all(db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        out.push(ProjectOption {
            id: row
                .try_get("id")
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
            name: row
                .try_get("name")
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
        });
    }
    Ok(out)
}

async fn list_archives(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<ArchivesQuery>,
) -> Result<Json<ArchivesResponse>, StatusCode> {
    let user_id = current_user_id(&state, &headers).await?;
    let manifests = fetch_manifests(&state.db, user_id, &query).await?;
    let projects = projects_dropdown(&state.db, user_id).await?;
    Ok(Json(ArchivesResponse {
        manifests,
        projects,
    }))
}

async fn list_archives_admin(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<ArchivesQuery>,
) -> Result<Json<ArchivesResponse>, StatusCode> {
    let sub = crate::auth::extract_current_user(&state.auth, &state.db, &headers)
        .await
        .ok_or(StatusCode::UNAUTHORIZED)?;
    let is_admin: bool = sqlx::query("SELECT is_admin FROM users WHERE auth_subject = $1")
        .bind(&sub)
        .fetch_optional(&state.db)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .and_then(|row| row.try_get("is_admin").ok())
        .ok_or(StatusCode::UNAUTHORIZED)?;
    if !is_admin {
        return Err(StatusCode::FORBIDDEN);
    }
    let user_id = query.user_id.ok_or(StatusCode::BAD_REQUEST)?;
    let manifests = fetch_manifests(&state.db, user_id, &query).await?;
    let projects = projects_dropdown(&state.db, user_id).await?;
    Ok(Json(ArchivesResponse {
        manifests,
        projects,
    }))
}
