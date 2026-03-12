use std::collections::HashMap;

use bytes::Bytes;
use openstack_dynamodb::DynamoDbProvider;
use openstack_service_framework::traits::{RequestContext, ServiceProvider};
use serde_json::{json, Value};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn make_ctx(operation: &str, body: Value) -> RequestContext {
    RequestContext {
        service: "dynamodb".to_string(),
        operation: operation.to_string(),
        region: "us-east-1".to_string(),
        account_id: "000000000000".to_string(),
        request_body: body.clone(),
        raw_body: Bytes::from(serde_json::to_vec(&body).unwrap()),
        headers: HashMap::new(),
        path: "/".to_string(),
        method: "POST".to_string(),
        query_params: HashMap::new(),
        spooled_body: None,
    }
}

fn body(resp: &openstack_service_framework::traits::DispatchResponse) -> Value {
    serde_json::from_slice(resp.body.as_bytes()).expect("response body is valid JSON")
}

// Create a simple table (pk only)
async fn create_pk_table(provider: &DynamoDbProvider, table_name: &str) {
    let resp = provider
        .dispatch(&make_ctx(
            "CreateTable",
            json!({
                "TableName": table_name,
                "KeySchema": [{ "AttributeName": "pk", "KeyType": "HASH" }],
                "AttributeDefinitions": [{ "AttributeName": "pk", "AttributeType": "S" }],
                "BillingMode": "PAY_PER_REQUEST"
            }),
        ))
        .await
        .unwrap();
    assert_eq!(
        resp.status_code,
        200,
        "CreateTable failed: {}",
        String::from_utf8_lossy(resp.body.as_bytes())
    );
}

// Create a table with pk + sk
async fn create_pksk_table(provider: &DynamoDbProvider, table_name: &str) {
    let resp = provider
        .dispatch(&make_ctx(
            "CreateTable",
            json!({
                "TableName": table_name,
                "KeySchema": [
                    { "AttributeName": "pk", "KeyType": "HASH" },
                    { "AttributeName": "sk", "KeyType": "RANGE" }
                ],
                "AttributeDefinitions": [
                    { "AttributeName": "pk", "AttributeType": "S" },
                    { "AttributeName": "sk", "AttributeType": "S" }
                ],
                "BillingMode": "PAY_PER_REQUEST"
            }),
        ))
        .await
        .unwrap();
    assert_eq!(
        resp.status_code,
        200,
        "CreateTable failed: {}",
        String::from_utf8_lossy(resp.body.as_bytes())
    );
}

// ---------------------------------------------------------------------------
// Table operation tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_create_table() {
    let provider = DynamoDbProvider::new();
    create_pk_table(&provider, "Users").await;

    let resp = provider
        .dispatch(&make_ctx("DescribeTable", json!({ "TableName": "Users" })))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
    let b = body(&resp);
    assert_eq!(b["Table"]["TableName"].as_str(), Some("Users"));
    assert_eq!(b["Table"]["TableStatus"].as_str(), Some("ACTIVE"));
}

