use std::collections::HashMap;

use bytes::Bytes;
use openstack_apigateway::ApiGatewayProvider;
use openstack_service_framework::traits::{DispatchResponse, RequestContext, ServiceProvider};
use serde_json::{json, Value};

fn make_ctx(operation: &str, body: Value, path: &str, method: &str) -> RequestContext {
    RequestContext {
        service: "apigateway".to_string(),
        operation: operation.to_string(),
        region: "us-east-1".to_string(),
        account_id: "000000000000".to_string(),
        request_body: body.clone(),
        raw_body: Bytes::from(serde_json::to_vec(&body).unwrap()),
        headers: HashMap::new(),
        path: path.to_string(),
        method: method.to_string(),
        query_params: HashMap::new(),
        spooled_body: None,
    }
}

fn body(resp: &DispatchResponse) -> Value {
    serde_json::from_slice(resp.body.as_bytes()).expect("valid JSON")
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_create_rest_api() {
    let p = ApiGatewayProvider::new();
    let resp = p
        .dispatch(&make_ctx(
            "CreateRestApi",
            json!({ "name": "my-api" }),
            "/restapis",
            "POST",
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 201);
    let b = body(&resp);
    assert_eq!(b["name"], "my-api");
    let api_id = b["id"].as_str().unwrap();
    assert!(!api_id.is_empty());
    let root_id = b["rootResourceId"].as_str().unwrap();
    assert!(!root_id.is_empty());
}

#[tokio::test]
async fn test_get_rest_apis() {
    let p = ApiGatewayProvider::new();
    // Create two APIs
    p.dispatch(&make_ctx(
        "CreateRestApi",
        json!({ "name": "api-a" }),
        "/restapis",
        "POST",
    ))
    .await
    .unwrap();
    p.dispatch(&make_ctx(
        "CreateRestApi",
        json!({ "name": "api-b" }),
        "/restapis",
        "POST",
    ))
    .await
    .unwrap();

    let resp = p
        .dispatch(&make_ctx("GetRestApis", json!({}), "/restapis", "GET"))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
    let b = body(&resp);
    let items = b["items"].as_array().unwrap();
    assert_eq!(items.len(), 2);
}

#[tokio::test]
async fn test_get_rest_api() {
    let p = ApiGatewayProvider::new();
    let create = p
        .dispatch(&make_ctx(
            "CreateRestApi",
            json!({ "name": "my-api" }),
            "/restapis",
            "POST",
        ))
        .await
        .unwrap();
    let api_id = body(&create)["id"].as_str().unwrap().to_string();

    let resp = p
        .dispatch(&make_ctx(
            "GetRestApi",
            json!({}),
            &format!("/restapis/{api_id}"),
            "GET",
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
    let b = body(&resp);
    assert_eq!(b["id"], api_id);
    assert_eq!(b["name"], "my-api");
}

#[tokio::test]
async fn test_get_rest_api_not_found() {
    let p = ApiGatewayProvider::new();
    let resp = p
        .dispatch(&make_ctx(
            "GetRestApi",
            json!({}),
            "/restapis/nonexistent",
            "GET",
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 404);
}

#[tokio::test]
async fn test_delete_rest_api() {
    let p = ApiGatewayProvider::new();
    let create = p
        .dispatch(&make_ctx(
            "CreateRestApi",
            json!({ "name": "to-delete" }),
            "/restapis",
            "POST",
        ))
        .await
        .unwrap();
    let api_id = body(&create)["id"].as_str().unwrap().to_string();

    let resp = p
        .dispatch(&make_ctx(
            "DeleteRestApi",
            json!({}),
            &format!("/restapis/{api_id}"),
            "DELETE",
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 202);

    // Verify gone
    let get_resp = p
        .dispatch(&make_ctx(
            "GetRestApi",
            json!({}),
            &format!("/restapis/{api_id}"),
            "GET",
        ))
        .await
        .unwrap();
    assert_eq!(get_resp.status_code, 404);
}

#[tokio::test]
async fn test_create_resource() {
    let p = ApiGatewayProvider::new();
    let create = p
        .dispatch(&make_ctx(
            "CreateRestApi",
            json!({ "name": "api" }),
            "/restapis",
            "POST",
        ))
        .await
        .unwrap();
    let cb = body(&create);
    let api_id = cb["id"].as_str().unwrap();
    let root_id = cb["rootResourceId"].as_str().unwrap();

    let resp = p
        .dispatch(&make_ctx(
            "CreateResource",
            json!({ "pathPart": "users" }),
            &format!("/restapis/{api_id}/resources/{root_id}"),
            "POST",
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 201);
    let b = body(&resp);
    assert_eq!(b["pathPart"], "users");
    assert_eq!(b["path"], "/users");
}

#[tokio::test]
async fn test_get_resources() {
    let p = ApiGatewayProvider::new();
    let create = p
        .dispatch(&make_ctx(
            "CreateRestApi",
            json!({ "name": "api" }),
            "/restapis",
            "POST",
        ))
        .await
        .unwrap();
    let cb = body(&create);
    let api_id = cb["id"].as_str().unwrap();
    let root_id = cb["rootResourceId"].as_str().unwrap();

    // Create a child resource
    p.dispatch(&make_ctx(
        "CreateResource",
        json!({ "pathPart": "items" }),
        &format!("/restapis/{api_id}/resources/{root_id}"),
        "POST",
    ))
    .await
    .unwrap();

    let resp = p
        .dispatch(&make_ctx(
            "GetResources",
            json!({}),
            &format!("/restapis/{api_id}/resources"),
            "GET",
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
    let b = body(&resp);
    let items = b["items"].as_array().unwrap();
    // root resource + items resource
    assert_eq!(items.len(), 2);
}

#[tokio::test]
async fn test_put_method_and_get_method() {
    let p = ApiGatewayProvider::new();
    let create = p
        .dispatch(&make_ctx(
            "CreateRestApi",
            json!({ "name": "api" }),
            "/restapis",
            "POST",
        ))
        .await
        .unwrap();
    let cb = body(&create);
    let api_id = cb["id"].as_str().unwrap();
    let root_id = cb["rootResourceId"].as_str().unwrap();

    // Create resource
    let res_resp = p
        .dispatch(&make_ctx(
            "CreateResource",
            json!({ "pathPart": "hello" }),
            &format!("/restapis/{api_id}/resources/{root_id}"),
            "POST",
        ))
        .await
        .unwrap();
    let resource_id = body(&res_resp)["id"].as_str().unwrap().to_string();

    // PutMethod
    let put_resp = p
        .dispatch(&make_ctx(
            "PutMethod",
            json!({ "authorizationType": "NONE" }),
            &format!("/restapis/{api_id}/resources/{resource_id}/methods/GET"),
            "PUT",
        ))
        .await
        .unwrap();
    assert_eq!(put_resp.status_code, 200);
    let b = body(&put_resp);
    assert_eq!(b["httpMethod"], "GET");

    // GetMethod
    let get_resp = p
        .dispatch(&make_ctx(
            "GetMethod",
            json!({}),
            &format!("/restapis/{api_id}/resources/{resource_id}/methods/GET"),
            "GET",
        ))
        .await
        .unwrap();
    assert_eq!(get_resp.status_code, 200);
    let b = body(&get_resp);
    assert_eq!(b["httpMethod"], "GET");
    assert_eq!(b["authorizationType"], "NONE");
}

#[tokio::test]
async fn test_put_integration() {
    let p = ApiGatewayProvider::new();
    let create = p
        .dispatch(&make_ctx(
            "CreateRestApi",
            json!({ "name": "api" }),
            "/restapis",
            "POST",
        ))
        .await
        .unwrap();
    let cb = body(&create);
    let api_id = cb["id"].as_str().unwrap();
    let root_id = cb["rootResourceId"].as_str().unwrap();

    let res_resp = p
        .dispatch(&make_ctx(
            "CreateResource",
            json!({ "pathPart": "fn" }),
            &format!("/restapis/{api_id}/resources/{root_id}"),
            "POST",
        ))
        .await
        .unwrap();
    let resource_id = body(&res_resp)["id"].as_str().unwrap().to_string();

    // PutMethod first
    p.dispatch(&make_ctx(
        "PutMethod",
        json!({ "authorizationType": "NONE" }),
        &format!("/restapis/{api_id}/resources/{resource_id}/methods/POST"),
        "PUT",
    ))
    .await
    .unwrap();

    // PutIntegration
    let int_resp = p
        .dispatch(&make_ctx(
            "PutIntegration",
            json!({
                "type": "AWS_PROXY",
                "uri": "arn:aws:apigateway:us-east-1:lambda:path/functions/arn:aws:lambda:us-east-1:000000000000:function:my-fn/invocations",
                "httpMethod": "POST"
            }),
            &format!("/restapis/{api_id}/resources/{resource_id}/methods/POST/integration"),
            "PUT",
        ))
        .await
        .unwrap();
    assert_eq!(int_resp.status_code, 200);
    let b = body(&int_resp);
    assert_eq!(b["type"], "AWS_PROXY");
}

#[tokio::test]
async fn test_create_deployment_and_get_stages() {
    let p = ApiGatewayProvider::new();
    let create = p
        .dispatch(&make_ctx(
            "CreateRestApi",
            json!({ "name": "api" }),
            "/restapis",
            "POST",
        ))
        .await
        .unwrap();
    let api_id = body(&create)["id"].as_str().unwrap().to_string();

    // CreateDeployment
    let dep_resp = p
        .dispatch(&make_ctx(
            "CreateDeployment",
            json!({ "stageName": "prod", "description": "first deploy" }),
            &format!("/restapis/{api_id}/deployments"),
            "POST",
        ))
        .await
        .unwrap();
    assert_eq!(dep_resp.status_code, 201);
    let dep_id = body(&dep_resp)["id"].as_str().unwrap().to_string();
    assert!(!dep_id.is_empty());

    // GetDeployments
    let deps_resp = p
        .dispatch(&make_ctx(
            "GetDeployments",
            json!({}),
            &format!("/restapis/{api_id}/deployments"),
            "GET",
        ))
        .await
        .unwrap();
    assert_eq!(deps_resp.status_code, 200);
    let items = body(&deps_resp)["items"].as_array().unwrap().clone();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["id"], dep_id);

    // GetStages
    let stages_resp = p
        .dispatch(&make_ctx(
            "GetStages",
            json!({}),
            &format!("/restapis/{api_id}/stages"),
            "GET",
        ))
        .await
        .unwrap();
    assert_eq!(stages_resp.status_code, 200);
    let sb = body(&stages_resp);
    let stage_items = sb["item"].as_array().unwrap();
    assert_eq!(stage_items.len(), 1);
    assert_eq!(stage_items[0]["stageName"], "prod");
    assert_eq!(stage_items[0]["deploymentId"], dep_id);
}

#[tokio::test]
async fn test_create_api_missing_name() {
    let p = ApiGatewayProvider::new();
    let resp = p
        .dispatch(&make_ctx("CreateRestApi", json!({}), "/restapis", "POST"))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 400);
}
