use std::collections::HashMap;

use bytes::Bytes;
use openstack_iam::IamProvider;
use openstack_service_framework::traits::{RequestContext, ServiceProvider};
use serde_json::json;

fn make_ctx(operation: &str, params: &[(&str, &str)]) -> RequestContext {
    // IAM uses query protocol — params go in query_params
    let mut qp: HashMap<String, String> = params
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    qp.insert("Action".to_string(), operation.to_string());
    RequestContext {
        service: "iam".to_string(),
        operation: operation.to_string(),
        region: "us-east-1".to_string(),
        account_id: "000000000000".to_string(),
        request_body: json!({}),
        raw_body: Bytes::new(),
        headers: HashMap::new(),
        path: "/".to_string(),
        method: "POST".to_string(),
        query_params: qp,
    }
}

fn body_str(resp: &openstack_service_framework::traits::DispatchResponse) -> String {
    String::from_utf8_lossy(&resp.body).to_string()
}

// ---------------------------------------------------------------------------
// User tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_create_and_get_user() {
    let p = IamProvider::new();
    let resp = p
        .dispatch(&make_ctx("CreateUser", &[("UserName", "alice")]))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200, "{}", body_str(&resp));
    assert!(body_str(&resp).contains("alice"));

    let resp = p
        .dispatch(&make_ctx("GetUser", &[("UserName", "alice")]))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200, "{}", body_str(&resp));
    assert!(body_str(&resp).contains("alice"));
}

#[tokio::test]
async fn test_create_user_duplicate() {
    let p = IamProvider::new();
    p.dispatch(&make_ctx("CreateUser", &[("UserName", "bob")]))
        .await
        .unwrap();
    let resp = p
        .dispatch(&make_ctx("CreateUser", &[("UserName", "bob")]))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 409, "{}", body_str(&resp));
    assert!(body_str(&resp).contains("EntityAlreadyExists"));
}

#[tokio::test]
async fn test_delete_user() {
    let p = IamProvider::new();
    p.dispatch(&make_ctx("CreateUser", &[("UserName", "carol")]))
        .await
        .unwrap();
    let resp = p
        .dispatch(&make_ctx("DeleteUser", &[("UserName", "carol")]))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200, "{}", body_str(&resp));

    let resp = p
        .dispatch(&make_ctx("GetUser", &[("UserName", "carol")]))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 404, "{}", body_str(&resp));
}

#[tokio::test]
async fn test_list_users() {
    let p = IamProvider::new();
    p.dispatch(&make_ctx("CreateUser", &[("UserName", "u1")]))
        .await
        .unwrap();
    p.dispatch(&make_ctx("CreateUser", &[("UserName", "u2")]))
        .await
        .unwrap();
    let resp = p.dispatch(&make_ctx("ListUsers", &[])).await.unwrap();
    assert_eq!(resp.status_code, 200);
    let b = body_str(&resp);
    assert!(b.contains("u1") && b.contains("u2"));
}

#[tokio::test]
async fn test_get_user_no_name_returns_default() {
    let p = IamProvider::new();
    let resp = p.dispatch(&make_ctx("GetUser", &[])).await.unwrap();
    assert_eq!(resp.status_code, 200);
    assert!(body_str(&resp).contains("default"));
}

// ---------------------------------------------------------------------------
// Role tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_create_and_get_role() {
    let p = IamProvider::new();
    let resp = p
        .dispatch(&make_ctx(
            "CreateRole",
            &[
                ("RoleName", "my-role"),
                ("AssumeRolePolicyDocument", r#"{"Version":"2012-10-17"}"#),
            ],
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200, "{}", body_str(&resp));

    let resp = p
        .dispatch(&make_ctx("GetRole", &[("RoleName", "my-role")]))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
    assert!(body_str(&resp).contains("my-role"));
}

#[tokio::test]
async fn test_delete_role() {
    let p = IamProvider::new();
    p.dispatch(&make_ctx("CreateRole", &[("RoleName", "temp-role")]))
        .await
        .unwrap();
    let resp = p
        .dispatch(&make_ctx("DeleteRole", &[("RoleName", "temp-role")]))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);

    let resp = p
        .dispatch(&make_ctx("GetRole", &[("RoleName", "temp-role")]))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 404);
}

