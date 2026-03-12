/// Multi-tenancy integration tests: verify per-account and per-region isolation
/// across multiple service providers.
///
/// These tests use multiple providers (SQS, DynamoDB) to verify that:
/// 1. Resources created under account A are not visible to account B
/// 2. Resources created in region A are not visible in region B
/// 3. The same account + region sees only its own resources
use std::collections::HashMap;

use bytes::Bytes;
use openstack_service_framework::traits::{RequestContext, ServiceProvider};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

#[allow(dead_code)]
fn make_ctx(
    service: &str,
    operation: &str,
    region: &str,
    account_id: &str,
    query_params: HashMap<String, String>,
) -> RequestContext {
    RequestContext {
        service: service.to_string(),
        operation: operation.to_string(),
        region: region.to_string(),
        account_id: account_id.to_string(),
        request_body: serde_json::json!({}),
        raw_body: Bytes::new(),
        headers: HashMap::new(),
        path: "/".to_string(),
        method: "POST".to_string(),
        query_params,
        spooled_body: None,
    }
}

fn make_ctx_json(
    service: &str,
    operation: &str,
    region: &str,
    account_id: &str,
    body: serde_json::Value,
) -> RequestContext {
    RequestContext {
        service: service.to_string(),
        operation: operation.to_string(),
        region: region.to_string(),
        account_id: account_id.to_string(),
        request_body: body.clone(),
        raw_body: Bytes::from(serde_json::to_vec(&body).unwrap()),
        headers: HashMap::new(),
        path: "/".to_string(),
        method: "POST".to_string(),
        query_params: HashMap::new(),
        spooled_body: None,
    }
}

fn body_str(resp: &openstack_service_framework::traits::DispatchResponse) -> String {
    String::from_utf8_lossy(resp.body.as_bytes()).to_string()
}

fn body_json(resp: &openstack_service_framework::traits::DispatchResponse) -> serde_json::Value {
    serde_json::from_slice(resp.body.as_bytes()).expect("valid JSON")
}

// ---------------------------------------------------------------------------
// SQS multi-account isolation
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_sqs_multi_account_isolation() {
    use openstack_sqs::SqsProvider;

    let provider = SqsProvider::new();

    // Account A creates a queue
    let mut params_a = HashMap::new();
    params_a.insert("Action".to_string(), "CreateQueue".to_string());
    params_a.insert("QueueName".to_string(), "account-a-queue".to_string());
    let raw_a = b"Action=CreateQueue&QueueName=account-a-queue";
    let ctx_a = RequestContext {
        service: "sqs".to_string(),
        operation: String::new(),
        region: "us-east-1".to_string(),
        account_id: "111111111111".to_string(),
        request_body: serde_json::json!({}),
        raw_body: Bytes::from(raw_a.to_vec()),
        headers: HashMap::new(),
        path: "/".to_string(),
        method: "POST".to_string(),
        query_params: HashMap::new(),
        spooled_body: None,
    };
    let create_a = provider.dispatch(&ctx_a).await.unwrap();
    assert_eq!(create_a.status_code, 200);

    // Account B creates a different queue
    let raw_b = b"Action=CreateQueue&QueueName=account-b-queue";
    let ctx_b = RequestContext {
        service: "sqs".to_string(),
        operation: String::new(),
        region: "us-east-1".to_string(),
        account_id: "222222222222".to_string(),
        request_body: serde_json::json!({}),
        raw_body: Bytes::from(raw_b.to_vec()),
        headers: HashMap::new(),
        path: "/".to_string(),
        method: "POST".to_string(),
        query_params: HashMap::new(),
        spooled_body: None,
    };
    let create_b = provider.dispatch(&ctx_b).await.unwrap();
    assert_eq!(create_b.status_code, 200);

    // Account A lists queues — should only see its own
    let list_raw_a = b"Action=ListQueues";
    let list_ctx_a = RequestContext {
        service: "sqs".to_string(),
        operation: String::new(),
        region: "us-east-1".to_string(),
        account_id: "111111111111".to_string(),
        request_body: serde_json::json!({}),
        raw_body: Bytes::from(list_raw_a.to_vec()),
        headers: HashMap::new(),
        path: "/".to_string(),
        method: "POST".to_string(),
        query_params: HashMap::new(),
        spooled_body: None,
    };
    let list_a = provider.dispatch(&list_ctx_a).await.unwrap();
    let list_a_body = body_str(&list_a);
    assert!(
        list_a_body.contains("account-a-queue"),
        "Account A should see its queue"
    );
    assert!(
        !list_a_body.contains("account-b-queue"),
        "Account A should NOT see account B queue"
    );

    // Account B lists queues — should only see its own
    let list_raw_b = b"Action=ListQueues";
    let list_ctx_b = RequestContext {
        service: "sqs".to_string(),
        operation: String::new(),
        region: "us-east-1".to_string(),
        account_id: "222222222222".to_string(),
        request_body: serde_json::json!({}),
        raw_body: Bytes::from(list_raw_b.to_vec()),
        headers: HashMap::new(),
        path: "/".to_string(),
        method: "POST".to_string(),
        query_params: HashMap::new(),
        spooled_body: None,
    };
    let list_b = provider.dispatch(&list_ctx_b).await.unwrap();
    let list_b_body = body_str(&list_b);
    assert!(
        list_b_body.contains("account-b-queue"),
        "Account B should see its queue"
    );
    assert!(
        !list_b_body.contains("account-a-queue"),
        "Account B should NOT see account A queue"
    );
}

