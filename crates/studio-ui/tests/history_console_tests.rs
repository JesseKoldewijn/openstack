use std::collections::HashMap;

use openstack_studio_ui::{
    InteractionEntry, InteractionHistory, RawConsoleState, api::RawRequest, api::RawResponse,
};

#[test]
fn history_replay_and_filter() {
    let request = RawRequest {
        method: "GET".to_string(),
        path: "/_localstack/health".to_string(),
        query: HashMap::new(),
        headers: HashMap::new(),
        body: None,
    };

    let mut history = InteractionHistory::new(10);
    history.push(InteractionEntry {
        id: 1,
        timestamp_unix_ms: 1000,
        service: "internal".to_string(),
        status: 200,
        request: request.clone(),
    });

    let replay = history.replay_request(1).expect("request replay exists");
    assert_eq!(replay, request);
    assert_eq!(history.filter_by_service("internal").count(), 1);
    assert_eq!(history.filter_by_service("s3").count(), 0);
}

#[test]
fn console_state_roundtrip_and_apply_response() {
    let mut state = RawConsoleState::default();
    let request = RawRequest {
        method: "POST".to_string(),
        path: "/_localstack/info".to_string(),
        query: HashMap::new(),
        headers: HashMap::from([(String::from("x-test"), String::from("1"))]),
        body: Some("{}".to_string()),
    };
    state.apply_request(&request);

    let response = RawResponse {
        status: 200,
        headers: HashMap::new(),
        body: "{\"ok\":true}".to_string(),
    };
    state.apply_response(response.clone());

    assert_eq!(state.to_request(), request);
    assert_eq!(state.last_response, Some(response));
}
