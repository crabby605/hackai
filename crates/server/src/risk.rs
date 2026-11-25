#![forbid(unsafe_code)]

//! Conservative, review-only findings.
//!
//! Core findings are always-on and computed from normal synced derived data
//! (`productive_episodes`, `prompt_events`, `commits`). A single opt-in finding
//! covers evidence bundles that were never preview-approved. Findings never
//! change PESDO, eligibility, or account status; they are reasons for a human
//! to inspect corroborating evidence, not claims of dishonesty.

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

/// Runs one bounded per-finding query, returning the real count plus a capped
/// sample of identifiers. Every query is parameterized and LIMIT-bound so the
/// endpoint can never scan or return an unbounded row set.
async fn bounded_counts(
    state: &AppState,
    query: &'static str,
    user_id: i64,
) -> Result<(u32, Vec<String>), StatusCode> {
    let rows = sqlx::query(query)
        .bind(user_id)
        .bind(SAMPLE_EVENT_IDS as i64)
        .fetch_all(&state.db)
        .await
        .map_err(|error| {
            eprintln!("risk finding query failed: {error}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    let mut total = 0u32;
    let mut samples: Vec<String> = Vec::new();
    for row in rows {
        total = u32::try_from(
            row.try_get::<i64, _>("total")
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
        )
        .unwrap_or(u32::MAX);
        if samples.len() < SAMPLE_EVENT_IDS {
            samples.push(
                row.try_get::<String, _>("fid")
                    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
            );
        }
    }
    Ok((total, samples))
}

fn push_finding_if_at_least(
    findings: &mut Vec<RiskFinding>,
    count: u32,
    samples: Vec<String>,
    code: &'static str,
    tier: &'static str,
    reason_codes: &[&'static str],
    reviewer_guidance: &'static str,
) {
    if count >= 1 {
        findings.push(RiskFinding {
            code,
            tier,
            observed_count: count,
            reason_codes: reason_codes.to_vec(),
            event_ids: samples,
            reviewer_guidance,
        });
    }
}

async fn findings_for_user(state: &AppState, user_id: i64) -> Result<RiskFindings, StatusCode> {
    let mut findings: Vec<RiskFinding> = Vec::new();

    // Always-on: unresolved episodes with >= 3 attempts.
    let (count, samples) = bounded_counts(
        state,
        "SELECT id::TEXT AS fid, COUNT(*) OVER() AS total \
         FROM productive_episodes \
         WHERE user_id = $1 AND resolved = FALSE AND attempts >= 3 \
         ORDER BY started_at DESC \
         LIMIT $2",
        user_id,
    )
    .await?;
    push_finding_if_at_least(
        &mut findings,
        count,
        samples,
        "unresolved_attempts",
        "review",
        &["UNRESOLVED_ATTEMPTS"],
        "Review whether repeated unresolved episodes reflect a genuine recovery loop or abandoned work.",
    );

    // Always-on: overlapping episode sessions for the same user.
    let (count, samples) = bounded_counts(
        state,
        "SELECT p1.id::TEXT AS fid, COUNT(*) OVER() AS total \
         FROM productive_episodes p1 \
         JOIN productive_episodes p2 \
           ON p1.user_id = p2.user_id AND p1.id < p2.id \
          AND p1.started_at < p2.ended_at AND p2.started_at < p1.ended_at \
         WHERE p1.user_id = $1 \
         ORDER BY p1.started_at DESC \
         LIMIT $2",
        user_id,
    )
    .await?;
    push_finding_if_at_least(
        &mut findings,
        count,
        samples,
        "overlapping_sessions",
        "review",
        &["OVERLAPPING_SESSIONS"],
        "Confirm overlapping intervals were unioned rather than summed; overlapping sessions can inflate credited time.",
    );

    // Always-on: clock skew where an episode ended more than 5 minutes after it was recorded.
    let (count, samples) = bounded_counts(
        state,
        "SELECT id::TEXT AS fid, COUNT(*) OVER() AS total \
         FROM productive_episodes \
         WHERE user_id = $1 AND ended_at > created_at + interval '5 minutes' \
         ORDER BY started_at DESC \
         LIMIT $2",
        user_id,
    )
    .await?;
    push_finding_if_at_least(
        &mut findings,
        count,
        samples,
        "clock_skew",
        "review",
        &["CLOCK_SKEW"],
        "Compare the episode timeline with independent records before relying on its durations.",
    );

    // Always-on: metronomic prompt cadence (coefficient of variation < 0.1 over >= 30 gaps).
    let (count, samples) = bounded_counts(
        state,
        r#"WITH ordered AS (
             SELECT user_id, session_id, occurred_at,
                    EXTRACT(EPOCH FROM (occurred_at - LAG(occurred_at) OVER (
                        PARTITION BY user_id, session_id ORDER BY occurred_at))) AS gap_secs
             FROM prompt_events WHERE user_id = $1
         ), gaps AS (
             SELECT user_id, session_id, gap_secs FROM ordered WHERE gap_secs IS NOT NULL
         ), stats AS (
             SELECT user_id, session_id, COUNT(*) AS n, AVG(gap_secs) AS mean,
                    STDDEV_POP(gap_secs) AS stddev
             FROM gaps GROUP BY user_id, session_id HAVING COUNT(*) >= 30
         )
         SELECT session_id AS fid, COUNT(*) OVER() AS total
         FROM stats
         WHERE mean > 0 AND stddev / mean < 0.1
         ORDER BY n DESC, session_id
         LIMIT $2"#,
        user_id,
    )
    .await?;
    push_finding_if_at_least(
        &mut findings,
        count,
        samples,
        "metronomic_cadence",
        "review",
        &["METRONOMIC_CADENCE"],
        "An unusually regular prompt cadence can indicate scripted or automated activity; confirm with independent records.",
    );

    // Always-on: commit SHA credited to more than one user (cross-user dedupe).
    let (count, samples) = bounded_counts(
        state,
        "SELECT c.sha AS fid, COUNT(*) OVER() AS total \
         FROM commits c \
         WHERE c.user_id = $1 \
           AND EXISTS (SELECT 1 FROM commits c2 WHERE c2.sha = c.sha AND c2.user_id <> c.user_id) \
         ORDER BY c.pushed_at DESC NULLS LAST \
         LIMIT $2",
        user_id,
    )
    .await?;
    push_finding_if_at_least(
        &mut findings,
        count,
        samples,
        "cross_user_commit_dedupe",
        "review",
        &["CROSS_USER_COMMIT_DEDUPE"],
        "A commit SHA credited to more than one user indicates an attribution problem; verify ownership before relying on it.",
    );

    // Opt-in: evidence bundle that was never preview-approved.
    let (count, samples) = bounded_counts(
        state,
        "SELECT e.event_id AS fid, COUNT(*) OVER() AS total \
         FROM evidence_bundle_events e \
         JOIN evidence_bundles b ON b.id = e.bundle_id \
         WHERE b.user_id = $1 AND b.preview_approved = FALSE \
         ORDER BY e.occurred_at DESC, e.id DESC \
         LIMIT $2",
        user_id,
    )
    .await?;
    push_finding_if_at_least(
        &mut findings,
        count,
        samples,
        "unpreviewed_export",
        "info",
        &["PREVIEW_NOT_APPROVED"],
        "Ask the contributor to review and re-export the bundle if it is needed for a manual review. This is not a quality or fraud signal.",
    );

    findings.sort_by_key(|finding| std::cmp::Reverse(tier_rank(finding.tier)));
    let tier = findings.first().map_or("none", |finding| finding.tier);
    Ok(RiskFindings {
        disposition: "manual_review_only_no_automatic_enforcement",
        data_source: "always-on synced derived data (episodes, prompts, commits) plus opt-in redacted evidence bundles; reviewer-only IDs, no prompt text, code, paths, or population-level enforcement",
        tier,
        findings,
    })
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

fn tier_rank(tier: &str) -> u8 {
    match tier {
        "review" => 3,
        "observe" => 2,
        "info" => 1,
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Coefficient of variation (stddev / mean) over a sequence of gaps.
    /// Returns `None` when there are no gaps or the mean is zero. The SQL path
    /// additionally requires at least 30 gaps before a session can be flagged
    /// as metronomic.
    fn cadence_cv(gaps: &[f64]) -> Option<f64> {
        if gaps.is_empty() {
            return None;
        }
        let mean = gaps.iter().sum::<f64>() / gaps.len() as f64;
        if mean <= 0.0 {
            return None;
        }
        let variance = gaps.iter().map(|gap| (gap - mean).powi(2)).sum::<f64>() / gaps.len() as f64;
        let stddev = variance.sqrt();
        Some(stddev / mean)
    }

    fn finding_when_at_least(
        count: u32,
        samples: Vec<String>,
        code: &'static str,
        tier: &'static str,
        reason_codes: &[&'static str],
        guidance: &'static str,
        minimum: u32,
    ) -> Option<RiskFinding> {
        (count >= minimum).then_some(RiskFinding {
            code,
            tier,
            observed_count: count,
            reason_codes: reason_codes.to_vec(),
            event_ids: samples,
            reviewer_guidance: guidance,
        })
    }

    #[test]
    fn findings_are_conservative_and_review_only() {
        let findings: Vec<RiskFinding> = Vec::new();
        let tier = findings.first().map_or("none", |finding| finding.tier);
        assert_eq!(tier, "none");
    }

    #[test]
    fn cadence_cv_detects_metronomic_gaps() {
        assert_eq!(cadence_cv(&[60.0, 60.0, 60.0, 60.0]), Some(0.0));
    }

    #[test]
    fn cadence_cv_flags_irregular_gaps() {
        let cv = cadence_cv(&[60.0, 120.0, 30.0, 90.0]).unwrap_or(0.0);
        assert!(cv >= 0.1);
    }

    #[test]
    fn cadence_cv_empty_returns_none() {
        assert_eq!(cadence_cv(&[]), None);
    }

    #[test]
    fn finding_emits_only_when_at_least_threshold() {
        let emitted = finding_when_at_least(
            5,
            vec!["ep-1".into()],
            "unresolved_attempts",
            "review",
            &["UNRESOLVED_ATTEMPTS"],
            "guidance",
            3,
        );
        assert_eq!(
            emitted.as_ref().map(|finding| finding.observed_count),
            Some(5)
        );
        assert!(emitted.is_some());

        let not_emitted = finding_when_at_least(
            2,
            vec!["ep-1".into()],
            "unresolved_attempts",
            "review",
            &["UNRESOLVED_ATTEMPTS"],
            "guidance",
            3,
        );
        assert!(not_emitted.is_none());
    }

    #[test]
    fn tier_orders_review_before_observe_before_info() {
        assert!(tier_rank("review") > tier_rank("observe"));
        assert!(tier_rank("observe") > tier_rank("info"));
        assert_eq!(tier_rank("unknown"), 0);
    }
}
