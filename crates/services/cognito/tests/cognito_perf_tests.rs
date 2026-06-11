/// Performance tests for Cognito provider.
use std::collections::HashMap;
use std::time::Instant;

use bytes::Bytes;
use openstack_cognito::CognitoProvider;
use openstack_service_framework::traits::{RequestContext, ServiceProvider};
use serde_json::{Value, json};

fn make_ctx(operation: &str, body: Value) -> RequestContext {
    RequestContext {
        service: "cognito-idp".to_string(),
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

fn body_json(resp: &openstack_service_framework::traits::DispatchResponse) -> Value {
    serde_json::from_slice(resp.body.as_bytes()).expect("valid JSON")
}

#[tokio::test]
async fn perf_create_user_pool_throughput() {
    let p = CognitoProvider::new();
    let n = 100usize;
    let start = Instant::now();
    for i in 0..n {
        let resp = p
            .dispatch(&make_ctx(
                "CreateUserPool",
                json!({ "PoolName": format!("perf-pool-{i:03}") }),
            ))
            .await
            .unwrap();
        assert_eq!(
            resp.status_code,
            200,
            "{}",
            String::from_utf8_lossy(resp.body.as_bytes())
        );
    }
    let elapsed = start.elapsed();
    assert!(
        elapsed.as_millis() < 2000,
        "CreateUserPool x{n} took {}ms — expected <2000ms",
        elapsed.as_millis()
    );
}

#[tokio::test]
async fn perf_admin_create_user_throughput() {
    let p = CognitoProvider::new();
    let pool_resp = p
        .dispatch(&make_ctx(
            "CreateUserPool",
            json!({ "PoolName": "perf-user-pool" }),
        ))
        .await
        .unwrap();
    let pool_id = body_json(&pool_resp)["UserPool"]["Id"]
        .as_str()
        .unwrap()
        .to_string();

    let n = 200usize;
    let start = Instant::now();
    for i in 0..n {
        let resp = p
            .dispatch(&make_ctx(
                "AdminCreateUser",
                json!({
                    "UserPoolId": pool_id,
                    "Username": format!("perf-user-{i:03}"),
                }),
            ))
            .await
            .unwrap();
        assert_eq!(
            resp.status_code,
            200,
            "{}",
            String::from_utf8_lossy(resp.body.as_bytes())
        );
    }
    let elapsed = start.elapsed();
    assert!(
        elapsed.as_millis() < 2000,
        "AdminCreateUser x{n} took {}ms — expected <2000ms",
        elapsed.as_millis()
    );
}

#[tokio::test]
async fn perf_list_users_many() {
    let p = CognitoProvider::new();
    let pool_resp = p
        .dispatch(&make_ctx(
            "CreateUserPool",
            json!({ "PoolName": "perf-list-pool" }),
        ))
        .await
        .unwrap();
    let pool_id = body_json(&pool_resp)["UserPool"]["Id"]
        .as_str()
        .unwrap()
        .to_string();
    let n = 200usize;
    for i in 0..n {
        p.dispatch(&make_ctx(
            "AdminCreateUser",
            json!({ "UserPoolId": pool_id, "Username": format!("list-user-{i:03}") }),
        ))
        .await
        .unwrap();
    }

    let start = Instant::now();
    let resp = p
        .dispatch(&make_ctx("ListUsers", json!({ "UserPoolId": pool_id })))
        .await
        .unwrap();
    let elapsed = start.elapsed();
    assert_eq!(resp.status_code, 200);
    assert_eq!(body_json(&resp)["Users"].as_array().unwrap().len(), n);
    assert!(
        elapsed.as_millis() < 500,
        "ListUsers({n}) took {}ms — expected <500ms",
        elapsed.as_millis()
    );
}
