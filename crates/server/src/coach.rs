#![forbid(unsafe_code)]

//! Privacy-safe coaching summaries built only from opt-in derived evidence.
//!
//! This is intentionally a reporting layer, not a classifier or score input:
//! it never receives prompt text and does not invoke an LLM.  The local
//! processor remains responsible for the versioned dialogue labels.

use std::collections::HashMap;

use axum::{
    Json,
    extract::State,
    http::{HeaderMap, StatusCode},
    routing::get,
};
use serde::Serialize;
use sqlx::Row;

use crate::AppState;

const MAX_PATTERNS: usize = 3;
const SAMPLE_EVENT_IDS: usize = 3;

#[derive(Serialize)]
pub struct CoachSummary {
    /// The server only reports labels produced by the local, versioned processor.
    pub data_source: &'static str,
    pub recent_derived_events: i64,
    pub baseline_weekly_derived_events: Option<i64>,
    pub patterns: Vec<CoachPattern>,
}

#[derive(Serialize)]
pub struct CoachPattern {
    pub code: &'static str,
    pub observed_count: u32,
    /// Conservative relative priority for ordering tips; this is not PESDO.
    pub priority: &'static str,
    pub reason_codes: Vec<&'static str>,
    pub tip: &'static str,
    /// Opaque derived-event ids let the owner find supporting local evidence.
    pub evidence_event_ids: Vec<String>,
}

#[derive(Default)]
struct PatternAccumulator {
    count: u32,
    weight: u32,
    priority: &'static str,
    reasons: &'static [&'static str],
    tip: &'static str,
    event_ids: Vec<String>,
}

impl PatternAccumulator {
    fn observe(&mut self, event_id: String, weight: u32) {
        self.count = self.count.saturating_add(1);
        self.weight = self.weight.saturating_add(weight);
        if self.event_ids.len() < SAMPLE_EVENT_IDS {
            self.event_ids.push(event_id);
        }
    }
}

pub fn routes() -> axum::Router<AppState> {
    axum::Router::new().route("/api/v1/coach", get(summary))
}

async fn summary(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<CoachSummary>, StatusCode> {
    let user_id = current_user_id(&state, &headers).await?;
    let counts = sqlx::query(
        "SELECT \
            COUNT(*) FILTER (WHERE e.occurred_at >= now() - interval '7 days')::BIGINT AS recent, \
            COUNT(*) FILTER (WHERE e.occurred_at >= now() - interval '35 days' \
                              AND e.occurred_at < now() - interval '7 days')::BIGINT AS baseline \
         FROM evidence_bundle_events e \
         JOIN evidence_bundles b ON b.id = e.bundle_id \
         WHERE b.user_id = $1",
    )
    .bind(user_id)
    .fetch_one(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let recent: i64 = counts
        .try_get("recent")
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let baseline: i64 = counts
        .try_get("baseline")
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let rows = sqlx::query(
        "SELECT e.event_id, e.record \
         FROM evidence_bundle_events e \
         JOIN evidence_bundles b ON b.id = e.bundle_id \
         WHERE b.user_id = $1 AND e.occurred_at >= now() - interval '7 days' \
         ORDER BY e.occurred_at DESC, e.id DESC \
         LIMIT 5000",
    )
    .bind(user_id)
    .fetch_all(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let patterns = patterns_from_records(rows)?;
    Ok(Json(CoachSummary {
        data_source: "opt-in derived evidence; no prompt text or LLM judgment",
        recent_derived_events: recent,
        baseline_weekly_derived_events: (baseline > 0).then_some(baseline / 4),
        patterns,
    }))
}

fn patterns_from_records(
    rows: Vec<sqlx::postgres::PgRow>,
) -> Result<Vec<CoachPattern>, StatusCode> {
    let mut accumulators: HashMap<&'static str, PatternAccumulator> = HashMap::new();
    for row in rows {
        let event_id: String = row
            .try_get("event_id")
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        let record: serde_json::Value = row
            .try_get("record")
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        observe_record(&mut accumulators, event_id, &record);
    }
    let mut patterns: Vec<CoachPattern> = accumulators
        .into_iter()
        .map(|(code, value)| CoachPattern {
            code,
            observed_count: value.count,
            priority: value.priority,
            reason_codes: value.reasons.to_vec(),
            tip: value.tip,
            evidence_event_ids: value.event_ids,
        })
        .collect();
    patterns.sort_by(|left, right| {
        right
            .observed_count
            .cmp(&left.observed_count)
            .then_with(|| right.priority.cmp(left.priority))
            .then_with(|| left.code.cmp(right.code))
    });
    patterns.truncate(MAX_PATTERNS);
    Ok(patterns)
}

fn observe_record(
    patterns: &mut HashMap<&'static str, PatternAccumulator>,
    event_id: String,
    record: &serde_json::Value,
) {
    let dialogue = record.get("dialogue");
    let prompt = record.get("prompt_features");
    let user_act = dialogue
        .and_then(|value| value.get("user_act"))
        .and_then(serde_json::Value::as_str);
    let agent_state = dialogue
        .and_then(|value| value.get("agent_state"))
        .and_then(serde_json::Value::as_str);
    let verification = dialogue
        .and_then(|value| value.get("verification"))
        .and_then(serde_json::Value::as_str);
    let empty_context = dialogue.is_some_and(|value| {
        !value
            .get("ctx_evidence")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false)
            && !value
                .get("ctx_expectation")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false)
            && !value
                .get("ctx_location")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false)
    });
    if user_act == Some("report_failure") && empty_context {
        observe(
            patterns,
            "failure_without_grounding",
            event_id.clone(),
            3,
            "medium",
            &["CONTEXT_VECTOR_EMPTY"],
            "When reporting a failure, add one of: the observed error, what you expected, or the affected location.",
        );
    }
    if user_act == Some("approve")
        && (verification == Some("red") || agent_state == Some("edited_red"))
    {
        observe(
            patterns,
            "approved_after_red_verification",
            event_id.clone(),
            5,
            "high",
            &["VERIFICATION_RED", "APPROVE_AFTER_RED"],
            "Before approving, rerun the failing check or ask for the remaining failure to be addressed.",
        );
    }
    if agent_state == Some("looping") || has_reason(record, "REPEATED_FAILURE") {
        observe(
            patterns,
            "repeated_loop",
            event_id.clone(),
            2,
            "medium",
            &["REPEATED_FAILURE"],
            "Break the loop by supplying the latest error or by choosing a different debugging direction.",
        );
    }
    let fullcaps = prompt
        .and_then(|value| value.get("fullcaps"))
        .and_then(serde_json::Value::as_bool)
        == Some(true);
    let no_anchors = prompt
        .and_then(|value| value.get("anchor_count_bucket"))
        .and_then(serde_json::Value::as_str)
        == Some("zero");
    if fullcaps && no_anchors {
        observe(
            patterns,
            "fullcaps_unanchored_signal",
            event_id,
            1,
            "low",
            &["FULLCAPS", "ANCHOR_COUNT_ZERO"],
            "Add a concrete error, expectation, or location so the next request is easier to act on.",
        );
    }
}

