/// Studio runtime config endpoint.
///
/// Exposes the credentials and endpoint URL the Studio SPA needs to construct
/// properly signed AWS SDK requests.  The credentials are always the local
/// test credentials — this is never a real AWS account.
///
/// Route: GET /_localstack/studio-api/runtime-config
use axum::Json;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::response::IntoResponse;
use serde_json::json;

use crate::ApiState;

fn forwarded_proto(headers: &HeaderMap) -> Option<&str> {
    if let Some(proto) = headers
        .get("x-forwarded-proto")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split(',').next())
    {
        return Some(proto.trim());
    }

    // RFC 7239: Forwarded: for=1.2.3.4;proto=https;host=example.com
    headers
        .get("forwarded")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| {
            v.split(';').find_map(|part| {
                let part = part.trim();
                part.strip_prefix("proto=")
                    .map(|p| p.trim_matches('"').trim())
            })
        })
}

/// GET /_localstack/studio-api/runtime-config
///
/// Returns everything the Studio SPA needs to initialise an AWS SDK client:
/// - endpoint URL (the gateway itself)
/// - credentials (static local test credentials)
/// - default region
/// - polling intervals for storage and transaction auto-refresh
pub async fn get_runtime_config(
    State(state): State<ApiState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    // Derive externally visible endpoint origin from incoming request headers
    // to preserve https/reverse-proxy deployments.
    let host = headers
        .get("x-forwarded-host")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split(',').next())
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .or_else(|| {
            headers
                .get("host")
                .and_then(|v| v.to_str().ok())
                .map(str::trim)
                .filter(|v| !v.is_empty())
        })
        .unwrap_or(state.config.localstack_host.trim_end_matches('/'));

    let scheme = forwarded_proto(&headers).unwrap_or("http");
    let endpoint = format!("{}://{}", scheme, host.trim_end_matches('/'));

    Json(json!({
        "schema_version": "1.0",
        "endpoint": endpoint,
        "credentials": {
            "access_key_id":     "test",
            "secret_access_key": "test",
            "session_token":     null,
        },
        "region": "us-east-1",
        "polling": {
            // milliseconds between auto-refreshes of storage + transaction tabs
            "storage_interval_ms":      5000,
            "transactions_interval_ms": 3000,
        },
        "studio": {
            "api_base": "/_localstack/studio-api",
            "spa_base":  "/_localstack/studio",
        }
    }))
}