// ---------------------------------------------------------------------------
// DynamoDB multi-region isolation
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_dynamodb_multi_region_isolation() {
    use openstack_dynamodb::DynamoDbProvider;

    let provider = DynamoDbProvider::new();

    // Create a table in us-east-1
    let ctx_us = make_ctx_json(
        "dynamodb",
        "CreateTable",
        "us-east-1",
        "000000000000",
        serde_json::json!({
            "TableName": "east-table",
            "BillingMode": "PAY_PER_REQUEST",
            "AttributeDefinitions": [{ "AttributeName": "id", "AttributeType": "S" }],
            "KeySchema": [{ "AttributeName": "id", "KeyType": "HASH" }]
        }),
    );
    let create_east = provider.dispatch(&ctx_us).await.unwrap();
    assert_eq!(create_east.status_code, 200);

    // Create a table in eu-west-1
    let ctx_eu = make_ctx_json(
        "dynamodb",
        "CreateTable",
        "eu-west-1",
        "000000000000",
        serde_json::json!({
            "TableName": "west-table",
            "BillingMode": "PAY_PER_REQUEST",
            "AttributeDefinitions": [{ "AttributeName": "id", "AttributeType": "S" }],
            "KeySchema": [{ "AttributeName": "id", "KeyType": "HASH" }]
        }),
    );
    let create_west = provider.dispatch(&ctx_eu).await.unwrap();
    assert_eq!(create_west.status_code, 200);

    // List tables in us-east-1 — should only see east-table
    let list_east = provider
        .dispatch(&make_ctx_json(
            "dynamodb",
            "ListTables",
            "us-east-1",
            "000000000000",
            serde_json::json!({}),
        ))
        .await
        .unwrap();
    let east_body = body_json(&list_east);
    let east_tables = east_body["TableNames"].as_array().unwrap();
    assert!(
        east_tables.iter().any(|t| t == "east-table"),
        "us-east-1 should contain east-table"
    );
    assert!(
        !east_tables.iter().any(|t| t == "west-table"),
        "us-east-1 should NOT see west-table"
    );

    // List tables in eu-west-1 — should only see west-table
    let list_west = provider
        .dispatch(&make_ctx_json(
            "dynamodb",
            "ListTables",
            "eu-west-1",
            "000000000000",
            serde_json::json!({}),
        ))
        .await
        .unwrap();
    let west_body = body_json(&list_west);
    let west_tables = west_body["TableNames"].as_array().unwrap();
    assert!(
        west_tables.iter().any(|t| t == "west-table"),
        "eu-west-1 should contain west-table"
    );
    assert!(
        !west_tables.iter().any(|t| t == "east-table"),
        "eu-west-1 should NOT see east-table"
    );
}

