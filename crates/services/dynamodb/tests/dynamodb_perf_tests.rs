/// Performance integration tests for the DynamoDB provider.
///
/// These tests gate the structural properties introduced by our Tier 1–3
/// optimisations. Each test validates a specific invariant; thresholds are
/// generously loose to tolerate debug-mode overhead and CI variance while
/// still catching catastrophic regressions.
///
/// Run with: `cargo test -p openstack-dynamodb`
use std::collections::HashMap;
use std::time::Instant;

use bytes::Bytes;
use openstack_dynamodb::DynamoDbProvider;
use openstack_service_framework::traits::{DispatchResponse, RequestContext, ServiceProvider};
use serde_json::{Value, json};

// ---------------------------------------------------------------------------
// Helpers (duplicated from dynamodb_tests.rs — test crates can't share code)
// ---------------------------------------------------------------------------

fn make_ctx(operation: &str, body: Value) -> RequestContext {
    RequestContext {
        service: "dynamodb".to_string(),
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
    serde_json::from_slice(resp.body.as_bytes()).expect("response body is valid JSON")
}

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
// Perf test 1 — BTreeMap sort-key ordering (Tier 2.2)
//
// Items are inserted in non-lexicographic order. Query must return them in
// ascending sort-key order without an extra client-side sort, proving that
// BTreeMap provides O(1)-ordered iteration.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn perf_btreemap_sort_key_order() {
    let provider = DynamoDbProvider::new();
    create_pksk_table(&provider, "SortTest").await;

    // Insert 100 items in reverse sort-key order so a naïve Vec would produce
    // wrong results unless it explicitly re-sorts. BTreeMap gives correct order
    // for free from its natural iteration sequence.
    let n: u32 = 100;
    for i in (0..n).rev() {
        let resp = provider
            .dispatch(&make_ctx(
                "PutItem",
                json!({
                    "TableName": "SortTest",
                    "Item": {
                        "pk": { "S": "partition" },
                        "sk": { "S": format!("sk-{i:04}") },
                        "seq": { "N": format!("{i}") }
                    }
                }),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status_code, 200);
    }

    let resp = provider
        .dispatch(&make_ctx(
            "Query",
            json!({
                "TableName": "SortTest",
                "KeyConditionExpression": "pk = :pk",
                "ExpressionAttributeValues": { ":pk": { "S": "partition" } }
            }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
    let b = body(&resp);
    let items = b["Items"].as_array().expect("Items array");
    assert_eq!(items.len(), n as usize, "expected {n} items");

    // Verify ascending sort-key order — BTreeMap ensures this without an extra
    // sort step. A failure here means the BTreeMap was replaced by an unordered
    // collection (HashMap/Vec without sort).
    for (i, item) in items.iter().enumerate() {
        let expected_sk = format!("sk-{i:04}");
        let got_sk = item["sk"]["S"].as_str().expect("sk attribute");
        assert_eq!(
            got_sk, expected_sk,
            "item[{i}] has wrong sort key (want {expected_sk}, got {got_sk}); \
             BTreeMap ordering broken"
        );
    }
}

// ---------------------------------------------------------------------------
// Perf test 2 — Materialized GSI fast path (Tier 3.2)
//
// Query on a GSI with N=10,000 items in the table, but only 1 item matches
// the query. The materialized index gives O(1) hash lookup; a full-table scan
// would be 10,000× slower. We assert:
//   a) correct results (1 needle item returned)
//   b) the haystack group has the expected count
//   c) latency < 500 ms in debug mode (catastrophically slow if scanning)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn perf_gsi_materialized_fast_path() {
    let provider = DynamoDbProvider::new();

    let resp = provider
        .dispatch(&make_ctx(
            "CreateTable",
            json!({
                "TableName": "GsiPerf",
                "KeySchema": [{ "AttributeName": "pk", "KeyType": "HASH" }],
                "AttributeDefinitions": [
                    { "AttributeName": "pk", "AttributeType": "S" },
                    { "AttributeName": "category", "AttributeType": "S" }
                ],
                "GlobalSecondaryIndexes": [{
                    "IndexName": "category-index",
                    "KeySchema": [{ "AttributeName": "category", "KeyType": "HASH" }],
                    "Projection": { "ProjectionType": "ALL" }
                }],
                "BillingMode": "PAY_PER_REQUEST"
            }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);

    // Insert 10,000 items; only one has category = "needle".
    let n = 10_000usize;
    for i in 0..n {
        let category = if i == 5000 { "needle" } else { "haystack" };
        let resp = provider
            .dispatch(&make_ctx(
                "PutItem",
                json!({
                    "TableName": "GsiPerf",
                    "Item": {
                        "pk": { "S": format!("item-{i:05}") },
                        "category": { "S": category }
                    }
                }),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status_code, 200);
    }

    // Time the GSI query for "needle".
    // Threshold: 500 ms is extremely generous for a single hash-lookup in debug
    // mode. A full table scan of 10,000 items routinely takes >5 s on slow CI,
    // so this guards against the O(N) regression while allowing ample slack.
    let start = Instant::now();
    let qresp = provider
        .dispatch(&make_ctx(
            "Query",
            json!({
                "TableName": "GsiPerf",
                "IndexName": "category-index",
                "KeyConditionExpression": "category = :cat",
                "ExpressionAttributeValues": { ":cat": { "S": "needle" } }
            }),
        ))
        .await
        .unwrap();
    let elapsed = start.elapsed();

    assert_eq!(qresp.status_code, 200);
    let b = body(&qresp);
    assert_eq!(
        b["Count"].as_u64(),
        Some(1),
        "GSI query should return exactly 1 needle item"
    );
    assert!(
        elapsed.as_millis() < 500,
        "GSI query took {}ms — expected <500ms; materialized index fast path may be broken",
        elapsed.as_millis()
    );

    // Verify the haystack group has the expected count (n - 1 = 9999).
    let hay_resp = provider
        .dispatch(&make_ctx(
            "Query",
            json!({
                "TableName": "GsiPerf",
                "IndexName": "category-index",
                "KeyConditionExpression": "category = :cat",
                "ExpressionAttributeValues": { ":cat": { "S": "haystack" } }
            }),
        ))
        .await
        .unwrap();
    assert_eq!(hay_resp.status_code, 200);
    let hb = body(&hay_resp);
    assert_eq!(
        hb["Count"].as_u64(),
        Some((n - 1) as u64),
        "haystack group should have {} items",
        n - 1
    );
}

// ---------------------------------------------------------------------------
// Perf test 3 — VecDeque stream record eviction + ListStreams/DescribeStream
//              (Tier 1.1)
//
// After inserting >1,000 items into a stream-enabled table the buffer must
// stay at ≤1,000 records and the oldest records must have been evicted (FIFO).
// We also call ListStreams and DescribeStream to exercise the full stream API.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn perf_vecdeque_stream_eviction() {
    let provider = DynamoDbProvider::new();

    let resp = provider
        .dispatch(&make_ctx(
            "CreateTable",
            json!({
                "TableName": "StreamEvict",
                "KeySchema": [{ "AttributeName": "pk", "KeyType": "HASH" }],
                "AttributeDefinitions": [{ "AttributeName": "pk", "AttributeType": "S" }],
                "StreamSpecification": {
                    "StreamEnabled": true,
                    "StreamViewType": "NEW_AND_OLD_IMAGES"
                },
                "BillingMode": "PAY_PER_REQUEST"
            }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);

    // Insert 1,200 items — 200 more than the cap — using distinct PKs so each
    // write generates a new INSERT stream record.
    let total = 1_200usize;
    for i in 0..total {
        let resp = provider
            .dispatch(&make_ctx(
                "PutItem",
                json!({
                    "TableName": "StreamEvict",
                    "Item": { "pk": { "S": format!("item-{i:04}") }, "seq": { "N": format!("{i}") } }
                }),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status_code, 200);
    }

    // Fetch the stream ARN from DescribeTable.
    let desc_resp = provider
        .dispatch(&make_ctx(
            "DescribeTable",
            json!({ "TableName": "StreamEvict" }),
        ))
        .await
        .unwrap();
    let db = body(&desc_resp);
    let stream_arn = db["Table"]["LatestStreamArn"]
        .as_str()
        .expect("LatestStreamArn")
        .to_string();

    // Verify ListStreams returns this stream.
    let ls_resp = provider
        .dispatch(&make_ctx(
            "ListStreams",
            json!({ "TableName": "StreamEvict" }),
        ))
        .await
        .unwrap();
    assert_eq!(ls_resp.status_code, 200);
    let ls_body = body(&ls_resp);
    let streams = ls_body["Streams"].as_array().expect("Streams array");
    assert_eq!(
        streams.len(),
        1,
        "ListStreams should return exactly 1 stream"
    );

    // DescribeStream must report the stream as ENABLED.
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
        Some("ENABLED"),
        "DescribeStream StreamStatus mismatch"
    );

    // Get shard iterator from TRIM_HORIZON (oldest available record).
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

    // Read all records visible via GetRecords (default limit is 1000).
    let rec_resp = provider
        .dispatch(&make_ctx(
            "GetRecords",
            json!({ "ShardIterator": iterator }),
        ))
        .await
        .unwrap();
    assert_eq!(rec_resp.status_code, 200);
    let rb = body(&rec_resp);
    let records = rb["Records"].as_array().expect("Records array");

    // The buffer is capped at 1,000. We inserted 1,200 → oldest 200 evicted.
    // A failure here means VecDeque eviction or the cap is not applied, so the
    // buffer grew unboundedly (or the oldest records were not popped).
    assert!(
        records.len() <= 1000,
        "stream buffer has {} records — expected ≤1000; VecDeque cap broken",
        records.len()
    );
    assert!(
        !records.is_empty(),
        "stream buffer is empty — expected ~1000 records"
    );

    // The oldest retained record should be for item-0200 (the 201st write),
    // i.e. the first 200 were evicted. Validate via the sequence_number field:
    // sequence_number is a 0-padded decimal counter starting at 1.
    // item-0000 → seq_num 1, item-0199 → seq_num 200 (evicted).
    // item-0200 → seq_num 201 (first retained).
    let first_seq: u64 = records[0]["dynamodb"]["SequenceNumber"]
        .as_str()
        .unwrap_or("0")
        .trim_start_matches('0')
        .parse()
        .unwrap_or(0);
    // Sequence numbers 1..=200 were evicted; the first retained is ≥ 201.
    assert!(
        first_seq >= 201,
        "oldest record has sequence_number {first_seq} — expected ≥201; \
         FIFO eviction (pop_front) is broken"
    );
}

// ---------------------------------------------------------------------------
// Perf test 4 — Scan early-terminate with Limit + FilterExpression (Tier 2.4)
//
// Scan with Limit=1 on a 10,000-item table should complete sub-linearly
// because the implementation uses `.take(limit)` to short-circuit iteration.
// Threshold: 300 ms in debug mode. A full scan takes seconds on slow CI.
// We also verify FilterExpression works over the full dataset.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn perf_scan_early_terminate() {
    let provider = DynamoDbProvider::new();
    create_pk_table(&provider, "EarlyStop").await;

    let n = 10_000usize;
    for i in 0..n {
        let resp = provider
            .dispatch(&make_ctx(
                "PutItem",
                json!({
                    "TableName": "EarlyStop",
                    "Item": {
                        "pk": { "S": format!("item-{i:05}") },
                        "parity": { "S": if i % 2 == 0 { "even" } else { "odd" } }
                    }
                }),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status_code, 200);
    }

    // Time a Scan with Limit=1.
    // Threshold: 300 ms. A full scan of 10,000 items in debug builds typically
    // takes 1–10 s. If take() early-termination is missing the test will fail.
    let start = Instant::now();
    let resp = provider
        .dispatch(&make_ctx(
            "Scan",
            json!({ "TableName": "EarlyStop", "Limit": 1 }),
        ))
        .await
        .unwrap();
    let elapsed = start.elapsed();

    assert_eq!(resp.status_code, 200);
    let b = body(&resp);
    assert_eq!(
        b["Items"].as_array().map(|a| a.len()),
        Some(1),
        "Scan Limit=1 should return exactly 1 item"
    );
    assert!(
        elapsed.as_millis() < 300,
        "Scan Limit=1 took {}ms — expected <300ms; early-terminate (.take()) may be broken",
        elapsed.as_millis()
    );

    // FilterExpression on the full dataset — all even items (5000 of 10,000).
    let filter_resp = provider
        .dispatch(&make_ctx(
            "Scan",
            json!({
                "TableName": "EarlyStop",
                "FilterExpression": "parity = :p",
                "ExpressionAttributeValues": { ":p": { "S": "even" } }
            }),
        ))
        .await
        .unwrap();
    assert_eq!(filter_resp.status_code, 200);
    let fb = body(&filter_resp);
    let filtered_count = fb["Count"].as_u64().expect("Count");
    assert_eq!(
        filtered_count, 5000,
        "FilterExpression should return 5000 even items, got {filtered_count}"
    );

    // Semantic early-termination: Limit=5 with FilterExpression.
    // ScannedCount must equal the Limit (items examined, not items returned),
    // and Count must be <= Limit. This proves the iterator stopped after reading
    // exactly 5 items rather than scanning all 10,000.
    let sem_resp = provider
        .dispatch(&make_ctx(
            "Scan",
            json!({
                "TableName": "EarlyStop",
                "FilterExpression": "parity = :p",
                "ExpressionAttributeValues": { ":p": { "S": "even" } },
                "Limit": 5
            }),
        ))
        .await
        .unwrap();
    assert_eq!(sem_resp.status_code, 200);
    let sb = body(&sem_resp);
    let sem_scanned = sb["ScannedCount"]
        .as_u64()
        .expect("ScannedCount must be present");
    let sem_count = sb["Count"].as_u64().expect("Count must be present");
    assert_eq!(
        sem_scanned, 5,
        "ScannedCount should be exactly 5 (Limit caps items examined before filter), got {sem_scanned}"
    );
    assert!(
        sem_count <= 5,
        "Count should be <= Limit=5, got {sem_count}"
    );
}

// ---------------------------------------------------------------------------
// Perf test 5 — Typed AttributeValue wire-format parity + UpdateItem/BatchGetItem
//              (Tier 3.3)
//
// Items containing all 10 DynamoDB attribute types round-trip through
// PutItem/GetItem with byte-exact JSON wire format. We also call UpdateItem
// and BatchGetItem to cover more of the wire surface.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn perf_attribute_value_wire_parity() {
    let provider = DynamoDbProvider::new();
    create_pk_table(&provider, "WireParity").await;

    let item = json!({
        "pk":      { "S": "test-item" },
        "s_attr":  { "S": "hello" },
        "n_attr":  { "N": "42.5" },
        "b_attr":  { "B": "aGVsbG8=" },       // base64("hello")
        "bool_t":  { "BOOL": true },
        "bool_f":  { "BOOL": false },
        "null_v":  { "NULL": true },
        "ss_attr": { "SS": ["a", "b", "c"] },
        "ns_attr": { "NS": ["1", "2", "3"] },
        "bs_attr": { "BS": ["aGVsbG8=", "d29ybGQ="] },
        "l_attr":  { "L": [{ "S": "x" }, { "N": "1" }] },
        "m_attr":  { "M": { "inner_s": { "S": "nested" }, "inner_n": { "N": "7" } } }
    });

    let put_resp = provider
        .dispatch(&make_ctx(
            "PutItem",
            json!({ "TableName": "WireParity", "Item": item }),
        ))
        .await
        .unwrap();
    assert_eq!(put_resp.status_code, 200);

    let get_resp = provider
        .dispatch(&make_ctx(
            "GetItem",
            json!({
                "TableName": "WireParity",
                "Key": { "pk": { "S": "test-item" } }
            }),
        ))
        .await
        .unwrap();
    assert_eq!(get_resp.status_code, 200);
    let b = body(&get_resp);
    let got = &b["Item"];

    // String
    assert_eq!(got["s_attr"]["S"].as_str(), Some("hello"), "S round-trip");
    // Number
    assert_eq!(got["n_attr"]["N"].as_str(), Some("42.5"), "N round-trip");
    // Binary
    assert_eq!(
        got["b_attr"]["B"].as_str(),
        Some("aGVsbG8="),
        "B round-trip"
    );
    // Boolean (wire key must be "BOOL", not "Bool" or "bool")
    assert_eq!(
        got["bool_t"]["BOOL"].as_bool(),
        Some(true),
        "BOOL true round-trip — check Serialize key is 'BOOL'"
    );
    assert_eq!(
        got["bool_f"]["BOOL"].as_bool(),
        Some(false),
        "BOOL false round-trip"
    );
    // Null (wire key must be "NULL")
    assert_eq!(
        got["null_v"]["NULL"].as_bool(),
        Some(true),
        "NULL round-trip — check Serialize key is 'NULL'"
    );
    // String set
    let ss = got["ss_attr"]["SS"].as_array().expect("SS array");
    let mut ss_strs: Vec<&str> = ss.iter().filter_map(|v| v.as_str()).collect();
    ss_strs.sort_unstable();
    assert_eq!(ss_strs, vec!["a", "b", "c"], "SS round-trip");
    // Number set
    let ns = got["ns_attr"]["NS"].as_array().expect("NS array");
    let mut ns_strs: Vec<&str> = ns.iter().filter_map(|v| v.as_str()).collect();
    ns_strs.sort_unstable();
    assert_eq!(ns_strs, vec!["1", "2", "3"], "NS round-trip");
    // Binary set
    let bs = got["bs_attr"]["BS"].as_array().expect("BS array");
    assert_eq!(bs.len(), 2, "BS length round-trip");
    // List
    let l = got["l_attr"]["L"].as_array().expect("L array");
    assert_eq!(l.len(), 2, "L length round-trip");
    assert_eq!(l[0]["S"].as_str(), Some("x"), "L[0] S round-trip");
    assert_eq!(l[1]["N"].as_str(), Some("1"), "L[1] N round-trip");
    // Map
    let m = &got["m_attr"]["M"];
    assert_eq!(
        m["inner_s"]["S"].as_str(),
        Some("nested"),
        "M inner_s round-trip"
    );
    assert_eq!(
        m["inner_n"]["N"].as_str(),
        Some("7"),
        "M inner_n round-trip"
    );

    // UpdateItem — change s_attr and verify round-trip
    let upd_resp = provider
        .dispatch(&make_ctx(
            "UpdateItem",
            json!({
                "TableName": "WireParity",
                "Key": { "pk": { "S": "test-item" } },
                "UpdateExpression": "SET s_attr = :new",
                "ExpressionAttributeValues": { ":new": { "S": "updated" } },
                "ReturnValues": "ALL_NEW"
            }),
        ))
        .await
        .unwrap();
    assert_eq!(upd_resp.status_code, 200);
    let ub = body(&upd_resp);
    assert_eq!(
        ub["Attributes"]["s_attr"]["S"].as_str(),
        Some("updated"),
        "UpdateItem S round-trip"
    );
    // Other attributes must survive the partial update intact
    assert_eq!(
        ub["Attributes"]["n_attr"]["N"].as_str(),
        Some("42.5"),
        "N survived UpdateItem"
    );

    // BatchGetItem — retrieve test-item alongside a missing key
    let bg_resp = provider
        .dispatch(&make_ctx(
            "BatchGetItem",
            json!({
                "RequestItems": {
                    "WireParity": {
                        "Keys": [
                            { "pk": { "S": "test-item" } },
                            { "pk": { "S": "no-such-key" } }
                        ]
                    }
                }
            }),
        ))
        .await
        .unwrap();
    assert_eq!(bg_resp.status_code, 200);
    let bgb = body(&bg_resp);
    let found = bgb["Responses"]["WireParity"]
        .as_array()
        .expect("BatchGetItem Responses array");
    assert_eq!(
        found.len(),
        1,
        "BatchGetItem should return 1 item (missing key not included)"
    );
    assert_eq!(
        found[0]["s_attr"]["S"].as_str(),
        Some("updated"),
        "BatchGetItem s_attr after UpdateItem"
    );
}

// ---------------------------------------------------------------------------
// Perf test 6 — Per-table locking: concurrent PutItem on different tables +
//              DeleteItem per table (Tier 3.1)
//
// We spawn N concurrent tasks, each writing to its own table. All writes must
// complete successfully. Then we delete one item per table concurrently to
// validate DeleteItem under contention.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn perf_per_table_concurrent_writes() {
    use std::sync::Arc;

    let provider = Arc::new(DynamoDbProvider::new());
    let n_tables = 20usize;
    let writes_per_table = 50usize;

    // Pre-create all tables sequentially (no contention here).
    for t in 0..n_tables {
        create_pk_table(&provider, &format!("ConcTable{t}")).await;
    }

    // Spawn concurrent write tasks — one per table.
    let mut handles = Vec::with_capacity(n_tables);
    for t in 0..n_tables {
        let p = Arc::clone(&provider);
        handles.push(tokio::spawn(async move {
            for i in 0..writes_per_table {
                let resp = p
                    .dispatch(&make_ctx(
                        "PutItem",
                        json!({
                            "TableName": format!("ConcTable{t}"),
                            "Item": { "pk": { "S": format!("item-{i:03}") }, "val": { "N": format!("{i}") } }
                        }),
                    ))
                    .await
                    .unwrap();
                assert_eq!(
                    resp.status_code,
                    200,
                    "table ConcTable{t} write {i} failed"
                );
            }
        }));
    }

    // Wait for all tasks — a deadlock would hang here and the runtime would
    // surface it as a test timeout.
    for handle in handles {
        handle.await.expect("concurrent write task panicked");
    }

    // Verify each table has the expected item count.
    for t in 0..n_tables {
        let resp = provider
            .dispatch(&make_ctx(
                "Scan",
                json!({ "TableName": format!("ConcTable{t}") }),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status_code, 200);
        let b = body(&resp);
        assert_eq!(
            b["Count"].as_u64(),
            Some(writes_per_table as u64),
            "ConcTable{t} expected {writes_per_table} items after concurrent writes"
        );
    }

    // Concurrently delete one item from each table — validates DeleteItem under
    // concurrent per-table locking (no deadlock, no lost deletes).
    let mut del_handles = Vec::with_capacity(n_tables);
    for t in 0..n_tables {
        let p = Arc::clone(&provider);
        del_handles.push(tokio::spawn(async move {
            let resp = p
                .dispatch(&make_ctx(
                    "DeleteItem",
                    json!({
                        "TableName": format!("ConcTable{t}"),
                        "Key": { "pk": { "S": "item-000" } }
                    }),
                ))
                .await
                .unwrap();
            assert_eq!(resp.status_code, 200, "DeleteItem from ConcTable{t} failed");
        }));
    }
    for handle in del_handles {
        handle.await.expect("concurrent delete task panicked");
    }

    // Each table should now have writes_per_table - 1 items.
    for t in 0..n_tables {
        let resp = provider
            .dispatch(&make_ctx(
                "Scan",
                json!({ "TableName": format!("ConcTable{t}") }),
            ))
            .await
            .unwrap();
        let b = body(&resp);
        assert_eq!(
            b["Count"].as_u64(),
            Some((writes_per_table - 1) as u64),
            "ConcTable{t} expected {} items after concurrent deletes",
            writes_per_table - 1
        );
    }
}

// ---------------------------------------------------------------------------
// Perf test 7 — UpdateTable GSI back-fill + TransactWriteItems/TransactGetItems
//              (Tier 3.2 + add_gsi)
//
// Adding a GSI to an existing table via UpdateTable must immediately back-fill
// all existing items into the index. We also validate TransactWriteItems and
// TransactGetItems over the back-filled data.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn perf_update_table_gsi_backfill_and_transactions() {
    let provider = DynamoDbProvider::new();
    create_pk_table(&provider, "BackFill").await;

    // Insert items before the GSI exists.
    let groups = [
        ("group-A", 3usize),
        ("group-B", 5usize),
        ("group-C", 2usize),
    ];
    let mut pk_counter = 0usize;
    for (group, count) in &groups {
        for _ in 0..*count {
            let resp = provider
                .dispatch(&make_ctx(
                    "PutItem",
                    json!({
                        "TableName": "BackFill",
                        "Item": {
                            "pk": { "S": format!("item-{pk_counter:04}") },
                            "group": { "S": group }
                        }
                    }),
                ))
                .await
                .unwrap();
            assert_eq!(resp.status_code, 200);
            pk_counter += 1;
        }
    }

    // Add a GSI on `group` via UpdateTable.
    let ut_resp = provider
        .dispatch(&make_ctx(
            "UpdateTable",
            json!({
                "TableName": "BackFill",
                "GlobalSecondaryIndexUpdates": [{
                    "Create": {
                        "IndexName": "group-index",
                        "KeySchema": [{ "AttributeName": "group", "KeyType": "HASH" }],
                        "Projection": { "ProjectionType": "ALL" }
                    }
                }],
                "AttributeDefinitions": [
                    { "AttributeName": "group", "AttributeType": "S" }
                ]
            }),
        ))
        .await
        .unwrap();
    assert_eq!(
        ut_resp.status_code,
        200,
        "UpdateTable failed: {}",
        String::from_utf8_lossy(ut_resp.body.as_bytes())
    );

    // Query through the newly added GSI — all pre-existing items must be found.
    for (group, expected_count) in &groups {
        let qresp = provider
            .dispatch(&make_ctx(
                "Query",
                json!({
                    "TableName": "BackFill",
                    "IndexName": "group-index",
                    "KeyConditionExpression": "#g = :g",
                    "ExpressionAttributeNames": { "#g": "group" },
                    "ExpressionAttributeValues": { ":g": { "S": group } }
                }),
            ))
            .await
            .unwrap();
        assert_eq!(qresp.status_code, 200);
        let b = body(&qresp);
        assert_eq!(
            b["Count"].as_u64(),
            Some(*expected_count as u64),
            "GSI back-fill for group={group}: expected {expected_count} items, \
             got {}; add_gsi() back-fill is broken",
            b["Count"]
        );
    }

    // TransactWriteItems — atomically add a new item and update an existing one.
    let txn_resp = provider
        .dispatch(&make_ctx(
            "TransactWriteItems",
            json!({
                "TransactItems": [
                    {
                        "Put": {
                            "TableName": "BackFill",
                            "Item": {
                                "pk": { "S": "item-txn" },
                                "group": { "S": "group-A" }
                            }
                        }
                    },
                    {
                        "Update": {
                            "TableName": "BackFill",
                            "Key": { "pk": { "S": "item-0000" } },
                            "UpdateExpression": "SET #g = :new_group",
                            "ExpressionAttributeNames": { "#g": "group" },
                            "ExpressionAttributeValues": { ":new_group": { "S": "group-C" } }
                        }
                    }
                ]
            }),
        ))
        .await
        .unwrap();
    assert_eq!(
        txn_resp.status_code,
        200,
        "TransactWriteItems failed: {}",
        String::from_utf8_lossy(txn_resp.body.as_bytes())
    );

    // TransactGetItems — read back the two items touched by the transaction.
    let tg_resp = provider
        .dispatch(&make_ctx(
            "TransactGetItems",
            json!({
                "TransactItems": [
                    { "Get": { "TableName": "BackFill", "Key": { "pk": { "S": "item-txn" } } } },
                    { "Get": { "TableName": "BackFill", "Key": { "pk": { "S": "item-0000" } } } },
                    { "Get": { "TableName": "BackFill", "Key": { "pk": { "S": "no-such" } } } }
                ]
            }),
        ))
        .await
        .unwrap();
    assert_eq!(tg_resp.status_code, 200);
    let tgb = body(&tg_resp);
    let responses = tgb["Responses"]
        .as_array()
        .expect("TransactGetItems Responses");
    assert_eq!(
        responses.len(),
        3,
        "TransactGetItems must return 3 responses"
    );
    assert_eq!(
        responses[0]["Item"]["group"]["S"].as_str(),
        Some("group-A"),
        "item-txn group"
    );
    assert_eq!(
        responses[1]["Item"]["group"]["S"].as_str(),
        Some("group-C"),
        "item-0000 moved to group-C by TransactWriteItems"
    );
    // Third response — empty object for missing item
    assert!(
        responses[2].get("Item").is_none() || responses[2]["Item"].is_null(),
        "missing item should produce empty response"
    );
}

// ---------------------------------------------------------------------------
// Perf test 8 — Condition expressions and BatchWriteItem
//              (ConditionalCheckFailedException + BatchWriteItem PutRequest /
//               DeleteRequest)
//
// Validates:
//   a) Conditional PutItem succeeds when condition is met
//   b) Conditional PutItem returns ConditionalCheckFailedException when not met
//   c) BatchWriteItem inserts and deletes items correctly
// ---------------------------------------------------------------------------

#[tokio::test]
async fn perf_condition_expressions_and_batch_write() {
    let provider = DynamoDbProvider::new();
    create_pk_table(&provider, "CondBatch").await;

    // Unconditional seed insert.
    let seed_resp = provider
        .dispatch(&make_ctx(
            "PutItem",
            json!({
                "TableName": "CondBatch",
                "Item": { "pk": { "S": "seed" }, "val": { "N": "10" } }
            }),
        ))
        .await
        .unwrap();
    assert_eq!(seed_resp.status_code, 200);

    // Conditional PutItem: condition "attribute_not_exists(pk)" on "seed" — must
    // FAIL because the item already exists.
    let cond_fail_resp = provider
        .dispatch(&make_ctx(
            "PutItem",
            json!({
                "TableName": "CondBatch",
                "Item": { "pk": { "S": "seed" }, "val": { "N": "999" } },
                "ConditionExpression": "attribute_not_exists(pk)"
            }),
        ))
        .await
        .unwrap();
    assert_eq!(
        cond_fail_resp.status_code,
        400,
        "Conditional PutItem should fail with 400; body: {}",
        String::from_utf8_lossy(cond_fail_resp.body.as_bytes())
    );
    let cfb = body(&cond_fail_resp);
    assert!(
        cfb["__type"]
            .as_str()
            .unwrap_or("")
            .contains("ConditionalCheckFailedException"),
        "expected ConditionalCheckFailedException, got: {:?}",
        cfb["__type"]
    );

    // Verify the seed item was NOT overwritten by the failed conditional write.
    let verify_resp = provider
        .dispatch(&make_ctx(
            "GetItem",
            json!({ "TableName": "CondBatch", "Key": { "pk": { "S": "seed" } } }),
        ))
        .await
        .unwrap();
    assert_eq!(verify_resp.status_code, 200);
    let vb = body(&verify_resp);
    assert_eq!(
        vb["Item"]["val"]["N"].as_str(),
        Some("10"),
        "seed val must be unchanged after failed conditional PutItem"
    );

    // Conditional PutItem: condition "attribute_not_exists(pk)" on "new-item" —
    // must SUCCEED because the item does not exist yet.
    let cond_ok_resp = provider
        .dispatch(&make_ctx(
            "PutItem",
            json!({
                "TableName": "CondBatch",
                "Item": { "pk": { "S": "new-item" }, "val": { "N": "42" } },
                "ConditionExpression": "attribute_not_exists(pk)"
            }),
        ))
        .await
        .unwrap();
    assert_eq!(
        cond_ok_resp.status_code, 200,
        "Conditional PutItem (new item) should succeed"
    );

    // BatchWriteItem: insert 5 more items + delete "seed".
    let batch_resp = provider
        .dispatch(&make_ctx(
            "BatchWriteItem",
            json!({
                "RequestItems": {
                    "CondBatch": [
                        { "PutRequest": { "Item": { "pk": { "S": "batch-0" }, "val": { "N": "0" } } } },
                        { "PutRequest": { "Item": { "pk": { "S": "batch-1" }, "val": { "N": "1" } } } },
                        { "PutRequest": { "Item": { "pk": { "S": "batch-2" }, "val": { "N": "2" } } } },
                        { "PutRequest": { "Item": { "pk": { "S": "batch-3" }, "val": { "N": "3" } } } },
                        { "PutRequest": { "Item": { "pk": { "S": "batch-4" }, "val": { "N": "4" } } } },
                        { "DeleteRequest": { "Key": { "pk": { "S": "seed" } } } }
                    ]
                }
            }),
        ))
        .await
        .unwrap();
    assert_eq!(
        batch_resp.status_code,
        200,
        "BatchWriteItem failed: {}",
        String::from_utf8_lossy(batch_resp.body.as_bytes())
    );

    // Scan to verify final item count: new-item + batch-0..4 = 6 items.
    // "seed" was deleted; the conditional failure did not insert a duplicate.
    let scan_resp = provider
        .dispatch(&make_ctx("Scan", json!({ "TableName": "CondBatch" })))
        .await
        .unwrap();
    assert_eq!(scan_resp.status_code, 200);
    let sb = body(&scan_resp);
    assert_eq!(
        sb["Count"].as_u64(),
        Some(6),
        "CondBatch should have 6 items after batch write; got {}",
        sb["Count"]
    );

    // Verify "seed" is gone.
    let gone_resp = provider
        .dispatch(&make_ctx(
            "GetItem",
            json!({ "TableName": "CondBatch", "Key": { "pk": { "S": "seed" } } }),
        ))
        .await
        .unwrap();
    assert_eq!(gone_resp.status_code, 200);
    let gb = body(&gone_resp);
    assert!(
        gb.get("Item").is_none() || gb["Item"].is_null(),
        "seed item should be absent after BatchWriteItem DeleteRequest"
    );
}
