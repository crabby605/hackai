#![forbid(unsafe_code)]

use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::Json,
};
use serde::{Deserialize, Serialize};
use sqlx::Row;

use crate::{AppState, get_user_id};

#[derive(Debug, Deserialize)]
pub struct ProjectsQuery {
    #[serde(default)]
    pub sort: String,
}

#[derive(Serialize)]
pub struct ProjectSummary {
    pub id: i64,
    pub name: String,
    pub total_seconds: i64,
    pub human_seconds: i64,
    pub agent_seconds: i64,
    pub last_active: Option<String>,
    pub avg_pesdo_score_permille: Option<i32>,
}

#[derive(Serialize)]
pub struct ProjectDetail {
    pub id: String,
    pub name: String,
    pub total_seconds: i64,
    pub human_seconds: i64,
    pub agent_seconds: i64,
    pub last_active: Option<String>,
    pub episodes: i64,
    pub rollups: Vec<RollupDay>,
    pub git_url: Option<String>,
}

#[derive(Serialize)]
pub struct RollupDay {
    pub day: String,
    pub total_seconds: i64,
    pub human_seconds: i64,
    pub agent_seconds: i64,
}

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
    pub pesdo_score_permille: Option<i32>,
}

#[derive(Deserialize)]
pub struct UpdateProjectRequest {
    pub name: Option<String>,
    pub git_url: Option<String>,
}

