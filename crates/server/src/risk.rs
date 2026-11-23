#![forbid(unsafe_code)]

//! Conservative, review-only findings from opt-in derived evidence.
//!
//! These findings are intentionally computed at read time from the closed,
//! redacted evidence schema. They never change PESDO, eligibility, or account
//! status. A finding is a reason for a human to inspect corroborating evidence,
//! not a claim that a person acted dishonestly.

use std::collections::HashMap;

use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    routing::get,
};
use serde::Serialize;
use sqlx::Row;

use crate::AppState;

const SAMPLE_EVENT_IDS: usize = 5;
const MAX_EVENTS: i64 = 5_000;

#[derive(Serialize)]
pub struct RiskFindings {
    /// This is deliberately a review aid, never an enforcement decision.
    pub disposition: &'static str,
    pub data_source: &'static str,
    pub tier: &'static str,
    pub findings: Vec<RiskFinding>,
}

#[derive(Serialize)]
pub struct RiskFinding {
    pub code: &'static str,
    pub tier: &'static str,
    pub observed_count: u32,
    pub reason_codes: Vec<&'static str>,
    pub event_ids: Vec<String>,
    pub reviewer_guidance: &'static str,
}

#[derive(Default)]
struct FindingAccumulator {
    count: u32,
    event_ids: Vec<String>,
}

impl FindingAccumulator {
    fn observe(&mut self, event_id: String) {
        self.count = self.count.saturating_add(1);
        if self.event_ids.len() < SAMPLE_EVENT_IDS {
            self.event_ids.push(event_id);
        }
    }
}

pub fn routes() -> axum::Router<AppState> {
    axum::Router::new()
        .route("/api/v1/risk-findings", get(mine))
        .route(
            "/api/v1/admin/users/{user_id}/risk-findings",
            get(for_admin),
        )
}

