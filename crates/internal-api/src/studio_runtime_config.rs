/// Studio runtime config endpoint.
///
/// Exposes the credentials and endpoint URL the Studio SPA needs to construct
/// properly signed AWS SDK requests.  The credentials are always the local
/// test credentials — this is never a real AWS account.
///
/// Route: GET /_localstack/studio-api/runtime-config
use axum::Json;
use axum::extract::State;
use axum::response::IntoResponse;
use serde_json::json;

use crate::ApiState;

/// GET /_localstack/studio-api/runtime-config
///
/// Returns everything the Studio SPA needs to initialise an AWS SDK client:
/// - endpoint URL (the gateway itself)
/// - credentials (static local test credentials)
/// - default region
/// - polling intervals for storage and transaction auto-refresh
pub async fn get_runtime_config(State(state): State<ApiState>) -> impl IntoResponse {
    // Determine the externally-reachable gateway base URL.
    // If LOCALSTACK_HOST is set we use it; otherwise fall back to localhost:4566.
    let host = state.config.localstack_host.trim_end_matches('/');
    let endpoint = format!("http://{}", host);

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
