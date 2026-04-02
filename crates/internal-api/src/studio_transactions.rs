/// Studio transactions API: real-time cross-service request/response log.
///
/// When Studio is disabled (`ApiState::transaction_log` is `None`) all
/// read endpoints return empty results and record/clear are silent no-ops.
/// This keeps the binary lean in headless / benchmark mode.
use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use openstack_studio_ui::{TransactionOutcome, to_provider_slug};
use serde::Deserialize;
use serde_json::json;

use crate::ApiState;

#[derive(Debug, Deserialize)]
pub struct TransactionQueryParams {
    pub outcome: Option<String>,
    pub limit: Option<usize>,
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

fn empty_summary() -> serde_json::Value {
    json!({ "total": 0, "success": 0, "client_error": 0, "server_error": 0, "pending": 0, "avg_duration_ms": null })
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

pub async fn list_all_transactions(
    State(state): State<ApiState>,
    Query(params): Query<TransactionQueryParams>,
) -> impl IntoResponse {
    let Some(log_arc) = &state.transaction_log else {
        return Json(json!({
            "schema_version": "1.0",
            "summary": empty_summary(),
            "transactions": [],
            "_studio_disabled": true,
        }));
    };
    let log = log_arc.lock().await;
    let limit = params.limit.unwrap_or(200).min(2000);
    let outcome_filter = params.outcome.as_deref().and_then(parse_outcome);
    let guided_only = params.guided_only.unwrap_or(false);
    let records: Vec<serde_json::Value> = log
        .list()
        .filter(|r| {
            let ok = outcome_filter.map(|o| r.outcome == o).unwrap_or(true);
            ok && (!guided_only || r.from_guided_flow)
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

pub async fn list_service_transactions(
    State(state): State<ApiState>,
    Path(service): Path<String>,
    Query(params): Query<TransactionQueryParams>,
) -> impl IntoResponse {
    let canonical = to_provider_slug(&service).to_string();
    let Some(log_arc) = &state.transaction_log else {
        return Json(json!({
            "schema_version": "1.0",
            "service": canonical,
            "total": 0,
            "transactions": [],
        }));
    };
    let log = log_arc.lock().await;
    let limit = params.limit.unwrap_or(200).min(2000);
    let outcome_filter = params.outcome.as_deref().and_then(parse_outcome);
    let guided_only = params.guided_only.unwrap_or(false);
    let records: Vec<serde_json::Value> = log
        .for_service(&canonical)
        .filter(|r| {
            let ok = outcome_filter.map(|o| r.outcome == o).unwrap_or(true);
            ok && (!guided_only || r.from_guided_flow)
        })
        .take(limit)
        .map(record_to_json)
        .collect();
    let total = log.for_service(&canonical).count();
    Json(json!({
        "schema_version": "1.0",
        "service": canonical,
        "total": total,
        "transactions": records,
    }))
}

pub async fn record_transaction(
    State(state): State<ApiState>,
    Json(payload): Json<RecordTransactionRequest>,
) -> impl IntoResponse {
    use openstack_studio_ui::TransactionRecord;

    let Some(log_arc) = &state.transaction_log else {
        // Studio disabled — silently discard, return a fake ID.
        return (StatusCode::CREATED, Json(json!({ "id": 0 }))).into_response();
    };

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

    let mut log = log_arc.lock().await;
    let id = log.push(record);
    (StatusCode::CREATED, Json(json!({ "id": id }))).into_response()
}

pub async fn clear_transactions(State(state): State<ApiState>) -> impl IntoResponse {
    let Some(log_arc) = &state.transaction_log else {
        return Json(json!({ "cleared": 0 }));
    };
    let mut log = log_arc.lock().await;
    let count = log.len();
    log.clear();
    Json(json!({ "cleared": count }))
}
