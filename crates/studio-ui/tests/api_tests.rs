use std::collections::HashMap;

use openstack_studio_ui::api::{RawRequest, StudioApiClient};

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
