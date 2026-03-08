use std::collections::HashMap;

use base64::{Engine as _, engine::general_purpose::STANDARD as B64};
use bytes::Bytes;
use openstack_lambda::LambdaProvider;
use openstack_service_framework::traits::{DispatchResponse, RequestContext, ServiceProvider};
use serde_json::{Value, json};

// ---------------------------------------------------------------------------
// Test helpers
// ---------------------------------------------------------------------------

fn make_ctx(operation: &str, body: Value) -> RequestContext {
    RequestContext {
        service: "lambda".to_string(),
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

fn make_ctx_with_path(operation: &str, body: Value, path: &str) -> RequestContext {
    RequestContext {
        service: "lambda".to_string(),
        operation: operation.to_string(),
        region: "us-east-1".to_string(),
        account_id: "000000000000".to_string(),
        request_body: body.clone(),
        raw_body: Bytes::from(serde_json::to_vec(&body).unwrap()),
        headers: HashMap::new(),
        path: path.to_string(),
        method: "POST".to_string(),
        query_params: HashMap::new(),
    }
}

fn body(resp: &DispatchResponse) -> Value {
    serde_json::from_slice(&resp.body).expect("response body is valid JSON")
}

fn body_str(resp: &DispatchResponse) -> String {
    String::from_utf8_lossy(&resp.body).to_string()
}

/// Make a minimal valid base64-encoded zip containing a handler file.
fn make_zip_b64(filename: &str, content: &str) -> String {
    use std::io::Write;
    let mut buf = Vec::new();
    {
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(&mut buf));
        let opts = zip::write::FileOptions::<()>::default()
            .compression_method(zip::CompressionMethod::Stored);
        zip.start_file(filename, opts).unwrap();
        zip.write_all(content.as_bytes()).unwrap();
        zip.finish().unwrap();
    }
    B64.encode(&buf)
}

async fn create_function(p: &LambdaProvider, name: &str) -> DispatchResponse {
    let zip = make_zip_b64("lambda_function.py", "def handler(e, c): return {}");
    p.dispatch(&make_ctx(
        "CreateFunction",
        json!({
            "FunctionName": name,
            "Runtime": "python3.12",
            "Handler": "lambda_function.handler",
            "Role": "arn:aws:iam::000000000000:role/test-role",
            "Code": { "ZipFile": zip },
        }),
    ))
    .await
    .unwrap()
}

// ---------------------------------------------------------------------------
// CreateFunction
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_create_function() {
    let p = LambdaProvider::new();
    let resp = create_function(&p, "my-func").await;
    assert_eq!(resp.status_code, 201, "unexpected: {}", body_str(&resp));
    let b = body(&resp);
    assert_eq!(b["FunctionName"], "my-func");
    assert_eq!(b["Runtime"], "python3.12");
    assert!(b["FunctionArn"].as_str().unwrap().contains("my-func"));
    assert_eq!(b["State"], "Active");
}

#[tokio::test]
async fn test_create_function_duplicate() {
    let p = LambdaProvider::new();
    create_function(&p, "dup-func").await;
    let resp = create_function(&p, "dup-func").await;
    assert_eq!(resp.status_code, 409);
    assert!(body_str(&resp).contains("ResourceConflictException"));
}

// ---------------------------------------------------------------------------
// GetFunction / GetFunctionConfiguration
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_get_function() {
    let p = LambdaProvider::new();
    create_function(&p, "get-me").await;

    let resp = p
        .dispatch(&make_ctx_with_path(
            "GetFunction",
            json!({}),
            "/2015-03-31/functions/get-me",
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200, "{}", body_str(&resp));
    let b = body(&resp);
    assert_eq!(b["Configuration"]["FunctionName"], "get-me");
}

#[tokio::test]
async fn test_get_function_not_found() {
    let p = LambdaProvider::new();
    let resp = p
        .dispatch(&make_ctx_with_path(
            "GetFunction",
            json!({}),
            "/2015-03-31/functions/no-such-func",
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 404);
}

#[tokio::test]
async fn test_get_function_configuration() {
    let p = LambdaProvider::new();
    create_function(&p, "cfg-func").await;

    let resp = p
        .dispatch(&make_ctx_with_path(
            "GetFunctionConfiguration",
            json!({}),
            "/2015-03-31/functions/cfg-func",
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200, "{}", body_str(&resp));
    assert_eq!(body(&resp)["Runtime"], "python3.12");
}

// ---------------------------------------------------------------------------
// ListFunctions
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_list_functions() {
    let p = LambdaProvider::new();
    create_function(&p, "fn-a").await;
    create_function(&p, "fn-b").await;

    let resp = p
        .dispatch(&make_ctx("ListFunctions", json!({})))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
    let fns = body(&resp)["Functions"].as_array().unwrap().clone();
    let names: Vec<&str> = fns
        .iter()
        .filter_map(|f| f["FunctionName"].as_str())
        .collect();
    assert!(names.contains(&"fn-a"));
    assert!(names.contains(&"fn-b"));
}

// ---------------------------------------------------------------------------
// DeleteFunction
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_delete_function() {
    let p = LambdaProvider::new();
    create_function(&p, "del-func").await;

    let resp = p
        .dispatch(&make_ctx_with_path(
            "DeleteFunction",
            json!({}),
            "/2015-03-31/functions/del-func",
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 204, "{}", body_str(&resp));

    // Confirm it's gone
    let resp2 = p
        .dispatch(&make_ctx_with_path(
            "GetFunction",
            json!({}),
            "/2015-03-31/functions/del-func",
        ))
        .await
        .unwrap();
    assert_eq!(resp2.status_code, 404);
}

// ---------------------------------------------------------------------------
// UpdateFunctionCode
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_update_function_code() {
    let p = LambdaProvider::new();
    create_function(&p, "upd-code").await;

    let new_zip = make_zip_b64("lambda_function.py", "def handler(e, c): return {'v': 2}");
    let resp = p
        .dispatch(&make_ctx_with_path(
            "UpdateFunctionCode",
            json!({ "ZipFile": new_zip }),
            "/2015-03-31/functions/upd-code/code",
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200, "{}", body_str(&resp));
    let b = body(&resp);
    // SHA256 should be different now
    assert!(!b["CodeSha256"].as_str().unwrap().is_empty());
}

// ---------------------------------------------------------------------------
// UpdateFunctionConfiguration
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_update_function_configuration() {
    let p = LambdaProvider::new();
    create_function(&p, "upd-cfg").await;

    let resp = p
        .dispatch(&make_ctx_with_path(
            "UpdateFunctionConfiguration",
            json!({ "Timeout": 60, "MemorySize": 256, "Description": "updated" }),
            "/2015-03-31/functions/upd-cfg",
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200, "{}", body_str(&resp));
    let b = body(&resp);
    assert_eq!(b["Timeout"], 60);
    assert_eq!(b["MemorySize"], 256);
    assert_eq!(b["Description"], "updated");
}

// ---------------------------------------------------------------------------
// PublishVersion
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_publish_version() {
    let p = LambdaProvider::new();
    create_function(&p, "ver-func").await;

    let resp = p
        .dispatch(&make_ctx_with_path(
            "PublishVersion",
            json!({ "Description": "v1" }),
            "/2015-03-31/functions/ver-func/versions",
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200, "{}", body_str(&resp));
    assert_eq!(body(&resp)["Version"], "1");
}

// ---------------------------------------------------------------------------
// PublishLayerVersion / GetLayerVersion / ListLayerVersions
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_publish_layer_version() {
    let p = LambdaProvider::new();
    let zip = make_zip_b64("lib/helper.py", "pass");

    let resp = p
        .dispatch(&make_ctx_with_path(
            "PublishLayerVersion",
            json!({
                "Description": "my layer",
                "CompatibleRuntimes": ["python3.12"],
                "Content": { "ZipFile": zip },
            }),
            "/2015-03-31/layers/my-layer/versions",
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 201, "{}", body_str(&resp));
    let b = body(&resp);
    assert_eq!(b["Version"], 1);
    assert!(
        b["LayerVersionArn"]
            .as_str()
            .unwrap()
            .contains("my-layer:1")
    );
}

#[tokio::test]
async fn test_get_layer_version() {
    let p = LambdaProvider::new();
    let zip = make_zip_b64("lib/helper.py", "pass");

    p.dispatch(&make_ctx_with_path(
        "PublishLayerVersion",
        json!({ "Content": { "ZipFile": zip } }),
        "/2015-03-31/layers/layer-x/versions",
    ))
    .await
    .unwrap();

    let resp = p
        .dispatch(&make_ctx_with_path(
            "GetLayerVersion",
            json!({}),
            "/2015-03-31/layers/layer-x/versions/1",
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200, "{}", body_str(&resp));
    assert_eq!(body(&resp)["Version"], 1);
}

#[tokio::test]
async fn test_list_layer_versions() {
    let p = LambdaProvider::new();
    let zip = make_zip_b64("x.py", "");

    for _ in 0..3 {
        p.dispatch(&make_ctx_with_path(
            "PublishLayerVersion",
            json!({ "Content": { "ZipFile": zip } }),
            "/2015-03-31/layers/multi-layer/versions",
        ))
        .await
        .unwrap();
    }

    let resp = p
        .dispatch(&make_ctx_with_path(
            "ListLayerVersions",
            json!({}),
            "/2015-03-31/layers/multi-layer/versions",
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
    assert_eq!(body(&resp)["LayerVersions"].as_array().unwrap().len(), 3);
}

// ---------------------------------------------------------------------------
// Event Source Mappings
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_create_event_source_mapping() {
    let p = LambdaProvider::new();
    create_function(&p, "esm-func").await;

    let resp = p
        .dispatch(&make_ctx(
            "CreateEventSourceMapping",
            json!({
                "FunctionName": "esm-func",
                "EventSourceArn": "arn:aws:sqs:us-east-1:000000000000:my-queue",
                "BatchSize": 5,
            }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 202, "{}", body_str(&resp));
    let b = body(&resp);
    assert!(!b["UUID"].as_str().unwrap().is_empty());
    assert_eq!(b["BatchSize"], 5);
    assert_eq!(b["State"], "Enabled");
}

#[tokio::test]
async fn test_list_event_source_mappings() {
    let p = LambdaProvider::new();
    create_function(&p, "esm-list-func").await;

    for i in 0..2 {
        p.dispatch(&make_ctx(
            "CreateEventSourceMapping",
            json!({
                "FunctionName": "esm-list-func",
                "EventSourceArn": format!("arn:aws:sqs:us-east-1:000000000000:q-{i}"),
            }),
        ))
        .await
        .unwrap();
    }

    let resp = p
        .dispatch(&make_ctx("ListEventSourceMappings", json!({})))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
    assert_eq!(
        body(&resp)["EventSourceMappings"].as_array().unwrap().len(),
        2
    );
}

#[tokio::test]
async fn test_delete_event_source_mapping() {
    let p = LambdaProvider::new();
    create_function(&p, "esm-del-func").await;

    let create_resp = p
        .dispatch(&make_ctx(
            "CreateEventSourceMapping",
            json!({
                "FunctionName": "esm-del-func",
                "EventSourceArn": "arn:aws:sqs:us-east-1:000000000000:del-q",
            }),
        ))
        .await
        .unwrap();
    let uuid = body(&create_resp)["UUID"].as_str().unwrap().to_string();

    let del_resp = p
        .dispatch(&make_ctx_with_path(
            "DeleteEventSourceMapping",
            json!({}),
            &format!("/2015-03-31/event-source-mappings/{uuid}"),
        ))
        .await
        .unwrap();
    assert_eq!(del_resp.status_code, 200);
    assert_eq!(body(&del_resp)["State"], "Deleting");
}

// ---------------------------------------------------------------------------
// Aliases
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_create_alias() {
    let p = LambdaProvider::new();
    create_function(&p, "alias-func").await;

    let resp = p
        .dispatch(&make_ctx_with_path(
            "CreateAlias",
            json!({ "Name": "prod", "FunctionVersion": "$LATEST" }),
            "/2015-03-31/functions/alias-func/aliases",
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 201, "{}", body_str(&resp));
    assert_eq!(body(&resp)["Name"], "prod");
}

#[tokio::test]
async fn test_list_aliases() {
    let p = LambdaProvider::new();
    create_function(&p, "alias-list-func").await;

    for name in &["dev", "staging", "prod"] {
        p.dispatch(&make_ctx_with_path(
            "CreateAlias",
            json!({ "Name": name, "FunctionVersion": "$LATEST" }),
            "/2015-03-31/functions/alias-list-func/aliases",
        ))
        .await
        .unwrap();
    }

    let resp = p
        .dispatch(&make_ctx_with_path(
            "ListAliases",
            json!({}),
            "/2015-03-31/functions/alias-list-func/aliases",
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
    assert_eq!(body(&resp)["Aliases"].as_array().unwrap().len(), 3);
}

// ---------------------------------------------------------------------------
// Docker integration tests (require Docker — skipped in CI unless --include-ignored)
// ---------------------------------------------------------------------------

/// Create a minimal Python Lambda zip with a simple handler.
#[allow(dead_code)]
fn make_python_handler_zip() -> String {
    use std::io::Write;
    let code = r#"import json
def handler(event, context):
    return {"statusCode": 200, "body": json.dumps({"message": "hello from lambda", "input": event})}
"#;
    let mut buf = Vec::new();
    {
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(&mut buf));
        let opts = zip::write::FileOptions::<()>::default()
            .compression_method(zip::CompressionMethod::Stored);
        zip.start_file("lambda_function.py", opts).unwrap();
        zip.write_all(code.as_bytes()).unwrap();
        zip.finish().unwrap();
    }
    B64.encode(&buf)
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn test_docker_invoke_python() {
    let p = LambdaProvider::new();
    let zip = make_python_handler_zip();

    // Create function
    let create_resp = p
        .dispatch(&make_ctx(
            "CreateFunction",
            json!({
                "FunctionName": "docker-python-fn",
                "Runtime": "python3.12",
                "Handler": "lambda_function.handler",
                "Role": "arn:aws:iam::000000000000:role/test-role",
                "Timeout": 30,
                "Code": { "ZipFile": zip },
            }),
        ))
        .await
        .unwrap();
    assert_eq!(create_resp.status_code, 201, "{}", body_str(&create_resp));

    // Invoke with path + raw body
    let invoke_ctx = RequestContext {
        service: "lambda".to_string(),
        operation: "Invoke".to_string(),
        region: "us-east-1".to_string(),
        account_id: "000000000000".to_string(),
        request_body: json!({}),
        raw_body: Bytes::from(b"{\"key\": \"value\"}".to_vec()),
        headers: HashMap::new(),
        path: "/2015-03-31/functions/docker-python-fn/invocations".to_string(),
        method: "POST".to_string(),
        query_params: HashMap::new(),
    };
    let invoke_resp = p.dispatch(&invoke_ctx).await.unwrap();
    assert_eq!(invoke_resp.status_code, 200, "{}", body_str(&invoke_resp));
    let b = body(&invoke_resp);
    assert_eq!(b["statusCode"], 200);
    let inner: Value = serde_json::from_str(b["body"].as_str().unwrap()).unwrap();
    assert_eq!(inner["message"], "hello from lambda");
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn test_docker_invoke_async() {
    let p = LambdaProvider::new();
    let zip = make_python_handler_zip();

    p.dispatch(&make_ctx(
        "CreateFunction",
        json!({
            "FunctionName": "async-fn",
            "Runtime": "python3.12",
            "Handler": "lambda_function.handler",
            "Role": "arn:aws:iam::000000000000:role/test-role",
            "Timeout": 30,
            "Code": { "ZipFile": zip },
        }),
    ))
    .await
    .unwrap();

    // Async (Event) invocation — should return 202 immediately
    let invoke_ctx = RequestContext {
        service: "lambda".to_string(),
        operation: "Invoke".to_string(),
        region: "us-east-1".to_string(),
        account_id: "000000000000".to_string(),
        request_body: json!({}),
        raw_body: Bytes::from(b"{}".to_vec()),
        headers: {
            let mut h = HashMap::new();
            h.insert("x-amz-invocation-type".to_string(), "Event".to_string());
            h
        },
        path: "/2015-03-31/functions/async-fn/invocations".to_string(),
        method: "POST".to_string(),
        query_params: HashMap::new(),
    };
    let resp = p.dispatch(&invoke_ctx).await.unwrap();
    assert_eq!(resp.status_code, 202);
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn test_docker_timeout_enforcement() {
    let p = LambdaProvider::new();

    use std::io::Write;
    let slow_code = r#"import time
def handler(event, context):
    time.sleep(60)
    return {}
"#;
    let mut buf = Vec::new();
    {
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(&mut buf));
        let opts = zip::write::FileOptions::<()>::default()
            .compression_method(zip::CompressionMethod::Stored);
        zip.start_file("lambda_function.py", opts).unwrap();
        zip.write_all(slow_code.as_bytes()).unwrap();
        zip.finish().unwrap();
    }
    let zip_b64 = B64.encode(&buf);

    p.dispatch(&make_ctx(
        "CreateFunction",
        json!({
            "FunctionName": "timeout-fn",
            "Runtime": "python3.12",
            "Handler": "lambda_function.handler",
            "Role": "arn:aws:iam::000000000000:role/test-role",
            "Timeout": 1,
            "Code": { "ZipFile": zip_b64 },
        }),
    ))
    .await
    .unwrap();

    let invoke_ctx = RequestContext {
        service: "lambda".to_string(),
        operation: "Invoke".to_string(),
        region: "us-east-1".to_string(),
        account_id: "000000000000".to_string(),
        request_body: json!({}),
        raw_body: Bytes::from(b"{}".to_vec()),
        headers: HashMap::new(),
        path: "/2015-03-31/functions/timeout-fn/invocations".to_string(),
        method: "POST".to_string(),
        query_params: HashMap::new(),
    };
    let resp = p.dispatch(&invoke_ctx).await.unwrap();
    // Should time out — 429 TooManyRequestsException or 500 ContainerError
    assert!(
        resp.status_code == 429 || resp.status_code == 500,
        "expected timeout/error, got {} body={}",
        resp.status_code,
        body_str(&resp)
    );
}
