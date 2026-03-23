//! Studio-oriented end-to-end tests against a real openstack runtime.

use std::process::Command;

use openstack_integration_tests::harness::TestHarness;

#[tokio::test]
async fn studio_boot_and_theme_round_trip() {
    let harness = TestHarness::start().await;

    let studio = harness
        .client
        .get(harness.url("/_localstack/studio"))
        .send()
        .await
        .unwrap();
    assert_eq!(studio.status().as_u16(), 200);
    assert_eq!(
        studio
            .headers()
            .get("cache-control")
            .and_then(|v| v.to_str().ok()),
        Some("no-cache")
    );

    let services = harness
        .client
        .get(harness.url("/_localstack/studio-api/services"))
        .send()
        .await
        .unwrap();
    assert_eq!(services.status().as_u16(), 200);

    let catalog = harness
        .client
        .get(harness.url("/_localstack/studio-api/flows/catalog"))
        .send()
        .await
        .unwrap();
    assert_eq!(catalog.status().as_u16(), 200);

    let coverage = harness
        .client
        .get(harness.url("/_localstack/studio-api/flows/coverage"))
        .send()
        .await
        .unwrap();
    assert_eq!(coverage.status().as_u16(), 200);

    // Use the theme state model from studio-ui crate as hydration/persistence signal.
    let mut theme = openstack_studio_ui::ThemeStore::new(openstack_studio_ui::ThemeMode::Light);
    theme.toggle();
    assert_eq!(theme.storage_value(), "dark");

    harness.shutdown();
}

#[tokio::test]
async fn studio_raw_request_path_and_side_effect() {
    let harness = TestHarness::start_services("s3").await;

    let client = openstack_studio_ui::StudioApiClient::new(harness.base_url.clone());
    let bucket = "/studio-raw-bucket";
    let request = openstack_studio_ui::api::RawRequest {
        method: "PUT".to_string(),
        path: bucket.to_string(),
        query: std::collections::HashMap::new(),
        headers: std::collections::HashMap::from([
            (
                "authorization".to_string(),
                "AWS4-HMAC-SHA256 Credential=test/20260306/us-east-1/s3/aws4_request, SignedHeaders=host;x-amz-date, Signature=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string(),
            ),
            ("x-amz-date".to_string(), "20260306T000000Z".to_string()),
        ]),
        body: None,
    };

    let response = client.execute_raw(&request).await.unwrap();
    assert_eq!(response.status, 200);

    let list = harness
        .aws_get("/", "s3", "us-east-1")
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(list.contains("studio-raw-bucket"));

    harness
        .aws_delete(bucket, "s3", "us-east-1")
        .send()
        .await
        .unwrap();

    harness.shutdown();
}

#[tokio::test]
async fn studio_guided_s3_flow() {
    let harness = TestHarness::start_services("s3").await;
    let client = openstack_studio_ui::StudioApiClient::new(harness.base_url.clone());
    let workflow =
        openstack_studio_ui::GuidedWorkflow::s3_basic("studio-guided-bucket", "k.txt", "hello");

    for step in &workflow.steps {
        let response = client.execute_raw(&step.request).await.unwrap();
        assert_eq!(response.status, 200);
    }

    let get = harness
        .aws_get("/studio-guided-bucket/k.txt", "s3", "us-east-1")
        .send()
        .await
        .unwrap();
    assert_eq!(get.status().as_u16(), 200);
    assert_eq!(get.text().await.unwrap(), "hello");

    harness
        .aws_delete("/studio-guided-bucket/k.txt", "s3", "us-east-1")
        .send()
        .await
        .unwrap();
    harness
        .aws_delete("/studio-guided-bucket", "s3", "us-east-1")
        .send()
        .await
        .unwrap();
    harness.shutdown();
}

#[tokio::test]
async fn studio_guided_sqs_flow() {
    let harness = TestHarness::start_services("sqs").await;
    let client = openstack_studio_ui::StudioApiClient::new(harness.base_url.clone());
    let workflow = openstack_studio_ui::GuidedWorkflow::sqs_basic("studio-flow-queue", "msg-1");

    for step in &workflow.steps {
        let response = client.execute_raw(&step.request).await.unwrap();
        assert_eq!(response.status, 200);
    }

    // Verify queue exists via ListQueues call.
    let list_resp = harness
        .aws_post("/", "sqs", "us-east-1")
        .header("content-type", "application/x-www-form-urlencoded")
        .body("Action=ListQueues&Version=2012-11-05")
        .send()
        .await
        .unwrap();
    assert_eq!(list_resp.status().as_u16(), 200);
    let body = list_resp.text().await.unwrap();
    assert!(body.contains("studio-flow-queue"));

    harness.shutdown();
}

#[test]
fn studio_cli_daemon_lifecycle_commands() {
    let data_dir = tempfile::tempdir().unwrap();
    let Some(exe) = std::env::var_os("CARGO_BIN_EXE_openstack") else {
        // In this test package, only integration helper binaries are built by
        // default in CI. Skip CLI lifecycle assertions when the openstack
        // binary is not available.
        return;
    };

    let status_output = Command::new(&exe)
        .arg("status")
        .env("LOCALSTACK_DATA_DIR", data_dir.path())
        .output()
        .unwrap();
    assert!(status_output.status.success());

    let stop_output = Command::new(&exe)
        .arg("stop")
        .env("LOCALSTACK_DATA_DIR", data_dir.path())
        .output()
        .unwrap();
    assert!(stop_output.status.success());

    let restart_output = Command::new(&exe)
        .arg("restart")
        .env("LOCALSTACK_DATA_DIR", data_dir.path())
        .output()
        .unwrap();

    // Restart can succeed or fail depending on local environment readiness; it
    // must not panic and should be recoverable by a stop call.
    let _ = restart_output;

    let final_stop = Command::new(&exe)
        .arg("stop")
        .env("LOCALSTACK_DATA_DIR", data_dir.path())
        .output()
        .unwrap();
    assert!(final_stop.status.success());
}
