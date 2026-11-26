#![forbid(unsafe_code)]

//! Opt-in ingestion for privacy-preserving evidence bundles.
//!
//! This endpoint intentionally accepts a closed, derived-only schema.  It does
//! not accept prompt text, code, paths, tool output, or arbitrary JSON metadata.

use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::Json as ResponseJson,
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use sqlx::Row;

use crate::AppState;

const MAX_EVENTS: usize = 5_000;
const MAX_REASON_CODES: usize = 16;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvidenceBundleRequest {
    pub bundle_id: String,
    pub schema_version: u16,
    pub processor_version: String,
    pub processor_hash: String,
    pub formula_version: String,
    pub client_version: String,
    pub generated_at_unix_ms: i64,
    #[serde(default)]
    pub batch_seq: Option<i64>,
    #[serde(default)]
    pub prev_batch_hash: Option<String>,
    pub preview_approved: bool,
    pub events: Vec<DerivedEvent>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DerivedEvent {
    pub event_id: String,
    pub ts_utc_ms: i64,
    pub ts_seq_idx: u32,
    pub source_kind: SourceKind,
    pub harness: Harness,
    pub event_kind: EventKind,
    pub session_id_hash: String,
    #[serde(default)]
    pub authorship: Option<Authorship>,
    #[serde(default)]
    pub project_id_hash: Option<String>,
    #[serde(default)]
    pub machine_id_hash: Option<String>,
    #[serde(default)]
    pub duration_ms: Option<u64>,
    #[serde(default)]
    pub lines_added: Option<u32>,
    #[serde(default)]
    pub lines_removed: Option<u32>,
    #[serde(default)]
    pub test_exit_code: Option<i32>,
    #[serde(default)]
    pub prompt_features: Option<PromptFeatures>,
    #[serde(default)]
    pub dialogue: Option<DialogueTrace>,
    #[serde(default)]
    pub reason_codes: Vec<ReasonCode>,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    Transcript,
    Git,
    FsWatch,
    Otel,
    Hook,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Harness {
    ClaudeCode,
    Codex,
    Opencode,
    Gemini,
    Aider,
    QwenCode,
    KimiCode,
    ZCode,
    Copilot,
    Unknown,
}

/// Typed authorship for a derived event.  Only positive human or agent
/// evidence is labelled; anything unresolved (initial commits, dirty worktrees,
/// unmatched records) is unknown, never human by default.
#[derive(Deserialize, Serialize, Clone, Copy, Debug, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Authorship {
    Human,
    Agent,
    Unknown,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    PromptSubmit,
    QueueOp,
    AgentTurnEnd,
    ToolCall,
    ToolResult,
    PermissionRequest,
    PermissionDecision,
    Interrupt,
    HumanEdit,
    TestRun,
    BuildRun,
    Commit,
    Compaction,
    Clear,
    Resume,
    SessionStart,
    SessionEnd,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PromptFeatures {
    pub length_bucket: LengthBucket,
    pub anchor_count_bucket: CountBucket,
    pub fullcaps: bool,
    #[serde(default)]
    pub simhash64: Option<String>,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LengthBucket {
    Empty,
    Short,
    Medium,
    Long,
    VeryLong,
}
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CountBucket {
    Zero,
    One,
    TwoToThree,
    FourPlus,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DialogueTrace {
    pub user_act: UserAct,
    pub act_confidence_bucket: ConfidenceBucket,
    pub ctx_evidence: bool,
    pub ctx_expectation: bool,
    pub ctx_location: bool,
    pub agent_state: AgentState,
    pub verification: Verification,
    pub files_changed_bucket: CountBucket,
    pub tool_errors: u16,
    #[serde(default)]
    pub reply_to_event_id: Option<String>,
    pub rule_version: String,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UserAct {
    Specify,
    Nudge,
    ReportFailure,
    Steer,
    Decide,
    Approve,
    RejectOrRevert,
    Interrupt,
    AskToUnderstand,
    Meta,
}
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfidenceBucket {
    Low,
    Medium,
    High,
}
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentState {
    Asked,
    Planned,
    Explained,
    EditedGreen,
    EditedRed,
    EditedUnverified,
    ClaimedDoneUnverified,
    ToolError,
    Blocked,
    Looping,
}
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Verification {
    Green,
    Red,
    None,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ReasonCode {
    AnchorFound,
    ErrorPreviouslyObserved,
    TestPassed,
    TestFailed,
    RepeatedEdit,
    RepeatedFailure,
    HumanEditObserved,
    CommitObserved,
    PresenceCapped,
    ConcurrentSession,
    ClockSkew,
}

#[derive(Serialize)]
pub struct EvidenceBundleResponse {
    pub status: &'static str,
    pub bundle_id: String,
    pub events_stored: usize,
}

#[derive(Serialize)]
pub struct EvidenceBundleSummary {
    pub bundle_id: String,
    pub processor_version: String,
    pub processor_hash: String,
    pub formula_version: String,
    pub generated_at: String,
    pub received_at: String,
    pub event_count: i64,
    pub preview_approved: bool,
}

#[derive(Serialize)]
pub struct EvidenceBundleDetail {
    #[serde(flatten)]
    pub summary: EvidenceBundleSummary,
    pub batch_seq: Option<i64>,
    pub prev_batch_hash: Option<String>,
    pub events: Vec<StoredDerivedEvent>,
}

#[derive(Serialize)]
pub struct StoredDerivedEvent {
    pub event_id: String,
    pub occurred_at: String,
    pub event_kind: String,
    pub session_id_hash: String,
    pub record: serde_json::Value,
}

pub fn routes() -> axum::Router<AppState> {
    axum::Router::new()
        .route(
            "/api/v1/evidence-bundles",
            post(ingest_bundle).get(list_mine),
        )
        .route("/api/v1/evidence-bundles/{bundle_id}", get(get_mine))
        .route(
            "/api/v1/admin/users/{user_id}/evidence-bundles",
            get(list_for_admin),
        )
        .route(
            "/api/v1/admin/users/{user_id}/evidence-bundles/{bundle_id}",
            get(get_for_admin),
        )
}

async fn ingest_bundle(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<EvidenceBundleRequest>,
) -> Result<ResponseJson<EvidenceBundleResponse>, StatusCode> {
    validate_bundle(&request).map_err(|_| StatusCode::UNPROCESSABLE_ENTITY)?;
    let sub = crate::auth::extract_current_user(&state.auth, &state.db, &headers)
        .await
        .ok_or(StatusCode::UNAUTHORIZED)?;
    if crate::auth::is_user_in_cooldown(&state.db, &sub).await {
        return Err(StatusCode::TOO_MANY_REQUESTS);
    }
    let user_id: i64 = sqlx::query("SELECT id FROM users WHERE auth_subject = $1")
        .bind(sub)
        .fetch_optional(&state.db)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::UNAUTHORIZED)?
        .try_get("id")
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let mut transaction = state
        .db
        .begin()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let bundle = sqlx::query("INSERT INTO evidence_bundles (user_id, bundle_id, schema_version, processor_version, processor_hash, formula_version, client_version, generated_at, batch_seq, prev_batch_hash, preview_approved) VALUES ($1,$2,$3,$4,$5,$6,$7,to_timestamp($8::double precision / 1000),$9,$10,$11) ON CONFLICT (user_id, bundle_id) DO NOTHING RETURNING id")
        .bind(user_id).bind(&request.bundle_id).bind(i32::from(request.schema_version)).bind(&request.processor_version).bind(&request.processor_hash).bind(&request.formula_version).bind(&request.client_version).bind(request.generated_at_unix_ms).bind(request.batch_seq).bind(&request.prev_batch_hash).bind(request.preview_approved)
        .fetch_optional(&mut *transaction).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let Some(bundle) = bundle else {
        transaction
            .commit()
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        return Ok(ResponseJson(EvidenceBundleResponse {
            status: "already_ingested",
            bundle_id: request.bundle_id,
            events_stored: 0,
        }));
    };
    let bundle_id: i64 = bundle
        .try_get("id")
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    for event in &request.events {
        sqlx::query("INSERT INTO evidence_bundle_events (bundle_id, event_id, occurred_at, event_kind, session_id_hash, record) VALUES ($1,$2,to_timestamp($3::double precision / 1000),$4,$5,$6)")
            .bind(bundle_id).bind(&event.event_id).bind(event.ts_utc_ms).bind(event_kind_name(&event.event_kind)).bind(&event.session_id_hash).bind(sqlx::types::Json(event))
            .execute(&mut *transaction).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    }
    transaction
        .commit()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(ResponseJson(EvidenceBundleResponse {
        status: "ingested",
        bundle_id: request.bundle_id,
        events_stored: request.events.len(),
    }))
}

async fn list_mine(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<ResponseJson<Vec<EvidenceBundleSummary>>, StatusCode> {
    list_bundles(&state, current_user_id(&state, &headers).await?)
        .await
        .map(ResponseJson)
}

async fn get_mine(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(bundle_id): Path<String>,
) -> Result<ResponseJson<EvidenceBundleDetail>, StatusCode> {
    if !valid_id(&bundle_id, 128) {
        return Err(StatusCode::BAD_REQUEST);
    }
    bundle_detail(&state, current_user_id(&state, &headers).await?, &bundle_id)
        .await
        .map(ResponseJson)
}

async fn list_for_admin(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(user_id): Path<i64>,
) -> Result<ResponseJson<Vec<EvidenceBundleSummary>>, StatusCode> {
    require_admin(&state, &headers).await?;
    list_bundles(&state, user_id).await.map(ResponseJson)
}

async fn get_for_admin(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((user_id, bundle_id)): Path<(i64, String)>,
) -> Result<ResponseJson<EvidenceBundleDetail>, StatusCode> {
    if !valid_id(&bundle_id, 128) {
        return Err(StatusCode::BAD_REQUEST);
    }
    require_admin(&state, &headers).await?;
    bundle_detail(&state, user_id, &bundle_id)
        .await
        .map(ResponseJson)
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

async fn require_admin(state: &AppState, headers: &HeaderMap) -> Result<(), StatusCode> {
    let user_id = current_user_id(state, headers).await?;
    let row = sqlx::query("SELECT is_admin FROM users WHERE id = $1")
        .bind(user_id)
        .fetch_one(&state.db)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if row
        .try_get::<bool, _>("is_admin")
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    {
        Ok(())
    } else {
        Err(StatusCode::FORBIDDEN)
    }
}

async fn list_bundles(
    state: &AppState,
    user_id: i64,
) -> Result<Vec<EvidenceBundleSummary>, StatusCode> {
    let rows = sqlx::query("SELECT b.bundle_id, b.processor_version, b.processor_hash, b.formula_version, b.generated_at::TEXT AS generated_at, b.received_at::TEXT AS received_at, b.preview_approved, COUNT(e.id) AS event_count FROM evidence_bundles b LEFT JOIN evidence_bundle_events e ON e.bundle_id = b.id WHERE b.user_id = $1 GROUP BY b.id ORDER BY b.received_at DESC LIMIT 100")
        .bind(user_id).fetch_all(&state.db).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    rows.iter().map(summary_from_row).collect()
}

async fn bundle_detail(
    state: &AppState,
    user_id: i64,
    requested_bundle_id: &str,
) -> Result<EvidenceBundleDetail, StatusCode> {
    let row = sqlx::query("SELECT b.id, b.bundle_id, b.processor_version, b.processor_hash, b.formula_version, b.generated_at::TEXT AS generated_at, b.received_at::TEXT AS received_at, b.preview_approved, b.batch_seq, b.prev_batch_hash, COUNT(e.id) AS event_count FROM evidence_bundles b LEFT JOIN evidence_bundle_events e ON e.bundle_id = b.id WHERE b.user_id = $1 AND b.bundle_id = $2 GROUP BY b.id")
        .bind(user_id).bind(requested_bundle_id).fetch_optional(&state.db).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?.ok_or(StatusCode::NOT_FOUND)?;
    let id: i64 = row
        .try_get("id")
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let event_rows = sqlx::query("SELECT event_id, occurred_at::TEXT AS occurred_at, event_kind, session_id_hash, record FROM evidence_bundle_events WHERE bundle_id = $1 ORDER BY occurred_at, id").bind(id).fetch_all(&state.db).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let events = event_rows
        .iter()
        .map(|event| {
            Ok::<StoredDerivedEvent, StatusCode>(StoredDerivedEvent {
                event_id: event
                    .try_get("event_id")
                    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
                occurred_at: event
                    .try_get("occurred_at")
                    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
                event_kind: event
                    .try_get("event_kind")
                    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
                session_id_hash: event
                    .try_get("session_id_hash")
                    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
                record: event
                    .try_get("record")
                    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(EvidenceBundleDetail {
        summary: summary_from_row(&row)?,
        batch_seq: row
            .try_get("batch_seq")
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
        prev_batch_hash: row
            .try_get("prev_batch_hash")
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
        events,
    })
}

fn summary_from_row(row: &sqlx::postgres::PgRow) -> Result<EvidenceBundleSummary, StatusCode> {
    Ok(EvidenceBundleSummary {
        bundle_id: row
            .try_get("bundle_id")
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
        processor_version: row
            .try_get("processor_version")
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
        processor_hash: row
            .try_get("processor_hash")
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
        formula_version: row
            .try_get("formula_version")
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
        generated_at: row
            .try_get("generated_at")
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
        received_at: row
            .try_get("received_at")
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
        event_count: row
            .try_get("event_count")
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
        preview_approved: row
            .try_get("preview_approved")
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
    })
}

fn event_kind_name(value: &EventKind) -> &'static str {
    match value {
        EventKind::PromptSubmit => "prompt_submit",
        EventKind::QueueOp => "queue_op",
        EventKind::AgentTurnEnd => "agent_turn_end",
        EventKind::ToolCall => "tool_call",
        EventKind::ToolResult => "tool_result",
        EventKind::PermissionRequest => "permission_request",
        EventKind::PermissionDecision => "permission_decision",
        EventKind::Interrupt => "interrupt",
        EventKind::HumanEdit => "human_edit",
        EventKind::TestRun => "test_run",
        EventKind::BuildRun => "build_run",
        EventKind::Commit => "commit",
        EventKind::Compaction => "compaction",
        EventKind::Clear => "clear",
        EventKind::Resume => "resume",
        EventKind::SessionStart => "session_start",
        EventKind::SessionEnd => "session_end",
    }
}

fn valid_hash(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}
fn valid_id(value: &str, max_len: usize) -> bool {
    !value.is_empty()
        && value.len() <= max_len
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':'))
}

fn validate_bundle(bundle: &EvidenceBundleRequest) -> Result<(), ()> {
    if !valid_id(&bundle.bundle_id, 128)
        || bundle.schema_version == 0
        || !valid_id(&bundle.processor_version, 128)
        || !valid_hash(&bundle.processor_hash)
        || !valid_id(&bundle.formula_version, 128)
        || !valid_id(&bundle.client_version, 128)
        || bundle.generated_at_unix_ms < 0
        || bundle.batch_seq.is_some_and(|value| value < 0)
        || bundle
            .prev_batch_hash
            .as_deref()
            .is_some_and(|value| !valid_hash(value))
        || bundle.events.is_empty()
        || bundle.events.len() > MAX_EVENTS
    {
        return Err(());
    }
    for event in &bundle.events {
        if !valid_id(&event.event_id, 128)
            || event.ts_utc_ms < 0
            || !valid_id(&event.session_id_hash, 128)
            || event.session_id_hash.len() < 16
            || event
                .project_id_hash
                .as_deref()
                .is_some_and(|value| !valid_hash(value))
            || event
                .machine_id_hash
                .as_deref()
                .is_some_and(|value| !valid_hash(value))
            || event.duration_ms.is_some_and(|value| value > 86_400_000)
            || event.reason_codes.len() > MAX_REASON_CODES
            || event.prompt_features.as_ref().is_some_and(|features| {
                features.simhash64.as_deref().is_some_and(|value| {
                    value.len() != 16 || !value.bytes().all(|byte| byte.is_ascii_hexdigit())
                })
            })
        {
            return Err(());
        }
        if let Some(dialogue) = &event.dialogue
            && (!valid_id(&dialogue.rule_version, 128)
                || dialogue
                    .reply_to_event_id
                    .as_deref()
                    .is_some_and(|value| !valid_id(value, 128)))
        {
            return Err(());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_raw_or_non_derived_bundle_values() {
        let value = serde_json::json!({"bundle_id":"bundle-1", "schema_version":1, "processor_version":"v1", "processor_hash":"a".repeat(64), "formula_version":"v1", "client_version":"v1", "generated_at_unix_ms":1, "preview_approved":true, "events":[], "prompt_text":"do the thing"});
        assert!(serde_json::from_value::<EvidenceBundleRequest>(value).is_err());
    }

    #[test]
    fn accepts_typed_authorship_in_closed_schema() {
        let value = serde_json::json!({
            "bundle_id":"bundle-1",
            "schema_version":1,
            "processor_version":"v1",
            "processor_hash":"a".repeat(64),
            "formula_version":"v1",
            "client_version":"v1",
            "generated_at_unix_ms":1,
            "preview_approved":true,
            "events":[
                {"event_id":"evt-1","ts_utc_ms":1,"ts_seq_idx":0,"source_kind":"git","harness":"claude_code","event_kind":"commit","session_id_hash":"x".repeat(32),"authorship":"agent"}
            ]
        });
        assert!(serde_json::from_value::<EvidenceBundleRequest>(value).is_ok());
    }

    #[test]
    fn missing_authorship_defaults_to_unknown() {
        let value = serde_json::json!({
            "bundle_id":"bundle-1",
            "schema_version":1,
            "processor_version":"v1",
            "processor_hash":"a".repeat(64),
            "formula_version":"v1",
            "client_version":"v1",
            "generated_at_unix_ms":1,
            "preview_approved":true,
            "events":[
                {"event_id":"evt-1","ts_utc_ms":1,"ts_seq_idx":0,"source_kind":"git","harness":"claude_code","event_kind":"commit","session_id_hash":"x".repeat(32)}
            ]
        });
        let request: EvidenceBundleRequest =
            serde_json::from_value(value).unwrap_or_else(|_| unreachable!());
        assert_eq!(request.events[0].authorship, None);
    }

    #[test]
    fn rejects_unknown_fields_still_enforced() {
        let value = serde_json::json!({
            "bundle_id":"bundle-1",
            "schema_version":1,
            "processor_version":"v1",
            "processor_hash":"a".repeat(64),
            "formula_version":"v1",
            "client_version":"v1",
            "generated_at_unix_ms":1,
            "preview_approved":true,
            "events":[
                {"event_id":"evt-1","ts_utc_ms":1,"ts_seq_idx":0,"source_kind":"git","harness":"claude_code","event_kind":"commit","session_id_hash":"x".repeat(32),"authorship":"agent","prompt_text":"leak"}
            ]
        });
        assert!(serde_json::from_value::<EvidenceBundleRequest>(value).is_err());
    }
}
