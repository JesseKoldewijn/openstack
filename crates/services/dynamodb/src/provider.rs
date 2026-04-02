use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use dashmap::DashMap;
use openstack_service_framework::traits::{
    DispatchError, DispatchResponse, RequestContext, ResponseBody, ServiceProvider,
};
use openstack_state::AccountRegionBundle;
use serde::Serialize;
use serde_json::{Value, json};
use tokio::sync::Mutex;
use tracing::warn;

use crate::store::{
    AttributeDefinition, AttributeValue, DynamoDbStore, GlobalSecondaryIndex, Item,
    KeySchemaElement, KeyType, LocalSecondaryIndex, Projection, RangeCondition,
    StreamSpecification, Table, apply_update_expression, av_from_json, check_condition,
    evaluate_filter, item_from_json_map,
};

pub struct DynamoDbProvider {
    store: Arc<AccountRegionBundle<DynamoDbStore>>,
    /// Per-table Mutex pool keyed by `"account_id/region/table_name"`.
    ///
    /// Every mutating operation (PutItem, UpdateItem, DeleteItem,
    /// BatchWriteItem, TransactWriteItems) acquires the lock for each table
    /// it touches before performing any DashMap read or write. This ensures
    /// TransactWriteItems' validate→apply two-pass is fully isolated from
    /// concurrent single-item writers on the same table. Operations on
    /// disjoint tables remain fully concurrent.
    table_locks: Arc<DashMap<String, Arc<Mutex<()>>>>,
}

impl DynamoDbProvider {
    pub fn new() -> Self {
        Self {
            store: Arc::new(AccountRegionBundle::new()),
            table_locks: Arc::new(DashMap::new()),
        }
    }

    /// Clone the inner Arc<DashMap<String, Table>> for an account+region,
    /// releasing the outer shard lock immediately.
    fn get_tables(&self, account_id: &str, region: &str) -> Option<Arc<DashMap<String, Table>>> {
        self.store.get(account_id, region).map(|s| s.tables_ref())
    }

    /// Get-or-create the inner Arc<DashMap<String, Table>>, releasing the
    /// outer shard lock immediately after cloning the Arc.
    fn get_or_create_tables(&self, account_id: &str, region: &str) -> Arc<DashMap<String, Table>> {
        self.store.get_or_create(account_id, region).tables_ref()
    }

    /// Return the per-table `Mutex` for the given (account, region, table)
    /// triple, creating it if it does not yet exist.
    ///
    /// The mutex is held by the caller across **all** reads and writes for the
    /// table within a single logical operation, ensuring linearisability.
    fn get_table_lock(&self, account_id: &str, region: &str, table_name: &str) -> Arc<Mutex<()>> {
        let key = format!("{account_id}/{region}/{table_name}");
        Arc::clone(
            self.table_locks
                .entry(key)
                .or_insert_with(|| Arc::new(Mutex::new(())))
                .value(),
        )
    }
}

impl Default for DynamoDbProvider {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// JSON response helpers
// ---------------------------------------------------------------------------

fn json_ok(body: Value) -> DispatchResponse {
    DispatchResponse {
        status_code: 200,
        body: ResponseBody::Buffered(Bytes::from(serde_json::to_vec(&body).unwrap())),
        content_type: Cow::Borrowed("application/x-amz-json-1.0"),
        headers: Vec::new(),
    }
}

/// Serialize `val` directly to JSON bytes, bypassing the intermediate
/// `serde_json::Value` tree that `json_ok(json!({...}))` would produce.
fn serialize_response<T: Serialize>(val: &T) -> DispatchResponse {
    DispatchResponse {
        status_code: 200,
        body: ResponseBody::Buffered(Bytes::from(serde_json::to_vec(val).unwrap())),
        content_type: Cow::Borrowed("application/x-amz-json-1.0"),
        headers: Vec::new(),
    }
}

// ---------------------------------------------------------------------------
// Thin response wrapper structs — derive Serialize to avoid double-serialization
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct GetItemResp<'a> {
    #[serde(rename = "Item", skip_serializing_if = "Option::is_none")]
    item: Option<&'a Item>,
}

#[derive(Serialize)]
struct QueryResp<'a> {
    #[serde(rename = "Items")]
    items: &'a [Item],
    #[serde(rename = "Count")]
    count: usize,
    #[serde(rename = "ScannedCount")]
    scanned_count: usize,
}

#[derive(Serialize)]
struct MutationResp<'a> {
    #[serde(rename = "Attributes", skip_serializing_if = "Option::is_none")]
    attributes: Option<&'a Item>,
}

fn json_error(code: &str, message: &str, status: u16) -> DispatchResponse {
    DispatchResponse {
        status_code: status,
        body: ResponseBody::Buffered(Bytes::from(
            serde_json::to_vec(&json!({
                "__type": code,
                "message": message,
            }))
            .unwrap(),
        )),
        content_type: Cow::Borrowed("application/json"),
        headers: Vec::new(),
    }
}

// ---------------------------------------------------------------------------
// Parsing helpers
// ---------------------------------------------------------------------------

