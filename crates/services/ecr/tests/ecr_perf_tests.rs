/// Performance tests for ECR provider.
///
/// These test timing and throughput of DescribeImages and supporting operations
/// to catch regressions from linear scans over large image sets.
///
/// Run with: `cargo test -p openstack-ecr`
use std::collections::HashMap;
use std::time::Instant;

use bytes::Bytes;
use openstack_ecr::EcrProvider;
use openstack_service_framework::traits::{RequestContext, ServiceProvider};
use serde_json::{json, Value};

fn make_ctx(operation: &str, body: Value) -> RequestContext {
    RequestContext {
        service: "ecr".to_string(),
        operation: operation.to_string(),
        region: "us-east-1".to_string(),
        account_id: "000000000000".to_string(),
        request_body: body.clone(),
        raw_body: Some(Bytes::from(serde_json::to_vec(&body).unwrap())),
        headers: Default::default(),
        path: "/".to_string(),
        method: "POST".to_string(),
        query_params: HashMap::new(),
        request_id: String::new(),
        spooled_body: None,
        body_reader: None,
    }
}

async fn create_repo(p: &EcrProvider, name: &str) {
    let resp = p
        .dispatch(&make_ctx(
            "CreateRepository",
            json!({ "repositoryName": name }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
}

async fn push_image(p: &EcrProvider, repo: &str, tag: &str) {
    let resp = p
        .dispatch(&make_ctx(
            "PutImage",
            json!({
                "repositoryName": repo,
                "imageManifest": format!(r#"{{"schemaVersion":2,"tag":"{}"}}"#, tag),
                "imageTag": tag,
            }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
}

// ---------------------------------------------------------------------------
// Perf 1 — DescribeImages over 200-image repository completes in <500 ms
// ---------------------------------------------------------------------------

#[tokio::test]
async fn perf_describe_images_large_repo() {
    let p = EcrProvider::new();
    create_repo(&p, "perf-large-repo").await;

    let n = 200usize;
    for i in 0..n {
        push_image(&p, "perf-large-repo", &format!("v{i:04}")).await;
    }

    let start = Instant::now();
    let resp = p
        .dispatch(&make_ctx(
            "DescribeImages",
            json!({ "repositoryName": "perf-large-repo" }),
        ))
        .await
        .unwrap();
    let elapsed = start.elapsed();

    assert_eq!(resp.status_code, 200);
    let body: Value = serde_json::from_slice(resp.body.as_bytes()).unwrap();
    let count = body["imageDetails"].as_array().unwrap().len();
    assert_eq!(count, n, "expected {n} image details, got {count}");

    assert!(
        elapsed.as_millis() < 500,
        "DescribeImages({n} images) took {}ms — expected <500ms",
        elapsed.as_millis()
    );
}

// ---------------------------------------------------------------------------
// Perf 2 — PutImage + DescribeImages round-trip for 50 repositories
// ---------------------------------------------------------------------------

#[tokio::test]
async fn perf_put_image_and_describe_round_trip() {
    let p = EcrProvider::new();
    let n = 50usize;

    // Create repos and push one image each
    for i in 0..n {
        let repo = format!("perf-repo-{i:03}");
        create_repo(&p, &repo).await;
        push_image(&p, &repo, "latest").await;
    }

    // Now time DescribeImages across all repos
    let start = Instant::now();
    for i in 0..n {
        let repo = format!("perf-repo-{i:03}");
        let resp = p
            .dispatch(&make_ctx(
                "DescribeImages",
                json!({ "repositoryName": repo }),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status_code, 200);
        let body: Value = serde_json::from_slice(resp.body.as_bytes()).unwrap();
        assert_eq!(body["imageDetails"].as_array().unwrap().len(), 1);
    }
    let elapsed = start.elapsed();

    assert!(
        elapsed.as_millis() < 2000,
        "DescribeImages x{n} repos took {}ms — expected <2000ms",
        elapsed.as_millis()
    );
}