#[tokio::test]
async fn test_list_roles() {
    let p = IamProvider::new();
    p.dispatch(&make_ctx("CreateRole", &[("RoleName", "r1")]))
        .await
        .unwrap();
    let resp = p.dispatch(&make_ctx("ListRoles", &[])).await.unwrap();
    assert_eq!(resp.status_code, 200);
    assert!(body_str(&resp).contains("r1"));
}

// ---------------------------------------------------------------------------
// Policy tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_create_and_get_policy() {
    let p = IamProvider::new();
    let resp = p
        .dispatch(&make_ctx(
            "CreatePolicy",
            &[
                ("PolicyName", "my-policy"),
                (
                    "PolicyDocument",
                    r#"{"Version":"2012-10-17","Statement":[]}"#,
                ),
            ],
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200, "{}", body_str(&resp));

    // GetPolicy requires ARN
    let b = body_str(&resp);
    // extract ARN from XML
    let arn_start = b.find("<Arn>").map(|i| i + 5).unwrap_or(0);
    let arn_end = b.find("</Arn>").unwrap_or(0);
    let arn = &b[arn_start..arn_end];
    assert!(!arn.is_empty());

    let resp = p
        .dispatch(&make_ctx("GetPolicy", &[("PolicyArn", arn)]))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
}

#[tokio::test]
async fn test_list_policies() {
    let p = IamProvider::new();
    p.dispatch(&make_ctx(
        "CreatePolicy",
        &[("PolicyName", "pol1"), ("PolicyDocument", "{}")],
    ))
    .await
    .unwrap();
    let resp = p.dispatch(&make_ctx("ListPolicies", &[])).await.unwrap();
    assert_eq!(resp.status_code, 200);
    assert!(body_str(&resp).contains("pol1"));
}

// ---------------------------------------------------------------------------
// Attach policy / group tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_attach_user_policy() {
    let p = IamProvider::new();
    p.dispatch(&make_ctx("CreateUser", &[("UserName", "dave")]))
        .await
        .unwrap();
    let resp = p
        .dispatch(&make_ctx(
            "AttachUserPolicy",
            &[
                ("UserName", "dave"),
                ("PolicyArn", "arn:aws:iam::aws:policy/ReadOnlyAccess"),
            ],
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200, "{}", body_str(&resp));
}

#[tokio::test]
async fn test_attach_role_policy() {
    let p = IamProvider::new();
    p.dispatch(&make_ctx("CreateRole", &[("RoleName", "svc-role")]))
        .await
        .unwrap();
    let resp = p
        .dispatch(&make_ctx(
            "AttachRolePolicy",
            &[
                ("RoleName", "svc-role"),
                ("PolicyArn", "arn:aws:iam::aws:policy/ReadOnlyAccess"),
            ],
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200, "{}", body_str(&resp));
}

#[tokio::test]
async fn test_put_role_policy() {
    let p = IamProvider::new();
    p.dispatch(&make_ctx("CreateRole", &[("RoleName", "inline-role")]))
        .await
        .unwrap();
    let resp = p
        .dispatch(&make_ctx(
            "PutRolePolicy",
            &[
                ("RoleName", "inline-role"),
                ("PolicyName", "inline-pol"),
                ("PolicyDocument", r#"{"Version":"2012-10-17"}"#),
            ],
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200, "{}", body_str(&resp));
}

#[tokio::test]
async fn test_create_group_and_add_user() {
    let p = IamProvider::new();
    p.dispatch(&make_ctx("CreateUser", &[("UserName", "eve")]))
        .await
        .unwrap();
    p.dispatch(&make_ctx("CreateGroup", &[("GroupName", "devs")]))
        .await
        .unwrap();
    let resp = p
        .dispatch(&make_ctx(
            "AddUserToGroup",
            &[("GroupName", "devs"), ("UserName", "eve")],
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200, "{}", body_str(&resp));
}

#[tokio::test]
async fn test_list_groups() {
    let p = IamProvider::new();
    p.dispatch(&make_ctx("CreateGroup", &[("GroupName", "admins")]))
        .await
        .unwrap();
    let resp = p.dispatch(&make_ctx("ListGroups", &[])).await.unwrap();
    assert_eq!(resp.status_code, 200);
    assert!(body_str(&resp).contains("admins"));
}

#[tokio::test]
async fn test_assume_role() {
    let p = IamProvider::new();
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
    assert!(b.contains("AccessKeyId") && b.contains("SecretAccessKey"));
}
