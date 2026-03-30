/// Performance tests for the Lambda provider.
///
/// Run with: `cargo test -p openstack-lambda`
use std::collections::HashMap;
use std::io::Write as IoWrite;
use std::time::Instant;

use base64::{Engine as _, engine::general_purpose::STANDARD as B64};
use bytes::Bytes;
use openstack_lambda::LambdaProvider;
use openstack_service_framework::traits::{RequestContext, ServiceProvider};
use serde_json::{Value, json};

fn make_ctx(operation: &str, body: Value) -> RequestContext {
    RequestContext {
        service: "lambda".to_string(),
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

fn make_ctx_with_path(operation: &str, body: Value, path: &str) -> RequestContext {
    RequestContext {
        path: path.to_string(),
        ..make_ctx(operation, body)
    }
}

fn make_zip_b64(filename: &str, content: &str) -> String {
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

async fn create_function(p: &LambdaProvider, name: &str) {
    let zip = make_zip_b64("lambda_function.py", "def handler(e, c): return {}");
    let resp = p
        .dispatch(&make_ctx(
            "CreateFunction",
            json!({
                "FunctionName": name,
                "Runtime": "python3.12",
                "Handler": "lambda_function.handler",
                "Role": "arn:aws:iam::000000000000:role/role",
                "Code": { "ZipFile": zip },
            }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 201);
}

// ---------------------------------------------------------------------------
// Perf 1 — AddPermission × 50 and GetPolicy must complete under 500 ms
// ---------------------------------------------------------------------------

#[tokio::test]
async fn perf_add_permission_throughput() {
    let p = LambdaProvider::new();
    create_function(&p, "perf-perm-func").await;

    let n = 50usize;
    let start = Instant::now();
    for i in 0..n {
        let resp = p
            .dispatch(&make_ctx_with_path(
                "AddPermission",
                json!({
                    "StatementId": format!("sid-{i}"),
                    "Action": "lambda:InvokeFunction",
                    "Principal": "s3.amazonaws.com",
                }),
                "/2015-03-31/functions/perf-perm-func/policy",
            ))
            .await
            .unwrap();
        assert_eq!(resp.status_code, 200, "AddPermission sid-{i} failed");
    }

    let get_resp = p
        .dispatch(&make_ctx_with_path(
            "GetPolicy",
            json!({}),
            "/2015-03-31/functions/perf-perm-func/policy",
        ))
        .await
        .unwrap();
    assert_eq!(get_resp.status_code, 200);
    let elapsed = start.elapsed();

    let body: Value = serde_json::from_slice(get_resp.body.as_bytes()).unwrap();
    let policy: Value = serde_json::from_str(body["Policy"].as_str().unwrap()).unwrap();
    let stmt_count = policy["Statement"].as_array().unwrap().len();
    assert_eq!(stmt_count, n, "expected {n} statements");

    assert!(
        elapsed.as_millis() < 500,
        "AddPermission×{n} + GetPolicy took {}ms — expected <500ms",
        elapsed.as_millis()
    );
}

// ---------------------------------------------------------------------------
// Perf 2 — CreateFunction × 100 and ListFunctions must complete under 2 s
// ---------------------------------------------------------------------------

#[tokio::test]
async fn perf_create_and_list_functions() {
    let p = LambdaProvider::new();
    let n = 100usize;

    let start = Instant::now();
    for i in 0..n {
        create_function(&p, &format!("perf-fn-{i:03}")).await;
    }

    let list_resp = p
        .dispatch(&make_ctx("ListFunctions", json!({})))
        .await
        .unwrap();
    assert_eq!(list_resp.status_code, 200);
    let elapsed = start.elapsed();

    let body: Value = serde_json::from_slice(list_resp.body.as_bytes()).unwrap();
    let count = body["Functions"].as_array().unwrap().len();
    assert!(count >= n, "expected at least {n} functions, got {count}");

    assert!(
        elapsed.as_millis() < 2000,
        "CreateFunction×{n} + ListFunctions took {}ms — expected <2000ms",
        elapsed.as_millis()
    );
}

// ---------------------------------------------------------------------------
// Perf 3 — RemovePermission round-trip: add 10 permissions then remove them
//           all in under 500 ms
// ---------------------------------------------------------------------------

#[tokio::test]
async fn perf_remove_permission() {
    let p = LambdaProvider::new();
    create_function(&p, "perf-rm-perm-func").await;

    let n = 10usize;
    // Add n permissions
    for i in 0..n {
        let resp = p
            .dispatch(&make_ctx_with_path(
                "AddPermission",
                json!({
                    "StatementId": format!("perf-sid-{i}"),
                    "Action": "lambda:InvokeFunction",
                    "Principal": "s3.amazonaws.com",
                }),
                "/2015-03-31/functions/perf-rm-perm-func/policy",
            ))
            .await
            .unwrap();
        assert_eq!(resp.status_code, 200, "AddPermission perf-sid-{i} failed");
    }

    let start = Instant::now();
    for i in 0..n {
        let resp = p
            .dispatch(&make_ctx_with_path(
                "RemovePermission",
                json!({}),
                &format!("/2015-03-31/functions/perf-rm-perm-func/policy/perf-sid-{i}"),
            ))
            .await
            .unwrap();
        assert_eq!(
            resp.status_code, 204,
            "RemovePermission perf-sid-{i} failed"
        );
    }
    let elapsed = start.elapsed();

    // Verify policy is now empty
    let get_resp = p
        .dispatch(&make_ctx_with_path(
            "GetPolicy",
            json!({}),
            "/2015-03-31/functions/perf-rm-perm-func/policy",
        ))
        .await
        .unwrap();
    // No statements → 404 (no policy)
    assert_eq!(
        get_resp.status_code, 404,
        "expected empty policy (404) after removing all statements"
    );

    assert!(
        elapsed.as_millis() < 500,
        "RemovePermission×{n} took {}ms — expected <500ms",
        elapsed.as_millis()
    );
}
