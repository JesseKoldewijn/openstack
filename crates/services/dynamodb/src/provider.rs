use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use openstack_service_framework::traits::{
    DispatchError, DispatchResponse, RequestContext, ServiceProvider,
};
use openstack_state::AccountRegionBundle;
use serde_json::{Value, json};
use tracing::warn;

use crate::store::{
    AttributeDefinition, DynamoDbStore, GlobalSecondaryIndex, Item, KeySchemaElement, KeyType,
    LocalSecondaryIndex, Projection, RangeCondition, StreamSpecification, apply_update_expression,
    check_condition, evaluate_filter,
};

pub struct DynamoDbProvider {
    store: Arc<AccountRegionBundle<DynamoDbStore>>,
}

impl DynamoDbProvider {
    pub fn new() -> Self {
        Self {
            store: Arc::new(AccountRegionBundle::new()),
        }
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
        body: Bytes::from(serde_json::to_vec(&body).unwrap()),
        content_type: "application/x-amz-json-1.0".to_string(),
        headers: Vec::new(),
    }
}

fn json_error(code: &str, message: &str, status: u16) -> DispatchResponse {
    DispatchResponse {
        status_code: status,
        body: Bytes::from(
            serde_json::to_vec(&json!({
                "__type": code,
                "message": message,
            }))
            .unwrap(),
        ),
        content_type: "application/x-amz-json-1.0".to_string(),
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

fn parse_expr_values(v: Option<&Value>) -> HashMap<String, Value> {
    v.and_then(|m| m.as_object())
        .map(|o| o.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Key condition expression → RangeCondition parser
// ---------------------------------------------------------------------------

fn parse_key_condition(
    expr: &str,
    range_key_name: &str,
    attr_names: &HashMap<String, String>,
    attr_values: &HashMap<String, Value>,
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
                let prefix = val
                    .get("S")
                    .and_then(|s| s.as_str())
                    .unwrap_or("")
                    .to_string();
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
                let lo = resolve_attr_value(lo_str, attr_values);
                let hi = resolve_attr_value(hi_str, attr_values);
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
                let rhs_str_val = av_to_string(&rhs);
                match *op {
                    "=" => {
                        if lhs == range_key_name {
                            range_cond = Some(RangeCondition::Eq(rhs));
                        } else {
                            hash_val = rhs_str_val;
                        }
                    }
                    "<" => {
                        if lhs == range_key_name {
                            range_cond = Some(RangeCondition::Lt(rhs));
                        }
                    }
                    "<=" => {
                        if lhs == range_key_name {
                            range_cond = Some(RangeCondition::Lte(rhs));
                        }
                    }
                    ">" => {
                        if lhs == range_key_name {
                            range_cond = Some(RangeCondition::Gt(rhs));
                        }
                    }
                    ">=" => {
                        if lhs == range_key_name {
                            range_cond = Some(RangeCondition::Gte(rhs));
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

fn resolve_attr_value(val: &str, attr_values: &HashMap<String, Value>) -> Value {
    attr_values.get(val).cloned().unwrap_or(Value::Null)
}

fn av_to_string(v: &Value) -> Option<String> {
    if let Some(s) = v.get("S").and_then(|s| s.as_str()) {
        return Some(s.to_string());
    }
    if let Some(n) = v.get("N").and_then(|s| s.as_str()) {
        return Some(n.to_string());
    }
    None
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

fn project_item(
    item: &Item,
    projection: Option<&str>,
    _attr_names: &HashMap<String, String>,
) -> Item {
    match projection {
        None | Some("") => item.clone(),
        Some(expr) => {
            let attrs: Vec<&str> = expr.split(',').map(|s| s.trim()).collect();
            item.iter()
                .filter(|(k, _)| attrs.contains(&k.as_str()))
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect()
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

                let mut store = self.store.get_or_create(&ctx.account_id, &ctx.region);
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
                let desc = table_description(store.get_table(&name).unwrap());
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
                let mut store = self.store.get_or_create(&ctx.account_id, &ctx.region);
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
                let store = self.store.get_or_create(&ctx.account_id, &ctx.region);
                match store.get_table(&name) {
                    None => Ok(json_error(
                        "ResourceNotFoundException",
                        "Cannot do operations on a non-existent table",
                        400,
                    )),
                    Some(table) => Ok(json_ok(json!({ "Table": table_description(table) }))),
                }
            }

            "ListTables" => {
                let store = self.store.get_or_create(&ctx.account_id, &ctx.region);
                let limit = body.get("Limit").and_then(|v| v.as_u64()).unwrap_or(100) as usize;
                let exclusive_start = body.get("ExclusiveStartTableName").and_then(|v| v.as_str());

                let mut names: Vec<String> = store
                    .list_table_names()
                    .iter()
                    .map(|s| s.to_string())
                    .collect();
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
                let mut store = self.store.get_or_create(&ctx.account_id, &ctx.region);
                match store.get_table_mut(&name) {
                    None => Ok(json_error(
                        "ResourceNotFoundException",
                        "Cannot do operations on a non-existent table",
                        400,
                    )),
                    Some(table) => {
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
                                    table.global_secondary_indexes.push(gsi);
                                } else if let Some(delete) = update.get("Delete") {
                                    let idx_name = delete
                                        .get("IndexName")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("");
                                    table
                                        .global_secondary_indexes
                                        .retain(|g| g.index_name != idx_name);
                                }
                            }
                        }
                        let desc = table_description(table);
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
                    Some(m) => m.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
                    None => return Ok(json_error("ValidationException", "Item is required", 400)),
                };
                let condition = body.get("ConditionExpression").and_then(|v| v.as_str());
                let attr_names = parse_expr_names(body.get("ExpressionAttributeNames"));
                let attr_values = parse_expr_values(body.get("ExpressionAttributeValues"));
                let return_values = body
                    .get("ReturnValues")
                    .and_then(|v| v.as_str())
                    .unwrap_or("NONE");

                let mut store = self.store.get_or_create(&ctx.account_id, &ctx.region);
                let table = match store.get_table_mut(&name) {
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
                let mut resp = json!({});
                if return_values == "ALL_OLD"
                    && let Some(old_item) = old
                {
                    resp["Attributes"] = json!(old_item);
                }
                Ok(json_ok(resp))
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
                    Some(m) => m.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
                    None => return Ok(json_error("ValidationException", "Key is required", 400)),
                };
                let projection = body.get("ProjectionExpression").and_then(|v| v.as_str());
                let attr_names = parse_expr_names(body.get("ExpressionAttributeNames"));

                let store = self.store.get_or_create(&ctx.account_id, &ctx.region);
                let table = match store.get_table(&name) {
                    None => {
                        return Ok(json_error(
                            "ResourceNotFoundException",
                            "Cannot do operations on a non-existent table",
                            400,
                        ));
                    }
                    Some(t) => t,
                };

                match table.get_item(&key) {
                    None => Ok(json_ok(json!({}))),
                    Some(item) => {
                        let out = project_item(item, projection, &attr_names);
                        Ok(json_ok(json!({ "Item": out })))
                    }
                }
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
                    Some(m) => m.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
                    None => return Ok(json_error("ValidationException", "Key is required", 400)),
                };
                let condition = body.get("ConditionExpression").and_then(|v| v.as_str());
                let attr_names = parse_expr_names(body.get("ExpressionAttributeNames"));
                let attr_values = parse_expr_values(body.get("ExpressionAttributeValues"));
                let return_values = body
                    .get("ReturnValues")
                    .and_then(|v| v.as_str())
                    .unwrap_or("NONE");

                let mut store = self.store.get_or_create(&ctx.account_id, &ctx.region);
                let table = match store.get_table_mut(&name) {
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
                    Some(m) => m.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
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

                let mut store = self.store.get_or_create(&ctx.account_id, &ctx.region);
                let table = match store.get_table_mut(&name) {
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

                let store = self.store.get_or_create(&ctx.account_id, &ctx.region);
                let table = match store.get_table(&name) {
                    None => {
                        return Ok(json_error(
                            "ResourceNotFoundException",
                            "Cannot do operations on a non-existent table",
                            400,
                        ));
                    }
                    Some(t) => t,
                };

                // Determine range key for index
                let range_key_name = if let Some(idx) = index_name {
                    table
                        .global_secondary_indexes
                        .iter()
                        .find(|g| g.index_name == idx)
                        .and_then(|g| {
                            g.key_schema
                                .iter()
                                .find(|k| k.key_type == KeyType::RANGE)
                                .map(|k| k.attribute_name.clone())
                        })
                        .or_else(|| {
                            table
                                .local_secondary_indexes
                                .iter()
                                .find(|l| l.index_name == idx)
                                .and_then(|l| {
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

                let mut items: Vec<Item> = table
                    .query(&hash_val, range_cond.as_ref(), index_name, scan_forward)
                    .into_iter()
                    .filter(|item| {
                        if let Some(fe) = filter_expr {
                            evaluate_filter(item, fe, &attr_names, &attr_values)
                        } else {
                            true
                        }
                    })
                    .map(|item| project_item(item, projection_expr, &attr_names))
                    .collect();

                let total_count = items.len();
                if let Some(lim) = limit {
                    items.truncate(lim);
                }

                let count = items.len();
                let mut resp = json!({
                    "Items": items,
                    "Count": count,
                    "ScannedCount": total_count,
                });
                if select == "COUNT" {
                    resp["Items"] = json!([]);
                    resp["Count"] = json!(total_count);
                }
                Ok(json_ok(resp))
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
                let _index_name = body.get("IndexName").and_then(|v| v.as_str());
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

                let store = self.store.get_or_create(&ctx.account_id, &ctx.region);
                let table = match store.get_table(&name) {
                    None => {
                        return Ok(json_error(
                            "ResourceNotFoundException",
                            "Cannot do operations on a non-existent table",
                            400,
                        ));
                    }
                    Some(t) => t,
                };

                // For index scans, we scan all items
                let all_items: Vec<&Item> = table.all_items();

                let scanned_count = all_items.len();
                let mut items: Vec<Item> = all_items
                    .into_iter()
                    .filter(|item| {
                        if let Some(fe) = filter_expr {
                            evaluate_filter(item, fe, &attr_names, &attr_values)
                        } else {
                            true
                        }
                    })
                    .map(|item| project_item(item, projection_expr, &attr_names))
                    .collect();

                if let Some(lim) = limit {
                    items.truncate(lim);
                }

                let count = items.len();
                let mut resp = json!({
                    "Items": items,
                    "Count": count,
                    "ScannedCount": scanned_count,
                });
                if select == "COUNT" {
                    resp["Items"] = json!([]);
                }
                Ok(json_ok(resp))
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

                let store = self.store.get_or_create(&ctx.account_id, &ctx.region);
                let mut responses: serde_json::Map<String, Value> = serde_json::Map::new();
                let unprocessed: serde_json::Map<String, Value> = serde_json::Map::new();

                for (table_name, req) in &request_items {
                    let keys_arr = req.get("Keys").and_then(|v| v.as_array());
                    let projection_expr = req.get("ProjectionExpression").and_then(|v| v.as_str());
                    let attr_names = parse_expr_names(req.get("ExpressionAttributeNames"));

                    match store.get_table(table_name) {
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
                                        .map(|m| {
                                            m.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
                                        })
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

                let mut store = self.store.get_or_create(&ctx.account_id, &ctx.region);
                let unprocessed: serde_json::Map<String, Value> = serde_json::Map::new();

                for (table_name, requests) in &request_items {
                    let reqs = match requests.as_array() {
                        Some(arr) => arr,
                        None => continue,
                    };
                    let table = match store.get_table_mut(table_name) {
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
                                let item: Item = item_val
                                    .iter()
                                    .map(|(k, v)| (k.clone(), v.clone()))
                                    .collect();
                                table.put_item(item);
                            }
                        } else if let Some(del) = req.get("DeleteRequest")
                            && let Some(key_val) = del.get("Key").and_then(|v| v.as_object())
                        {
                            let key: Item = key_val
                                .iter()
                                .map(|(k, v)| (k.clone(), v.clone()))
                                .collect();
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

                let store = self.store.get_or_create(&ctx.account_id, &ctx.region);
                let mut responses = Vec::new();

                for ti in &transact_items {
                    if let Some(get) = ti.get("Get") {
                        let table_name =
                            get.get("TableName").and_then(|v| v.as_str()).unwrap_or("");
                        let key: Item = get
                            .get("Key")
                            .and_then(|v| v.as_object())
                            .map(|m| m.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
                            .unwrap_or_default();
                        let projection_expr =
                            get.get("ProjectionExpression").and_then(|v| v.as_str());
                        let attr_names = parse_expr_names(get.get("ExpressionAttributeNames"));

                        match store.get_table(table_name) {
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

                let mut store = self.store.get_or_create(&ctx.account_id, &ctx.region);

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
                                    .map(|m| {
                                        m.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
                                    })
                                    .unwrap_or_default();

                                if let Some(table) = store.get_table(table_name) {
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
                }

                // Second pass: apply writes
                for ti in &transact_items {
                    if let Some(put) = ti.get("Put") {
                        let table_name =
                            put.get("TableName").and_then(|v| v.as_str()).unwrap_or("");
                        let item: Item = put
                            .get("Item")
                            .and_then(|v| v.as_object())
                            .map(|m| m.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
                            .unwrap_or_default();
                        if let Some(table) = store.get_table_mut(table_name) {
                            table.put_item(item);
                        }
                    } else if let Some(del) = ti.get("Delete") {
                        let table_name =
                            del.get("TableName").and_then(|v| v.as_str()).unwrap_or("");
                        let key: Item = del
                            .get("Key")
                            .and_then(|v| v.as_object())
                            .map(|m| m.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
                            .unwrap_or_default();
                        if let Some(table) = store.get_table_mut(table_name) {
                            table.delete_item(&key);
                        }
                    } else if let Some(upd) = ti.get("Update") {
                        let table_name =
                            upd.get("TableName").and_then(|v| v.as_str()).unwrap_or("");
                        let key: Item = upd
                            .get("Key")
                            .and_then(|v| v.as_object())
                            .map(|m| m.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
                            .unwrap_or_default();
                        let update_expr = upd.get("UpdateExpression").and_then(|v| v.as_str());
                        let attr_names = parse_expr_names(upd.get("ExpressionAttributeNames"));
                        let attr_values = parse_expr_values(upd.get("ExpressionAttributeValues"));
                        if let Some(table) = store.get_table_mut(table_name)
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

                let store = self.store.get_or_create(&ctx.account_id, &ctx.region);
                // Find the table with this stream ARN
                let table = store
                    .tables
                    .values()
                    .find(|t| t.stream_arn.as_deref() == Some(&stream_arn));

                match table {
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
                let store = self.store.get_or_create(&ctx.account_id, &ctx.region);
                let streams: Vec<Value> = store
                    .tables
                    .values()
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

                let store = self.store.get_or_create(&ctx.account_id, &ctx.region);
                let table = store
                    .tables
                    .values()
                    .find(|t| t.stream_arn.as_deref() == Some(&stream_arn));

                match table {
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

                let store = self.store.get_or_create(&ctx.account_id, &ctx.region);
                let table = store
                    .tables
                    .values()
                    .find(|t| t.stream_arn.as_deref() == Some(&stream_arn));

                match table {
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
}
