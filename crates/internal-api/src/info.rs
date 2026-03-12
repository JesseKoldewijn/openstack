use axum::Json;
use axum::extract::State;
use axum::response::IntoResponse;
use serde_json::json;

use crate::ApiState;

/// Returns a JSON object describing the running service and host environment.
///
/// The response includes version and edition, license and session state, uptime in seconds,
/// runtime and system information, a `studio` section with local studio endpoints and guided
/// flow metadata, and a `daemon` section indicating whether the process is managed and its PID.
///
/// # Examples
///
/// ```no_run
/// use axum::{Router, routing::get, http::Request};
/// // mount the handler and send a request to observe the JSON response
/// let app = Router::new().route("/info", get(crate::handlers::get_info));
/// // sending a request is omitted here; calling the handler will return a JSON body
/// ```
pub async fn get_info(State(state): State<ApiState>) -> impl IntoResponse {
    let uptime_secs = state.start_time.elapsed().as_secs();

    let body = json!({
        "version": env!("CARGO_PKG_VERSION"),
        "edition": "community",
        "is_license_activated": false,
        "session_id": state.session_id,
        "uptime": uptime_secs,
        "runtime_info": {
            "implementation": "openstack",
            "language": "rust",
        },
        "system": {
            "platform": std::env::consts::OS,
            "arch": std::env::consts::ARCH,
        },
        "studio": {
            "enabled": true,
            "base_path": "/_localstack/studio",
            "api_base_path": "/_localstack/studio-api",
            "guided_flow": {
                "manifest_schema_version": "1.2",
                "catalog_endpoint": "/_localstack/studio-api/flows/catalog",
                "coverage_endpoint": "/_localstack/studio-api/flows/coverage"
            }
        },
        "daemon": {
            "managed": std::env::var("OPENSTACK_DAEMON_CHILD").ok().as_deref() == Some("1"),
            "pid": std::process::id(),
        }
    });
    Json(body)
}
