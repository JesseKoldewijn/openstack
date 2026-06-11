/// Performance tests for SSM provider.
///
/// These cover the parameter-store write, read, and path-scan hot paths.
use std::collections::HashMap;
use std::time::Instant;

use bytes::Bytes;
use openstack_service_framework::traits::{DispatchResponse, RequestContext, ServiceProvider};
use openstack_ssm::SsmProvider;
use serde_json::{Value, json};

fn make_ctx(operation: &str, body: Value) -> RequestContext {
    RequestContext {
        service: "ssm".to_string(),
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

fn body(resp: &DispatchResponse) -> Value {
    serde_json::from_slice(resp.body.as_bytes()).expect("response body is valid JSON")
}

async fn put_parameter(p: &SsmProvider, name: &str, value: &str) {
    let resp = p
        .dispatch(&make_ctx(
            "PutParameter",
            json!({
                "Name": name,
                "Value": value,
                "Type": "String",
            }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
}

#[tokio::test]
async fn perf_put_parameter_throughput() {
    let p = SsmProvider::new();
    let n = 100usize;

    let start = Instant::now();
    for i in 0..n {
        let resp = p
            .dispatch(&make_ctx(
                "PutParameter",
                json!({
                    "Name": format!("/perf/put/{i:03}"),
                    "Value": format!("value-{i:03}"),
                    "Type": "String",
                }),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status_code, 200);
    }
    let elapsed = start.elapsed();

    assert!(
        elapsed.as_millis() < 2000,
        "PutParameter x{n} took {}ms — expected <2000ms",
        elapsed.as_millis()
    );
}

#[tokio::test]
async fn perf_get_parameter_many() {
    let p = SsmProvider::new();
    let n = 100usize;
    for i in 0..n {
        let resp = p
            .dispatch(&make_ctx(
                "PutParameter",
                json!({
                    "Name": format!("/perf/get/{i:03}"),
                    "Value": format!("value-{i:03}"),
                    "Type": "String",
                }),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status_code, 200);
    }

    let start = Instant::now();
    for i in 0..n {
        let resp = p
            .dispatch(&make_ctx(
                "GetParameter",
                json!({ "Name": format!("/perf/get/{i:03}") }),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status_code, 200);
    }
    let elapsed = start.elapsed();

    assert!(
        elapsed.as_millis() < 1000,
        "GetParameter x{n} took {}ms — expected <1000ms",
        elapsed.as_millis()
    );
}

#[tokio::test]
async fn perf_get_parameters_by_path_many() {
    let p = SsmProvider::new();
    let n = 100usize;
    for i in 0..n {
        let resp = p
            .dispatch(&make_ctx(
                "PutParameter",
                json!({
                    "Name": format!("/perf/path/db/{i:03}"),
                    "Value": format!("value-{i:03}"),
                    "Type": "String",
                }),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status_code, 200);
    }

    let start = Instant::now();
    let resp = p
        .dispatch(&make_ctx(
            "GetParametersByPath",
            json!({
                "Path": "/perf/path/db",
                "Recursive": true,
            }),
        ))
        .await
        .unwrap();
    let elapsed = start.elapsed();
    assert_eq!(resp.status_code, 200);
    assert_eq!(body(&resp)["Parameters"].as_array().unwrap().len(), n);

    assert!(
        elapsed.as_millis() < 500,
        "GetParametersByPath({n}) took {}ms — expected <500ms",
        elapsed.as_millis()
    );
}

#[tokio::test]
async fn perf_put_and_delete_parameter_round_trip() {
    let p = SsmProvider::new();
    let n = 100usize;

    let start = Instant::now();
    for i in 0..n {
        let name = format!("/perf/delete/{i:03}");
        put_parameter(&p, &name, &format!("value-{i:03}")).await;
        let resp = p
            .dispatch(&make_ctx("DeleteParameter", json!({ "Name": name })))
            .await
            .unwrap();
        assert_eq!(resp.status_code, 200);
    }
    let elapsed = start.elapsed();

    assert!(
        elapsed.as_millis() < 1500,
        "PutParameter/DeleteParameter x{n} took {}ms — expected <1500ms",
        elapsed.as_millis()
    );
}
