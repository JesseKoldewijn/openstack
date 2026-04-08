/// Performance tests for CloudFormation provider.
///
/// These cover stack creation, listing, and template retrieval for the
/// in-memory stack engine.
use std::collections::HashMap;
use std::time::Instant;

use openstack_cloudformation::CloudFormationProvider;
use openstack_service_framework::traits::{DispatchResponse, RequestContext, ServiceProvider};
use serde_json::json;

fn make_ctx(operation: &str, params: HashMap<String, String>) -> RequestContext {
    RequestContext {
        service: "cloudformation".to_string(),
        operation: operation.to_string(),
        region: "us-east-1".to_string(),
        account_id: "000000000000".to_string(),
        request_body: json!({}),
        raw_body: None,
        headers: Default::default(),
        path: "/".to_string(),
        method: "POST".to_string(),
        query_params: params,
        request_id: String::new(),
        spooled_body: None,
        body_reader: None,
    }
}

fn body_str(resp: &DispatchResponse) -> String {
    String::from_utf8_lossy(resp.body.as_bytes()).to_string()
}

fn minimal_template() -> String {
    serde_json::to_string(&json!({
        "Resources": {
            "Bucket": {
                "Type": "AWS::S3::Bucket",
                "Properties": {}
            }
        }
    }))
    .unwrap()
}

async fn create_stack(p: &CloudFormationProvider, name: &str) {
    let mut params = HashMap::new();
    params.insert("StackName".to_string(), name.to_string());
    params.insert("TemplateBody".to_string(), minimal_template());
    let resp = p.dispatch(&make_ctx("CreateStack", params)).await.unwrap();
    assert_eq!(resp.status_code, 200);
}

#[tokio::test]
async fn perf_create_stack_throughput() {
    let p = CloudFormationProvider::new();
    let n = 100usize;

    let start = Instant::now();
    for i in 0..n {
        create_stack(&p, &format!("perf-stack-{i:03}")).await;
    }
    let elapsed = start.elapsed();

    assert!(
        elapsed.as_millis() < 2500,
        "CreateStack x{n} took {}ms — expected <2500ms",
        elapsed.as_millis()
    );
}

#[tokio::test]
async fn perf_describe_stacks_many() {
    let p = CloudFormationProvider::new();
    let n = 100usize;
    for i in 0..n {
        create_stack(&p, &format!("perf-desc-stack-{i:03}")).await;
    }

    let start = Instant::now();
    let resp = p
        .dispatch(&make_ctx("DescribeStacks", HashMap::new()))
        .await
        .unwrap();
    let elapsed = start.elapsed();
    assert_eq!(resp.status_code, 200);
    let xml = body_str(&resp);
    assert!(xml.contains("perf-desc-stack-000"));
    assert!(xml.contains("perf-desc-stack-099"));

    assert!(
        elapsed.as_millis() < 500,
        "DescribeStacks({n}) took {}ms — expected <500ms",
        elapsed.as_millis()
    );
}

#[tokio::test]
async fn perf_get_template_round_trip() {
    let p = CloudFormationProvider::new();
    create_stack(&p, "perf-template-stack").await;

    let start = Instant::now();
    for _ in 0..100usize {
        let mut params = HashMap::new();
        params.insert("StackName".to_string(), "perf-template-stack".to_string());
        let resp = p.dispatch(&make_ctx("GetTemplate", params)).await.unwrap();
        assert_eq!(resp.status_code, 200);
    }
    let elapsed = start.elapsed();

    assert!(
        elapsed.as_millis() < 1000,
        "GetTemplate x100 took {}ms — expected <1000ms",
        elapsed.as_millis()
    );
}