fn observe(
    patterns: &mut HashMap<&'static str, PatternAccumulator>,
    code: &'static str,
    event_id: String,
    weight: u32,
    priority: &'static str,
    reasons: &'static [&'static str],
    tip: &'static str,
) {
    let accumulator = patterns.entry(code).or_insert_with(|| PatternAccumulator {
        priority,
        reasons,
        tip,
        ..PatternAccumulator::default()
    });
    accumulator.observe(event_id, weight);
}

fn has_reason(record: &serde_json::Value, target: &str) -> bool {
    record
        .get("reason_codes")
        .and_then(serde_json::Value::as_array)
        .is_some_and(|codes| codes.iter().any(|value| value.as_str() == Some(target)))
}

async fn current_user_id(state: &AppState, headers: &HeaderMap) -> Result<i64, StatusCode> {
    let sub = crate::auth::extract_current_user(&state.auth, &state.db, headers)
        .await
        .ok_or(StatusCode::UNAUTHORIZED)?;
    let row = sqlx::query("SELECT id FROM users WHERE auth_subject = $1")
        .bind(sub)
        .fetch_optional(&state.db)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::UNAUTHORIZED)?;
    row.try_get("id")
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derives_coach_patterns_from_closed_enum_records() {
        let records = vec![
            (
                "a",
                serde_json::json!({"dialogue":{"user_act":"report_failure","ctx_evidence":false,"ctx_expectation":false,"ctx_location":false}}),
            ),
            (
                "b",
                serde_json::json!({"dialogue":{"user_act":"approve","agent_state":"edited_red","verification":"red"}}),
            ),
            (
                "c",
                serde_json::json!({"prompt_features":{"fullcaps":true,"anchor_count_bucket":"zero"}}),
            ),
        ];
        let mut values = HashMap::new();
        for (id, record) in records {
            observe_record(&mut values, id.into(), &record);
        }
        assert_eq!(
            values
                .get("failure_without_grounding")
                .map(|item| item.count),
            Some(1)
        );
        assert_eq!(
            values
                .get("approved_after_red_verification")
                .map(|item| item.count),
            Some(1)
        );
        assert_eq!(
            values
                .get("fullcaps_unanchored_signal")
                .map(|item| item.count),
            Some(1)
        );
    }

    #[test]
    fn fullcaps_is_not_a_dialogue_label() {
        let mut values = HashMap::new();
        observe_record(
            &mut values,
            "a".into(),
            &serde_json::json!({"prompt_features":{"fullcaps":true,"anchor_count_bucket":"zero"}}),
        );
        assert_eq!(
            values
                .get("fullcaps_unanchored_signal")
                .map(|item| item.priority),
            Some("low")
        );
    }
}