async fn mine(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<RiskFindings>, StatusCode> {
    let user_id = current_user_id(&state, &headers).await?;
    findings_for_user(&state, user_id).await.map(Json)
}

async fn for_admin(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(user_id): Path<i64>,
) -> Result<Json<RiskFindings>, StatusCode> {
    require_admin(&state, &headers).await?;
    findings_for_user(&state, user_id).await.map(Json)
}

async fn findings_for_user(state: &AppState, user_id: i64) -> Result<RiskFindings, StatusCode> {
    let rows = sqlx::query(
        "SELECT e.event_id, e.record, b.preview_approved \
         FROM evidence_bundle_events e \
         JOIN evidence_bundles b ON b.id = e.bundle_id \
         WHERE b.user_id = $1 \
         ORDER BY e.occurred_at DESC, e.id DESC LIMIT $2",
    )
    .bind(user_id)
    .bind(MAX_EVENTS)
    .fetch_all(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    findings_from_rows(rows)
}

fn findings_from_rows(rows: Vec<sqlx::postgres::PgRow>) -> Result<RiskFindings, StatusCode> {
    let mut accumulators: HashMap<&'static str, FindingAccumulator> = HashMap::new();
    for row in rows {
        let event_id: String = row
            .try_get("event_id")
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        let record: serde_json::Value = row
            .try_get("record")
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        let preview_approved: bool = row
            .try_get("preview_approved")
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        observe_record(&mut accumulators, event_id, &record, preview_approved);
    }

    let mut findings = Vec::new();
    push_finding(
        &mut findings,
        &accumulators,
        "clock_skew_observed",
        "review",
        &["CLOCK_SKEW"],
        "Compare the event sequence with independent records before relying on its durations. Do not take automatic action.",
    );
    push_finding(
        &mut findings,
        &accumulators,
        "concurrent_sessions_observed",
        "observe",
        &["CONCURRENT_SESSION"],
        "Confirm that concurrent intervals were unioned rather than summed. This signal alone does not establish inflated activity.",
    );
    push_finding_if_at_least(
        &mut findings,
        &accumulators,
        "repeated_failures_observed",
        3,
        "observe",
        &["REPEATED_FAILURE"],
        "Review the derived sequence for a genuine recovery loop; repeated failures can be normal development work.",
    );
    push_finding(
        &mut findings,
        &accumulators,
        "unpreviewed_export",
        "info",
        &["PREVIEW_NOT_APPROVED"],
        "Ask the contributor to review and re-export the bundle if it is needed for a manual review. This is not a quality or fraud signal.",
    );

    findings.sort_by_key(|finding| std::cmp::Reverse(tier_rank(finding.tier)));
    let tier = findings.first().map_or("none", |finding| finding.tier);
    Ok(RiskFindings {
        disposition: "manual_review_only_no_automatic_enforcement",
        data_source: "opt-in derived evidence only; no prompt text, GitHub lookup, or population-level data",
        tier,
        findings,
    })
}

fn observe_record(
    accumulators: &mut HashMap<&'static str, FindingAccumulator>,
    event_id: String,
    record: &serde_json::Value,
    preview_approved: bool,
) {
    if !preview_approved {
        accumulators
            .entry("unpreviewed_export")
            .or_default()
            .observe(event_id.clone());
    }
    let Some(reason_codes) = record
        .get("reason_codes")
        .and_then(serde_json::Value::as_array)
    else {
        return;
    };
    for code in reason_codes.iter().filter_map(serde_json::Value::as_str) {
        let finding = match code {
            "CLOCK_SKEW" => Some("clock_skew_observed"),
            "CONCURRENT_SESSION" => Some("concurrent_sessions_observed"),
            "REPEATED_FAILURE" => Some("repeated_failures_observed"),
            _ => None,
        };
        if let Some(finding) = finding {
            accumulators
                .entry(finding)
                .or_default()
                .observe(event_id.clone());
        }
    }
}

fn push_finding(
    findings: &mut Vec<RiskFinding>,
    accumulators: &HashMap<&'static str, FindingAccumulator>,
    code: &'static str,
    tier: &'static str,
    reason_codes: &[&'static str],
    reviewer_guidance: &'static str,
) {
    push_finding_if_at_least(
        findings,
        accumulators,
        code,
        1,
        tier,
        reason_codes,
        reviewer_guidance,
    );
}

fn push_finding_if_at_least(
    findings: &mut Vec<RiskFinding>,
    accumulators: &HashMap<&'static str, FindingAccumulator>,
    code: &'static str,
    minimum: u32,
    tier: &'static str,
    reason_codes: &[&'static str],
    reviewer_guidance: &'static str,
) {
    if let Some(value) = accumulators
        .get(code)
        .filter(|value| value.count >= minimum)
    {
        findings.push(RiskFinding {
            code,
            tier,
            observed_count: value.count,
            reason_codes: reason_codes.to_vec(),
            event_ids: value.event_ids.clone(),
            reviewer_guidance,
        });
    }
}

fn tier_rank(tier: &str) -> u8 {
    match tier {
        "review" => 3,
        "observe" => 2,
        "info" => 1,
        _ => 0,
    }
}

async fn current_user_id(state: &AppState, headers: &HeaderMap) -> Result<i64, StatusCode> {
    let subject = crate::auth::extract_current_user(&state.auth, &state.db, headers)
        .await
        .ok_or(StatusCode::UNAUTHORIZED)?;
    let row = sqlx::query("SELECT id FROM users WHERE auth_subject = $1")
        .bind(subject)
        .fetch_optional(&state.db)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::UNAUTHORIZED)?;
    row.try_get("id")
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

async fn require_admin(state: &AppState, headers: &HeaderMap) -> Result<(), StatusCode> {
    let user_id = current_user_id(state, headers).await?;
    let is_admin: bool = sqlx::query_scalar("SELECT is_admin FROM users WHERE id = $1")
        .bind(user_id)
        .fetch_one(&state.db)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if is_admin {
        Ok(())
    } else {
        Err(StatusCode::FORBIDDEN)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn findings_are_conservative_and_review_only() {
        let rows = Vec::new();
        let result = findings_from_rows(rows);
        assert!(result.is_ok());
        if let Ok(result) = result {
            assert_eq!(result.tier, "none");
            assert_eq!(
                result.disposition,
                "manual_review_only_no_automatic_enforcement"
            );
        }
    }

    #[test]
    fn maps_only_closed_reason_codes() {
        let mut values = HashMap::new();
        observe_record(
            &mut values,
            "event-1".into(),
            &serde_json::json!({"reason_codes":["CLOCK_SKEW", "UNTRUSTED_TEXT"]}),
            true,
        );
        assert_eq!(
            values.get("clock_skew_observed").map(|item| item.count),
            Some(1)
        );
        assert!(!values.contains_key("UNTRUSTED_TEXT"));
    }
}
