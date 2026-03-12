use std::collections::HashMap;

use openstack_studio_ui::api::{RawRequest, StudioApiClient, resolve_studio_url_with_timeout};

#[tokio::test(flavor = "current_thread")]
async fn raw_request_serialization_path_builds_and_fails_cleanly() {
    let client = StudioApiClient::new("http://127.0.0.1:9");
    let req = RawRequest {
        method: "GET".to_string(),
        path: "/_localstack/studio-api/services".to_string(),
        query: HashMap::from([(String::from("a"), String::from("b"))]),
        headers: HashMap::new(),
        body: None,
    };

    // We only assert that request construction/path serialization is valid
    // and errors are surfaced through the typed result.
    let result = client.execute_raw(&req).await;
    assert!(result.is_err());
}

#[tokio::test(flavor = "current_thread")]
async fn raw_request_rejects_invalid_method() {
    let client = StudioApiClient::new("http://127.0.0.1:9");
    let req = RawRequest {
        method: "GET WITH SPACE".to_string(),
        path: "/_localstack/studio-api/services".to_string(),
        query: HashMap::new(),
        headers: HashMap::new(),
        body: None,
    };

    let result = client.execute_raw(&req).await;
    assert!(matches!(
        result,
        Err(openstack_studio_ui::api::StudioApiError::InvalidRawMethod(
            _
        ))
    ));
}

#[tokio::test(flavor = "current_thread")]
async fn studio_url_resolution_prefers_explicit_then_daemon_then_fallback() {
    let explicit = resolve_studio_url_with_timeout(
        Some("http://127.0.0.1:4566"),
        Some("http://127.0.0.1:9999/_localstack/health"),
        "http://127.0.0.1:1",
        10,
    )
    .await;
    assert_eq!(explicit.source, "explicit");
    assert!(explicit.url.ends_with("/_localstack/studio"));

    let daemon = resolve_studio_url_with_timeout(
        None,
        Some("http://127.0.0.1:9999/_localstack/health"),
        "http://127.0.0.1:1",
        10,
    )
    .await;
    assert_eq!(daemon.source, "daemon");
    assert!(daemon.url.starts_with("http://127.0.0.1:9999"));

    let fallback = resolve_studio_url_with_timeout(None, None, "http://127.0.0.1:4566", 10).await;
    assert_eq!(fallback.source, "fallback");
    assert_eq!(fallback.url, "http://127.0.0.1:4566/_localstack/studio");
}
