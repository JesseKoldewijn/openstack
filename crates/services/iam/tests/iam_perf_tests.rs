/// Performance tests for IAM provider.
///
/// These cover user creation/listing plus role lifecycle read paths.
use std::collections::HashMap;
use std::time::Instant;

use openstack_iam::IamProvider;
use openstack_service_framework::traits::{DispatchResponse, RequestContext, ServiceProvider};
use serde_json::json;

fn make_ctx(operation: &str, params: &[(&str, &str)]) -> RequestContext {
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
        raw_body: None,
        headers: Default::default(),
        path: "/".to_string(),
        method: "POST".to_string(),
        query_params: qp,
        request_id: String::new(),
        spooled_body: None,
        body_reader: None,
    }
}

fn body_str(resp: &DispatchResponse) -> String {
    String::from_utf8_lossy(resp.body.as_bytes()).to_string()
}

async fn create_user(p: &IamProvider, user_name: &str) {
    let resp = p
        .dispatch(&make_ctx("CreateUser", &[("UserName", user_name)]))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200, "{}", body_str(&resp));
}

async fn create_role(p: &IamProvider, role_name: &str) {
    let resp = p
        .dispatch(&make_ctx(
            "CreateRole",
            &[
                ("RoleName", role_name),
                ("AssumeRolePolicyDocument", r#"{"Version":"2012-10-17"}"#),
            ],
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200, "{}", body_str(&resp));
}

#[tokio::test]
async fn perf_create_user_throughput() {
    let p = IamProvider::new();
    let n = 100usize;

    let start = Instant::now();
    for i in 0..n {
        create_user(&p, &format!("perf-user-{i:03}")).await;
    }
    let elapsed = start.elapsed();

    assert!(
        elapsed.as_millis() < 2500,
        "CreateUser x{n} took {}ms — expected <2500ms",
        elapsed.as_millis()
    );
}

#[tokio::test]
async fn perf_list_users_many() {
    let p = IamProvider::new();
    let n = 100usize;
    for i in 0..n {
        create_user(&p, &format!("perf-list-user-{i:03}")).await;
    }

    let start = Instant::now();
    let resp = p.dispatch(&make_ctx("ListUsers", &[])).await.unwrap();
    let elapsed = start.elapsed();
    assert_eq!(resp.status_code, 200, "{}", body_str(&resp));
    let xml = body_str(&resp);
    assert!(xml.contains("perf-list-user-000"));
    assert!(xml.contains("perf-list-user-099"));

    assert!(
        elapsed.as_millis() < 500,
        "ListUsers({n}) took {}ms — expected <500ms",
        elapsed.as_millis()
    );
}

#[tokio::test]
async fn perf_create_and_get_role_round_trip() {
    let p = IamProvider::new();
    let n = 100usize;

    let start = Instant::now();
    for i in 0..n {
        let role_name = format!("perf-role-{i:03}");
        create_role(&p, &role_name).await;
        let resp = p
            .dispatch(&make_ctx("GetRole", &[("RoleName", &role_name)]))
            .await
            .unwrap();
        assert_eq!(resp.status_code, 200, "{}", body_str(&resp));
        assert!(body_str(&resp).contains(&role_name));
    }
    let elapsed = start.elapsed();

    assert!(
        elapsed.as_millis() < 2500,
        "CreateRole/GetRole x{n} took {}ms — expected <2500ms",
        elapsed.as_millis()
    );
}