fn parse_key_schema(arr: &Value) -> Vec<KeySchemaElement> {
    arr.as_array()
        .map(|v| {
            v.iter()
                .filter_map(|ks| {
                    let attr = ks.get("AttributeName")?.as_str()?.to_string();
                    let kt = match ks.get("KeyType")?.as_str()? {
                        "HASH" => KeyType::HASH,
                        _ => KeyType::RANGE,
                    };
                    Some(KeySchemaElement {
                        attribute_name: attr,
                        key_type: kt,
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn parse_attr_defs(arr: &Value) -> Vec<AttributeDefinition> {
    arr.as_array()
        .map(|v| {
            v.iter()
                .filter_map(|a| {
                    Some(AttributeDefinition {
                        attribute_name: a.get("AttributeName")?.as_str()?.to_string(),
                        attribute_type: a.get("AttributeType")?.as_str()?.to_string(),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn parse_projection(p: Option<&Value>) -> Projection {
    match p {
        None => Projection {
            projection_type: "ALL".to_string(),
            non_key_attributes: vec![],
        },
        Some(v) => Projection {
            projection_type: v
                .get("ProjectionType")
                .and_then(|s| s.as_str())
                .unwrap_or("ALL")
                .to_string(),
            non_key_attributes: v
                .get("NonKeyAttributes")
                .and_then(|a| a.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|s| s.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default(),
        },
    }
}

fn parse_gsis(arr: &Value) -> Vec<GlobalSecondaryIndex> {
    arr.as_array()
        .map(|v| {
            v.iter()
                .filter_map(|g| {
                    Some(GlobalSecondaryIndex {
                        index_name: g.get("IndexName")?.as_str()?.to_string(),
                        key_schema: parse_key_schema(g.get("KeySchema").unwrap_or(&Value::Null)),
                        projection: parse_projection(g.get("Projection")),
                        item_count: 0,
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn parse_lsis(arr: &Value) -> Vec<LocalSecondaryIndex> {
    arr.as_array()
        .map(|v| {
            v.iter()
                .filter_map(|l| {
                    Some(LocalSecondaryIndex {
                        index_name: l.get("IndexName")?.as_str()?.to_string(),
                        key_schema: parse_key_schema(l.get("KeySchema").unwrap_or(&Value::Null)),
                        projection: parse_projection(l.get("Projection")),
                        item_count: 0,
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn parse_stream_spec(v: Option<&Value>) -> StreamSpecification {
    match v {
        None => StreamSpecification::default(),
        Some(s) => StreamSpecification {
            stream_enabled: s
                .get("StreamEnabled")
                .and_then(|b| b.as_bool())
                .unwrap_or(false),
            stream_view_type: s
                .get("StreamViewType")
                .and_then(|t| t.as_str())
                .map(String::from),
        },
    }
}

fn parse_expr_names(v: Option<&Value>) -> HashMap<String, String> {
    v.and_then(|m| m.as_object())
        .map(|o| {
            o.iter()
                .filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_string())))
                .collect()
        })
        .unwrap_or_default()
}

fn parse_expr_values(v: Option<&Value>) -> HashMap<String, AttributeValue> {
    v.and_then(|m| m.as_object())
        .map(|o| {
            o.iter()
                .map(|(k, v)| (k.clone(), av_from_json(v)))
                .collect()
        })
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Key condition expression → RangeCondition parser
// ---------------------------------------------------------------------------

fn parse_key_condition(
    expr: &str,
    range_key_name: &str,
    attr_names: &HashMap<String, String>,
    attr_values: &HashMap<String, AttributeValue>,
) -> (Option<String>, Option<RangeCondition>) {
    // Returns (hash_key_value_str, range_condition)
    // Expression is like: "#pk = :pk AND #sk BETWEEN :lo AND :hi"
    let expr = expr.trim();
    let parts: Vec<&str> = split_top_level_and(expr);

    let mut hash_val: Option<String> = None;
    let mut range_cond: Option<RangeCondition> = None;

    for part in parts {
        let part = part.trim();
        // Check begins_with
        if part.to_lowercase().starts_with("begins_with(") && part.ends_with(')') {
            let inner = &part[12..part.len() - 1];
            let comps: Vec<&str> = inner.splitn(2, ',').collect();
            if comps.len() == 2 {
                let name = resolve_attr_name(comps[0].trim(), attr_names);
                let val = resolve_attr_value(comps[1].trim(), attr_values);
                let prefix = if let AttributeValue::S(s) = val {
                    s.clone()
                } else {
                    String::new()
                };
                if name == range_key_name {
                    range_cond = Some(RangeCondition::BeginsWith(prefix));
                }
            }
            continue;
        }
        // BETWEEN
        let upper = part.to_uppercase();
        if let Some(between_pos) = find_keyword_pos(&upper, " BETWEEN ") {
            let lhs = part[..between_pos].trim();
            let rest = &part[between_pos + 9..];
            let and_pos = find_keyword_pos(&rest.to_uppercase(), " AND ");
            if let Some(ap) = and_pos {
                let lo_str = rest[..ap].trim();
                let hi_str = rest[ap + 5..].trim();
                let name = resolve_attr_name(lhs, attr_names);
                let lo = resolve_attr_value(lo_str, attr_values).clone();
                let hi = resolve_attr_value(hi_str, attr_values).clone();
                if name == range_key_name {
                    range_cond = Some(RangeCondition::Between(lo, hi));
                } else {
                    // hash key — rare
                }
            }
            continue;
        }
        // comparison operators
        for op in &["<=", ">=", "<>", "<", ">", "="] {
            if let Some(pos) = part.find(op) {
                let lhs = resolve_attr_name(part[..pos].trim(), attr_names);
                let rhs_str = part[pos + op.len()..].trim();
                let rhs = resolve_attr_value(rhs_str, attr_values);
                let rhs_str_val = av_to_string(rhs);
                match *op {
                    "=" => {
                        if lhs == range_key_name {
                            range_cond = Some(RangeCondition::Eq(rhs.clone()));
                        } else {
                            hash_val = rhs_str_val;
                        }
                    }
                    "<" => {
                        if lhs == range_key_name {
                            range_cond = Some(RangeCondition::Lt(rhs.clone()));
                        }
                    }
                    "<=" => {
                        if lhs == range_key_name {
                            range_cond = Some(RangeCondition::Lte(rhs.clone()));
                        }
                    }
                    ">" => {
                        if lhs == range_key_name {
                            range_cond = Some(RangeCondition::Gt(rhs.clone()));
                        }
                    }
                    ">=" => {
                        if lhs == range_key_name {
                            range_cond = Some(RangeCondition::Gte(rhs.clone()));
                        }
                    }
                    _ => {}
                }
                break;
            }
        }
    }

    (hash_val, range_cond)
}

fn split_top_level_and(expr: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth = 0i32;
    let mut start = 0;
    let bytes = expr.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'(' => depth += 1,
            b')' => depth -= 1,
            _ => {}
        }
        if depth == 0 {
            let upper_slice = &expr[i..];
            if upper_slice.len() >= 5 {
                let candidate = &upper_slice[..5];
                if candidate.eq_ignore_ascii_case(" AND ") {
                    parts.push(&expr[start..i]);
                    start = i + 5;
                    i = start;
                    continue;
                }
            }
        }
        i += 1;
    }
    parts.push(&expr[start..]);
    parts
}

fn find_keyword_pos(haystack: &str, keyword: &str) -> Option<usize> {
    let mut depth = 0i32;
    let bytes = haystack.as_bytes();
    let klen = keyword.len();
    for i in 0..bytes.len() {
        match bytes[i] {
            b'(' => depth += 1,
            b')' => depth -= 1,
            _ => {}
        }
        if depth == 0 && i + klen <= bytes.len() && &haystack[i..i + klen] == keyword {
            return Some(i);
        }
    }
    None
}

fn resolve_attr_name(name: &str, attr_names: &HashMap<String, String>) -> String {
    attr_names
        .get(name)
        .cloned()
        .unwrap_or_else(|| name.to_string())
}

fn resolve_attr_value<'a>(
    val: &str,
    attr_values: &'a HashMap<String, AttributeValue>,
) -> &'a AttributeValue {
    static NULL_AV: AttributeValue = AttributeValue::Null;
    attr_values.get(val).unwrap_or(&NULL_AV)
}

fn av_to_string(v: &AttributeValue) -> Option<String> {
    match v {
        AttributeValue::S(s) => Some(s.clone()),
        AttributeValue::N(n) => Some(n.clone()),
        AttributeValue::B(b) => Some(b.clone()),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Table description serializer
// ---------------------------------------------------------------------------

fn table_description(table: &crate::store::Table) -> Value {
    let key_schema: Vec<Value> = table
        .key_schema
        .iter()
        .map(|k| {
            json!({
                "AttributeName": k.attribute_name,
                "KeyType": format!("{:?}", k.key_type),
            })
        })
        .collect();

    let attr_defs: Vec<Value> = table
        .attribute_definitions
        .iter()
        .map(|a| {
            json!({
                "AttributeName": a.attribute_name,
                "AttributeType": a.attribute_type,
            })
        })
        .collect();

    let gsis: Vec<Value> = table
        .global_secondary_indexes
        .iter()
        .map(|g| {
            let ks: Vec<Value> = g
                .key_schema
                .iter()
                .map(|k| {
                    json!({
                        "AttributeName": k.attribute_name,
                        "KeyType": format!("{:?}", k.key_type),
                    })
                })
                .collect();
            json!({
                "IndexName": g.index_name,
                "KeySchema": ks,
                "Projection": {
                    "ProjectionType": g.projection.projection_type,
                    "NonKeyAttributes": g.projection.non_key_attributes,
                },
                "IndexStatus": "ACTIVE",
                "ItemCount": g.item_count,
                "IndexSizeBytes": 0,
                "IndexArn": format!("{}/{}", table.table_arn, g.index_name),
            })
        })
        .collect();

    let lsis: Vec<Value> = table
        .local_secondary_indexes
        .iter()
        .map(|l| {
            let ks: Vec<Value> = l
                .key_schema
                .iter()
                .map(|k| {
                    json!({
                        "AttributeName": k.attribute_name,
                        "KeyType": format!("{:?}", k.key_type),
                    })
                })
                .collect();
            json!({
                "IndexName": l.index_name,
                "KeySchema": ks,
                "Projection": {
                    "ProjectionType": l.projection.projection_type,
                    "NonKeyAttributes": l.projection.non_key_attributes,
                },
                "ItemCount": l.item_count,
                "IndexSizeBytes": 0,
                "IndexArn": format!("{}/{}", table.table_arn, l.index_name),
            })
        })
        .collect();

    let mut desc = json!({
        "TableName": table.table_name,
        "TableArn": table.table_arn,
        "TableId": table.table_id,
        "TableStatus": format!("{:?}", table.status),
        "CreationDateTime": table.created.timestamp() as f64,
        "KeySchema": key_schema,
        "AttributeDefinitions": attr_defs,
        "BillingModeSummary": { "BillingMode": table.billing_mode },
        "ItemCount": table.item_count,
        "TableSizeBytes": table.table_size_bytes,
        "StreamSpecification": {
            "StreamEnabled": table.stream_specification.stream_enabled,
            "StreamViewType": table.stream_specification.stream_view_type,
        },
    });

    if !gsis.is_empty() {
        desc["GlobalSecondaryIndexes"] = json!(gsis);
    }
    if !lsis.is_empty() {
        desc["LocalSecondaryIndexes"] = json!(lsis);
    }
    if let Some(arn) = &table.stream_arn {
        desc["LatestStreamArn"] = json!(arn);
        desc["LatestStreamLabel"] = json!(arn.split('/').next_back().unwrap_or(""));
    }

    desc
}

// ---------------------------------------------------------------------------
// Project item fields
// ---------------------------------------------------------------------------

fn project_item<'a>(
    item: &'a Item,
    projection: Option<&str>,
    attr_names: &HashMap<String, String>,
) -> Cow<'a, Item> {
    match projection {
        None | Some("") => Cow::Borrowed(item),
        Some(expr) => {
            // Resolve ExpressionAttributeNames placeholders (#name → actual name)
            // before splitting on commas.
            let resolved: String = {
                let mut out = String::with_capacity(expr.len());
                let mut rest = expr;
                while let Some(hash_pos) = rest.find('#') {
                    out.push_str(&rest[..hash_pos]);
                    let after = &rest[hash_pos..];
                    // Placeholder ends at the first char that is not alphanumeric or '_'
                    let end = after[1..]
                        .find(|c: char| !c.is_alphanumeric() && c != '_')
                        .map(|i| i + 1)
                        .unwrap_or(after.len());
                    let placeholder = &after[..end];
                    if let Some(real) = attr_names.get(placeholder) {
                        out.push_str(real);
                    } else {
                        out.push_str(placeholder);
                    }
                    rest = &after[end..];
                }
                out.push_str(rest);
                out
            };
            let attrs: Vec<&str> = resolved.split(',').map(|s| s.trim()).collect();
            Cow::Owned(
                item.iter()
                    .filter(|(k, _)| attrs.contains(&k.as_str()))
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect(),
            )
        }
    }
}

// ---------------------------------------------------------------------------
// Shard iterator simulation
// ---------------------------------------------------------------------------

fn make_shard_iterator(stream_arn: &str, seq: u64) -> String {
    format!("{}::shard-0000000001::{:020}", stream_arn, seq)
}

fn parse_shard_iterator(it: &str) -> Option<(String, u64)> {
    let parts: Vec<&str> = it.splitn(3, "::").collect();
    if parts.len() == 3 {
        let seq = parts[2].trim_start_matches('0').parse::<u64>().unwrap_or(0);
        Some((parts[0].to_string(), seq))
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// ServiceProvider impl
// ---------------------------------------------------------------------------

#[async_trait]
impl ServiceProvider for DynamoDbProvider {
    fn service_name(&self) -> &str {
        "dynamodb"
    }

    async fn dispatch(&self, ctx: &RequestContext) -> Result<DispatchResponse, DispatchError> {
        let op_start = std::time::Instant::now();
        let op = ctx.operation.as_str();
        let body = &ctx.request_body;

        let response = match op {
            // ---------------------------------------------------------------
            // Table operations
            // ---------------------------------------------------------------
            "CreateTable" => {
                let name = match body.get("TableName").and_then(|v| v.as_str()) {
                    Some(n) => n.to_string(),
                    None => {
                        return Ok(json_error(
                            "ValidationException",
                            "TableName is required",
                            400,
                        ));
                    }
                };
                let key_schema = parse_key_schema(body.get("KeySchema").unwrap_or(&Value::Null));
                let attr_defs =
                    parse_attr_defs(body.get("AttributeDefinitions").unwrap_or(&Value::Null));
                let gsis = parse_gsis(body.get("GlobalSecondaryIndexes").unwrap_or(&Value::Null));
                let lsis = parse_lsis(body.get("LocalSecondaryIndexes").unwrap_or(&Value::Null));
                let stream_spec = parse_stream_spec(body.get("StreamSpecification"));

                // Clone the Arc immediately so the outer shard lock is released
                // before we do any work on the inner table map.
                let desc = {
                    let store = self.store.get_or_create(&ctx.account_id, &ctx.region);
                    if store.get_table(&name).is_some() {
                        return Ok(json_error(
                            "ResourceInUseException",
                            &format!("Table already exists: {name}"),
                            400,
                        ));
                    }
                    store.create_table(
                        &name,
                        &ctx.account_id,
                        &ctx.region,
                        key_schema,
                        attr_defs,
                        gsis,
                        lsis,
                        stream_spec,
                    );
                    // Build the description while still holding the shard lock so
                    // a concurrent DeleteTable cannot remove the entry underneath us.
                    match store.get_table(&name) {
                        Some(t) => table_description(&t),
                        None => {
                            return Ok(json_error(
                                "InternalFailure",
                                "Table was created but could not be retrieved",
                                500,
                            ));
                        }
                    }
                    // outer RefMut (shard lock) released here
                };
                Ok(json_ok(json!({ "TableDescription": desc })))
            }

            "DeleteTable" => {
                let name = match body.get("TableName").and_then(|v| v.as_str()) {
                    Some(n) => n.to_string(),
                    None => {
                        return Ok(json_error(
                            "ValidationException",
                            "TableName is required",
                            400,
                        ));
                    }
                };
                let store = self.store.get_or_create(&ctx.account_id, &ctx.region);
                match store.delete_table(&name) {
                    None => Ok(json_error(
                        "ResourceNotFoundException",
                        "Cannot do operations on a non-existent table",
                        400,
                    )),
                    Some(table) => Ok(json_ok(
                        json!({ "TableDescription": table_description(&table) }),
                    )),
                }
            }

            "DescribeTable" => {
                let name = match body.get("TableName").and_then(|v| v.as_str()) {
                    Some(n) => n.to_string(),
                    None => {
                        return Ok(json_error(
                            "ValidationException",
                            "TableName is required",
                            400,
                        ));
                    }
                };
                let Some(tables) = self.get_tables(&ctx.account_id, &ctx.region) else {
                    return Ok(json_error(
                        "ResourceNotFoundException",
                        "Cannot do operations on a non-existent table",
                        400,
                    ));
                };
                match tables.get(&name) {
                    None => Ok(json_error(
                        "ResourceNotFoundException",
                        "Cannot do operations on a non-existent table",
                        400,
                    )),
                    Some(table) => Ok(json_ok(json!({ "Table": table_description(&table) }))),
                }
            }

            "ListTables" => {
                let Some(store) = self.store.get(&ctx.account_id, &ctx.region) else {
                    return Ok(json_ok(json!({ "TableNames": [] })));
                };
                let limit = body.get("Limit").and_then(|v| v.as_u64()).unwrap_or(100) as usize;
                let exclusive_start = body.get("ExclusiveStartTableName").and_then(|v| v.as_str());

                let mut names: Vec<String> = store.list_table_names();
                names.sort();

                let start_idx = if let Some(start) = exclusive_start {
                    names
                        .iter()
                        .position(|n| n == start)
                        .map(|p| p + 1)
                        .unwrap_or(0)
                } else {
                    0
                };

                let page: Vec<&str> = names[start_idx..]
                    .iter()
                    .take(limit)
                    .map(|s| s.as_str())
                    .collect();
                let last_evaluated = if start_idx + page.len() < names.len() {
                    page.last().map(|s| s.to_string())
                } else {
                    None
                };

                let mut resp = json!({ "TableNames": page });
                if let Some(last) = last_evaluated {
                    resp["LastEvaluatedTableName"] = json!(last);
                }
                Ok(json_ok(resp))
            }

            "UpdateTable" => {
                let name = match body.get("TableName").and_then(|v| v.as_str()) {
                    Some(n) => n.to_string(),
                    None => {
                        return Ok(json_error(
                            "ValidationException",
                            "TableName is required",
                            400,
                        ));
                    }
                };
                let tables = self.get_or_create_tables(&ctx.account_id, &ctx.region);
                match tables.get_mut(&name) {
                    None => Ok(json_error(
                        "ResourceNotFoundException",
                        "Cannot do operations on a non-existent table",
                        400,
                    )),
                    Some(mut table) => {
                        // Handle stream updates
                        if let Some(ss) = body.get("StreamSpecification") {
                            let new_spec = parse_stream_spec(Some(ss));
                            if new_spec.stream_enabled && !table.stream_specification.stream_enabled
                            {
                                let arn = format!(
                                    "{}/stream/{}",
                                    table.table_arn,
                                    chrono::Utc::now().timestamp()
                                );
                                table.stream_arn = Some(arn);
                            }
                            table.stream_specification = new_spec;
                        }
                        // Handle GSI updates (create/delete)
                        if let Some(gsi_updates) = body
                            .get("GlobalSecondaryIndexUpdates")
                            .and_then(|v| v.as_array())
                        {
                            for update in gsi_updates {
                                if let Some(create) = update.get("Create") {
                                    let gsi = GlobalSecondaryIndex {
                                        index_name: create
                                            .get("IndexName")
                                            .and_then(|v| v.as_str())
                                            .unwrap_or("")
                                            .to_string(),
                                        key_schema: parse_key_schema(
                                            create.get("KeySchema").unwrap_or(&Value::Null),
                                        ),
                                        projection: parse_projection(create.get("Projection")),
                                        item_count: 0,
                                    };
                                    // add_gsi back-fills from existing items and
                                    // pushes to global_secondary_indexes.
                                    table.add_gsi(gsi);
                                } else if let Some(delete) = update.get("Delete") {
                                    let idx_name = delete
                                        .get("IndexName")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("");
                                    // remove_gsi drops the materialized data and
                                    // removes from global_secondary_indexes.
                                    table.remove_gsi(idx_name);
                                }
                            }
                        }
                        let desc = table_description(&table);
                        Ok(json_ok(json!({ "TableDescription": desc })))
                    }
                }
            }

            // ---------------------------------------------------------------
            // Item operations
            // ---------------------------------------------------------------
            "PutItem" => {
                let name = match body.get("TableName").and_then(|v| v.as_str()) {
                    Some(n) => n.to_string(),
                    None => {
                        return Ok(json_error(
                            "ValidationException",
                            "TableName is required",
                            400,
                        ));
                    }
                };
                let item: Item = match body.get("Item").and_then(|v| v.as_object()) {
                    Some(m) => item_from_json_map(m),
                    None => return Ok(json_error("ValidationException", "Item is required", 400)),
                };
                let condition = body.get("ConditionExpression").and_then(|v| v.as_str());
                let attr_names = parse_expr_names(body.get("ExpressionAttributeNames"));
                let attr_values = parse_expr_values(body.get("ExpressionAttributeValues"));
                let return_values = body
                    .get("ReturnValues")
                    .and_then(|v| v.as_str())
                    .unwrap_or("NONE");

                let _table_lock = self.get_table_lock(&ctx.account_id, &ctx.region, &name);
                let _table_guard = _table_lock.lock().await;

                let tables = self.get_or_create_tables(&ctx.account_id, &ctx.region);
                let mut table = match tables.get_mut(&name) {
                    None => {
                        return Ok(json_error(
                            "ResourceNotFoundException",
                            "Cannot do operations on a non-existent table",
                            400,
                        ));
                    }
                    Some(t) => t,
                };

                // Check condition
                if let Some(cond) = condition {
                    let existing = table.get_item(&item).cloned();
                    if let Err(e) =
                        check_condition(existing.as_ref(), cond, &attr_names, &attr_values)
                    {
                        return Ok(json_error("ConditionalCheckFailedException", &e, 400));
                    }
                }

                let old = table.put_item(item);
                // Opt 4: drop the write lock before serialization.
                drop(table);
                Ok(serialize_response(&MutationResp {
                    attributes: if return_values == "ALL_OLD" {
                        old.as_ref()
                    } else {
                        None
                    },
                }))
            }

            "GetItem" => {
                let name = match body.get("TableName").and_then(|v| v.as_str()) {
                    Some(n) => n.to_string(),
                    None => {
                        return Ok(json_error(
                            "ValidationException",
                            "TableName is required",
                            400,
                        ));
                    }
                };
                let key: Item = match body.get("Key").and_then(|v| v.as_object()) {
                    Some(m) => item_from_json_map(m),
                    None => return Ok(json_error("ValidationException", "Key is required", 400)),
                };
                let projection = body.get("ProjectionExpression").and_then(|v| v.as_str());
                let attr_names = parse_expr_names(body.get("ExpressionAttributeNames"));

                let Some(tables) = self.get_tables(&ctx.account_id, &ctx.region) else {
                    return Ok(json_error(
                        "ResourceNotFoundException",
                        "Cannot do operations on a non-existent table",
                        400,
                    ));
                };
                let table = match tables.get(&name) {
                    None => {
                        return Ok(json_error(
                            "ResourceNotFoundException",
                            "Cannot do operations on a non-existent table",
                            400,
                        ));
                    }
                    Some(t) => t,
                };

                // Opt 2: project_item returns Cow::Borrowed for no-projection (no clone).
                // Opt 1: serialize_response writes bytes directly — lock is held only
                //        during this fast byte-serialization pass, then dropped.
                let resp = match table.get_item(&key) {
                    None => serialize_response(&GetItemResp { item: None }),
                    Some(item) => {
                        let out = project_item(item, projection, &attr_names);
                        serialize_response(&GetItemResp {
                            item: Some(out.as_ref()),
                        })
                    }
                };
                // Opt 4: explicitly drop the shard lock before returning.
                drop(table);
                Ok(resp)
            }

            "DeleteItem" => {
                let name = match body.get("TableName").and_then(|v| v.as_str()) {
                    Some(n) => n.to_string(),
                    None => {
                        return Ok(json_error(
                            "ValidationException",
                            "TableName is required",
                            400,
                        ));
                    }
                };
                let key: Item = match body.get("Key").and_then(|v| v.as_object()) {
                    Some(m) => item_from_json_map(m),
                    None => return Ok(json_error("ValidationException", "Key is required", 400)),
                };
                let condition = body.get("ConditionExpression").and_then(|v| v.as_str());
                let attr_names = parse_expr_names(body.get("ExpressionAttributeNames"));
                let attr_values = parse_expr_values(body.get("ExpressionAttributeValues"));
                let return_values = body
                    .get("ReturnValues")
                    .and_then(|v| v.as_str())
                    .unwrap_or("NONE");

                let _table_lock = self.get_table_lock(&ctx.account_id, &ctx.region, &name);
                let _table_guard = _table_lock.lock().await;

                let tables = self.get_or_create_tables(&ctx.account_id, &ctx.region);
                let mut table = match tables.get_mut(&name) {
                    None => {
                        return Ok(json_error(
                            "ResourceNotFoundException",
                            "Cannot do operations on a non-existent table",
                            400,
                        ));
                    }
                    Some(t) => t,
                };

                if let Some(cond) = condition {
                    let existing = table.get_item(&key).cloned();
                    if let Err(e) =
                        check_condition(existing.as_ref(), cond, &attr_names, &attr_values)
                    {
                        return Ok(json_error("ConditionalCheckFailedException", &e, 400));
                    }
                }

                let old = table.delete_item(&key);
                let mut resp = json!({});
                if return_values == "ALL_OLD"
                    && let Some(old_item) = old
                {
                    resp["Attributes"] = json!(old_item);
                }
                Ok(json_ok(resp))
            }

            "UpdateItem" => {
                let name = match body.get("TableName").and_then(|v| v.as_str()) {
                    Some(n) => n.to_string(),
                    None => {
                        return Ok(json_error(
                            "ValidationException",
                            "TableName is required",
                            400,
                        ));
                    }
                };
                let key: Item = match body.get("Key").and_then(|v| v.as_object()) {
                    Some(m) => item_from_json_map(m),
                    None => return Ok(json_error("ValidationException", "Key is required", 400)),
                };
                let update_expr = body.get("UpdateExpression").and_then(|v| v.as_str());
                let condition = body.get("ConditionExpression").and_then(|v| v.as_str());
                let attr_names = parse_expr_names(body.get("ExpressionAttributeNames"));
                let attr_values = parse_expr_values(body.get("ExpressionAttributeValues"));
                let return_values = body
                    .get("ReturnValues")
                    .and_then(|v| v.as_str())
                    .unwrap_or("NONE");

                let _table_lock = self.get_table_lock(&ctx.account_id, &ctx.region, &name);
                let _table_guard = _table_lock.lock().await;

                let tables = self.get_or_create_tables(&ctx.account_id, &ctx.region);
                let mut table = match tables.get_mut(&name) {
                    None => {
                        return Ok(json_error(
                            "ResourceNotFoundException",
                            "Cannot do operations on a non-existent table",
                            400,
                        ));
                    }
                    Some(t) => t,
                };

                // Check condition on existing item
                if let Some(cond) = condition {
                    let existing = table.get_item(&key).cloned();
                    if let Err(e) =
                        check_condition(existing.as_ref(), cond, &attr_names, &attr_values)
                    {
                        return Ok(json_error("ConditionalCheckFailedException", &e, 400));
                    }
                }

                // Get or create item
                let (hk_opt, sk_opt) = {
                    let hk = table.extract_key_from_item(&key);
                    hk.map(|(h, s)| (Some(h), Some(s))).unwrap_or((None, None))
                };
                let hk = match hk_opt {
                    Some(h) => h,
                    None => return Ok(json_error("ValidationException", "Missing key", 400)),
                };
                let sk = sk_opt.unwrap_or_default();

                let old_item = table.items.get(&hk).and_then(|p| p.get(&sk)).cloned();

                let mut item = old_item.clone().unwrap_or_else(|| key.clone());

                if let Some(expr) = update_expr {
                    apply_update_expression(&mut item, expr, &attr_names, &attr_values);
                }

                let new_item = item.clone();
                table.put_item(new_item.clone());

                let mut resp = json!({});
                match return_values {
                    "ALL_NEW" => resp["Attributes"] = json!(new_item),
                    "ALL_OLD" => {
                        if let Some(old) = old_item {
                            resp["Attributes"] = json!(old);
                        }
                    }
                    "UPDATED_NEW" => resp["Attributes"] = json!(new_item),
                    "UPDATED_OLD" => {
                        if let Some(old) = old_item {
                            resp["Attributes"] = json!(old);
                        }
                    }
                    _ => {}
                }
                Ok(json_ok(resp))
            }

            // ---------------------------------------------------------------
            // Query
            // ---------------------------------------------------------------
            "Query" => {
                let name = match body.get("TableName").and_then(|v| v.as_str()) {
                    Some(n) => n.to_string(),
                    None => {
                        return Ok(json_error(
                            "ValidationException",
                            "TableName is required",
                            400,
                        ));
                    }
                };
                let key_cond_expr = body
                    .get("KeyConditionExpression")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let filter_expr = body.get("FilterExpression").and_then(|v| v.as_str());
                let projection_expr = body.get("ProjectionExpression").and_then(|v| v.as_str());
                let index_name = body.get("IndexName").and_then(|v| v.as_str());
                let scan_forward = body
                    .get("ScanIndexForward")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(true);
                let limit = body
                    .get("Limit")
                    .and_then(|v| v.as_u64())
                    .map(|v| v as usize);
                let attr_names = parse_expr_names(body.get("ExpressionAttributeNames"));
                let attr_values = parse_expr_values(body.get("ExpressionAttributeValues"));
                let select = body
                    .get("Select")
                    .and_then(|v| v.as_str())
                    .unwrap_or("ALL_ATTRIBUTES");

                let Some(tables) = self.get_tables(&ctx.account_id, &ctx.region) else {
                    return Ok(json_error(
                        "ResourceNotFoundException",
                        "Cannot do operations on a non-existent table",
                        400,
                    ));
                };
                let table = match tables.get(&name) {
                    None => {
                        return Ok(json_error(
                            "ResourceNotFoundException",
                            "Cannot do operations on a non-existent table",
                            400,
                        ));
                    }
                    Some(t) => t,
                };

                // Determine range key for index — reject unknown IndexName up-front.
                let range_key_name = if let Some(idx) = index_name {
                    let gsi = table
                        .global_secondary_indexes
                        .iter()
                        .find(|g| g.index_name == idx);
                    let lsi = table
                        .local_secondary_indexes
                        .iter()
                        .find(|l| l.index_name == idx);
                    if gsi.is_none() && lsi.is_none() {
                        return Ok(json_error(
                            "ValidationException",
                            &format!("The table does not have the specified index: {idx}"),
                            400,
                        ));
                    }
                    gsi.and_then(|g| {
                        g.key_schema
                            .iter()
                            .find(|k| k.key_type == KeyType::RANGE)
                            .map(|k| k.attribute_name.clone())
                    })
                    .or_else(|| {
                        lsi.and_then(|l| {
                            l.key_schema
                                .iter()
                                .find(|k| k.key_type == KeyType::RANGE)
                                .map(|k| k.attribute_name.clone())
                        })
                    })
                    .unwrap_or_default()
                } else {
                    table.range_key_name().unwrap_or("").to_string()
                };

                let (hash_val, range_cond) =
                    parse_key_condition(key_cond_expr, &range_key_name, &attr_names, &attr_values);

                let hash_val = match hash_val {
                    Some(h) => h,
                    None => {
                        return Ok(json_error(
                            "ValidationException",
                            "Query condition missed key schema element: partition key",
                            400,
                        ));
                    }
                };

                // Apply Limit to items *examined* (DynamoDB semantics), then filter.
                // scanned_count = items read before FilterExpression; count = items returned.
                let raw_matches: Vec<&Item> = {
                    let all = table.query(&hash_val, range_cond.as_ref(), index_name, scan_forward);
                    if let Some(lim) = limit {
                        all.into_iter().take(lim).collect()
                    } else {
                        all.into_iter().collect()
                    }
                };
                let scanned_count = raw_matches.len();

                // Collect owned items under the lock; Cow::into_owned() moves
                // already-projected items (Cow::Owned) without extra allocation.
                let items: Vec<Item> = raw_matches
                    .into_iter()
                    .filter(|item| {
                        if let Some(fe) = filter_expr {
                            evaluate_filter(item, fe, &attr_names, &attr_values)
                        } else {
                            true
                        }
                    })
                    .map(|item| project_item(item, projection_expr, &attr_names).into_owned())
                    .collect();

                let count = items.len();

                // Opt 4: release the shard lock before serialization.
                drop(table);

                // Opt 1: serialize directly — no intermediate serde_json::Value tree.
                let (out_items, out_count) = if select == "COUNT" {
                    (Vec::new(), count)
                } else {
                    (items, count)
                };
                Ok(serialize_response(&QueryResp {
                    items: &out_items,
                    count: out_count,
                    scanned_count,
                }))
            }

            // ---------------------------------------------------------------
            // Scan
            // ---------------------------------------------------------------
            "Scan" => {
                let name = match body.get("TableName").and_then(|v| v.as_str()) {
                    Some(n) => n.to_string(),
                    None => {
                        return Ok(json_error(
                            "ValidationException",
                            "TableName is required",
                            400,
                        ));
                    }
                };
                let filter_expr = body.get("FilterExpression").and_then(|v| v.as_str());
                let projection_expr = body.get("ProjectionExpression").and_then(|v| v.as_str());
                let index_name = body.get("IndexName").and_then(|v| v.as_str());
                let limit = body
                    .get("Limit")
                    .and_then(|v| v.as_u64())
                    .map(|v| v as usize);
                let attr_names = parse_expr_names(body.get("ExpressionAttributeNames"));
                let attr_values = parse_expr_values(body.get("ExpressionAttributeValues"));
                let select = body
                    .get("Select")
                    .and_then(|v| v.as_str())
                    .unwrap_or("ALL_ATTRIBUTES");

                let Some(tables) = self.get_tables(&ctx.account_id, &ctx.region) else {
                    return Ok(json_error(
                        "ResourceNotFoundException",
                        "Cannot do operations on a non-existent table",
                        400,
                    ));
                };
                let table = match tables.get(&name) {
                    None => {
                        return Ok(json_error(
                            "ResourceNotFoundException",
                            "Cannot do operations on a non-existent table",
                            400,
                        ));
                    }
                    Some(t) => t,
                };

                // When scanning a secondary index, only items that carry both
                // the index hash key AND (if present) the index range key are
                // included — mirroring DynamoDB sparse-index semantics.
                let index_keys: Option<(&str, Option<&str>)> = match index_name {
                    None => None,
                    Some(idx) => match table.index_hash_key(idx) {
                        Some(hk) => Some((hk, table.index_range_key(idx))),
                        None => {
                            return Ok(json_error(
                                "ValidationException",
                                &format!("The table does not have the specified index: {idx}"),
                                400,
                            ));
                        }
                    },
                };
                let all_items: Vec<&Item> = table.all_items();

                // scanned_count = items considered before filter (after index pruning)
                let pre_filter: Vec<&Item> = if let Some((hk, rk_opt)) = index_keys {
                    all_items
                        .into_iter()
                        .filter(|item| {
                            item.contains_key(hk) && rk_opt.is_none_or(|rk| item.contains_key(rk))
                        })
                        .collect()
                } else {
                    all_items
                };
                // Apply Limit to items *examined* before the filter (DynamoDB semantics).
                // scanned_count = items read from the index/table (capped by Limit).
                let examined: Vec<&Item> = if let Some(lim) = limit {
                    pre_filter.into_iter().take(lim).collect()
                } else {
                    pre_filter
                };
                let scanned_count = examined.len();

                // Apply FilterExpression after the limit-based examination cap.
                // Collect owned items under the lock; Cow::into_owned() is free
                // when already projected (Cow::Owned), or clones the borrow otherwise.
                let items: Vec<Item> = examined
                    .into_iter()
                    .filter(|item| {
                        if let Some(fe) = filter_expr {
                            evaluate_filter(item, fe, &attr_names, &attr_values)
                        } else {
                            true
                        }
                    })
                    .map(|item| project_item(item, projection_expr, &attr_names).into_owned())
                    .collect();

                let count = items.len();

                // Opt 4: release the shard lock before serialization.
                drop(table);

                // Opt 1: serialize directly — no intermediate serde_json::Value tree.
                let (out_items, out_count) = if select == "COUNT" {
                    (Vec::new(), count)
                } else {
                    (items, count)
                };
                Ok(serialize_response(&QueryResp {
                    items: &out_items,
                    count: out_count,
                    scanned_count,
                }))
            }

            // ---------------------------------------------------------------
            // Batch operations
            // ---------------------------------------------------------------
            "BatchGetItem" => {
                let request_items = match body.get("RequestItems").and_then(|v| v.as_object()) {
                    Some(m) => m.clone(),
                    None => {
                        return Ok(json_error(
                            "ValidationException",
                            "RequestItems is required",
                            400,
                        ));
                    }
                };

                let Some(tables) = self.get_tables(&ctx.account_id, &ctx.region) else {
                    return Ok(json_error(
                        "ResourceNotFoundException",
                        "Cannot do operations on a non-existent table",
                        400,
                    ));
                };
                let mut responses: serde_json::Map<String, Value> = serde_json::Map::new();
                let unprocessed: serde_json::Map<String, Value> = serde_json::Map::new();

                for (table_name, req) in &request_items {
                    let keys_arr = req.get("Keys").and_then(|v| v.as_array());
                    let projection_expr = req.get("ProjectionExpression").and_then(|v| v.as_str());
                    let attr_names = parse_expr_names(req.get("ExpressionAttributeNames"));

                    match tables.get(table_name) {
                        None => {
                            return Ok(json_error(
                                "ResourceNotFoundException",
                                &format!("Table {table_name} not found"),
                                400,
                            ));
                        }
                        Some(table) => {
                            let mut found = Vec::new();
                            if let Some(keys) = keys_arr {
                                for key_val in keys {
                                    let key: Item = key_val
                                        .as_object()
                                        .map(item_from_json_map)
                                        .unwrap_or_default();
                                    if let Some(item) = table.get_item(&key) {
                                        found.push(json!(project_item(
                                            item,
                                            projection_expr,
                                            &attr_names
                                        )));
                                    }
                                }
                            }
                            responses.insert(table_name.clone(), json!(found));
                        }
                    }
                }

                Ok(json_ok(json!({
                    "Responses": responses,
                    "UnprocessedKeys": unprocessed,
                })))
            }

            "BatchWriteItem" => {
                let request_items = match body.get("RequestItems").and_then(|v| v.as_object()) {
                    Some(m) => m.clone(),
                    None => {
                        return Ok(json_error(
                            "ValidationException",
                            "RequestItems is required",
                            400,
                        ));
                    }
                };

                let tables = self.get_or_create_tables(&ctx.account_id, &ctx.region);
                let unprocessed: serde_json::Map<String, Value> = serde_json::Map::new();

                for (table_name, requests) in &request_items {
                    let reqs = match requests.as_array() {
                        Some(arr) => arr,
                        None => continue,
                    };
                    let _table_lock = self.get_table_lock(&ctx.account_id, &ctx.region, table_name);
                    let _table_guard = _table_lock.lock().await;
                    let mut table = match tables.get_mut(table_name) {
                        None => {
                            return Ok(json_error(
                                "ResourceNotFoundException",
                                &format!("Table {table_name} not found"),
                                400,
                            ));
                        }
                        Some(t) => t,
                    };

                    for req in reqs {
                        if let Some(put) = req.get("PutRequest") {
                            if let Some(item_val) = put.get("Item").and_then(|v| v.as_object()) {
                                let item: Item = item_from_json_map(item_val);
                                table.put_item(item);
                            }
                        } else if let Some(del) = req.get("DeleteRequest")
                            && let Some(key_val) = del.get("Key").and_then(|v| v.as_object())
                        {
                            let key: Item = item_from_json_map(key_val);
                            table.delete_item(&key);
                        }
                    }
                }

                Ok(json_ok(json!({
                    "UnprocessedItems": unprocessed,
                })))
            }

            // ---------------------------------------------------------------
            // Transactions
            // ---------------------------------------------------------------
            "TransactGetItems" => {
                let transact_items = match body.get("TransactItems").and_then(|v| v.as_array()) {
                    Some(arr) => arr.clone(),
                    None => {
                        return Ok(json_error(
                            "ValidationException",
                            "TransactItems is required",
                            400,
                        ));
                    }
                };

                let Some(tables) = self.get_tables(&ctx.account_id, &ctx.region) else {
                    return Ok(json_ok(json!({
                        "Responses": vec![json!({}); transact_items.len()]
                    })));
                };
                let mut responses = Vec::new();

                for ti in &transact_items {
                    if let Some(get) = ti.get("Get") {
                        let table_name =
                            get.get("TableName").and_then(|v| v.as_str()).unwrap_or("");
                        let key: Item = get
                            .get("Key")
                            .and_then(|v| v.as_object())
                            .map(item_from_json_map)
                            .unwrap_or_default();
                        let projection_expr =
                            get.get("ProjectionExpression").and_then(|v| v.as_str());
                        let attr_names = parse_expr_names(get.get("ExpressionAttributeNames"));

                        match tables.get(table_name) {
                            None => responses.push(json!({})),
                            Some(table) => match table.get_item(&key) {
                                None => responses.push(json!({})),
                                Some(item) => {
                                    responses.push(json!({ "Item": project_item(item, projection_expr, &attr_names) }));
                                }
                            },
                        }
                    } else {
                        responses.push(json!({}));
                    }
                }

                Ok(json_ok(json!({ "Responses": responses })))
            }

            "TransactWriteItems" => {
                let transact_items = match body.get("TransactItems").and_then(|v| v.as_array()) {
                    Some(arr) => arr.clone(),
                    None => {
                        return Ok(json_error(
                            "ValidationException",
                            "TransactItems is required",
                            400,
                        ));
                    }
                };

                let tables = self.get_or_create_tables(&ctx.account_id, &ctx.region);

                // Acquire per-table locks in sorted order to prevent deadlocks.
                // All tables touched by this transaction are locked before the
                // validate→apply two-pass begins, preventing interleaving with
                // concurrent single-item writers or other transactions.
                let table_names: Vec<String> = {
                    let mut seen = std::collections::HashSet::new();
                    for ti in &transact_items {
                        for op_name in &["Put", "Delete", "Update", "ConditionCheck"] {
                            if let Some(op) = ti.get(op_name)
                                && let Some(tn) = op.get("TableName").and_then(|v| v.as_str())
                            {
                                seen.insert(tn.to_string());
                            }
                        }
                    }
                    let mut v: Vec<String> = seen.into_iter().collect();
                    v.sort();
                    v
                };
                // Collect Arcs first (must be declared before _table_guards so they
                // outlive the guards that borrow from them — Rust drops in reverse order).
                let table_arcs: Vec<Arc<Mutex<()>>> = table_names
                    .iter()
                    .map(|tn| self.get_table_lock(&ctx.account_id, &ctx.region, tn))
                    .collect();
                let mut _table_guards = Vec::with_capacity(table_names.len());
                for arc in &table_arcs {
                    _table_guards.push(arc.lock().await);
                }
                // Suppress unused-variable warning when table_names is empty.
                let _ = &table_names;

                // B9: fail fast if any referenced table does not exist.
                for ti in &transact_items {
                    for op_name in &["Put", "Delete", "Update", "ConditionCheck"] {
                        if let Some(op) = ti.get(op_name) {
                            let table_name =
                                op.get("TableName").and_then(|v| v.as_str()).unwrap_or("");
                            if tables.get(table_name).is_none() {
                                return Ok(json_error(
                                    "TransactionCanceledException",
                                    &format!(
                                        "Transaction cancelled: table {table_name} does not exist [ResourceNotFoundException]"
                                    ),
                                    400,
                                ));
                            }
                        }
                    }
                }

                // First pass: validate all conditions
                for ti in &transact_items {
                    for op_name in &["Put", "Delete", "Update", "ConditionCheck"] {
                        if let Some(op) = ti.get(op_name) {
                            let table_name =
                                op.get("TableName").and_then(|v| v.as_str()).unwrap_or("");
                            let condition = op.get("ConditionExpression").and_then(|v| v.as_str());
                            if let Some(cond) = condition {
                                let attr_names =
                                    parse_expr_names(op.get("ExpressionAttributeNames"));
                                let attr_values =
                                    parse_expr_values(op.get("ExpressionAttributeValues"));
                                let key: Item = op
                                    .get("Key")
                                    .or_else(|| op.get("Item"))
                                    .and_then(|v| v.as_object())
                                    .map(item_from_json_map)
                                    .unwrap_or_default();

                                // Table existence already validated above; unwrap is safe.
                                let table = tables.get(table_name).unwrap();
                                let existing = table.get_item(&key).cloned();
                                if check_condition(
                                    existing.as_ref(),
                                    cond,
                                    &attr_names,
                                    &attr_values,
                                )
                                .is_err()
                                {
                                    return Ok(json_error(
                                        "TransactionCanceledException",
                                        "Transaction cancelled, please refer cancellation reasons for specific reasons [ConditionalCheckFailed]",
                                        400,
                                    ));
                                }
                            }
                        }
                    }
                }

                // Second pass: apply writes
                for ti in &transact_items {
                    if let Some(put) = ti.get("Put") {
                        let table_name =
                            put.get("TableName").and_then(|v| v.as_str()).unwrap_or("");
                        let item: Item = put
                            .get("Item")
                            .and_then(|v| v.as_object())
                            .map(item_from_json_map)
                            .unwrap_or_default();
                        if let Some(mut table) = tables.get_mut(table_name) {
                            table.put_item(item);
                        }
                    } else if let Some(del) = ti.get("Delete") {
                        let table_name =
                            del.get("TableName").and_then(|v| v.as_str()).unwrap_or("");
                        let key: Item = del
                            .get("Key")
                            .and_then(|v| v.as_object())
                            .map(item_from_json_map)
                            .unwrap_or_default();
                        if let Some(mut table) = tables.get_mut(table_name) {
                            table.delete_item(&key);
                        }
                    } else if let Some(upd) = ti.get("Update") {
                        let table_name =
                            upd.get("TableName").and_then(|v| v.as_str()).unwrap_or("");
                        let key: Item = upd
                            .get("Key")
                            .and_then(|v| v.as_object())
                            .map(item_from_json_map)
                            .unwrap_or_default();
                        let update_expr = upd.get("UpdateExpression").and_then(|v| v.as_str());
                        let attr_names = parse_expr_names(upd.get("ExpressionAttributeNames"));
                        let attr_values = parse_expr_values(upd.get("ExpressionAttributeValues"));
                        if let Some(mut table) = tables.get_mut(table_name)
                            && let Some((hk, sk)) = table.extract_key_from_item(&key)
                        {
                            let mut item = table
                                .items
                                .get(&hk)
                                .and_then(|p| p.get(&sk))
                                .cloned()
                                .unwrap_or_else(|| key.clone());
                            if let Some(expr) = update_expr {
                                apply_update_expression(&mut item, expr, &attr_names, &attr_values);
                            }
                            table.put_item(item);
                        }
                    }
                    // ConditionCheck: already validated in first pass, no writes
                }

                Ok(json_ok(json!({})))
            }

            // ---------------------------------------------------------------
            // Stream operations
            // ---------------------------------------------------------------
            "DescribeStream" => {
                let stream_arn = match body.get("StreamArn").and_then(|v| v.as_str()) {
                    Some(s) => s.to_string(),
                    None => {
                        return Ok(json_error(
                            "ValidationException",
                            "StreamArn is required",
                            400,
                        ));
                    }
                };

                let Some(tables) = self.get_tables(&ctx.account_id, &ctx.region) else {
                    return Ok(json_error(
                        "ResourceNotFoundException",
                        "Stream not found",
                        400,
                    ));
                };
                // Find the table with this stream ARN
                let found = tables
                    .iter()
                    .find(|t| t.stream_arn.as_deref() == Some(&stream_arn));

                match found {
                    None => Ok(json_error(
                        "ResourceNotFoundException",
                        "Stream not found",
                        400,
                    )),
                    Some(t) => Ok(json_ok(json!({
                        "StreamDescription": {
                            "StreamArn": stream_arn,
                            "StreamLabel": stream_arn.split('/').next_back().unwrap_or(""),
                            "StreamStatus": "ENABLED",
                            "StreamViewType": t.stream_specification.stream_view_type,
                            "TableName": t.table_name,
                            "Shards": [{
                                "ShardId": "shardId-00000000001",
                                "SequenceNumberRange": {
                                    "StartingSequenceNumber": "00000000000000000001",
                                }
                            }],
                        }
                    }))),
                }
            }

            "ListStreams" => {
                let table_name_filter = body.get("TableName").and_then(|v| v.as_str());
                let Some(tables) = self.get_tables(&ctx.account_id, &ctx.region) else {
                    return Ok(json_ok(json!({ "Streams": [] })));
                };
                let streams: Vec<Value> = tables
                    .iter()
                    .filter(|t| {
                        t.stream_specification.stream_enabled
                            && table_name_filter.map(|n| t.table_name == n).unwrap_or(true)
                    })
                    .filter_map(|t| {
                        t.stream_arn.as_ref().map(|arn| {
                            json!({
                                "StreamArn": arn,
                                "TableName": t.table_name,
                                "StreamLabel": arn.split('/').next_back().unwrap_or(""),
                            })
                        })
                    })
                    .collect();

                Ok(json_ok(json!({ "Streams": streams })))
            }

            "GetShardIterator" => {
                let stream_arn = match body.get("StreamArn").and_then(|v| v.as_str()) {
                    Some(s) => s.to_string(),
                    None => {
                        return Ok(json_error(
                            "ValidationException",
                            "StreamArn is required",
                            400,
                        ));
                    }
                };
                let shard_iterator_type = body
                    .get("ShardIteratorType")
                    .and_then(|v| v.as_str())
                    .unwrap_or("TRIM_HORIZON");
                let sequence_number = body.get("SequenceNumber").and_then(|v| v.as_str());

                let Some(tables) = self.get_tables(&ctx.account_id, &ctx.region) else {
                    return Ok(json_error(
                        "ResourceNotFoundException",
                        "Stream not found",
                        400,
                    ));
                };
                let found = tables
                    .iter()
                    .find(|t| t.stream_arn.as_deref() == Some(&stream_arn));

                match found {
                    None => Ok(json_error(
                        "ResourceNotFoundException",
                        "Stream not found",
                        400,
                    )),
                    Some(t) => {
                        let seq = match shard_iterator_type {
                            "TRIM_HORIZON" => 0u64,
                            "LATEST" => t.stream_sequence,
                            "AT_SEQUENCE_NUMBER" => sequence_number
                                .and_then(|s| s.trim_start_matches('0').parse::<u64>().ok())
                                .unwrap_or(0),
                            "AFTER_SEQUENCE_NUMBER" => sequence_number
                                .and_then(|s| s.trim_start_matches('0').parse::<u64>().ok())
                                .map(|n| n + 1)
                                .unwrap_or(0),
                            _ => 0,
                        };
                        let iterator = make_shard_iterator(&stream_arn, seq);
                        Ok(json_ok(json!({ "ShardIterator": iterator })))
                    }
                }
            }

            "GetRecords" => {
                let shard_iterator = match body.get("ShardIterator").and_then(|v| v.as_str()) {
                    Some(s) => s.to_string(),
                    None => {
                        return Ok(json_error(
                            "ValidationException",
                            "ShardIterator is required",
                            400,
                        ));
                    }
                };
                let limit = body.get("Limit").and_then(|v| v.as_u64()).unwrap_or(1000) as usize;

                let (stream_arn, start_seq) = match parse_shard_iterator(&shard_iterator) {
                    Some(p) => p,
                    None => {
                        return Ok(json_error(
                            "ValidationException",
                            "Invalid ShardIterator",
                            400,
                        ));
                    }
                };

                let Some(tables) = self.get_tables(&ctx.account_id, &ctx.region) else {
                    return Ok(json_error(
                        "ResourceNotFoundException",
                        "Stream not found",
                        400,
                    ));
                };
                let found = tables
                    .iter()
                    .find(|t| t.stream_arn.as_deref() == Some(&stream_arn));

                match found {
                    None => Ok(json_error(
                        "ResourceNotFoundException",
                        "Stream not found",
                        400,
                    )),
                    Some(t) => {
                        let records: Vec<Value> = t
                            .stream_records
                            .iter()
                            .filter(|r| {
                                r.sequence_number
                                    .trim_start_matches('0')
                                    .parse::<u64>()
                                    .unwrap_or(0)
                                    >= start_seq
                            })
                            .take(limit)
                            .map(|r| {
                                let mut rec = json!({
                                    "eventID": uuid::Uuid::new_v4().to_string(),
                                    "eventName": r.event_name,
                                    "eventVersion": "1.1",
                                    "eventSource": "aws:dynamodb",
                                    "awsRegion": ctx.region,
                                    "dynamodb": {
                                        "Keys": r.keys,
                                        "SequenceNumber": r.sequence_number,
                                        "SizeBytes": 100,
                                        "StreamViewType": t.stream_specification.stream_view_type,
                                        "ApproximateCreationDateTime": r.approximate_creation_date_time,
                                    }
                                });
                                if let Some(ni) = &r.new_image {
                                    rec["dynamodb"]["NewImage"] = json!(ni);
                                }
                                if let Some(oi) = &r.old_image {
                                    rec["dynamodb"]["OldImage"] = json!(oi);
                                }
                                rec
                            })
                            .collect();

                        let next_seq = records
                            .last()
                            .and_then(|r| r["dynamodb"]["SequenceNumber"].as_str())
                            .and_then(|s| s.trim_start_matches('0').parse::<u64>().ok())
                            .map(|n| n + 1)
                            .unwrap_or(start_seq);

                        let next_iterator = make_shard_iterator(&stream_arn, next_seq);
                        Ok(json_ok(json!({
                            "Records": records,
                            "NextShardIterator": next_iterator,
                        })))
                    }
                }
            }

            _ => {
                warn!(
                    service = "dynamodb",
                    operation = %ctx.operation,
                    "Operation not yet implemented"
                );
                Ok(json_error(
                    "NotImplementedException",
                    &format!("Operation not implemented: {}", ctx.operation),
                    501,
                ))
            }
        };

        if response.is_ok() {
            tracing::debug!(
                service = "dynamodb",
                operation = %op,
                op_latency_us = op_start.elapsed().as_micros(),
                "DynamoDB operation complete"
            );
        }

        response
    }

    async fn storage_snapshot(&self) -> Option<serde_json::Value> {
        use serde_json::json;
        let mut tables = Vec::new();
        for entry in self.store.iter() {
            let store = entry.value();
            for table in store.tables.iter() {
                tables.push(json!({
                    "id": table.table_arn.clone(),
                    "kind": "table",
                    "created_at": table.created.to_rfc3339(),
                    "attributes": [
                        {"key": "name", "value": table.table_name.clone()},
                        {"key": "status", "value": format!("{:?}", table.status)},
                        {"key": "item_count", "value": table.item_count.to_string()},
                        {"key": "billing_mode", "value": table.billing_mode.clone()},
                    ]
                }));
            }
        }
        Some(json!({ "kind": "dynamodb", "tables": tables }))
    }
}
