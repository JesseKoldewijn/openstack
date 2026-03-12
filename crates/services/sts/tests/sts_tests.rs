use std::collections::HashMap;

use bytes::Bytes;
use openstack_service_framework::traits::{RequestContext, ServiceProvider};
use openstack_sts::StsProvider;

fn make_ctx(operation: &str, params: &[(&str, &str)]) -> RequestContext {
    let mut qp: HashMap<String, String> = params
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    qp.insert("Action".to_string(), operation.to_string());
    RequestContext {
        service: "sts".to_string(),
        operation: operation.to_string(),
        region: "us-east-1".to_string(),
        account_id: "000000000000".to_string(),
        request_body: serde_json::json!({}),
        raw_body: Bytes::new(),
        headers: HashMap::new(),
        path: "/".to_string(),
        method: "POST".to_string(),
        query_params: qp,
        spooled_body: None,
    }
}

fn body_str(resp: &openstack_service_framework::traits::DispatchResponse) -> String {
    String::from_utf8_lossy(resp.body.as_bytes()).to_string()
}

#[tokio::test]
async fn test_get_caller_identity() {
    let p = StsProvider::new();
    let resp = p
        .dispatch(&make_ctx("GetCallerIdentity", &[]))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200, "{}", body_str(&resp));
    let b = body_str(&resp);
    assert!(b.contains("000000000000"), "Expected account id in: {b}");
    assert!(b.contains("UserId"));
    assert!(b.contains("Arn"));
}

#[tokio::test]
async fn test_assume_role() {
    let p = StsProvider::new();
    let resp = p
        .dispatch(&make_ctx(
            "AssumeRole",
            &[
                ("RoleArn", "arn:aws:iam::000000000000:role/my-role"),
                ("RoleSessionName", "test-session"),
            ],
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200, "{}", body_str(&resp));
    let b = body_str(&resp);
    assert!(b.contains("AccessKeyId"));
    assert!(b.contains("SecretAccessKey"));
    assert!(b.contains("SessionToken"));
    assert!(b.contains("test-session"));
}

#[tokio::test]
async fn test_get_session_token() {
    let p = StsProvider::new();
    let resp = p.dispatch(&make_ctx("GetSessionToken", &[])).await.unwrap();
    assert_eq!(resp.status_code, 200, "{}", body_str(&resp));
    let b = body_str(&resp);
    assert!(b.contains("AccessKeyId"));
    assert!(b.contains("SecretAccessKey"));
}

#[tokio::test]
async fn test_get_access_key_info() {
    let p = StsProvider::new();
    let resp = p
        .dispatch(&make_ctx(
            "GetAccessKeyInfo",
            &[("AccessKeyId", "AKIAIOSFODNN7EXAMPLE")],
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200, "{}", body_str(&resp));
    assert!(body_str(&resp).contains("000000000000"));
}

#[tokio::test]
async fn test_unknown_operation() {
    let p = StsProvider::new();
    let resp = p.dispatch(&make_ctx("ListInstances", &[])).await.unwrap();
    assert_eq!(resp.status_code, 501);
}
