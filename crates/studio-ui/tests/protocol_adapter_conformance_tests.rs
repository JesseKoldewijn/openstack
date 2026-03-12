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

/// Runs the REST-XML protocol adapter conformance fixture located at "fixtures/protocol-rest-xml.json".
///
/// # Examples
///
/// ```
/// // Executes the test fixture; will panic on failure.
/// run_fixture("fixtures/protocol-rest-xml.json");
/// ```
#[test]
fn adapter_conformance_rest_xml_fixture() {
    run_fixture("fixtures/protocol-rest-xml.json");
}

#[test]
fn adapter_conformance_rest_json_fixture() {
    run_fixture("fixtures/protocol-rest-json.json");
}

/// Ensures transient server errors are classified as retryable for all protocol classes and that client errors are not.
///
/// The test normalizes a 503 adapter response for each supported ProtocolClass and asserts `retryable` is `true`,
/// then normalizes a 400 client error for RestJson and asserts `retryable` is `false`.
///
/// # Examples
///
/// ```
/// use std::collections::HashMap;
/// let err = AdapterResponse { status: 503, headers: HashMap::new(), body: "transient".into() };
/// for protocol in [ProtocolClass::Query, ProtocolClass::JsonTarget, ProtocolClass::RestXml, ProtocolClass::RestJson] {
///     let normalized = normalize_error(protocol, &err).unwrap();
///     assert!(normalized.retryable);
/// }
/// let client_err = AdapterResponse { status: 400, headers: HashMap::new(), body: "bad request".into() };
/// let normalized = normalize_error(ProtocolClass::RestJson, &client_err).unwrap();
/// assert!(!normalized.retryable);
/// ```
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

/// Execute a protocol adapter fixture file from the repository's `tests/` directory and
/// assert that the adapter produced all expected capture values.
///
/// This helper loads the JSON fixture at `tests/<path>`, maps the fixture's `protocol` to
/// a `ProtocolClass`, constructs a `NormalizedOperation` and `AdapterResponse` from the
/// fixture data, runs the protocol adapter, and asserts that every key listed in the
/// fixture's `captures` appears in the adapter result.
///
/// # Examples
///
/// ```
/// // Loads and runs tests/fixtures/protocol-query.json and asserts captures exist.
/// run_fixture("fixtures/protocol-query.json");
/// ```
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
