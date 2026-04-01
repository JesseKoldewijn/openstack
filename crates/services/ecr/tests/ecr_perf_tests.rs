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
use serde_json::{Value, json};

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

    // Create repos first (not timed)
    for i in 0..n {
        create_repo(&p, &format!("perf-repo-{i:03}")).await;
    }

    // Time PutImage × n
    let start = Instant::now();
    for i in 0..n {
        let resp = p
            .dispatch(&make_ctx(
                "PutImage",
                json!({
                    "repositoryName": format!("perf-repo-{i:03}"),
                    "imageManifest": "{}",
                    "imageTag": "latest",
                }),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status_code, 200, "PutImage failed on repo {i}");
    }
    let put_elapsed = start.elapsed();
    assert!(
        put_elapsed.as_millis() < 2000,
        "PutImage x{n} took {}ms — expected <2000ms",
        put_elapsed.as_millis()
    );

    // Time DescribeImages × n
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

// ---------------------------------------------------------------------------
// Perf — ListImages × 50 repos in under 500 ms
// ---------------------------------------------------------------------------

#[tokio::test]
async fn perf_list_images() {
    let p = EcrProvider::new();
    let n = 50usize;

    for i in 0..n {
        let repo = format!("perf-list-repo-{i:03}");
        create_repo(&p, &repo).await;
        push_image(&p, &repo, "v1").await;
    }

    let start = Instant::now();
    for i in 0..n {
        let resp = p
            .dispatch(&make_ctx(
                "ListImages",
                json!({ "repositoryName": format!("perf-list-repo-{i:03}") }),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status_code, 200);
    }
    let elapsed = start.elapsed();
    assert!(
        elapsed.as_millis() < 500,
        "ListImages x{n} took {}ms — expected <500ms",
        elapsed.as_millis()
    );
}

// ---------------------------------------------------------------------------
// Perf 3 — BatchDeleteImage × 200 images by tag in under 200 ms
// ---------------------------------------------------------------------------

#[tokio::test]
async fn perf_batch_delete_image_by_tag() {
    let p = EcrProvider::new();
    create_repo(&p, "perf-del-repo").await;

    let n = 200usize;
    for i in 0..n {
        push_image(&p, "perf-del-repo", &format!("tag-{i:04}")).await;
    }

    // Build imageIds array for all n images
    let image_ids: Vec<serde_json::Value> = (0..n)
        .map(|i| json!({ "imageTag": format!("tag-{i:04}") }))
        .collect();

    let start = Instant::now();
    let resp = p
        .dispatch(&make_ctx(
            "BatchDeleteImage",
            json!({
                "repositoryName": "perf-del-repo",
                "imageIds": image_ids,
            }),
        ))
        .await
        .unwrap();
    let elapsed = start.elapsed();

    assert_eq!(resp.status_code, 200);
    let body: Value = serde_json::from_slice(resp.body.as_bytes()).unwrap();
    assert_eq!(
        body["imageIds"].as_array().unwrap().len(),
        n,
        "expected {n} deleted"
    );
    assert_eq!(
        body["failures"].as_array().unwrap().len(),
        0,
        "expected 0 failures"
    );

    assert!(
        elapsed.as_millis() < 200,
        "BatchDeleteImage×{n} by tag took {}ms — expected <200ms",
        elapsed.as_millis()
    );
}

// ---------------------------------------------------------------------------
// Perf 4 — BatchGetImage × 100 images by tag in under 100 ms
// ---------------------------------------------------------------------------

#[tokio::test]
async fn perf_batch_get_image_by_tag() {
    let p = EcrProvider::new();
    create_repo(&p, "perf-get-repo").await;

    let n = 100usize;
    for i in 0..n {
        push_image(&p, "perf-get-repo", &format!("img-{i:04}")).await;
    }

    let image_ids: Vec<serde_json::Value> = (0..n)
        .map(|i| json!({ "imageTag": format!("img-{i:04}") }))
        .collect();

    let start = Instant::now();
    let resp = p
        .dispatch(&make_ctx(
            "BatchGetImage",
            json!({
                "repositoryName": "perf-get-repo",
                "imageIds": image_ids,
            }),
        ))
        .await
        .unwrap();
    let elapsed = start.elapsed();

    assert_eq!(resp.status_code, 200);
    let body: Value = serde_json::from_slice(resp.body.as_bytes()).unwrap();
    assert_eq!(body["images"].as_array().unwrap().len(), n);

    assert!(
        elapsed.as_millis() < 100,
        "BatchGetImage×{n} by tag took {}ms — expected <100ms",
        elapsed.as_millis()
    );
}
