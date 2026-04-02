/// Studio transactions API: real-time cross-service request/response log.
///
/// Routes:
///   GET  /_localstack/studio-api/transactions
///   GET  /_localstack/studio-api/transactions/{service}
///   POST /_localstack/studio-api/transactions/record   (gateway → internal, internal use)
///   DELETE /_localstack/studio-api/transactions        (clear log)
use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use openstack_studio_ui::TransactionOutcome;
use serde::Deserialize;
use serde_json::json;

use crate::ApiState;

// ---------------------------------------------------------------------------
// Query params
// ---------------------------------------------------------------------------

/// Filter parameters for GET /transactions and GET /transactions/{service}.
#[derive(Debug, Deserialize)]
pub struct TransactionQueryParams {
    /// Filter by outcome: `success`, `client_error`, `server_error`, `pending`.
    pub outcome: Option<String>,
    /// Maximum number of entries to return (default 200, max 2000).
    pub limit: Option<usize>,
    /// Include only guided-flow transactions.
    pub guided_only: Option<bool>,
}

fn parse_outcome(s: &str) -> Option<TransactionOutcome> {
    match s {
        "success" => Some(TransactionOutcome::Success),
        "client_error" => Some(TransactionOutcome::ClientError),
        "server_error" => Some(TransactionOutcome::ServerError),
        "pending" => Some(TransactionOutcome::Pending),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Inbound record payload
// ---------------------------------------------------------------------------

/// Payload posted by the gateway to append a completed transaction.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordTransactionRequest {
    pub service: String,
    pub operation: Option<String>,
    pub method: String,
    pub path: String,
    pub status: u16,
    pub request_body_preview: Option<String>,
    pub response_body_preview: Option<String>,
    pub started_at_ms: u64,
    pub duration_ms: u64,
    pub from_guided_flow: Option<bool>,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn record_to_json(r: &openstack_studio_ui::TransactionRecord) -> serde_json::Value {
    json!({
        "id": r.id,
        "service": r.service,
        "operation": r.operation,
        "method": r.method,
        "path": r.path,
        "status": r.status,
        "outcome": r.outcome.label(),
        "duration_ms": r.duration_ms,
        "started_at_ms": r.started_at_ms,
        "from_guided_flow": r.from_guided_flow,
        "request_body_preview": r.request_body_preview,
        "response_body_preview": r.response_body_preview,
    })
}

fn summary_to_json(log: &openstack_studio_ui::TransactionLog) -> serde_json::Value {
    let s = log.summary();
    json!({
        "total": s.total,
        "success": s.success,
        "client_error": s.client_error,
        "server_error": s.server_error,
        "pending": s.pending,
        "avg_duration_ms": s.avg_duration_ms,
    })
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// GET /_localstack/studio-api/transactions
///
/// Returns the global transaction log, optionally filtered.
pub async fn list_all_transactions(
    State(state): State<ApiState>,
    Query(params): Query<TransactionQueryParams>,
) -> impl IntoResponse {
    let log = state.transaction_log.lock().await;
    let limit = params.limit.unwrap_or(200).min(2000);
    let outcome_filter = params.outcome.as_deref().and_then(parse_outcome);
    let guided_only = params.guided_only.unwrap_or(false);

    let records: Vec<serde_json::Value> = log
        .list()
        .filter(|r| {
            let outcome_ok = outcome_filter.map(|o| r.outcome == o).unwrap_or(true);
            let guided_ok = !guided_only || r.from_guided_flow;
            outcome_ok && guided_ok
        })
        .take(limit)
        .map(record_to_json)
        .collect();

    Json(json!({
        "schema_version": "1.0",
        "summary": summary_to_json(&log),
        "transactions": records,
    }))
}

/// GET /_localstack/studio-api/transactions/{service}
///
/// Returns transactions for a single service, optionally filtered.
pub async fn list_service_transactions(
    State(state): State<ApiState>,
    Path(service): Path<String>,
    Query(params): Query<TransactionQueryParams>,
) -> impl IntoResponse {
    let log = state.transaction_log.lock().await;
    let limit = params.limit.unwrap_or(200).min(2000);
    let outcome_filter = params.outcome.as_deref().and_then(parse_outcome);
    let guided_only = params.guided_only.unwrap_or(false);

    let records: Vec<serde_json::Value> = log
        .for_service(&service)
        .filter(|r| {
            let outcome_ok = outcome_filter.map(|o| r.outcome == o).unwrap_or(true);
            let guided_ok = !guided_only || r.from_guided_flow;
            outcome_ok && guided_ok
        })
        .take(limit)
        .map(record_to_json)
        .collect();

    let total = log.for_service(&service).count();

    Json(json!({
        "schema_version": "1.0",
        "service": service,
        "total": total,
        "transactions": records,
    }))
}

/// POST /_localstack/studio-api/transactions/record
///
/// Appends a completed transaction to the log.
/// Called by the gateway after every handled request (fire-and-forget).
pub async fn record_transaction(
    State(state): State<ApiState>,
    Json(payload): Json<RecordTransactionRequest>,
) -> impl IntoResponse {
    use openstack_studio_ui::TransactionRecord;

    let mut record = TransactionRecord::new(
        0,
        payload.service,
        payload.method,
        payload.path,
        payload.started_at_ms,
    );

    if let Some(op) = payload.operation {
        record = record.with_operation(op);
    }
    if let Some(body) = payload.request_body_preview {
        record = record.with_request_body(body);
    }
    if payload.from_guided_flow.unwrap_or(false) {
        record = record.with_guided();
    }

    let response_body = payload.response_body_preview.as_deref().unwrap_or("");
    record = record.complete(payload.status, response_body, payload.duration_ms);

    let mut log = state.transaction_log.lock().await;
    let id = log.push(record);

    (
        StatusCode::CREATED,
        Json(json!({ "id": id })),
    )
        .into_response()
}

/// DELETE /_localstack/studio-api/transactions
///
/// Clears the in-memory transaction log.  Useful for test isolation.
pub async fn clear_transactions(State(state): State<ApiState>) -> impl IntoResponse {
    let mut log = state.transaction_log.lock().await;
    let count = log.len();
    log.clear();
    Json(json!({ "cleared": count }))
}