pub async fn projects_list(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<ProjectsQuery>,
) -> Result<Json<Vec<ProjectSummary>>, StatusCode> {
    let sub = crate::auth::extract_current_user(&state.auth, &state.db, &headers)
        .await
        .ok_or(StatusCode::UNAUTHORIZED)?;
    let user_id = get_user_id(&state.db, &sub).await.map_err(|error| {
        eprintln!("projects user lookup failed: {error}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let rows = match query.sort.as_str() {
        "last_worked_desc" => {
            sqlx::query(
                "SELECT p.id, p.name, \
             COALESCE(SUM(r.credited_seconds), 0)::BIGINT AS total_seconds, \
             COALESCE(SUM(r.human_seconds), 0)::BIGINT AS human_seconds, \
             COALESCE(SUM(r.agent_seconds), 0)::BIGINT AS agent_seconds, \
             to_char(MAX(r.day), 'YYYY-MM-DD') AS last_active, \
             (SELECT ROUND(AVG(e.pesdo_score_permille))::INTEGER FROM productive_episodes e WHERE e.project_id = p.id AND e.user_id = $1) AS avg_pesdo_score_permille \
          FROM projects p \
          LEFT JOIN rollups r ON r.project_id = p.id AND r.user_id = $1 \
          WHERE p.owner_id = $1 AND p.deleted_at IS NULL \
          GROUP BY p.id, p.name \
          ORDER BY last_active DESC NULLS LAST",
            )
            .bind(user_id)
            .fetch_all(&state.db)
            .await
        }
        "last_worked_asc" => {
            sqlx::query(
                "SELECT p.id, p.name, \
             COALESCE(SUM(r.credited_seconds), 0)::BIGINT AS total_seconds, \
             COALESCE(SUM(r.human_seconds), 0)::BIGINT AS human_seconds, \
             COALESCE(SUM(r.agent_seconds), 0)::BIGINT AS agent_seconds, \
             to_char(MAX(r.day), 'YYYY-MM-DD') AS last_active, \
             (SELECT ROUND(AVG(e.pesdo_score_permille))::INTEGER FROM productive_episodes e WHERE e.project_id = p.id AND e.user_id = $1) AS avg_pesdo_score_permille \
          FROM projects p \
          LEFT JOIN rollups r ON r.project_id = p.id AND r.user_id = $1 \
          WHERE p.owner_id = $1 AND p.deleted_at IS NULL \
          GROUP BY p.id, p.name \
          ORDER BY last_active ASC NULLS FIRST",
            )
            .bind(user_id)
            .fetch_all(&state.db)
            .await
        }
        "name_asc" => {
            sqlx::query(
                "SELECT p.id, p.name, \
             COALESCE(SUM(r.credited_seconds), 0)::BIGINT AS total_seconds, \
             COALESCE(SUM(r.human_seconds), 0)::BIGINT AS human_seconds, \
             COALESCE(SUM(r.agent_seconds), 0)::BIGINT AS agent_seconds, \
             to_char(MAX(r.day), 'YYYY-MM-DD') AS last_active, \
             (SELECT ROUND(AVG(e.pesdo_score_permille))::INTEGER FROM productive_episodes e WHERE e.project_id = p.id AND e.user_id = $1) AS avg_pesdo_score_permille \
          FROM projects p \
          LEFT JOIN rollups r ON r.project_id = p.id AND r.user_id = $1 \
          WHERE p.owner_id = $1 AND p.deleted_at IS NULL \
          GROUP BY p.id, p.name \
          ORDER BY p.name ASC",
            )
            .bind(user_id)
            .fetch_all(&state.db)
            .await
        }
        "name_desc" => {
            sqlx::query(
                "SELECT p.id, p.name, \
             COALESCE(SUM(r.credited_seconds), 0)::BIGINT AS total_seconds, \
             COALESCE(SUM(r.human_seconds), 0)::BIGINT AS human_seconds, \
             COALESCE(SUM(r.agent_seconds), 0)::BIGINT AS agent_seconds, \
             to_char(MAX(r.day), 'YYYY-MM-DD') AS last_active, \
             (SELECT ROUND(AVG(e.pesdo_score_permille))::INTEGER FROM productive_episodes e WHERE e.project_id = p.id AND e.user_id = $1) AS avg_pesdo_score_permille \
          FROM projects p \
          LEFT JOIN rollups r ON r.project_id = p.id AND r.user_id = $1 \
          WHERE p.owner_id = $1 AND p.deleted_at IS NULL \
          GROUP BY p.id, p.name \
          ORDER BY p.name DESC",
            )
            .bind(user_id)
            .fetch_all(&state.db)
            .await
        }
        "time_asc" => {
            sqlx::query(
                "SELECT p.id, p.name, \
             COALESCE(SUM(r.credited_seconds), 0)::BIGINT AS total_seconds, \
             COALESCE(SUM(r.human_seconds), 0)::BIGINT AS human_seconds, \
             COALESCE(SUM(r.agent_seconds), 0)::BIGINT AS agent_seconds, \
             to_char(MAX(r.day), 'YYYY-MM-DD') AS last_active, \
             (SELECT ROUND(AVG(e.pesdo_score_permille))::INTEGER FROM productive_episodes e WHERE e.project_id = p.id AND e.user_id = $1) AS avg_pesdo_score_permille \
          FROM projects p \
          LEFT JOIN rollups r ON r.project_id = p.id AND r.user_id = $1 \
          WHERE p.owner_id = $1 AND p.deleted_at IS NULL \
          GROUP BY p.id, p.name \
          ORDER BY total_seconds ASC",
            )
            .bind(user_id)
            .fetch_all(&state.db)
            .await
        }
        _ => {
            sqlx::query(
                "SELECT p.id, p.name, \
             COALESCE(SUM(r.credited_seconds), 0)::BIGINT AS total_seconds, \
             COALESCE(SUM(r.human_seconds), 0)::BIGINT AS human_seconds, \
             COALESCE(SUM(r.agent_seconds), 0)::BIGINT AS agent_seconds, \
             to_char(MAX(r.day), 'YYYY-MM-DD') AS last_active, \
             (SELECT ROUND(AVG(e.pesdo_score_permille))::INTEGER FROM productive_episodes e WHERE e.project_id = p.id AND e.user_id = $1) AS avg_pesdo_score_permille \
          FROM projects p \
          LEFT JOIN rollups r ON r.project_id = p.id AND r.user_id = $1 \
          WHERE p.owner_id = $1 AND p.deleted_at IS NULL \
          GROUP BY p.id, p.name \
          ORDER BY total_seconds DESC",
            )
            .bind(user_id)
            .fetch_all(&state.db)
            .await
        }
    }
    .map_err(|error| {
        eprintln!("projects query failed: {error}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        out.push(ProjectSummary {
            id: row
                .try_get("id")
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
            name: row
                .try_get("name")
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
            total_seconds: row
                .try_get("total_seconds")
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
            human_seconds: row
                .try_get("human_seconds")
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
            agent_seconds: row
                .try_get("agent_seconds")
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
            last_active: row
                .try_get("last_active")
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
            avg_pesdo_score_permille: row
                .try_get("avg_pesdo_score_permille")
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
        });
    }
    Ok(Json(out))
}

pub async fn project_detail(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> Result<Json<ProjectDetail>, StatusCode> {
    let sub = crate::auth::extract_current_user(&state.auth, &state.db, &headers)
        .await
        .ok_or(StatusCode::UNAUTHORIZED)?;
    let user_id = get_user_id(&state.db, &sub)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let project = sqlx::query(
        "SELECT p.id, p.name, \
                 COALESCE(r.total_seconds, 0)::BIGINT AS total_seconds, \
                 COALESCE(r.human_seconds, 0)::BIGINT AS human_seconds, \
                 COALESCE(r.agent_seconds, 0)::BIGINT AS agent_seconds, \
                 r.last_active, repo.remote_url AS git_url, \
                 COALESCE(e.episodes, 0)::BIGINT AS episodes \
          FROM projects p \
          LEFT JOIN ( \
              SELECT project_id, \
                  SUM(credited_seconds) AS total_seconds, \
                  SUM(human_seconds) AS human_seconds, \
                  SUM(agent_seconds) AS agent_seconds, \
                  to_char(MAX(day), 'YYYY-MM-DD') AS last_active \
              FROM rollups WHERE user_id = $1 GROUP BY project_id \
          ) r ON r.project_id = p.id \
          LEFT JOIN repositories repo ON repo.project_id = p.id \
          LEFT JOIN ( \
              SELECT project_id, COUNT(*) AS episodes \
              FROM productive_episodes WHERE user_id = $1 GROUP BY project_id \
          ) e ON e.project_id = p.id \
          WHERE p.id = $2 AND p.owner_id = $1 AND p.deleted_at IS NULL",
    )
    .bind(user_id)
    .bind(id)
    .fetch_optional(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    .ok_or(StatusCode::NOT_FOUND)?;

    let rollups = sqlx::query(
        "SELECT to_char(day, 'YYYY-MM-DD') AS day, \
                 COALESCE(credited_seconds, 0)::BIGINT AS total_seconds, \
                 COALESCE(human_seconds, 0)::BIGINT AS human_seconds, \
                 COALESCE(agent_seconds, 0)::BIGINT AS agent_seconds \
          FROM rollups \
          WHERE project_id = $1 AND user_id = $2 \
          ORDER BY day DESC \
          LIMIT 60",
    )
    .bind(id)
    .bind(user_id)
    .fetch_all(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let rollup_days: Vec<RollupDay> = rollups
        .into_iter()
        .map(|row| RollupDay {
            day: row.try_get("day").unwrap_or_default(),
            total_seconds: row.try_get("total_seconds").unwrap_or(0),
            human_seconds: row.try_get("human_seconds").unwrap_or(0),
            agent_seconds: row.try_get("agent_seconds").unwrap_or(0),
        })
        .collect();

    Ok(Json(ProjectDetail {
        id: project
            .try_get::<i64, _>("id")
            .unwrap_or_default()
            .to_string(),
        name: project.try_get("name").unwrap_or_default(),
        total_seconds: project.try_get("total_seconds").unwrap_or(0),
        human_seconds: project.try_get("human_seconds").unwrap_or(0),
        agent_seconds: project.try_get("agent_seconds").unwrap_or(0),
        last_active: project.try_get("last_active").ok().flatten(),
        episodes: project.try_get("episodes").unwrap_or(0),
        rollups: rollup_days,
        git_url: project.try_get("git_url").ok().flatten(),
    }))
}

pub async fn project_detail_by_name(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(name): Path<String>,
) -> Result<Json<ProjectDetail>, StatusCode> {
    let sub = crate::auth::extract_current_user(&state.auth, &state.db, &headers)
        .await
        .ok_or(StatusCode::UNAUTHORIZED)?;
    let user_id = get_user_id(&state.db, &sub)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let project = sqlx::query(
        "SELECT p.id, p.name, \
                 COALESCE(r.total_seconds, 0)::BIGINT AS total_seconds, \
                 COALESCE(r.human_seconds, 0)::BIGINT AS human_seconds, \
                 COALESCE(r.agent_seconds, 0)::BIGINT AS agent_seconds, \
                 r.last_active, repo.remote_url AS git_url, \
                 COALESCE(e.episodes, 0)::BIGINT AS episodes \
          FROM projects p \
          LEFT JOIN ( \
              SELECT project_id, \
                  SUM(credited_seconds) AS total_seconds, \
                  SUM(human_seconds) AS human_seconds, \
                  SUM(agent_seconds) AS agent_seconds, \
                  to_char(MAX(day), 'YYYY-MM-DD') AS last_active \
              FROM rollups WHERE user_id = $1 GROUP BY project_id \
          ) r ON r.project_id = p.id \
          LEFT JOIN repositories repo ON repo.project_id = p.id \
          LEFT JOIN ( \
              SELECT project_id, COUNT(*) AS episodes \
              FROM productive_episodes WHERE user_id = $1 GROUP BY project_id \
          ) e ON e.project_id = p.id \
          WHERE p.name = $2 AND p.owner_id = $1 AND p.deleted_at IS NULL",
    )
    .bind(user_id)
    .bind(&name)
    .fetch_optional(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    .ok_or(StatusCode::NOT_FOUND)?;

    let id: i64 = project.try_get("id").unwrap_or_default();
    let rollups = sqlx::query(
        "SELECT to_char(day, 'YYYY-MM-DD') AS day, \
                 COALESCE(credited_seconds, 0)::BIGINT AS total_seconds, \
                 COALESCE(human_seconds, 0)::BIGINT AS human_seconds, \
                 COALESCE(agent_seconds, 0)::BIGINT AS agent_seconds \
          FROM rollups \
          WHERE project_id = $1 AND user_id = $2 \
          ORDER BY day DESC \
          LIMIT 60",
    )
    .bind(id)
    .bind(user_id)
    .fetch_all(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let rollup_days: Vec<RollupDay> = rollups
        .into_iter()
        .map(|row| RollupDay {
            day: row.try_get("day").unwrap_or_default(),
            total_seconds: row.try_get("total_seconds").unwrap_or(0),
            human_seconds: row.try_get("human_seconds").unwrap_or(0),
            agent_seconds: row.try_get("agent_seconds").unwrap_or(0),
        })
        .collect();

    Ok(Json(ProjectDetail {
        id: project
            .try_get::<i64, _>("id")
            .unwrap_or_default()
            .to_string(),
        name: project.try_get("name").unwrap_or_default(),
        total_seconds: project.try_get("total_seconds").unwrap_or(0),
        human_seconds: project.try_get("human_seconds").unwrap_or(0),
        agent_seconds: project.try_get("agent_seconds").unwrap_or(0),
        last_active: project.try_get("last_active").ok().flatten(),
        episodes: project.try_get("episodes").unwrap_or(0),
        rollups: rollup_days,
        git_url: project.try_get("git_url").ok().flatten(),
    }))
}

pub async fn project_episodes(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
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
                human_edit_seconds, agent_edit_seconds, credited_seconds, resolved, attempts, pesdo_score_permille \
          FROM productive_episodes \
          WHERE project_id = $1 AND user_id = $2 \
          ORDER BY started_at DESC \
          LIMIT 100",
    )
    .bind(id)
    .bind(user_id)
    .fetch_all(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let episodes: Vec<EpisodeSummary> = rows
        .into_iter()
        .map(|row| EpisodeSummary {
            id: row.try_get::<i64, _>("id").unwrap_or_default().to_string(),
            started_at: row.try_get("started_at").unwrap_or_default(),
            ended_at: row.try_get("ended_at").unwrap_or_default(),
            duration: row.try_get("duration").unwrap_or(0),
            human_seconds: row.try_get("human_edit_seconds").unwrap_or(0),
            agent_seconds: row.try_get("agent_edit_seconds").unwrap_or(0),
            credited_seconds: row.try_get("credited_seconds").unwrap_or(0),
            resolved: row.try_get("resolved").unwrap_or(false),
            attempts: row.try_get("attempts").unwrap_or(1),
            pesdo_score_permille: row.try_get("pesdo_score_permille").ok(),
        })
        .collect();

    Ok(Json(episodes))
}

pub async fn project_episodes_by_name(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(name): Path<String>,
) -> Result<Json<Vec<EpisodeSummary>>, StatusCode> {
    let sub = crate::auth::extract_current_user(&state.auth, &state.db, &headers)
        .await
        .ok_or(StatusCode::UNAUTHORIZED)?;
    let user_id = get_user_id(&state.db, &sub)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let project_id = sqlx::query_scalar::<_, i64>(
        "SELECT id FROM projects WHERE name = $1 AND owner_id = $2 AND deleted_at IS NULL",
    )
    .bind(&name)
    .bind(user_id)
    .fetch_optional(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    .ok_or(StatusCode::NOT_FOUND)?;

    let rows = sqlx::query(
        "SELECT id, \
                 to_char(started_at, 'YYYY-MM-DD HH24:MI') AS started_at, \
                 to_char(ended_at, 'YYYY-MM-DD HH24:MI') AS ended_at, \
                 EXTRACT(EPOCH FROM (ended_at - started_at))::BIGINT AS duration, \
                human_edit_seconds, agent_edit_seconds, credited_seconds, resolved, attempts, pesdo_score_permille \
          FROM productive_episodes \
          WHERE project_id = $1 AND user_id = $2 \
          ORDER BY started_at DESC \
          LIMIT 100",
    )
    .bind(project_id)
    .bind(user_id)
    .fetch_all(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let episodes: Vec<EpisodeSummary> = rows
        .into_iter()
        .map(|row| EpisodeSummary {
            id: row.try_get::<i64, _>("id").unwrap_or_default().to_string(),
            started_at: row.try_get("started_at").unwrap_or_default(),
            ended_at: row.try_get("ended_at").unwrap_or_default(),
            duration: row.try_get("duration").unwrap_or(0),
            human_seconds: row.try_get("human_edit_seconds").unwrap_or(0),
            agent_seconds: row.try_get("agent_edit_seconds").unwrap_or(0),
            credited_seconds: row.try_get("credited_seconds").unwrap_or(0),
            resolved: row.try_get("resolved").unwrap_or(false),
            attempts: row.try_get("attempts").unwrap_or(1),
            pesdo_score_permille: row.try_get("pesdo_score_permille").ok(),
        })
        .collect();

    Ok(Json(episodes))
}

pub async fn delete_project(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let sub = crate::auth::extract_current_user(&state.auth, &state.db, &headers)
        .await
        .ok_or(StatusCode::UNAUTHORIZED)?;
    let user_id = get_user_id(&state.db, &sub)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let deleted = sqlx::query(
        "UPDATE projects SET deleted_at = now() WHERE id = $1 AND owner_id = $2 AND deleted_at IS NULL RETURNING id",
    )
    .bind(id)
    .bind(user_id)
    .fetch_optional(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    .ok_or(StatusCode::NOT_FOUND)?;

    Ok(Json(
        serde_json::json!({ "status": "deleted", "id": deleted.try_get::<i64, _>("id").unwrap_or(id) }),
    ))
}

pub async fn update_project(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Json(body): Json<UpdateProjectRequest>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let sub = crate::auth::extract_current_user(&state.auth, &state.db, &headers)
        .await
        .ok_or(StatusCode::UNAUTHORIZED)?;
    let user_id = get_user_id(&state.db, &sub)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let name = body.name.map(|value| value.trim().to_owned());
    if name.as_ref().is_some_and(|value| {
        value.is_empty() || value.len() > 128 || value.chars().any(char::is_control)
    }) {
        return Err(StatusCode::BAD_REQUEST);
    }
    let git_url = body.git_url.map(|value| value.trim().to_owned());
    if git_url.as_ref().is_some_and(|value| {
        value.len() > 2_048
            || url::Url::parse(value)
                .ok()
                .is_none_or(|parsed| !matches!(parsed.scheme(), "https" | "http"))
    }) {
        return Err(StatusCode::BAD_REQUEST);
    }
    if name.is_none() && git_url.is_none() {
        return Err(StatusCode::BAD_REQUEST);
    }

    let mut transaction = state
        .db
        .begin()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let updated = sqlx::query(
        "UPDATE projects SET name = COALESCE($1, name) WHERE id = $2 AND owner_id = $3 AND deleted_at IS NULL RETURNING name",
    )
    .bind(name.as_deref())
    .bind(id)
    .bind(user_id)
    .fetch_optional(&mut *transaction)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    .ok_or(StatusCode::NOT_FOUND)?;

    if let Some(remote_url) = git_url.as_deref() {
        let result = sqlx::query("UPDATE repositories SET remote_url = $1 WHERE project_id = $2")
            .bind(remote_url)
            .bind(id)
            .execute(&mut *transaction)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        if result.rows_affected() == 0 {
            sqlx::query("INSERT INTO repositories (project_id, remote_url) VALUES ($1, $2)")
                .bind(id)
                .bind(remote_url)
                .execute(&mut *transaction)
                .await
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        }
    }
    transaction
        .commit()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(serde_json::json!({
        "status": "updated",
        "name": updated.try_get::<String, _>("name").unwrap_or_default(),
        "git_url": git_url,
    })))
}

pub async fn merge_projects(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let sub = crate::auth::extract_current_user(&state.auth, &state.db, &headers)
        .await
        .ok_or(StatusCode::UNAUTHORIZED)?;
    let user_id = get_user_id(&state.db, &sub)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let source = body
        .get("source_project_id")
        .and_then(|v| v.as_i64())
        .ok_or(StatusCode::BAD_REQUEST)?;
    let target = body
        .get("target_project_id")
        .and_then(|v| v.as_i64())
        .ok_or(StatusCode::BAD_REQUEST)?;

    if source == target {
        return Err(StatusCode::BAD_REQUEST);
    }

    let mut tx = state
        .db
        .begin()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let moved = sqlx::query(
        "UPDATE productive_episodes SET project_id = $1 WHERE project_id = $2 AND user_id = $3",
    )
    .bind(target)
    .bind(source)
    .bind(user_id)
    .execute(&mut *tx)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let _ = sqlx::query(
        "UPDATE projects SET deleted_at = now() WHERE id = $1 AND owner_id = $2 AND deleted_at IS NULL",
    )
    .bind(source)
    .bind(user_id)
    .execute(&mut *tx)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    tx.commit()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(serde_json::json!({
        "status": "merged",
        "moved_episodes": moved.rows_affected(),
    })))
}

pub fn routes() -> axum::Router<AppState> {
    axum::Router::new()
        .route("/api/v1/projects", axum::routing::get(projects_list))
        .route(
            "/api/v1/projects/{id}",
            axum::routing::delete(delete_project).patch(update_project),
        )
        .route(
            "/api/v1/projects/{id}/detail",
            axum::routing::get(project_detail),
        )
        .route(
            "/api/v1/projects/{id}/episodes",
            axum::routing::get(project_episodes),
        )
        .route(
            "/api/v1/projects/by-name/{name}/detail",
            axum::routing::get(project_detail_by_name),
        )
        .route(
            "/api/v1/projects/by-name/{name}/episodes",
            axum::routing::get(project_episodes_by_name),
        )
        .route(
            "/api/v1/projects/merge",
            axum::routing::post(merge_projects),
        )
}
