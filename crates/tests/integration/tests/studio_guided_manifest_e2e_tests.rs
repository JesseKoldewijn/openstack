use openstack_integration_tests::harness::TestHarness;

/// Integration test that verifies the guided catalog exposes representative protocols for key services.
///
/// Asserts the catalog response includes entries for: `sqs` with protocol `query`, `dynamodb` with `json_target`, `s3` with `rest_xml`, and `lambda` with `rest_json`.
///
/// # Examples
///
/// ```
/// # use openstack_integration_tests::harness::TestHarness;
/// # use serde_json::Value;
/// #[tokio::test]
/// async fn example_guided_catalog_check() {
///     let harness = TestHarness::start().await;
///     let resp = harness
///         .client
///         .get(harness.url("/_localstack/studio-api/flows/catalog"))
///         .send()
///         .await
///         .unwrap();
///     assert_eq!(resp.status().as_u16(), 200);
///     let body: Value = resp.json().await.unwrap();
///     let services = body["services"].as_array().unwrap();
///     assert!(services.iter().any(|s| s["service"] == "sqs" && s["protocol"] == "query"));
///     harness.shutdown();
/// }
/// ```
#[tokio::test]
async fn guided_catalog_includes_protocol_representatives() {
    let harness = TestHarness::start().await;

    let response = harness
        .client
        .get(harness.url("/_localstack/studio-api/flows/catalog"))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status().as_u16(), 200);

    let body: serde_json::Value = response.json().await.unwrap();
    let services = body["services"].as_array().unwrap();
    assert!(
        services
            .iter()
            .any(|s| s["service"] == "sqs" && s["protocol"] == "query")
    );
    assert!(
        services
            .iter()
            .any(|s| s["service"] == "dynamodb" && s["protocol"] == "json_target")
    );
    assert!(
        services
            .iter()
            .any(|s| s["service"] == "s3" && s["protocol"] == "rest_xml")
    );
    assert!(
        services
            .iter()
            .any(|s| s["service"] == "lambda" && s["protocol"] == "rest_json")
    );

    harness.shutdown();
}

/// Validates that the guided coverage endpoint reports matrix services and a minimum supported-service count.
///
/// The test requests "/_localstack/studio-api/flows/coverage", asserts a 200 response, verifies that
/// `counts.supported_services` is at least 24, and checks that the returned services include `"s3"`, `"sqs"`, and `"lambda"`.
///
/// # Examples
///
/// ```no_run
/// # async fn example() {
/// guided_coverage_reports_all_matrix_services().await;
/// # }
/// ```
#[tokio::test]
async fn guided_coverage_reports_all_matrix_services() {
    let harness = TestHarness::start().await;

    let response = harness
        .client
        .get(harness.url("/_localstack/studio-api/flows/coverage"))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status().as_u16(), 200);
    let body: serde_json::Value = response.json().await.unwrap();
    let services = body["services"].as_array().unwrap();
    assert!(body["counts"]["supported_services"].as_u64().unwrap_or(0) >= 24);
    assert!(services.iter().any(|s| s["service"] == "s3"));
    assert!(services.iter().any(|s| s["service"] == "sqs"));
    assert!(services.iter().any(|s| s["service"] == "lambda"));

    harness.shutdown();
}
