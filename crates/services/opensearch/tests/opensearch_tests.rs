use std::collections::HashMap;

use bytes::Bytes;
use openstack_opensearch::OpenSearchProvider;
use openstack_service_framework::traits::{RequestContext, ServiceProvider};
use serde_json::{json, Value};

fn make_ctx(operation: &str, body: Value, path: &str, method: &str) -> RequestContext {
    RequestContext {
        service: "opensearch".to_string(),
        operation: operation.to_string(),
        region: "us-east-1".to_string(),
        account_id: "000000000000".to_string(),
        request_body: body.clone(),
        raw_body: Bytes::from(serde_json::to_vec(&body).unwrap()),
        headers: HashMap::new(),
        path: path.to_string(),
        method: method.to_string(),
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
async fn test_create_domain() {
    let p = OpenSearchProvider::new();
    let resp = p
        .dispatch(&make_ctx(
            "CreateDomain",
            json!({
                "DomainName": "my-domain",
                "EngineVersion": "OpenSearch_2.5",
                "ClusterConfig": {
                    "InstanceType": "t3.small.search",
                    "InstanceCount": 1
                }
            }),
            "/2021-01-01/opensearch/domain",
            "POST",
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
    assert_eq!(resp.content_type, "application/json");
    let b = body_json(&resp);
    assert_eq!(b["DomainStatus"]["DomainName"], "my-domain");
    let arn = b["DomainStatus"]["ARN"].as_str().unwrap();
    assert!(arn.contains("000000000000"));
    assert!(arn.contains("my-domain"));
    assert_eq!(b["DomainStatus"]["Processing"], false);
    assert_eq!(b["DomainStatus"]["Created"], true);
}

#[tokio::test]
async fn test_create_domain_duplicate_fails() {
    let p = OpenSearchProvider::new();
    p.dispatch(&make_ctx(
        "CreateDomain",
        json!({ "DomainName": "dup-domain" }),
        "/2021-01-01/opensearch/domain",
        "POST",
    ))
    .await
    .unwrap();

    let resp = p
        .dispatch(&make_ctx(
            "CreateDomain",
            json!({ "DomainName": "dup-domain" }),
            "/2021-01-01/opensearch/domain",
            "POST",
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 409);
    let b = body_json(&resp);
    assert!(b["code"]
        .as_str()
        .unwrap()
        .contains("ResourceAlreadyExistsException"));
}

#[tokio::test]
async fn test_describe_domain() {
    let p = OpenSearchProvider::new();
    p.dispatch(&make_ctx(
        "CreateDomain",
        json!({ "DomainName": "desc-domain" }),
        "/2021-01-01/opensearch/domain",
        "POST",
    ))
    .await
    .unwrap();

    let resp = p
        .dispatch(&make_ctx(
            "DescribeDomain",
            json!({}),
            "/2021-01-01/opensearch/domain/desc-domain",
            "GET",
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
    let b = body_json(&resp);
    assert_eq!(b["DomainStatus"]["DomainName"], "desc-domain");
    assert!(b["DomainStatus"]["Endpoint"].is_string());
}

#[tokio::test]
async fn test_describe_domain_not_found() {
    let p = OpenSearchProvider::new();
    let resp = p
        .dispatch(&make_ctx(
            "DescribeDomain",
            json!({}),
            "/2021-01-01/opensearch/domain/nonexistent",
            "GET",
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 409);
    let b = body_json(&resp);
    assert!(b["code"]
        .as_str()
        .unwrap()
        .contains("ResourceNotFoundException"));
}

#[tokio::test]
async fn test_list_domain_names() {
    let p = OpenSearchProvider::new();
    p.dispatch(&make_ctx(
        "CreateDomain",
        json!({ "DomainName": "list-domain-a" }),
        "/2021-01-01/opensearch/domain",
        "POST",
    ))
    .await
    .unwrap();
    p.dispatch(&make_ctx(
        "CreateDomain",
        json!({ "DomainName": "list-domain-b" }),
        "/2021-01-01/opensearch/domain",
        "POST",
    ))
    .await
    .unwrap();

    let resp = p
        .dispatch(&make_ctx(
            "ListDomainNames",
            json!({}),
            "/2021-01-01/opensearch/domain",
            "GET",
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
    let b = body_json(&resp);
    let domains = b["DomainNames"].as_array().unwrap();
    assert_eq!(domains.len(), 2);
    let names: Vec<&str> = domains
        .iter()
        .map(|d| d["DomainName"].as_str().unwrap())
        .collect();
    assert!(names.contains(&"list-domain-a"));
    assert!(names.contains(&"list-domain-b"));
}

#[tokio::test]
async fn test_delete_domain() {
    let p = OpenSearchProvider::new();
    p.dispatch(&make_ctx(
        "CreateDomain",
        json!({ "DomainName": "del-domain" }),
        "/2021-01-01/opensearch/domain",
        "POST",
    ))
    .await
    .unwrap();

    let resp = p
        .dispatch(&make_ctx(
            "DeleteDomain",
            json!({}),
            "/2021-01-01/opensearch/domain/del-domain",
            "DELETE",
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
    let b = body_json(&resp);
    assert_eq!(b["DomainStatus"]["DomainName"], "del-domain");
    assert_eq!(b["DomainStatus"]["Deleted"], true);

    // After delete, listing should return 0
    let list_resp = p
        .dispatch(&make_ctx(
            "ListDomainNames",
            json!({}),
            "/2021-01-01/opensearch/domain",
            "GET",
        ))
        .await
        .unwrap();
    let lb = body_json(&list_resp);
    assert_eq!(lb["DomainNames"].as_array().unwrap().len(), 0);
}
