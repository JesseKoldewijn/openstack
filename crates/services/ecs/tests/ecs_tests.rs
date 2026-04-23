use std::collections::HashMap;

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

// ---------------------------------------------------------------------------
// Clusters
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_create_cluster() {
    let p = EcsProvider::new();
    let resp = p
        .dispatch(&make_ctx(
            "CreateCluster",
            json!({ "clusterName": "my-cluster" }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
    assert_eq!(resp.content_type, "application/x-amz-json-1.1");
    let b = body_json(&resp);
    assert_eq!(b["cluster"]["clusterName"], "my-cluster");
    assert_eq!(b["cluster"]["status"], "ACTIVE");
    let arn = b["cluster"]["clusterArn"].as_str().unwrap();
    assert!(arn.contains("000000000000"));
    assert!(arn.contains("my-cluster"));
}

#[tokio::test]
async fn test_create_cluster_duplicate_fails() {
    let p = EcsProvider::new();
    p.dispatch(&make_ctx(
        "CreateCluster",
        json!({ "clusterName": "dup-cluster" }),
    ))
    .await
    .unwrap();
    let resp = p
        .dispatch(&make_ctx(
            "CreateCluster",
            json!({ "clusterName": "dup-cluster" }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 400);
    let b = body_json(&resp);
    assert!(b["__type"].as_str().unwrap().contains("AlreadyExists"));
}

#[tokio::test]
async fn test_describe_and_list_clusters() {
    let p = EcsProvider::new();
    for name in ["c1", "c2"] {
        p.dispatch(&make_ctx("CreateCluster", json!({ "clusterName": name })))
            .await
            .unwrap();
    }

    let desc_resp = p
        .dispatch(&make_ctx(
            "DescribeClusters",
            json!({ "clusters": ["c1", "c2"] }),
        ))
        .await
        .unwrap();
    assert_eq!(desc_resp.status_code, 200);
    let b = body_json(&desc_resp);
    assert_eq!(b["clusters"].as_array().unwrap().len(), 2);

    let list_resp = p
        .dispatch(&make_ctx("ListClusters", json!({})))
        .await
        .unwrap();
    let lb = body_json(&list_resp);
    assert_eq!(lb["clusterArns"].as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn test_delete_cluster() {
    let p = EcsProvider::new();
    p.dispatch(&make_ctx(
        "CreateCluster",
        json!({ "clusterName": "del-cluster" }),
    ))
    .await
    .unwrap();

    let resp = p
        .dispatch(&make_ctx(
            "DeleteCluster",
            json!({ "cluster": "del-cluster" }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
    let b = body_json(&resp);
    assert_eq!(b["cluster"]["clusterName"], "del-cluster");

    let list_resp = p
        .dispatch(&make_ctx("ListClusters", json!({})))
        .await
        .unwrap();
    assert_eq!(
        body_json(&list_resp)["clusterArns"]
            .as_array()
            .unwrap()
            .len(),
        0
    );
}

#[tokio::test]
async fn test_delete_cluster_not_found() {
    let p = EcsProvider::new();
    let resp = p
        .dispatch(&make_ctx(
            "DeleteCluster",
            json!({ "cluster": "nonexistent" }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 400);
    let b = body_json(&resp);
    assert!(b["__type"].as_str().unwrap().contains("ClusterNotFound"));
}

// ---------------------------------------------------------------------------
// Task Definitions
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_register_task_definition() {
    let p = EcsProvider::new();
    let resp = p
        .dispatch(&make_ctx(
            "RegisterTaskDefinition",
            json!({
                "family": "my-task",
                "containerDefinitions": [
                    {
                        "name": "web",
                        "image": "nginx:latest",
                        "cpu": 256,
                        "memory": 512,
                        "essential": true
                    }
                ],
                "cpu": "256",
                "memory": "512",
                "networkMode": "awsvpc"
            }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
    let b = body_json(&resp);
    assert_eq!(b["taskDefinition"]["family"], "my-task");
    assert_eq!(b["taskDefinition"]["revision"], 1);
    assert_eq!(b["taskDefinition"]["status"], "ACTIVE");
    let arn = b["taskDefinition"]["taskDefinitionArn"].as_str().unwrap();
    assert!(arn.contains("my-task:1"));
}

#[tokio::test]
async fn test_register_task_definition_increments_revision() {
    let p = EcsProvider::new();
    p.dispatch(&make_ctx(
        "RegisterTaskDefinition",
        json!({ "family": "versioned", "containerDefinitions": [] }),
    ))
    .await
    .unwrap();
    let resp = p
        .dispatch(&make_ctx(
            "RegisterTaskDefinition",
            json!({ "family": "versioned", "containerDefinitions": [] }),
        ))
        .await
        .unwrap();
    let b = body_json(&resp);
    assert_eq!(b["taskDefinition"]["revision"], 2);
}

#[tokio::test]
async fn test_describe_task_definition() {
    let p = EcsProvider::new();
    p.dispatch(&make_ctx(
        "RegisterTaskDefinition",
        json!({
            "family": "desc-task",
            "containerDefinitions": [{"name": "app", "image": "app:v1"}]
        }),
    ))
    .await
    .unwrap();

    let resp = p
        .dispatch(&make_ctx(
            "DescribeTaskDefinition",
            json!({ "taskDefinition": "desc-task" }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
    let b = body_json(&resp);
    assert_eq!(b["taskDefinition"]["family"], "desc-task");
    assert_eq!(
        b["taskDefinition"]["containerDefinitions"][0]["name"],
        "app"
    );
}

#[tokio::test]
async fn test_deregister_task_definition() {
    let p = EcsProvider::new();
    let reg_resp = p
        .dispatch(&make_ctx(
            "RegisterTaskDefinition",
            json!({ "family": "deregister-task", "containerDefinitions": [] }),
        ))
        .await
        .unwrap();
    let arn = body_json(&reg_resp)["taskDefinition"]["taskDefinitionArn"]
        .as_str()
        .unwrap()
        .to_string();

    let dereg_resp = p
        .dispatch(&make_ctx(
            "DeregisterTaskDefinition",
            json!({ "taskDefinition": arn }),
        ))
        .await
        .unwrap();
    assert_eq!(dereg_resp.status_code, 200);
    let b = body_json(&dereg_resp);
    assert_eq!(b["taskDefinition"]["status"], "INACTIVE");
}

#[tokio::test]
async fn test_list_task_definitions() {
    let p = EcsProvider::new();
    for fam in ["svc-a", "svc-b"] {
        p.dispatch(&make_ctx(
            "RegisterTaskDefinition",
            json!({ "family": fam, "containerDefinitions": [] }),
        ))
        .await
        .unwrap();
    }

    let resp = p
        .dispatch(&make_ctx("ListTaskDefinitions", json!({})))
        .await
        .unwrap();
    let b = body_json(&resp);
    assert_eq!(b["taskDefinitionArns"].as_array().unwrap().len(), 2);
}

// ---------------------------------------------------------------------------
// Services
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_create_and_list_service() {
    let p = EcsProvider::new();
    p.dispatch(&make_ctx(
        "CreateCluster",
        json!({ "clusterName": "svc-cluster" }),
    ))
    .await
    .unwrap();
    p.dispatch(&make_ctx(
        "RegisterTaskDefinition",
        json!({ "family": "web-task", "containerDefinitions": [] }),
    ))
    .await
    .unwrap();

    let resp = p
        .dispatch(&make_ctx(
            "CreateService",
            json!({
                "cluster": "svc-cluster",
                "serviceName": "web-svc",
                "taskDefinition": "web-task:1",
                "desiredCount": 3
            }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
    let b = body_json(&resp);
    assert_eq!(b["service"]["serviceName"], "web-svc");
    assert_eq!(b["service"]["desiredCount"], 3);
    assert_eq!(b["service"]["status"], "ACTIVE");

    let list_resp = p
        .dispatch(&make_ctx(
            "ListServices",
            json!({ "cluster": "svc-cluster" }),
        ))
        .await
        .unwrap();
    assert_eq!(
        body_json(&list_resp)["serviceArns"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

#[tokio::test]
async fn test_update_service() {
    let p = EcsProvider::new();
    p.dispatch(&make_ctx(
        "CreateCluster",
        json!({ "clusterName": "upd-cluster" }),
    ))
    .await
    .unwrap();
    p.dispatch(&make_ctx(
        "CreateService",
        json!({
            "cluster": "upd-cluster",
            "serviceName": "upd-svc",
            "taskDefinition": "td:1",
            "desiredCount": 1
        }),
    ))
    .await
    .unwrap();

    let resp = p
        .dispatch(&make_ctx(
            "UpdateService",
            json!({
                "cluster": "upd-cluster",
                "service": "upd-svc",
                "desiredCount": 5
            }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
    let b = body_json(&resp);
    assert_eq!(b["service"]["desiredCount"], 5);
}

#[tokio::test]
async fn test_delete_service() {
    let p = EcsProvider::new();
    p.dispatch(&make_ctx(
        "CreateCluster",
        json!({ "clusterName": "del-svc-cluster" }),
    ))
    .await
    .unwrap();
    p.dispatch(&make_ctx(
        "CreateService",
        json!({
            "cluster": "del-svc-cluster",
            "serviceName": "del-svc",
            "taskDefinition": "td:1",
            "desiredCount": 1
        }),
    ))
    .await
    .unwrap();

    let resp = p
        .dispatch(&make_ctx(
            "DeleteService",
            json!({ "cluster": "del-svc-cluster", "service": "del-svc" }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
    assert_eq!(body_json(&resp)["service"]["serviceName"], "del-svc");
}

// ---------------------------------------------------------------------------
// Tasks
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_run_and_list_tasks() {
    let p = EcsProvider::new();
    p.dispatch(&make_ctx(
        "CreateCluster",
        json!({ "clusterName": "task-cluster" }),
    ))
    .await
    .unwrap();
    p.dispatch(&make_ctx(
        "RegisterTaskDefinition",
        json!({ "family": "run-task", "containerDefinitions": [] }),
    ))
    .await
    .unwrap();

    let resp = p
        .dispatch(&make_ctx(
            "RunTask",
            json!({
                "cluster": "task-cluster",
                "taskDefinition": "run-task",
                "count": 2
            }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
    let b = body_json(&resp);
    assert_eq!(b["tasks"].as_array().unwrap().len(), 2);
    assert_eq!(b["tasks"][0]["lastStatus"], "RUNNING");

    let list_resp = p
        .dispatch(&make_ctx("ListTasks", json!({ "cluster": "task-cluster" })))
        .await
        .unwrap();
    assert_eq!(
        body_json(&list_resp)["taskArns"].as_array().unwrap().len(),
        2
    );
}

#[tokio::test]
async fn test_stop_task() {
    let p = EcsProvider::new();
    p.dispatch(&make_ctx(
        "CreateCluster",
        json!({ "clusterName": "stop-cluster" }),
    ))
    .await
    .unwrap();
    p.dispatch(&make_ctx(
        "RegisterTaskDefinition",
        json!({ "family": "stop-task-def", "containerDefinitions": [] }),
    ))
    .await
    .unwrap();

    let run_resp = p
        .dispatch(&make_ctx(
            "RunTask",
            json!({
                "cluster": "stop-cluster",
                "taskDefinition": "stop-task-def",
                "count": 1
            }),
        ))
        .await
        .unwrap();
    let task_arn = body_json(&run_resp)["tasks"][0]["taskArn"]
        .as_str()
        .unwrap()
        .to_string();

    let stop_resp = p
        .dispatch(&make_ctx(
            "StopTask",
            json!({
                "cluster": "stop-cluster",
                "task": task_arn,
                "reason": "test teardown"
            }),
        ))
        .await
        .unwrap();
    assert_eq!(stop_resp.status_code, 200);
    let sb = body_json(&stop_resp);
    assert_eq!(sb["task"]["lastStatus"], "STOPPED");
    assert_eq!(sb["task"]["stoppedReason"], "test teardown");
}

#[tokio::test]
async fn test_describe_tasks() {
    let p = EcsProvider::new();
    p.dispatch(&make_ctx(
        "CreateCluster",
        json!({ "clusterName": "desc-task-cluster" }),
    ))
    .await
    .unwrap();
    p.dispatch(&make_ctx(
        "RegisterTaskDefinition",
        json!({ "family": "desc-td", "containerDefinitions": [] }),
    ))
    .await
    .unwrap();

    let run_resp = p
        .dispatch(&make_ctx(
            "RunTask",
            json!({
                "cluster": "desc-task-cluster",
                "taskDefinition": "desc-td",
                "count": 1
            }),
        ))
        .await
        .unwrap();
    let task_arn = body_json(&run_resp)["tasks"][0]["taskArn"]
        .as_str()
        .unwrap()
        .to_string();

    let desc_resp = p
        .dispatch(&make_ctx(
            "DescribeTasks",
            json!({
                "cluster": "desc-task-cluster",
                "tasks": [task_arn]
            }),
        ))
        .await
        .unwrap();
    assert_eq!(desc_resp.status_code, 200);
    let b = body_json(&desc_resp);
    assert_eq!(b["tasks"].as_array().unwrap().len(), 1);
    assert_eq!(b["tasks"][0]["lastStatus"], "RUNNING");
}
