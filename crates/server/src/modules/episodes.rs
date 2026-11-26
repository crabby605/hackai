#![forbid(unsafe_code)]

use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::Json,
};
use serde::Serialize;
use sqlx::Row;

use crate::{AppState, get_user_id};

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

/// Owner-facing view: intentionally framed as practical next steps rather
/// than exposing behavioural labels or an internal user profile.
#[derive(Serialize)]
pub struct SessionImprovement {
    pub session_id: String,
    pub agent_name: String,
    pub pesdo_score_permille: Option<i32>,
    pub pesdo_delta: Option<i32>,
    pub turns: Vec<ImprovementTurn>,
}

#[derive(Serialize)]
pub struct ImprovementTurn {
    pub turn: i32,
    pub user_summary: &'static str,
    pub agent_summary: &'static str,
    pub impact: &'static str,
    pub next_step: &'static str,
}

pub async fn episodes_list(
    State(state): State<AppState>,
    headers: HeaderMap,
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
                attempts, resolved, \
                (credited_seconds * outcome_factor_permille / 1000)::BIGINT AS credited_seconds, \
                human_edit_seconds, agent_edit_seconds, pesdo_score_permille \
         FROM productive_episodes WHERE user_id = $1 \
         ORDER BY started_at DESC LIMIT 50",
    )
    .bind(user_id)
    .fetch_all(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let mut episodes = Vec::with_capacity(rows.len());
    for row in rows {
        episodes.push(EpisodeSummary {
            id: row
                .try_get::<i64, _>("id")
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
                .to_string(),
            started_at: row
                .try_get("started_at")
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
            ended_at: row
                .try_get("ended_at")
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
            duration: row
                .try_get("duration")
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
            human_seconds: row
                .try_get("human_edit_seconds")
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
            agent_seconds: row
                .try_get("agent_edit_seconds")
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
            credited_seconds: row
                .try_get("credited_seconds")
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
            resolved: row
                .try_get("resolved")
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
            attempts: row
                .try_get("attempts")
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
            pesdo_score_permille: row
                .try_get("pesdo_score_permille")
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
        });
    }
    Ok(Json(episodes))
}

pub fn routes() -> axum::Router<AppState> {
    axum::Router::new()
        .route("/api/v1/episodes", axum::routing::get(episodes_list))
        .route(
            "/api/v1/episodes/{id}/improvement",
            axum::routing::get(session_improvement),
        )
}

async fn session_improvement(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> Result<Json<SessionImprovement>, StatusCode> {
    let sub = crate::auth::extract_current_user(&state.auth, &state.db, &headers)
        .await
        .ok_or(StatusCode::UNAUTHORIZED)?;
    let user_id = get_user_id(&state.db, &sub)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let owned: Option<(i64, Option<i32>, Option<String>)> = sqlx::query_as(
        "SELECT project_id, pesdo_score_permille, harness \
         FROM productive_episodes WHERE id = $1 AND user_id = $2",
    )
    .bind(id)
    .bind(user_id)
    .fetch_optional(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let (project_id, pesdo_score_permille, harness) = owned.ok_or(StatusCode::NOT_FOUND)?;
    let prior_score: Option<i32> = sqlx::query_scalar(
        "SELECT pesdo_score_permille FROM productive_episodes \
         WHERE user_id = $1 AND project_id = $2 \
         AND started_at < (SELECT started_at FROM productive_episodes WHERE id = $3) \
         AND pesdo_score_permille IS NOT NULL ORDER BY started_at DESC LIMIT 1",
    )
    .bind(user_id)
    .bind(project_id)
    .bind(id)
    .fetch_optional(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    .flatten();

    let rows = sqlx::query(
        "SELECT turn_index, user_act, fullcaps, new_information, answers_question, agent_state, verification \
         FROM derived_session_transcript_turns WHERE episode_id = $1 ORDER BY turn_index ASC",
    )
    .bind(id)
    .fetch_all(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let turns = rows
        .into_iter()
        .map(|row| {
            let new_information: String = row.try_get("new_information").unwrap_or_default();
            let user_act: String = row.try_get("user_act").unwrap_or_default();
            let fullcaps: bool = row.try_get("fullcaps").unwrap_or(false);
            let answers_question: bool = row.try_get("answers_question").unwrap_or(false);
            let agent_state: String = row.try_get("agent_state").unwrap_or_default();
            let verification: String = row.try_get("verification").unwrap_or_default();
            let (impact, next_step) = improvement_copy(
                &new_information,
                answers_question,
                &agent_state,
                &verification,
            );
            ImprovementTurn {
                turn: row.try_get::<i32, _>("turn_index").unwrap_or(0) + 1,
                user_summary: user_copy(&user_act, fullcaps, &new_information),
                agent_summary: agent_copy(&agent_state, &verification),
                impact,
                next_step,
            }
        })
        .collect();
    Ok(Json(SessionImprovement {
        session_id: id.to_string(),
        agent_name: harness.unwrap_or_else(|| "Your coding agent".to_owned()),
        pesdo_score_permille,
        pesdo_delta: pesdo_score_permille
            .zip(prior_score)
            .map(|(current, prior)| current - prior),
        turns,
    }))
}

fn user_copy(user_act: &str, fullcaps: bool, new_information: &str) -> &'static str {
    if fullcaps && new_information != "high" {
        return "You made an urgent request without enough concrete context.";
    }
    match user_act {
        "specify" => "You gave the agent a concrete task or constraint.",
        "report_failure" => "You explained what did not work.",
        "steer" | "decide" => "You gave the session a clear direction.",
        "approve" => "You reviewed the result and moved the work forward.",
        "ask_to_understand" => "You paused to understand the work before continuing.",
        "nudge" => "You asked the agent to continue or retry.",
        _ => "You added another instruction to the collaboration.",
    }
}

fn agent_copy(agent_state: &str, verification: &str) -> &'static str {
    if verification == "green" {
        return "The agent produced a result that was verified.";
    }
    if verification == "red" || agent_state == "edited_red" {
        return "The agent made a change, but the result was still failing.";
    }
    match agent_state {
        "planned" => "The agent formed a plan for the next step.",
        "explained" => "The agent explained its reasoning or result.",
        "looping" => "The agent repeated an unproductive path.",
        "tool_error" => "The agent encountered a tool error.",
        "blocked" => "The agent could not continue without more input.",
        _ => "The agent continued the task, without a recorded verification yet.",
    }
}

fn improvement_copy(
    new_information: &str,
    answers_question: bool,
    agent_state: &str,
    verification: &str,
) -> (&'static str, &'static str) {
    if verification == "red" || agent_state == "edited_red" {
        return (
            "This turn ended with a failing or incomplete result.",
            "Ask for the remaining failure to be addressed, then rerun the check.",
        );
    }
    if agent_state == "looping" || agent_state == "tool_error" {
        return (
            "This turn hit a recovery loop or tool failure.",
            "Share the latest error or choose a different debugging direction.",
        );
    }
    if verification == "green" {
        return (
            "This turn reached a verified result.",
            "Keep the concrete context that helped make verification possible.",
        );
    }
    if new_information == "high" || answers_question {
        return (
            "This turn added useful direction to the session.",
            "Continue giving the specific observation, expectation, or decision.",
        );
    }
    (
        "This turn kept the session moving, but its outcome was not verified.",
        "Close the loop with a concrete check or a clearer next decision.",
    )
}
