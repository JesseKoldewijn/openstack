use std::collections::HashMap;

use openstack_studio_ui::{
    BindingContext, GuidedExecutionReport, GuidedExecutionState, GuidedFlow, GuidedManifest,
    GuidedStep, NormalizedOperation, ProtocolClass, RenderedGuidedFlow, render_guided_flow,
};

#[derive(Debug, serde::Deserialize)]
struct Fixture {
    service: String,
    protocol: String,
    steps: Vec<FixtureStep>,
}

#[derive(Debug, serde::Deserialize)]
struct FixtureStep {
    operation: FixtureOperation,
    response: FixtureResponse,
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

/// Integration test that validates the guided renderer produces expected output for multiple protocol-class fixtures.
///
/// This test loads a set of JSON fixtures (S3, SQS, DynamoDB, Lambda), builds a corresponding GuidedManifest,
/// GuidedFlow, and GuidedExecutionReport for each fixture, renders the flow, and asserts the rendered output
/// matches the fixture's expectations.
///
/// # Examples
///
/// ```
/// // Execute the test with the standard Rust test harness:
/// // cargo test --test guided_renderer_integration_tests
/// ```
#[test]
fn renderer_behavior_across_protocol_class_fixtures() {
    for path in [
        "fixtures/guided-flow-s3.json",
        "fixtures/guided-flow-sqs.json",
        "fixtures/guided-flow-dynamodb.json",
        "fixtures/guided-flow-lambda.json",
    ] {
        let fixture = load_fixture(path);
        let protocol = match fixture.protocol.as_str() {
            "query" => ProtocolClass::Query,
            "json_target" => ProtocolClass::JsonTarget,
            "rest_xml" => ProtocolClass::RestXml,
            _ => ProtocolClass::RestJson,
        };

        let flow = GuidedFlow {
            id: "fixture-flow".to_string(),
            level: "L1".to_string(),
            steps: fixture
                .steps
                .iter()
                .enumerate()
                .map(|(idx, step)| GuidedStep {
                    id: format!("step-{}", idx + 1),
                    title: format!("Step {}", idx + 1),
                    operation: NormalizedOperation {
                        method: step.operation.method.clone(),
                        path: step.operation.path.clone(),
                        headers: HashMap::new(),
                        query: HashMap::new(),
                        body: step.operation.body.clone(),
                    },
                    assertions: vec![openstack_studio_ui::FlowAssertion {
                        kind: "status".to_string(),
                        target: "status".to_string(),
                        expected: step.response.status.to_string(),
                    }],
                    captures: vec![],
                    error_guidance: None,
                })
                .collect(),
            cleanup: vec![],
        };

        let report = GuidedExecutionReport {
            state: GuidedExecutionState::Succeeded,
            outcomes: flow
                .steps
                .iter()
                .map(|step| openstack_studio_ui::StepOutcome {
                    step_id: step.id.clone(),
                    success: true,
                    attempts: 1,
                    status_code: Some(200),
                    error: None,
                })
                .collect(),
            cleanup: vec![],
            captures: BindingContext::default().captures,
        };

        let manifest = GuidedManifest {
            schema_version: "1.2".to_string(),
            service: fixture.service.clone(),
            protocol,
            flows: vec![flow.clone()],
        };

        let rendered = render_guided_flow(&manifest, &flow, Some(&report));
        assert_rendered(rendered, &fixture);
    }
}

/// Loads and deserializes a JSON fixture file from the repository's `tests` directory.
///
/// The `path` is interpreted relative to the crate root `tests` directory (e.g. `"fixtures/foo.json"`).
///
/// # Parameters
///
/// - `path`: Path of the fixture file relative to the crate's `tests` directory.
///
/// # Returns
///
/// A `Fixture` deserialized from the JSON file.
///
/// # Examples
///
/// ```
/// let fixture = load_fixture("guided-flow-s3.json");
/// assert_eq!(fixture.protocol, "rest_json");
/// ```
fn load_fixture(path: &str) -> Fixture {
    let fixture_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join(path);
    let raw = std::fs::read_to_string(fixture_path).expect("fixture must be readable");
    serde_json::from_str(&raw).expect("fixture must parse")
}

/// Asserts that a rendered guided flow matches the expectations defined by a fixture.
///
/// Performs a set of checks on `rendered` against `fixture`:
/// - `flow_id` equals `"fixture-flow"`.
/// - timeline length equals the number of fixture steps.
/// - every timeline item has status `"success"`.
/// - total assertions is at least the number of fixture steps.
/// - cleanup total is zero.
/// - there is no error guidance.
/// - for every fixture step, either the response body is non-empty or the response status is `200`.
///
/// Panics if any of the checks fail.
///
/// # Examples
///
/// ```
/// // Given a `rendered` and `fixture` prepared from a test fixture,
/// // assert_rendered(rendered, &fixture);
/// ```
fn assert_rendered(rendered: RenderedGuidedFlow, fixture: &Fixture) {
    assert_eq!(rendered.flow_id, "fixture-flow");
    assert_eq!(rendered.timeline.len(), fixture.steps.len());
    assert!(
        rendered
            .timeline
            .iter()
            .all(|item| item.status == "success")
    );
    assert!(rendered.assertions.total >= fixture.steps.len());
    assert_eq!(rendered.cleanup.total, 0);
    assert!(rendered.error_guidance.is_empty());
    assert!(
        fixture
            .steps
            .iter()
            .all(|step| !step.response.body.is_empty() || step.response.status == 200)
    );
}
