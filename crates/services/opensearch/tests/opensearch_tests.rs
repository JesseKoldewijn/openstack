use std::collections::HashMap;

use bytes::Bytes;
use openstack_opensearch::OpenSearchProvider;
use openstack_service_framework::traits::{RequestContext, ServiceProvider};
use serde_json::{Value, json};

fn make_ctx(operation: &str, body: Value, path: &str, method: &str) -> RequestContext {
    RequestContext {
        service: "opensearch".to_string(),
        operation: operation.to_string(),
        region: "us-east-1".to_string(),
        account_id: "000000000000".to_string(),
        request_body: body.clone(),
        raw_body: Some(Bytes::from(serde_json::to_vec(&body).unwrap())),
        headers: Default::default(),
        path: path.to_string(),
        method: method.to_string(),
        query_params: HashMap::new(),
        request_id: String::new(),
        spooled_body: None,
        body_reader: None,
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
    assert!(
        b["code"]
            .as_str()
            .unwrap()
            .contains("ResourceAlreadyExistsException")
    );
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
    assert_eq!(resp.status_code, 404);
    assert_eq!(resp.content_type, "application/json");
    let body = body_json(&resp);
    assert_eq!(body["code"], "ResourceNotFoundException");
    assert_eq!(body["message"], "Domain not found: nonexistent");
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
async fn test_update_domain_config() {
    let p = OpenSearchProvider::new();
    p.dispatch(&make_ctx(
        "CreateDomain",
        json!({ "DomainName": "upd-domain" }),
        "/2021-01-01/opensearch/domain",
        "POST",
    ))
    .await
    .unwrap();

    let resp = p
        .dispatch(&make_ctx(
            "UpdateDomainConfig",
            json!({
                "EngineVersion": "OpenSearch_2.11",
                "ClusterConfig": {
                    "InstanceType": "m6g.large.search",
                    "InstanceCount": 2
                }
            }),
            "/2021-01-01/opensearch/domain/upd-domain/config",
            "POST",
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
    let b = body_json(&resp);
    assert_eq!(
        b["DomainConfig"]["EngineVersion"]["Options"],
        "OpenSearch_2.11"
    );
    assert_eq!(
        b["DomainConfig"]["ClusterConfig"]["Options"]["InstanceType"],
        "m6g.large.search"
    );
    assert_eq!(
        b["DomainConfig"]["ClusterConfig"]["Options"]["InstanceCount"],
        2
    );

    let resp = p
        .dispatch(&make_ctx(
            "DescribeDomain",
            json!({}),
            "/2021-01-01/opensearch/domain/upd-domain",
            "GET",
        ))
        .await
        .unwrap();
    let b = body_json(&resp);
    assert_eq!(b["DomainStatus"]["EngineVersion"], "OpenSearch_2.11");
    assert_eq!(b["DomainStatus"]["ClusterConfig"]["InstanceCount"], 2);
}

#[tokio::test]
async fn test_describe_domain_config() {
    let p = OpenSearchProvider::new();
    p.dispatch(&make_ctx(
        "CreateDomain",
        json!({
            "DomainName": "config-domain",
            "EngineVersion": "OpenSearch_2.9",
            "ClusterConfig": {
                "InstanceType": "t3.medium.search",
                "InstanceCount": 3
            }
        }),
        "/2021-01-01/opensearch/domain",
        "POST",
    ))
    .await
    .unwrap();

    let resp = p
        .dispatch(&make_ctx(
            "DescribeDomainConfig",
            json!({}),
            "/2021-01-01/opensearch/domain/config-domain/config",
            "GET",
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
    let b = body_json(&resp);
    assert_eq!(
        b["DomainConfig"]["EngineVersion"]["Options"],
        "OpenSearch_2.9"
    );
    assert_eq!(
        b["DomainConfig"]["ClusterConfig"]["Options"]["InstanceType"],
        "t3.medium.search"
    );
    assert_eq!(
        b["DomainConfig"]["ClusterConfig"]["Options"]["InstanceCount"],
        3
    );
}

#[tokio::test]
async fn test_update_domain_config_not_found() {
    let p = OpenSearchProvider::new();
    let resp = p
        .dispatch(&make_ctx(
            "UpdateDomainConfig",
            json!({
                "EngineVersion": "OpenSearch_2.11",
                "ClusterConfig": {
                    "InstanceType": "m6g.large.search",
                    "InstanceCount": 2
                }
            }),
            "/2021-01-01/opensearch/domain/nonexistent/config",
            "POST",
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 404);
    let b = body_json(&resp);
    assert_eq!(b["code"], "ResourceNotFoundException");
    assert_eq!(b["message"], "Domain not found: nonexistent");
}

#[tokio::test]
async fn test_update_domain_config_rejects_large_instance_count() {
    let p = OpenSearchProvider::new();
    p.dispatch(&make_ctx(
        "CreateDomain",
        json!({ "DomainName": "huge-instance-domain" }),
        "/2021-01-01/opensearch/domain",
        "POST",
    ))
    .await
    .unwrap();

    let resp = p
        .dispatch(&make_ctx(
            "UpdateDomainConfig",
            json!({
                "ClusterConfig": {
                    "InstanceCount": 4294967296u64
                }
            }),
            "/2021-01-01/opensearch/domain/huge-instance-domain/config",
            "POST",
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 400);
    let b = body_json(&resp);
    assert_eq!(b["code"], "ValidationException");
    assert_eq!(b["message"], "ClusterConfig.InstanceCount is too large");
}

#[tokio::test]
async fn test_update_domain_config_rejects_invalid_instance_count_type() {
    let p = OpenSearchProvider::new();
    p.dispatch(&make_ctx(
        "CreateDomain",
        json!({ "DomainName": "invalid-instance-domain" }),
        "/2021-01-01/opensearch/domain",
        "POST",
    ))
    .await
    .unwrap();

    let resp = p
        .dispatch(&make_ctx(
            "UpdateDomainConfig",
            json!({
                "ClusterConfig": {
                    "InstanceCount": -1
                }
            }),
            "/2021-01-01/opensearch/domain/invalid-instance-domain/config",
            "POST",
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 400);
    let b = body_json(&resp);
    assert_eq!(b["code"], "ValidationException");
    assert_eq!(
        b["message"],
        "ClusterConfig.InstanceCount must be a non-negative integer"
    );
}

#[tokio::test]
async fn test_describe_domain_config_not_found() {
    let p = OpenSearchProvider::new();
    let resp = p
        .dispatch(&make_ctx(
            "DescribeDomainConfig",
            json!({}),
            "/2021-01-01/opensearch/domain/nonexistent/config",
            "GET",
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 404);
    let b = body_json(&resp);
    assert_eq!(b["code"], "ResourceNotFoundException");
    assert_eq!(b["message"], "Domain not found: nonexistent");
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

#[tokio::test]
async fn test_delete_domain_removes_tags() {
    let p = OpenSearchProvider::new();
    let create_resp = p
        .dispatch(&make_ctx(
            "CreateDomain",
            json!({ "DomainName": "tagged-del" }),
            "/2021-01-01/opensearch/domain",
            "POST",
        ))
        .await
        .unwrap();
    let arn = body_json(&create_resp)["DomainStatus"]["ARN"]
        .as_str()
        .unwrap()
        .to_string();

    p.dispatch(&make_ctx(
        "AddTags",
        json!({
            "ARN": arn,
            "TagList": [{"Key": "env", "Value": "test"}]
        }),
        "/2021-01-01/tags",
        "POST",
    ))
    .await
    .unwrap();

    p.dispatch(&make_ctx(
        "DeleteDomain",
        json!({}),
        "/2021-01-01/opensearch/domain/tagged-del",
        "DELETE",
    ))
    .await
    .unwrap();

    let mut list_ctx = make_ctx("ListTags", json!({}), "/2021-01-01/tags", "GET");
    list_ctx.query_params.insert("arn".to_string(), arn);
    let resp = p.dispatch(&list_ctx).await.unwrap();
    assert_eq!(resp.status_code, 404);
    let b = body_json(&resp);
    assert_eq!(b["code"], "ResourceNotFoundException");
}

// ---------------------------------------------------------------------------
// DescribeDomains (batch)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_describe_domains_batch() {
    let p = OpenSearchProvider::new();
    for name in ["batch-a", "batch-b", "batch-c"] {
        p.dispatch(&make_ctx(
            "CreateDomain",
            json!({ "DomainName": name }),
            "/2021-01-01/opensearch/domain",
            "POST",
        ))
        .await
        .unwrap();
    }

    let resp = p
        .dispatch(&make_ctx(
            "DescribeDomains",
            json!({ "DomainNames": ["batch-a", "batch-c"] }),
            "/2021-01-01/opensearch/domain-info",
            "POST",
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
    let b = body_json(&resp);
    let list = b["DomainStatusList"].as_array().unwrap();
    assert_eq!(list.len(), 2);
    let names: Vec<&str> = list
        .iter()
        .map(|d| d["DomainName"].as_str().unwrap())
        .collect();
    assert!(names.contains(&"batch-a"));
    assert!(names.contains(&"batch-c"));
    assert!(!names.contains(&"batch-b"));
}

#[tokio::test]
async fn test_describe_domains_empty_list() {
    let p = OpenSearchProvider::new();
    let resp = p
        .dispatch(&make_ctx(
            "DescribeDomains",
            json!({ "DomainNames": [] }),
            "/2021-01-01/opensearch/domain-info",
            "POST",
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
    let b = body_json(&resp);
    assert_eq!(b["DomainStatusList"].as_array().unwrap().len(), 0);
}

// ---------------------------------------------------------------------------
// Tags
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_add_and_list_tags() {
    let p = OpenSearchProvider::new();
    let create_resp = p
        .dispatch(&make_ctx(
            "CreateDomain",
            json!({ "DomainName": "tag-domain" }),
            "/2021-01-01/opensearch/domain",
            "POST",
        ))
        .await
        .unwrap();
    let arn = body_json(&create_resp)["DomainStatus"]["ARN"]
        .as_str()
        .unwrap()
        .to_string();

    p.dispatch(&make_ctx(
        "AddTags",
        json!({
            "ARN": arn,
            "TagList": [
                {"Key": "env", "Value": "test"},
                {"Key": "team", "Value": "platform"},
            ]
        }),
        "/2021-01-01/tags",
        "POST",
    ))
    .await
    .unwrap();

    let mut list_ctx = make_ctx("ListTags", json!({}), "/2021-01-01/tags", "GET");
    list_ctx.query_params.insert("arn".to_string(), arn.clone());
    let resp = p.dispatch(&list_ctx).await.unwrap();
    assert_eq!(resp.status_code, 200);
    let b = body_json(&resp);
    let tags = b["TagList"].as_array().unwrap();
    assert_eq!(tags.len(), 2);
    let keys: Vec<&str> = tags.iter().map(|t| t["Key"].as_str().unwrap()).collect();
    assert!(keys.contains(&"env"));
    assert!(keys.contains(&"team"));
}

#[tokio::test]
async fn test_remove_tags() {
    let p = OpenSearchProvider::new();
    let create_resp = p
        .dispatch(&make_ctx(
            "CreateDomain",
            json!({ "DomainName": "rmtag-domain" }),
            "/2021-01-01/opensearch/domain",
            "POST",
        ))
        .await
        .unwrap();
    let arn = body_json(&create_resp)["DomainStatus"]["ARN"]
        .as_str()
        .unwrap()
        .to_string();

    p.dispatch(&make_ctx(
        "AddTags",
        json!({
            "ARN": arn,
            "TagList": [
                {"Key": "keep", "Value": "yes"},
                {"Key": "remove", "Value": "this"},
            ]
        }),
        "/2021-01-01/tags",
        "POST",
    ))
    .await
    .unwrap();

    p.dispatch(&make_ctx(
        "RemoveTags",
        json!({ "ARN": arn, "TagKeys": ["remove"] }),
        "/2021-01-01/tags-removal",
        "POST",
    ))
    .await
    .unwrap();

    let mut list_ctx = make_ctx("ListTags", json!({}), "/2021-01-01/tags", "GET");
    list_ctx.query_params.insert("arn".to_string(), arn.clone());
    let resp = p.dispatch(&list_ctx).await.unwrap();
    let b = body_json(&resp);
    let tags = b["TagList"].as_array().unwrap();
    assert_eq!(tags.len(), 1);
    assert_eq!(tags[0]["Key"], "keep");
}

// ---------------------------------------------------------------------------
// GetCompatibleVersions / ListVersions
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_get_compatible_versions() {
    let p = OpenSearchProvider::new();
    let resp = p
        .dispatch(&make_ctx(
            "GetCompatibleVersions",
            json!({}),
            "/2021-01-01/opensearch/compatibleVersions",
            "GET",
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
    let b = body_json(&resp);
    let versions = b["CompatibleVersions"].as_array().unwrap();
    assert!(!versions.is_empty());
    assert!(
        versions
            .iter()
            .any(|v| v["SourceVersion"].as_str().unwrap().contains("OpenSearch"))
    );
}

#[tokio::test]
async fn test_list_versions() {
    let p = OpenSearchProvider::new();
    let resp = p
        .dispatch(&make_ctx(
            "ListVersions",
            json!({}),
            "/2021-01-01/opensearch/versions",
            "GET",
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
    let b = body_json(&resp);
    let versions = b["Versions"].as_array().unwrap();
    assert!(!versions.is_empty());
    assert!(
        versions
            .iter()
            .any(|v| v.as_str().unwrap().starts_with("OpenSearch"))
    );
}

// ---------------------------------------------------------------------------
// StartServiceSoftwareUpdate / CancelServiceSoftwareUpdate
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_start_and_cancel_service_software_update() {
    let p = OpenSearchProvider::new();
    p.dispatch(&make_ctx(
        "CreateDomain",
        json!({ "DomainName": "sw-update-domain" }),
        "/2021-01-01/opensearch/domain",
        "POST",
    ))
    .await
    .unwrap();

    let start_resp = p
        .dispatch(&make_ctx(
            "StartServiceSoftwareUpdate",
            json!({ "DomainName": "sw-update-domain" }),
            "/2021-01-01/opensearch/serviceSoftwareUpdate/start",
            "POST",
        ))
        .await
        .unwrap();
    assert_eq!(start_resp.status_code, 200);
    let b = body_json(&start_resp);
    assert_eq!(b["ServiceSoftwareOptions"]["UpdateStatus"], "IN_PROGRESS");
    assert_eq!(b["ServiceSoftwareOptions"]["Cancellable"], true);

    let cancel_resp = p
        .dispatch(&make_ctx(
            "CancelServiceSoftwareUpdate",
            json!({ "DomainName": "sw-update-domain" }),
            "/2021-01-01/opensearch/serviceSoftwareUpdate/cancel",
            "POST",
        ))
        .await
        .unwrap();
    assert_eq!(cancel_resp.status_code, 200);
    let cb = body_json(&cancel_resp);
    assert_eq!(cb["ServiceSoftwareOptions"]["UpdateStatus"], "NOT_ELIGIBLE");
    assert_eq!(cb["ServiceSoftwareOptions"]["Cancellable"], false);
}

#[tokio::test]
async fn test_start_service_software_update_domain_not_found() {
    let p = OpenSearchProvider::new();
    let resp = p
        .dispatch(&make_ctx(
            "StartServiceSoftwareUpdate",
            json!({ "DomainName": "nonexistent" }),
            "/2021-01-01/opensearch/serviceSoftwareUpdate/start",
            "POST",
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 404);
    let b = body_json(&resp);
    assert_eq!(b["code"], "ResourceNotFoundException");
    assert!(b["message"].as_str().unwrap().contains("nonexistent"));
}

#[tokio::test]
async fn test_cancel_service_software_update_domain_not_found() {
    let p = OpenSearchProvider::new();
    let resp = p
        .dispatch(&make_ctx(
            "CancelServiceSoftwareUpdate",
            json!({ "DomainName": "nonexistent" }),
            "/2021-01-01/opensearch/serviceSoftwareUpdate/cancel",
            "POST",
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 404);
    let b = body_json(&resp);
    assert_eq!(b["code"], "ResourceNotFoundException");
    assert!(b["message"].as_str().unwrap().contains("nonexistent"));
}
