use std::collections::HashMap;

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
        raw_body: Bytes::from(serde_json::to_vec(&body).unwrap()),
        headers: HashMap::new(),
        path: "/".to_string(),
        method: "POST".to_string(),
        query_params: HashMap::new(),
    }
}

fn body(resp: &DispatchResponse) -> Value {
    serde_json::from_slice(&resp.body).expect("valid JSON")
}

fn body_str(resp: &DispatchResponse) -> String {
    String::from_utf8_lossy(&resp.body).to_string()
}

fn simple_pass_machine() -> Value {
    json!({
        "StartAt": "Hello",
        "States": {
            "Hello": {
                "Type": "Pass",
                "Result": { "greeting": "hello" },
                "End": true
            }
        }
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_create_state_machine() {
    let p = StepFunctionsProvider::new();
    let resp = p
        .dispatch(&make_ctx(
            "CreateStateMachine",
            json!({
                "name": "my-machine",
                "definition": serde_json::to_string(&simple_pass_machine()).unwrap(),
                "roleArn": "arn:aws:iam::000000000000:role/sf-role",
            }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200, "{}", body_str(&resp));
    let b = body(&resp);
    assert!(
        b["stateMachineArn"]
            .as_str()
            .unwrap()
            .contains("my-machine")
    );
}

#[tokio::test]
async fn test_describe_state_machine() {
    let p = StepFunctionsProvider::new();
    let create_resp = p
        .dispatch(&make_ctx(
            "CreateStateMachine",
            json!({
                "name": "desc-machine",
                "definition": serde_json::to_string(&simple_pass_machine()).unwrap(),
                "roleArn": "arn:aws:iam::000000000000:role/sf-role",
            }),
        ))
        .await
        .unwrap();
    let arn = body(&create_resp)["stateMachineArn"]
        .as_str()
        .unwrap()
        .to_string();

    let resp = p
        .dispatch(&make_ctx(
            "DescribeStateMachine",
            json!({ "stateMachineArn": arn }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
    let b = body(&resp);
    assert_eq!(b["name"], "desc-machine");
    assert_eq!(b["status"], "ACTIVE");
}

#[tokio::test]
async fn test_list_state_machines() {
    let p = StepFunctionsProvider::new();
    p.dispatch(&make_ctx(
        "CreateStateMachine",
        json!({
            "name": "list-machine",
            "definition": serde_json::to_string(&simple_pass_machine()).unwrap(),
            "roleArn": "",
        }),
    ))
    .await
    .unwrap();

    let resp = p
        .dispatch(&make_ctx("ListStateMachines", json!({})))
        .await
        .unwrap();
    let b = body(&resp);
    let machines = b["stateMachines"].as_array().unwrap();
    assert!(machines.iter().any(|m| m["name"] == "list-machine"));
}

#[tokio::test]
async fn test_start_and_describe_execution() {
    let p = StepFunctionsProvider::new();
    let create_resp = p
        .dispatch(&make_ctx(
            "CreateStateMachine",
            json!({
                "name": "exec-machine",
                "definition": serde_json::to_string(&simple_pass_machine()).unwrap(),
                "roleArn": "",
            }),
        ))
        .await
        .unwrap();
    let sm_arn = body(&create_resp)["stateMachineArn"]
        .as_str()
        .unwrap()
        .to_string();

    let resp = p
        .dispatch(&make_ctx(
            "StartExecution",
            json!({
                "stateMachineArn": sm_arn,
                "input": serde_json::to_string(&json!({ "x": 1 })).unwrap(),
                "name": "exec-1",
            }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200, "{}", body_str(&resp));
    let b = body(&resp);
    let exec_arn = b["executionArn"].as_str().unwrap().to_string();
    assert!(exec_arn.contains("exec-machine"));

    let resp = p
        .dispatch(&make_ctx(
            "DescribeExecution",
            json!({ "executionArn": exec_arn }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
    let b = body(&resp);
    assert_eq!(b["status"], "SUCCEEDED");
    // Pass state replaces input with {"greeting":"hello"}
    let output: Value = serde_json::from_str(b["output"].as_str().unwrap()).unwrap();
    assert_eq!(output["greeting"], "hello");
}

#[tokio::test]
async fn test_asl_choice_state() {
    let p = StepFunctionsProvider::new();
    let definition = json!({
        "StartAt": "Check",
        "States": {
            "Check": {
                "Type": "Choice",
                "Choices": [
                    {
                        "Variable": "$.value",
                        "NumericGreaterThan": 10,
                        "Next": "Big"
                    }
                ],
                "Default": "Small"
            },
            "Big": {
                "Type": "Pass",
                "Result": { "size": "big" },
                "End": true
            },
            "Small": {
                "Type": "Pass",
                "Result": { "size": "small" },
                "End": true
            }
        }
    });

    let create_resp = p
        .dispatch(&make_ctx(
            "CreateStateMachine",
            json!({
                "name": "choice-machine",
                "definition": serde_json::to_string(&definition).unwrap(),
                "roleArn": "",
            }),
        ))
        .await
        .unwrap();
    let sm_arn = body(&create_resp)["stateMachineArn"]
        .as_str()
        .unwrap()
        .to_string();

    // Test with value > 10 -> "big"
    let resp = p
        .dispatch(&make_ctx(
            "StartExecution",
            json!({ "stateMachineArn": sm_arn, "input": serde_json::to_string(&json!({ "value": 20 })).unwrap() }),
        ))
        .await
        .unwrap();
    let exec_arn = body(&resp)["executionArn"].as_str().unwrap().to_string();
    let resp = p
        .dispatch(&make_ctx(
            "DescribeExecution",
            json!({ "executionArn": exec_arn }),
        ))
        .await
        .unwrap();
    let output: Value = serde_json::from_str(body(&resp)["output"].as_str().unwrap()).unwrap();
    assert_eq!(output["size"], "big");

    // Test with value <= 10 -> "small"
    let resp = p
        .dispatch(&make_ctx(
            "StartExecution",
            json!({ "stateMachineArn": sm_arn, "input": serde_json::to_string(&json!({ "value": 5 })).unwrap() }),
        ))
        .await
        .unwrap();
    let exec_arn = body(&resp)["executionArn"].as_str().unwrap().to_string();
    let resp = p
        .dispatch(&make_ctx(
            "DescribeExecution",
            json!({ "executionArn": exec_arn }),
        ))
        .await
        .unwrap();
    let output: Value = serde_json::from_str(body(&resp)["output"].as_str().unwrap()).unwrap();
    assert_eq!(output["size"], "small");
}

#[tokio::test]
async fn test_asl_succeed_fail_states() {
    let p = StepFunctionsProvider::new();
    let definition = json!({
        "StartAt": "Done",
        "States": {
            "Done": {
                "Type": "Succeed"
            }
        }
    });
    let create_resp = p
        .dispatch(&make_ctx(
            "CreateStateMachine",
            json!({
                "name": "succeed-machine",
                "definition": serde_json::to_string(&definition).unwrap(),
                "roleArn": "",
            }),
        ))
        .await
        .unwrap();
    let sm_arn = body(&create_resp)["stateMachineArn"]
        .as_str()
        .unwrap()
        .to_string();
    let resp = p
        .dispatch(&make_ctx(
            "StartExecution",
            json!({ "stateMachineArn": sm_arn, "input": "{}", "name": "succ-exec" }),
        ))
        .await
        .unwrap();
    let exec_arn = body(&resp)["executionArn"].as_str().unwrap().to_string();
    let resp = p
        .dispatch(&make_ctx(
            "DescribeExecution",
            json!({ "executionArn": exec_arn }),
        ))
        .await
        .unwrap();
    assert_eq!(body(&resp)["status"], "SUCCEEDED");
}

#[tokio::test]
async fn test_asl_fail_state() {
    let p = StepFunctionsProvider::new();
    let definition = json!({
        "StartAt": "Oops",
        "States": {
            "Oops": {
                "Type": "Fail",
                "Error": "MyError",
                "Cause": "Something went wrong"
            }
        }
    });
    let create_resp = p
        .dispatch(&make_ctx(
            "CreateStateMachine",
            json!({
                "name": "fail-machine",
                "definition": serde_json::to_string(&definition).unwrap(),
                "roleArn": "",
            }),
        ))
        .await
        .unwrap();
    let sm_arn = body(&create_resp)["stateMachineArn"]
        .as_str()
        .unwrap()
        .to_string();
    let resp = p
        .dispatch(&make_ctx(
            "StartExecution",
            json!({ "stateMachineArn": sm_arn, "input": "{}" }),
        ))
        .await
        .unwrap();
    let exec_arn = body(&resp)["executionArn"].as_str().unwrap().to_string();
    let resp = p
        .dispatch(&make_ctx(
            "DescribeExecution",
            json!({ "executionArn": exec_arn }),
        ))
        .await
        .unwrap();
    let b = body(&resp);
    assert_eq!(b["status"], "FAILED");
    assert_eq!(b["error"], "MyError");
}

#[tokio::test]
async fn test_list_executions() {
    let p = StepFunctionsProvider::new();
    let create_resp = p
        .dispatch(&make_ctx(
            "CreateStateMachine",
            json!({
                "name": "list-exec-machine",
                "definition": serde_json::to_string(&simple_pass_machine()).unwrap(),
                "roleArn": "",
            }),
        ))
        .await
        .unwrap();
    let sm_arn = body(&create_resp)["stateMachineArn"]
        .as_str()
        .unwrap()
        .to_string();

    for i in 0..3 {
        p.dispatch(&make_ctx(
            "StartExecution",
            json!({ "stateMachineArn": sm_arn, "input": "{}", "name": format!("exec-{i}") }),
        ))
        .await
        .unwrap();
    }

    let resp = p
        .dispatch(&make_ctx(
            "ListExecutions",
            json!({ "stateMachineArn": sm_arn }),
        ))
        .await
        .unwrap();
    let b = body(&resp);
    assert_eq!(b["executions"].as_array().unwrap().len(), 3);
}

#[tokio::test]
async fn test_delete_state_machine() {
    let p = StepFunctionsProvider::new();
    let create_resp = p
        .dispatch(&make_ctx(
            "CreateStateMachine",
            json!({
                "name": "del-machine",
                "definition": serde_json::to_string(&simple_pass_machine()).unwrap(),
                "roleArn": "",
            }),
        ))
        .await
        .unwrap();
    let arn = body(&create_resp)["stateMachineArn"]
        .as_str()
        .unwrap()
        .to_string();
    let resp = p
        .dispatch(&make_ctx(
            "DeleteStateMachine",
            json!({ "stateMachineArn": arn }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);

    let resp = p
        .dispatch(&make_ctx("ListStateMachines", json!({})))
        .await
        .unwrap();
    let b = body(&resp);
    assert!(
        !b["stateMachines"]
            .as_array()
            .unwrap()
            .iter()
            .any(|m| m["name"] == "del-machine")
    );
}
