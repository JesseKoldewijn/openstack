/// Performance tests for Step Functions provider.
///
/// These cover the most important control-plane and execution paths for the
/// in-memory state machine runtime.
use std::collections::HashMap;
use std::time::Instant;

use bytes::Bytes;
use openstack_service_framework::traits::{DispatchResponse, RequestContext, ServiceProvider};
use openstack_stepfunctions::StepFunctionsProvider;
use serde_json::{Value, json};

fn make_ctx(operation: &str, body: Value) -> RequestContext {
    RequestContext {
        service: "states".to_string(),
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
    serde_json::from_slice(resp.body.as_bytes()).expect("valid JSON")
}

fn simple_pass_machine() -> Value {
    json!({
        "StartAt": "Done",
        "States": {
            "Done": {
                "Type": "Pass",
                "Result": { "ok": true },
                "End": true
            }
        }
    })
}

async fn create_machine(p: &StepFunctionsProvider, name: &str) -> String {
    let resp = p
        .dispatch(&make_ctx(
            "CreateStateMachine",
            json!({
                "name": name,
                "definition": serde_json::to_string(&simple_pass_machine()).unwrap(),
                "roleArn": "arn:aws:iam::000000000000:role/sf-role",
            }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
    body(&resp)["stateMachineArn"].as_str().unwrap().to_string()
}

#[tokio::test]
async fn perf_create_state_machine_throughput() {
    let p = StepFunctionsProvider::new();
    let n = 100usize;

    let start = Instant::now();
    for i in 0..n {
        let resp = p
            .dispatch(&make_ctx(
                "CreateStateMachine",
                json!({
                    "name": format!("perf-machine-{i:03}"),
                    "definition": serde_json::to_string(&simple_pass_machine()).unwrap(),
                    "roleArn": "arn:aws:iam::000000000000:role/sf-role",
                }),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status_code, 200);
    }
    let elapsed = start.elapsed();

    assert!(
        elapsed.as_millis() < 2000,
        "CreateStateMachine x{n} took {}ms — expected <2000ms",
        elapsed.as_millis()
    );
}

#[tokio::test]
async fn perf_list_state_machines_many() {
    let p = StepFunctionsProvider::new();
    let n = 100usize;
    for i in 0..n {
        create_machine(&p, &format!("perf-list-machine-{i:03}")).await;
    }

    let start = Instant::now();
    let resp = p
        .dispatch(&make_ctx("ListStateMachines", json!({})))
        .await
        .unwrap();
    let elapsed = start.elapsed();
    assert_eq!(resp.status_code, 200);
    let b = body(&resp);
    assert!(b["stateMachines"].as_array().unwrap().len() >= n);

    assert!(
        elapsed.as_millis() < 500,
        "ListStateMachines({n}) took {}ms — expected <500ms",
        elapsed.as_millis()
    );
}

#[tokio::test]
async fn perf_start_execution_throughput() {
    let p = StepFunctionsProvider::new();
    let arn = create_machine(&p, "perf-exec-machine").await;
    let n = 200usize;

    let start = Instant::now();
    for i in 0..n {
        let resp = p
            .dispatch(&make_ctx(
                "StartExecution",
                json!({
                    "stateMachineArn": arn,
                    "input": serde_json::to_string(&json!({ "i": i })).unwrap(),
                    "name": format!("exec-{i:03}"),
                }),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status_code, 200);
    }
    let elapsed = start.elapsed();

    assert!(
        elapsed.as_millis() < 2000,
        "StartExecution x{n} took {}ms — expected <2000ms",
        elapsed.as_millis()
    );
}
