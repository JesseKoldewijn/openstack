use axum::Json;
use axum::extract::State;
use axum::response::IntoResponse;
use serde_json::json;

use crate::ApiState;

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
        }
    });
    Json(body)
}
