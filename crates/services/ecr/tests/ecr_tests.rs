use std::collections::HashMap;

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
        raw_body: Bytes::from(serde_json::to_vec(&body).unwrap()),
        headers: HashMap::new(),
        path: "/".to_string(),
        method: "POST".to_string(),
        query_params: HashMap::new(),
        spooled_body: None,
    }
}

fn body_json(resp: &openstack_service_framework::traits::DispatchResponse) -> Value {
    serde_json::from_slice(resp.body.as_bytes()).expect("valid JSON")
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_create_repository() {
    let p = EcrProvider::new();
    let resp = p
        .dispatch(&make_ctx(
            "CreateRepository",
            json!({ "repositoryName": "my-repo" }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
    assert_eq!(resp.content_type, "application/x-amz-json-1.1");
    let b = body_json(&resp);
    assert_eq!(b["repository"]["repositoryName"], "my-repo");
    let arn = b["repository"]["repositoryArn"].as_str().unwrap();
    assert!(arn.contains("000000000000"));
    assert!(arn.contains("my-repo"));
    let uri = b["repository"]["repositoryUri"].as_str().unwrap();
    assert!(uri.contains("my-repo"));
}

#[tokio::test]
async fn test_create_repository_duplicate_fails() {
    let p = EcrProvider::new();
    p.dispatch(&make_ctx(
        "CreateRepository",
        json!({ "repositoryName": "dup-repo" }),
    ))
    .await
    .unwrap();
    let resp = p
        .dispatch(&make_ctx(
            "CreateRepository",
            json!({ "repositoryName": "dup-repo" }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 400);
    let b = body_json(&resp);
    assert!(
        b["__type"]
            .as_str()
            .unwrap()
            .contains("RepositoryAlreadyExistsException")
    );
}

#[tokio::test]
async fn test_describe_repositories() {
    let p = EcrProvider::new();
    p.dispatch(&make_ctx(
        "CreateRepository",
        json!({ "repositoryName": "repo-a" }),
    ))
    .await
    .unwrap();
    p.dispatch(&make_ctx(
        "CreateRepository",
        json!({ "repositoryName": "repo-b" }),
    ))
    .await
    .unwrap();

    let resp = p
        .dispatch(&make_ctx("DescribeRepositories", json!({})))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
    let b = body_json(&resp);
    let repos = b["repositories"].as_array().unwrap();
    assert_eq!(repos.len(), 2);
    let names: Vec<&str> = repos
        .iter()
        .map(|r| r["repositoryName"].as_str().unwrap())
        .collect();
    assert!(names.contains(&"repo-a"));
    assert!(names.contains(&"repo-b"));
}

#[tokio::test]
async fn test_delete_repository() {
    let p = EcrProvider::new();
    p.dispatch(&make_ctx(
        "CreateRepository",
        json!({ "repositoryName": "del-repo" }),
    ))
    .await
    .unwrap();

    let resp = p
        .dispatch(&make_ctx(
            "DeleteRepository",
            json!({ "repositoryName": "del-repo" }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
    let b = body_json(&resp);
    assert_eq!(b["repository"]["repositoryName"], "del-repo");

    // After delete, describe should show 0 repos
    let desc_resp = p
        .dispatch(&make_ctx("DescribeRepositories", json!({})))
        .await
        .unwrap();
    let db = body_json(&desc_resp);
    assert_eq!(db["repositories"].as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn test_put_image_and_list() {
    let p = EcrProvider::new();
    p.dispatch(&make_ctx(
        "CreateRepository",
        json!({ "repositoryName": "img-repo" }),
    ))
    .await
    .unwrap();

    let resp = p
        .dispatch(&make_ctx(
            "PutImage",
            json!({
                "repositoryName": "img-repo",
                "imageManifest": r#"{"schemaVersion":2}"#,
                "imageTag": "latest"
            }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
    let b = body_json(&resp);
    let digest = b["image"]["imageId"]["imageDigest"].as_str().unwrap();
    assert!(digest.starts_with("sha256:"));

    // List images
    let list_resp = p
        .dispatch(&make_ctx(
            "ListImages",
            json!({ "repositoryName": "img-repo" }),
        ))
        .await
        .unwrap();
    let lb = body_json(&list_resp);
    let image_ids = lb["imageIds"].as_array().unwrap();
    assert_eq!(image_ids.len(), 1);
    assert!(
        image_ids[0]["imageDigest"]
            .as_str()
            .unwrap()
            .starts_with("sha256:")
    );
}

#[tokio::test]
async fn test_batch_get_image_by_tag() {
    let p = EcrProvider::new();
    p.dispatch(&make_ctx(
        "CreateRepository",
        json!({ "repositoryName": "batch-repo" }),
    ))
    .await
    .unwrap();
    p.dispatch(&make_ctx(
        "PutImage",
        json!({
            "repositoryName": "batch-repo",
            "imageManifest": r#"{"schemaVersion":2}"#,
            "imageTag": "v1.0"
        }),
    ))
    .await
    .unwrap();

    let resp = p
        .dispatch(&make_ctx(
            "BatchGetImage",
            json!({
                "repositoryName": "batch-repo",
                "imageIds": [{ "imageTag": "v1.0" }]
            }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
    let b = body_json(&resp);
    let images = b["images"].as_array().unwrap();
    assert_eq!(images.len(), 1);
    assert_eq!(images[0]["repositoryName"], "batch-repo");
}
