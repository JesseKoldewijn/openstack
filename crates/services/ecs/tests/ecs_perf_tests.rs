/// Performance tests for ECS provider.
use std::collections::HashMap;
use std::time::Instant;

use bytes::Bytes;
use openstack_ecs::EcsProvider;
use openstack_service_framework::traits::{RequestContext, ServiceProvider};
use serde_json::{Value, json};

fn make_ctx(operation: &str, body: Value) -> RequestContext {
    RequestContext {
        service: "ecs".to_string(),
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
async fn perf_create_cluster_throughput() {
    let p = EcsProvider::new();
    let n = 100usize;
    let start = Instant::now();
    for i in 0..n {
        let resp = p
            .dispatch(&make_ctx(
                "CreateCluster",
                json!({ "clusterName": format!("perf-cluster-{i:03}") }),
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
        "CreateCluster x{n} took {}ms — expected <2000ms",
        elapsed.as_millis()
    );
}

#[tokio::test]
async fn perf_list_clusters_many() {
    let p = EcsProvider::new();
    let n = 100usize;
    for i in 0..n {
        p.dispatch(&make_ctx(
            "CreateCluster",
            json!({ "clusterName": format!("list-cluster-{i:03}") }),
        ))
        .await
        .unwrap();
    }
    let start = Instant::now();
    let resp = p
        .dispatch(&make_ctx("ListClusters", json!({})))
        .await
        .unwrap();
    let elapsed = start.elapsed();
    assert_eq!(resp.status_code, 200);
    let b = body_json(&resp);
    assert_eq!(b["clusterArns"].as_array().unwrap().len(), n);
    assert!(
        elapsed.as_millis() < 500,
        "ListClusters({n}) took {}ms — expected <500ms",
        elapsed.as_millis()
    );
}

#[tokio::test]
async fn perf_register_task_definition_throughput() {
    let p = EcsProvider::new();
    let n = 100usize;
    let start = Instant::now();
    for i in 0..n {
        let resp = p
            .dispatch(&make_ctx(
                "RegisterTaskDefinition",
                json!({
                    "family": format!("perf-task-{i:03}"),
                    "containerDefinitions": [{"name": "app", "image": "nginx"}]
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
        "RegisterTaskDefinition x{n} took {}ms — expected <2000ms",
        elapsed.as_millis()
    );
}

#[tokio::test]
async fn perf_run_task_throughput() {
    let p = EcsProvider::new();
    p.dispatch(&make_ctx(
        "CreateCluster",
        json!({ "clusterName": "perf-run-cluster" }),
    ))
    .await
    .unwrap();
    p.dispatch(&make_ctx(
        "RegisterTaskDefinition",
        json!({ "family": "perf-run-td", "containerDefinitions": [] }),
    ))
    .await
    .unwrap();

    let n = 100usize;
    let start = Instant::now();
    for _ in 0..n {
        let resp = p
            .dispatch(&make_ctx(
                "RunTask",
                json!({
                    "cluster": "perf-run-cluster",
                    "taskDefinition": "perf-run-td",
                    "count": 1
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
        "RunTask x{n} took {}ms — expected <2000ms",
        elapsed.as_millis()
    );
}
