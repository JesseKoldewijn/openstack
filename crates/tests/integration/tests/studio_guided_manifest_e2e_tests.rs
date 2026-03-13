use openstack_integration_tests::harness::TestHarness;

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