// ---------------------------------------------------------------------------
// S3 multi-account isolation
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_s3_multi_account_isolation() {
    use openstack_s3::S3Provider;

    let provider = S3Provider::new("/tmp/openstack-test-s3-multitenancy");

    // Account A creates a bucket
    let ctx_a = RequestContext {
        service: "s3".to_string(),
        operation: "CreateBucket".to_string(),
        region: "us-east-1".to_string(),
        account_id: "111111111111".to_string(),
        request_body: serde_json::json!({}),
        raw_body: Bytes::new(),
        headers: HashMap::new(),
        path: "/account-a-bucket".to_string(),
        method: "PUT".to_string(),
        query_params: HashMap::new(),
        spooled_body: None,
    };
    let create_a = provider.dispatch(&ctx_a).await.unwrap();
    assert_eq!(create_a.status_code, 200);

    // Account B creates a bucket
    let ctx_b = RequestContext {
        service: "s3".to_string(),
        operation: "CreateBucket".to_string(),
        region: "us-east-1".to_string(),
        account_id: "222222222222".to_string(),
        request_body: serde_json::json!({}),
        raw_body: Bytes::new(),
        headers: HashMap::new(),
        path: "/account-b-bucket".to_string(),
        method: "PUT".to_string(),
        query_params: HashMap::new(),
        spooled_body: None,
    };
    let create_b = provider.dispatch(&ctx_b).await.unwrap();
    assert_eq!(create_b.status_code, 200);

    // Account A lists buckets — should only see account-a-bucket
    let list_ctx_a = RequestContext {
        service: "s3".to_string(),
        operation: "ListBuckets".to_string(),
        region: "us-east-1".to_string(),
        account_id: "111111111111".to_string(),
        request_body: serde_json::json!({}),
        raw_body: Bytes::new(),
        headers: HashMap::new(),
        path: "/".to_string(),
        method: "GET".to_string(),
        query_params: HashMap::new(),
        spooled_body: None,
    };
    let list_a = provider.dispatch(&list_ctx_a).await.unwrap();
    let list_a_body = body_str(&list_a);
    assert!(
        list_a_body.contains("account-a-bucket"),
        "Account A should see its bucket"
    );
    assert!(
        !list_a_body.contains("account-b-bucket"),
        "Account A should NOT see account B bucket"
    );
}

// ---------------------------------------------------------------------------
// ARN utility tests
// ---------------------------------------------------------------------------

#[test]
fn test_arn_utility_make_arn() {
    use openstack_service_framework::arn::make_arn;
    let arn = make_arn("sqs", "us-east-1", "000000000000", "my-queue");
    assert_eq!(arn, "arn:aws:sqs:us-east-1:000000000000:my-queue");
}

#[test]
fn test_arn_utility_global_service() {
    use openstack_service_framework::arn::make_arn;
    let arn = make_arn("iam", "", "123456789012", "role/MyRole");
    assert_eq!(arn, "arn:aws:iam::123456789012:role/MyRole");
}

#[test]
fn test_arn_utility_typed() {
    use openstack_service_framework::arn::make_arn_typed;
    let arn = make_arn_typed("lambda", "us-west-2", "999999999999", "function", "my-fn");
    assert_eq!(arn, "arn:aws:lambda:us-west-2:999999999999:function:my-fn");
}

#[test]
fn test_arn_from_ctx() {
    use openstack_service_framework::arn::arn_from_ctx;
    use openstack_service_framework::traits::RequestContext;

    let ctx = RequestContext::new("sns", "CreateTopic", "ap-southeast-1", "000000000000");
    let arn = arn_from_ctx(&ctx, "topic/my-topic");
    assert_eq!(
        arn,
        "arn:aws:sns:ap-southeast-1:000000000000:topic/my-topic"
    );
}

// ---------------------------------------------------------------------------
// Deterministic account ID derivation
// ---------------------------------------------------------------------------

#[test]
fn test_deterministic_account_id_derivation() {
    use openstack_gateway::sigv4::access_key_to_account_id;

    // Default account keys
    assert_eq!(access_key_to_account_id("test"), "000000000000");
    assert_eq!(access_key_to_account_id("mock"), "000000000000");
    assert_eq!(
        access_key_to_account_id("AKIAIOSFODNN7EXAMPLE"),
        "000000000000"
    );

    // Deterministic for unknown keys
    let id1 = access_key_to_account_id("AKIAUNKNOWN123");
    let id2 = access_key_to_account_id("AKIAUNKNOWN123");
    assert_eq!(id1, id2, "Same key must produce same account ID");
    assert_eq!(id1.len(), 12, "Account ID must be 12 digits");

    // Different keys map to different accounts
    let id_a = access_key_to_account_id("AKIAACCOUNTALPHA");
    let id_b = access_key_to_account_id("AKIAACCOUNTBETA0");
    assert_ne!(
        id_a, id_b,
        "Different keys should map to different accounts"
    );
}