#[tokio::test]
async fn test_create_table_already_exists() {
    let provider = DynamoDbProvider::new();
    create_pk_table(&provider, "Users").await;

    let resp = provider
        .dispatch(&make_ctx(
            "CreateTable",
            json!({
                "TableName": "Users",
                "KeySchema": [{ "AttributeName": "pk", "KeyType": "HASH" }],
                "AttributeDefinitions": [{ "AttributeName": "pk", "AttributeType": "S" }],
            }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 400);
    let b = body(&resp);
    assert!(b["__type"]
        .as_str()
        .unwrap()
        .contains("ResourceInUseException"));
}

#[tokio::test]
async fn test_delete_table() {
    let provider = DynamoDbProvider::new();
    create_pk_table(&provider, "TempTable").await;

    let resp = provider
        .dispatch(&make_ctx(
            "DeleteTable",
            json!({ "TableName": "TempTable" }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);

    // should be gone now
    let resp2 = provider
        .dispatch(&make_ctx(
            "DescribeTable",
            json!({ "TableName": "TempTable" }),
        ))
        .await
        .unwrap();
    assert_eq!(resp2.status_code, 400);
    let b = body(&resp2);
    assert!(b["__type"]
        .as_str()
        .unwrap()
        .contains("ResourceNotFoundException"));
}

#[tokio::test]
async fn test_list_tables() {
    let provider = DynamoDbProvider::new();
    create_pk_table(&provider, "TableA").await;
    create_pk_table(&provider, "TableB").await;
    create_pk_table(&provider, "TableC").await;

    let resp = provider
        .dispatch(&make_ctx("ListTables", json!({})))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
    let b = body(&resp);
    let names = b["TableNames"].as_array().unwrap();
    let name_strs: Vec<&str> = names.iter().filter_map(|v| v.as_str()).collect();
    assert!(name_strs.contains(&"TableA"));
    assert!(name_strs.contains(&"TableB"));
    assert!(name_strs.contains(&"TableC"));
}

#[tokio::test]
async fn test_list_tables_pagination() {
    let provider = DynamoDbProvider::new();
    for i in 0..5 {
        create_pk_table(&provider, &format!("PageTable{i}")).await;
    }

    let resp = provider
        .dispatch(&make_ctx("ListTables", json!({ "Limit": 2 })))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
    let b = body(&resp);
    let names = b["TableNames"].as_array().unwrap();
    assert_eq!(names.len(), 2);
    assert!(b["LastEvaluatedTableName"].is_string());
}

// ---------------------------------------------------------------------------
// Item operation tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_put_and_get_item() {
    let provider = DynamoDbProvider::new();
    create_pk_table(&provider, "Items").await;

    // PutItem
    let put_resp = provider
        .dispatch(&make_ctx(
            "PutItem",
            json!({
                "TableName": "Items",
                "Item": {
                    "pk": { "S": "user-1" },
                    "name": { "S": "Alice" },
                    "age": { "N": "30" }
                }
            }),
        ))
        .await
        .unwrap();
    assert_eq!(put_resp.status_code, 200);

    // GetItem
    let get_resp = provider
        .dispatch(&make_ctx(
            "GetItem",
            json!({
                "TableName": "Items",
                "Key": { "pk": { "S": "user-1" } }
            }),
        ))
        .await
        .unwrap();
    assert_eq!(get_resp.status_code, 200);
    let b = body(&get_resp);
    assert_eq!(b["Item"]["name"]["S"].as_str(), Some("Alice"));
    assert_eq!(b["Item"]["age"]["N"].as_str(), Some("30"));
}

#[tokio::test]
async fn test_get_item_not_found() {
    let provider = DynamoDbProvider::new();
    create_pk_table(&provider, "Items").await;

    let resp = provider
        .dispatch(&make_ctx(
            "GetItem",
            json!({
                "TableName": "Items",
                "Key": { "pk": { "S": "nonexistent" } }
            }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
    let b = body(&resp);
    // Empty response when item not found
    assert!(b.get("Item").is_none() || b["Item"].is_null());
}

#[tokio::test]
async fn test_delete_item() {
    let provider = DynamoDbProvider::new();
    create_pk_table(&provider, "Items").await;

    provider
        .dispatch(&make_ctx(
            "PutItem",
            json!({
                "TableName": "Items",
                "Item": { "pk": { "S": "to-delete" }, "val": { "S": "hello" } }
            }),
        ))
        .await
        .unwrap();

    let del_resp = provider
        .dispatch(&make_ctx(
            "DeleteItem",
            json!({
                "TableName": "Items",
                "Key": { "pk": { "S": "to-delete" } },
                "ReturnValues": "ALL_OLD"
            }),
        ))
        .await
        .unwrap();
    assert_eq!(del_resp.status_code, 200);
    let b = body(&del_resp);
    assert_eq!(b["Attributes"]["val"]["S"].as_str(), Some("hello"));

    // Verify gone
    let get_resp = provider
        .dispatch(&make_ctx(
            "GetItem",
            json!({
                "TableName": "Items",
                "Key": { "pk": { "S": "to-delete" } }
            }),
        ))
        .await
        .unwrap();
    let b2 = body(&get_resp);
    assert!(b2.get("Item").is_none() || b2["Item"].is_null());
}

#[tokio::test]
async fn test_update_item_set() {
    let provider = DynamoDbProvider::new();
    create_pk_table(&provider, "Items").await;

    // Create item
    provider
        .dispatch(&make_ctx(
            "PutItem",
            json!({
                "TableName": "Items",
                "Item": { "pk": { "S": "user-1" }, "name": { "S": "Alice" } }
            }),
        ))
        .await
        .unwrap();

    // Update it
    let upd_resp = provider
        .dispatch(&make_ctx(
            "UpdateItem",
            json!({
                "TableName": "Items",
                "Key": { "pk": { "S": "user-1" } },
                "UpdateExpression": "SET name = :new_name",
                "ExpressionAttributeValues": { ":new_name": { "S": "Bob" } },
                "ReturnValues": "ALL_NEW"
            }),
        ))
        .await
        .unwrap();
    assert_eq!(upd_resp.status_code, 200);
    let b = body(&upd_resp);
    assert_eq!(b["Attributes"]["name"]["S"].as_str(), Some("Bob"));
}

#[tokio::test]
async fn test_update_item_add_numeric() {
    let provider = DynamoDbProvider::new();
    create_pk_table(&provider, "Counters").await;

    provider
        .dispatch(&make_ctx(
            "PutItem",
            json!({
                "TableName": "Counters",
                "Item": { "pk": { "S": "counter-1" }, "count": { "N": "10" } }
            }),
        ))
        .await
        .unwrap();

    provider
        .dispatch(&make_ctx(
            "UpdateItem",
            json!({
                "TableName": "Counters",
                "Key": { "pk": { "S": "counter-1" } },
                "UpdateExpression": "ADD count :delta",
                "ExpressionAttributeValues": { ":delta": { "N": "5" } }
            }),
        ))
        .await
        .unwrap();

    let get_resp = provider
        .dispatch(&make_ctx(
            "GetItem",
            json!({
                "TableName": "Counters",
                "Key": { "pk": { "S": "counter-1" } }
            }),
        ))
        .await
        .unwrap();
    let b = body(&get_resp);
    assert_eq!(b["Item"]["count"]["N"].as_str(), Some("15"));
}

// ---------------------------------------------------------------------------
// Condition expression tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_put_item_condition_check_passes() {
    let provider = DynamoDbProvider::new();
    create_pk_table(&provider, "Items").await;

    // First put to create the item
    provider
        .dispatch(&make_ctx(
            "PutItem",
            json!({
                "TableName": "Items",
                "Item": { "pk": { "S": "item-1" }, "status": { "S": "active" } }
            }),
        ))
        .await
        .unwrap();

    // Conditional put — condition should pass
    let resp = provider
        .dispatch(&make_ctx(
            "PutItem",
            json!({
                "TableName": "Items",
                "Item": { "pk": { "S": "item-1" }, "status": { "S": "updated" } },
                "ConditionExpression": "attribute_exists(pk)"
            }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
}

#[tokio::test]
async fn test_put_item_condition_check_fails() {
    let provider = DynamoDbProvider::new();
    create_pk_table(&provider, "Items").await;

    let resp = provider
        .dispatch(&make_ctx(
            "PutItem",
            json!({
                "TableName": "Items",
                "Item": { "pk": { "S": "new-item" }, "val": { "S": "x" } },
                "ConditionExpression": "attribute_exists(pk)"
            }),
        ))
        .await
        .unwrap();
    // Condition fails because item doesn't exist yet
    assert_eq!(resp.status_code, 400);
    let b = body(&resp);
    assert!(b["__type"]
        .as_str()
        .unwrap()
        .contains("ConditionalCheckFailedException"));
}

// ---------------------------------------------------------------------------
// Query tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_query_by_pk() {
    let provider = DynamoDbProvider::new();
    create_pksk_table(&provider, "Orders").await;

    // Insert items for user-1
    for i in 0..3 {
        provider
            .dispatch(&make_ctx(
                "PutItem",
                json!({
                    "TableName": "Orders",
                    "Item": {
                        "pk": { "S": "user-1" },
                        "sk": { "S": format!("order-{i:03}") },
                        "total": { "N": format!("{}", (i + 1) * 10) }
                    }
                }),
            ))
            .await
            .unwrap();
    }
    // Insert item for different user
    provider
        .dispatch(&make_ctx(
            "PutItem",
            json!({
                "TableName": "Orders",
                "Item": {
                    "pk": { "S": "user-2" },
                    "sk": { "S": "order-000" },
                    "total": { "N": "99" }
                }
            }),
        ))
        .await
        .unwrap();

    let resp = provider
        .dispatch(&make_ctx(
            "Query",
            json!({
                "TableName": "Orders",
                "KeyConditionExpression": "pk = :pk",
                "ExpressionAttributeValues": { ":pk": { "S": "user-1" } }
            }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
    let b = body(&resp);
    assert_eq!(b["Count"].as_u64(), Some(3));
    let items = b["Items"].as_array().unwrap();
    assert_eq!(items.len(), 3);
}

#[tokio::test]
async fn test_query_with_sk_begins_with() {
    let provider = DynamoDbProvider::new();
    create_pksk_table(&provider, "Events").await;

    for suffix in &["2024-01-01", "2024-01-02", "2023-12-31"] {
        provider
            .dispatch(&make_ctx(
                "PutItem",
                json!({
                    "TableName": "Events",
                    "Item": {
                        "pk": { "S": "tenant-1" },
                        "sk": { "S": format!("event#{suffix}") },
                        "data": { "S": "payload" }
                    }
                }),
            ))
            .await
            .unwrap();
    }

    let resp = provider
        .dispatch(&make_ctx(
            "Query",
            json!({
                "TableName": "Events",
                "KeyConditionExpression": "pk = :pk AND begins_with(sk, :prefix)",
                "ExpressionAttributeValues": {
                    ":pk": { "S": "tenant-1" },
                    ":prefix": { "S": "event#2024" }
                }
            }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
    let b = body(&resp);
    assert_eq!(b["Count"].as_u64(), Some(2));
}

#[tokio::test]
async fn test_query_with_filter() {
    let provider = DynamoDbProvider::new();
    create_pksk_table(&provider, "Products").await;

    for i in 0..5 {
        provider
            .dispatch(&make_ctx(
                "PutItem",
                json!({
                    "TableName": "Products",
                    "Item": {
                        "pk": { "S": "category-A" },
                        "sk": { "S": format!("prod-{i:03}") },
                        "price": { "N": format!("{}", (i + 1) * 100) }
                    }
                }),
            ))
            .await
            .unwrap();
    }

    let resp = provider
        .dispatch(&make_ctx(
            "Query",
            json!({
                "TableName": "Products",
                "KeyConditionExpression": "pk = :pk",
                "FilterExpression": "price >= :min_price",
                "ExpressionAttributeValues": {
                    ":pk": { "S": "category-A" },
                    ":min_price": { "N": "300" }
                }
            }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
    let b = body(&resp);
    // prices: 100,200,300,400,500 — >= 300 gives 3
    assert_eq!(b["Count"].as_u64(), Some(3));
}

// ---------------------------------------------------------------------------
// Scan tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_scan_all() {
    let provider = DynamoDbProvider::new();
    create_pk_table(&provider, "Catalog").await;

    for i in 0..4 {
        provider
            .dispatch(&make_ctx(
                "PutItem",
                json!({
                    "TableName": "Catalog",
                    "Item": { "pk": { "S": format!("item-{i}") }, "v": { "N": format!("{i}") } }
                }),
            ))
            .await
            .unwrap();
    }

    let resp = provider
        .dispatch(&make_ctx("Scan", json!({ "TableName": "Catalog" })))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
    let b = body(&resp);
    assert_eq!(b["Count"].as_u64(), Some(4));
}

#[tokio::test]
async fn test_scan_with_filter() {
    let provider = DynamoDbProvider::new();
    create_pk_table(&provider, "Users").await;

    for (pk, active) in &[("u1", true), ("u2", false), ("u3", true)] {
        provider
            .dispatch(&make_ctx(
                "PutItem",
                json!({
                    "TableName": "Users",
                    "Item": {
                        "pk": { "S": pk },
                        "active": { "BOOL": active }
                    }
                }),
            ))
            .await
            .unwrap();
    }

    let resp = provider
        .dispatch(&make_ctx(
            "Scan",
            json!({
                "TableName": "Users",
                "FilterExpression": "attribute_exists(pk)"
            }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
    let b = body(&resp);
    assert_eq!(b["Count"].as_u64(), Some(3));
}

#[tokio::test]
async fn test_scan_with_limit() {
    let provider = DynamoDbProvider::new();
    create_pk_table(&provider, "BigTable").await;

    for i in 0..10 {
        provider
            .dispatch(&make_ctx(
                "PutItem",
                json!({
                    "TableName": "BigTable",
                    "Item": { "pk": { "S": format!("item-{i:03}") } }
                }),
            ))
            .await
            .unwrap();
    }

    let resp = provider
        .dispatch(&make_ctx(
            "Scan",
            json!({ "TableName": "BigTable", "Limit": 3 }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
    let b = body(&resp);
    assert_eq!(b["Items"].as_array().unwrap().len(), 3);
}

// ---------------------------------------------------------------------------
// Batch operation tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_batch_write_and_get() {
    let provider = DynamoDbProvider::new();
    create_pk_table(&provider, "BatchTable").await;

    // BatchWriteItem
    let write_resp = provider
        .dispatch(&make_ctx(
            "BatchWriteItem",
            json!({
                "RequestItems": {
                    "BatchTable": [
                        { "PutRequest": { "Item": { "pk": { "S": "a" }, "v": { "N": "1" } } } },
                        { "PutRequest": { "Item": { "pk": { "S": "b" }, "v": { "N": "2" } } } },
                        { "PutRequest": { "Item": { "pk": { "S": "c" }, "v": { "N": "3" } } } }
                    ]
                }
            }),
        ))
        .await
        .unwrap();
    assert_eq!(write_resp.status_code, 200);

    // BatchGetItem
    let get_resp = provider
        .dispatch(&make_ctx(
            "BatchGetItem",
            json!({
                "RequestItems": {
                    "BatchTable": {
                        "Keys": [
                            { "pk": { "S": "a" } },
                            { "pk": { "S": "c" } },
                            { "pk": { "S": "missing" } }
                        ]
                    }
                }
            }),
        ))
        .await
        .unwrap();
    assert_eq!(get_resp.status_code, 200);
    let b = body(&get_resp);
    let found = b["Responses"]["BatchTable"].as_array().unwrap();
    assert_eq!(found.len(), 2); // "missing" not returned
}

#[tokio::test]
async fn test_batch_write_delete() {
    let provider = DynamoDbProvider::new();
    create_pk_table(&provider, "BatchDel").await;

    // Insert 3 items
    for k in &["x", "y", "z"] {
        provider
            .dispatch(&make_ctx(
                "PutItem",
                json!({ "TableName": "BatchDel", "Item": { "pk": { "S": k } } }),
            ))
            .await
            .unwrap();
    }

    // Delete 2 via BatchWriteItem
    provider
        .dispatch(&make_ctx(
            "BatchWriteItem",
            json!({
                "RequestItems": {
                    "BatchDel": [
                        { "DeleteRequest": { "Key": { "pk": { "S": "x" } } } },
                        { "DeleteRequest": { "Key": { "pk": { "S": "y" } } } }
                    ]
                }
            }),
        ))
        .await
        .unwrap();

    let scan_resp = provider
        .dispatch(&make_ctx("Scan", json!({ "TableName": "BatchDel" })))
        .await
        .unwrap();
    let b = body(&scan_resp);
    assert_eq!(b["Count"].as_u64(), Some(1));
}

// ---------------------------------------------------------------------------
// Transaction tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_transact_write_items() {
    let provider = DynamoDbProvider::new();
    create_pk_table(&provider, "Accounts").await;

    provider
        .dispatch(&make_ctx(
            "PutItem",
            json!({
                "TableName": "Accounts",
                "Item": { "pk": { "S": "acc-1" }, "balance": { "N": "100" } }
            }),
        ))
        .await
        .unwrap();

    let txn_resp = provider
        .dispatch(&make_ctx(
            "TransactWriteItems",
            json!({
                "TransactItems": [
                    {
                        "Put": {
                            "TableName": "Accounts",
                            "Item": { "pk": { "S": "acc-2" }, "balance": { "N": "50" } }
                        }
                    },
                    {
                        "Update": {
                            "TableName": "Accounts",
                            "Key": { "pk": { "S": "acc-1" } },
                            "UpdateExpression": "SET balance = :new",
                            "ExpressionAttributeValues": { ":new": { "N": "50" } }
                        }
                    }
                ]
            }),
        ))
        .await
        .unwrap();
    assert_eq!(txn_resp.status_code, 200);

    let get1 = provider
        .dispatch(&make_ctx(
            "GetItem",
            json!({ "TableName": "Accounts", "Key": { "pk": { "S": "acc-1" } } }),
        ))
        .await
        .unwrap();
    let b1 = body(&get1);
    assert_eq!(b1["Item"]["balance"]["N"].as_str(), Some("50"));

    let get2 = provider
        .dispatch(&make_ctx(
            "GetItem",
            json!({ "TableName": "Accounts", "Key": { "pk": { "S": "acc-2" } } }),
        ))
        .await
        .unwrap();
    let b2 = body(&get2);
    assert_eq!(b2["Item"]["balance"]["N"].as_str(), Some("50"));
}

#[tokio::test]
async fn test_transact_write_condition_cancel() {
    let provider = DynamoDbProvider::new();
    create_pk_table(&provider, "Idempotent").await;

    // TransactWrite with a condition that fails — entire txn is cancelled
    let resp = provider
        .dispatch(&make_ctx(
            "TransactWriteItems",
            json!({
                "TransactItems": [
                    {
                        "ConditionCheck": {
                            "TableName": "Idempotent",
                            "Key": { "pk": { "S": "missing" } },
                            "ConditionExpression": "attribute_exists(pk)"
                        }
                    }
                ]
            }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 400);
    let b = body(&resp);
    assert!(b["__type"]
        .as_str()
        .unwrap()
        .contains("TransactionCanceledException"));
}

#[tokio::test]
async fn test_transact_get_items() {
    let provider = DynamoDbProvider::new();
    create_pk_table(&provider, "Docs").await;

    for (pk, content) in &[("doc-1", "alpha"), ("doc-2", "beta")] {
        provider
            .dispatch(&make_ctx(
                "PutItem",
                json!({
                    "TableName": "Docs",
                    "Item": { "pk": { "S": pk }, "content": { "S": content } }
                }),
            ))
            .await
            .unwrap();
    }

    let resp = provider
        .dispatch(&make_ctx(
            "TransactGetItems",
            json!({
                "TransactItems": [
                    { "Get": { "TableName": "Docs", "Key": { "pk": { "S": "doc-1" } } } },
                    { "Get": { "TableName": "Docs", "Key": { "pk": { "S": "doc-2" } } } },
                    { "Get": { "TableName": "Docs", "Key": { "pk": { "S": "doc-missing" } } } }
                ]
            }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
    let b = body(&resp);
    let responses = b["Responses"].as_array().unwrap();
    assert_eq!(responses.len(), 3);
    assert_eq!(responses[0]["Item"]["content"]["S"].as_str(), Some("alpha"));
    assert_eq!(responses[1]["Item"]["content"]["S"].as_str(), Some("beta"));
    // Third response — empty object for missing item
    assert!(responses[2].get("Item").is_none() || responses[2]["Item"].is_null());
}

// ---------------------------------------------------------------------------
// GSI tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_query_gsi() {
    let provider = DynamoDbProvider::new();

    // Create table with GSI
    let resp = provider
        .dispatch(&make_ctx(
            "CreateTable",
            json!({
                "TableName": "GsiTable",
                "KeySchema": [{ "AttributeName": "pk", "KeyType": "HASH" }],
                "AttributeDefinitions": [
                    { "AttributeName": "pk", "AttributeType": "S" },
                    { "AttributeName": "gsi_pk", "AttributeType": "S" }
                ],
                "GlobalSecondaryIndexes": [{
                    "IndexName": "gsi-index",
                    "KeySchema": [{ "AttributeName": "gsi_pk", "KeyType": "HASH" }],
                    "Projection": { "ProjectionType": "ALL" }
                }],
                "BillingMode": "PAY_PER_REQUEST"
            }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);

    // Put items with different gsi_pk values
    for (pk, gsi_pk) in &[
        ("item-1", "group-A"),
        ("item-2", "group-A"),
        ("item-3", "group-B"),
    ] {
        provider
            .dispatch(&make_ctx(
                "PutItem",
                json!({
                    "TableName": "GsiTable",
                    "Item": { "pk": { "S": pk }, "gsi_pk": { "S": gsi_pk } }
                }),
            ))
            .await
            .unwrap();
    }

    // Query by GSI
    let qresp = provider
        .dispatch(&make_ctx(
            "Query",
            json!({
                "TableName": "GsiTable",
                "IndexName": "gsi-index",
                "KeyConditionExpression": "gsi_pk = :v",
                "ExpressionAttributeValues": { ":v": { "S": "group-A" } }
            }),
        ))
        .await
        .unwrap();
    assert_eq!(qresp.status_code, 200);
    let b = body(&qresp);
    assert_eq!(b["Count"].as_u64(), Some(2));
}

// ---------------------------------------------------------------------------
// Stream tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_stream_list_and_describe() {
    let provider = DynamoDbProvider::new();

    // Create table with streams
    let resp = provider
        .dispatch(&make_ctx(
            "CreateTable",
            json!({
                "TableName": "StreamTable",
                "KeySchema": [{ "AttributeName": "pk", "KeyType": "HASH" }],
                "AttributeDefinitions": [{ "AttributeName": "pk", "AttributeType": "S" }],
                "StreamSpecification": { "StreamEnabled": true, "StreamViewType": "NEW_AND_OLD_IMAGES" }
            }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
    let b = body(&resp);
    let stream_arn = b["TableDescription"]["LatestStreamArn"]
        .as_str()
        .unwrap()
        .to_string();

    // ListStreams
    let ls_resp = provider
        .dispatch(&make_ctx(
            "ListStreams",
            json!({ "TableName": "StreamTable" }),
        ))
        .await
        .unwrap();
    assert_eq!(ls_resp.status_code, 200);
    let ls_body = body(&ls_resp);
    let streams = ls_body["Streams"].as_array().unwrap();
    assert_eq!(streams.len(), 1);

    // DescribeStream
    let ds_resp = provider
        .dispatch(&make_ctx(
            "DescribeStream",
            json!({ "StreamArn": stream_arn }),
        ))
        .await
        .unwrap();
    assert_eq!(ds_resp.status_code, 200);
    let ds_body = body(&ds_resp);
    assert_eq!(
        ds_body["StreamDescription"]["StreamStatus"].as_str(),
        Some("ENABLED")
    );
}

#[tokio::test]
async fn test_stream_get_records() {
    let provider = DynamoDbProvider::new();

    provider
        .dispatch(&make_ctx(
            "CreateTable",
            json!({
                "TableName": "StreamRec",
                "KeySchema": [{ "AttributeName": "pk", "KeyType": "HASH" }],
                "AttributeDefinitions": [{ "AttributeName": "pk", "AttributeType": "S" }],
                "StreamSpecification": { "StreamEnabled": true, "StreamViewType": "NEW_AND_OLD_IMAGES" }
            }),
        ))
        .await
        .unwrap();

    // Get stream ARN
    let desc = provider
        .dispatch(&make_ctx(
            "DescribeTable",
            json!({ "TableName": "StreamRec" }),
        ))
        .await
        .unwrap();
    let desc_body = body(&desc);
    let stream_arn = desc_body["Table"]["LatestStreamArn"]
        .as_str()
        .unwrap()
        .to_string();

    // Put some items to generate stream records
    for i in 0..3 {
        provider
            .dispatch(&make_ctx(
                "PutItem",
                json!({
                    "TableName": "StreamRec",
                    "Item": { "pk": { "S": format!("item-{i}") }, "v": { "N": format!("{i}") } }
                }),
            ))
            .await
            .unwrap();
    }

    // Get shard iterator
    let si_resp = provider
        .dispatch(&make_ctx(
            "GetShardIterator",
            json!({
                "StreamArn": stream_arn,
                "ShardId": "shardId-00000000001",
                "ShardIteratorType": "TRIM_HORIZON"
            }),
        ))
        .await
        .unwrap();
    assert_eq!(si_resp.status_code, 200);
    let si_body = body(&si_resp);
    let iterator = si_body["ShardIterator"].as_str().unwrap().to_string();

    // GetRecords
    let rec_resp = provider
        .dispatch(&make_ctx(
            "GetRecords",
            json!({ "ShardIterator": iterator }),
        ))
        .await
        .unwrap();
    assert_eq!(rec_resp.status_code, 200);
    let rec_body = body(&rec_resp);
    let records = rec_body["Records"].as_array().unwrap();
    assert_eq!(records.len(), 3);
    assert!(rec_body["NextShardIterator"].is_string());
}

// ---------------------------------------------------------------------------
// UpdateTable
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_update_table_enable_stream() {
    let provider = DynamoDbProvider::new();
    create_pk_table(&provider, "NoStream").await;

    let resp = provider
        .dispatch(&make_ctx(
            "UpdateTable",
            json!({
                "TableName": "NoStream",
                "StreamSpecification": { "StreamEnabled": true, "StreamViewType": "KEYS_ONLY" }
            }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
    let b = body(&resp);
    assert!(
        b["TableDescription"]["StreamSpecification"]["StreamEnabled"]
            .as_bool()
            .unwrap_or(false)
    );
}

// ---------------------------------------------------------------------------
// Table not found errors
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_operations_on_missing_table_return_404() {
    let provider = DynamoDbProvider::new();

    for (op, req_body) in &[
        (
            "PutItem",
            json!({ "TableName": "Ghost", "Item": { "pk": { "S": "x" } } }),
        ),
        (
            "GetItem",
            json!({ "TableName": "Ghost", "Key": { "pk": { "S": "x" } } }),
        ),
        (
            "DeleteItem",
            json!({ "TableName": "Ghost", "Key": { "pk": { "S": "x" } } }),
        ),
        (
            "UpdateItem",
            json!({ "TableName": "Ghost", "Key": { "pk": { "S": "x" } } }),
        ),
        (
            "Query",
            json!({ "TableName": "Ghost", "KeyConditionExpression": "pk = :v", "ExpressionAttributeValues": { ":v": { "S": "x" } } }),
        ),
        ("Scan", json!({ "TableName": "Ghost" })),
    ] {
        let resp = provider
            .dispatch(&make_ctx(op, req_body.clone()))
            .await
            .unwrap();
        assert_eq!(resp.status_code, 400, "expected 400 for {op}");
        let b = body(&resp);
        assert!(
            b["__type"]
                .as_str()
                .unwrap_or("")
                .contains("ResourceNotFoundException"),
            "op={op}, body={b}"
        );
    }
}
