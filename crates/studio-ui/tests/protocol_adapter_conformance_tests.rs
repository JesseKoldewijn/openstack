use std::collections::HashMap;

use openstack_studio_ui::guided_manifest::{NormalizedOperation, ProtocolClass};
use openstack_studio_ui::protocol_adapters::{
    AdapterResponse, execute_protocol_adapter, normalize_error,
};

#[derive(Debug, serde::Deserialize)]
struct Fixture {
    protocol: String,
    operation: FixtureOperation,
    response: FixtureResponse,
    captures: HashMap<String, String>,
}

#[derive(Debug, serde::Deserialize)]
struct FixtureOperation {
    method: String,
    path: String,
    body: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
struct FixtureResponse {
    status: u16,
    body: String,
}

#[test]
fn adapter_conformance_query_fixture() {
    run_fixture("fixtures/protocol-query.json");
}

#[test]
fn adapter_conformance_json_target_fixture() {
    run_fixture("fixtures/protocol-json-target.json");
}

#[test]
fn adapter_conformance_rest_xml_fixture() {
    run_fixture("fixtures/protocol-rest-xml.json");
}

#[test]
fn adapter_conformance_rest_json_fixture() {
    run_fixture("fixtures/protocol-rest-json.json");
}

#[test]
fn retryability_regression_for_protocol_specific_errors() {
    let error_response = AdapterResponse {
        status: 503,
        headers: HashMap::new(),
        body: "transient".to_string(),
    };

    for protocol in [
        ProtocolClass::Query,
        ProtocolClass::JsonTarget,
        ProtocolClass::RestXml,
        ProtocolClass::RestJson,
    ] {
        let normalized =
            normalize_error(protocol, &error_response).expect("error response must normalize");
        assert!(normalized.retryable);
    }

    let client_error = AdapterResponse {
        status: 400,
        headers: HashMap::new(),
        body: "bad request".to_string(),
    };
    let normalized =
        normalize_error(ProtocolClass::RestJson, &client_error).expect("must normalize");
    assert!(!normalized.retryable);
}

fn run_fixture(path: &str) {
    let fixture_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join(path);
    let raw = std::fs::read_to_string(fixture_path).expect("fixture must load");
    let fixture: Fixture = serde_json::from_str(&raw).expect("fixture must parse");

    let protocol = match fixture.protocol.as_str() {
        "query" => ProtocolClass::Query,
        "json_target" => ProtocolClass::JsonTarget,
        "rest_xml" => ProtocolClass::RestXml,
        "rest_json" => ProtocolClass::RestJson,
        other => panic!("unsupported fixture protocol: {other}"),
    };

    let operation = NormalizedOperation {
        method: fixture.operation.method,
        path: fixture.operation.path,
        headers: HashMap::new(),
        query: HashMap::new(),
        body: fixture.operation.body,
    };
    let response = AdapterResponse {
        status: fixture.response.status,
        headers: HashMap::new(),
        body: fixture.response.body,
    };

    let result = execute_protocol_adapter(protocol, &operation, &response, &fixture.captures)
        .expect("adapter fixture execution should succeed");
    for capture_name in fixture.captures.keys() {
        assert!(
            result.captures.contains_key(capture_name),
            "missing capture {}",
            capture_name
        );
    }
}
