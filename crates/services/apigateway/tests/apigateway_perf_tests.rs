/// Performance tests for API Gateway provider.
///
/// These cover the main control-plane paths for REST API creation and resource
/// tree manipulation.
use std::collections::HashMap;
use std::time::Instant;

use bytes::Bytes;
use openstack_apigateway::ApiGatewayProvider;
use openstack_service_framework::traits::{DispatchResponse, RequestContext, ServiceProvider};
use serde_json::{Value, json};

fn make_ctx(operation: &str, body: Value, path: &str, method: &str) -> RequestContext {
    RequestContext {
        service: "apigateway".to_string(),
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

fn body(resp: &DispatchResponse) -> Value {
    serde_json::from_slice(resp.body.as_bytes()).expect("valid JSON")
}

async fn create_api(p: &ApiGatewayProvider, name: &str) -> (String, String) {
    let resp = p
        .dispatch(&make_ctx(
            "CreateRestApi",
            json!({ "name": name }),
            "/restapis",
            "POST",
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 201);
    let b = body(&resp);
    (
        b["id"].as_str().unwrap().to_string(),
        b["rootResourceId"].as_str().unwrap().to_string(),
    )
}

#[tokio::test]
async fn perf_create_rest_api_throughput() {
    let p = ApiGatewayProvider::new();
    let n = 100usize;

    let start = Instant::now();
    for i in 0..n {
        let resp = p
            .dispatch(&make_ctx(
                "CreateRestApi",
                json!({ "name": format!("perf-api-{i:03}") }),
                "/restapis",
                "POST",
            ))
            .await
            .unwrap();
        assert_eq!(resp.status_code, 201);
    }
    let elapsed = start.elapsed();

    assert!(
        elapsed.as_millis() < 2000,
        "CreateRestApi x{n} took {}ms — expected <2000ms",
        elapsed.as_millis()
    );
}

#[tokio::test]
async fn perf_get_rest_apis_many() {
    let p = ApiGatewayProvider::new();
    let n = 100usize;
    for i in 0..n {
        let _ = create_api(&p, &format!("perf-list-api-{i:03}")).await;
    }

    let start = Instant::now();
    let resp = p
        .dispatch(&make_ctx("GetRestApis", json!({}), "/restapis", "GET"))
        .await
        .unwrap();
    let elapsed = start.elapsed();
    assert_eq!(resp.status_code, 200);
    let b = body(&resp);
    assert!(b["items"].as_array().unwrap().len() >= n);

    assert!(
        elapsed.as_millis() < 500,
        "GetRestApis({n}) took {}ms — expected <500ms",
        elapsed.as_millis()
    );
}

#[tokio::test]
async fn perf_create_resource_round_trip() {
    let p = ApiGatewayProvider::new();
    let (api_id, root_id) = create_api(&p, "perf-resource-api").await;

    let start = Instant::now();
    for i in 0..100usize {
        let resp = p
            .dispatch(&make_ctx(
                "CreateResource",
                json!({ "pathPart": format!("res-{i:03}") }),
                &format!("/restapis/{api_id}/resources/{root_id}"),
                "POST",
            ))
            .await
            .unwrap();
        assert_eq!(resp.status_code, 201);
    }
    let elapsed = start.elapsed();

    assert!(
        elapsed.as_millis() < 1500,
        "CreateResource x100 took {}ms — expected <1500ms",
        elapsed.as_millis()
    );
}

#[tokio::test]
async fn perf_get_and_delete_rest_api_round_trip() {
    let p = ApiGatewayProvider::new();
    let n = 100usize;

    let start = Instant::now();
    for i in 0..n {
        let (api_id, _) = create_api(&p, &format!("perf-round-trip-api-{i:03}")).await;

        let get_resp = p
            .dispatch(&make_ctx(
                "GetRestApi",
                json!({}),
                &format!("/restapis/{api_id}"),
                "GET",
            ))
            .await
            .unwrap();
        assert_eq!(get_resp.status_code, 200);

        let delete_resp = p
            .dispatch(&make_ctx(
                "DeleteRestApi",
                json!({}),
                &format!("/restapis/{api_id}"),
                "DELETE",
            ))
            .await
            .unwrap();
        assert_eq!(delete_resp.status_code, 202);
    }
    let elapsed = start.elapsed();

    assert!(
        elapsed.as_millis() < 2500,
        "GetRestApi+DeleteRestApi round-trip x{n} took {}ms — expected <2500ms",
        elapsed.as_millis()
    );
}
