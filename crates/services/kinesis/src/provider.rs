use std::sync::Arc;

use async_trait::async_trait;
use base64::{Engine as _, engine::general_purpose::STANDARD as B64};
use bytes::Bytes;
use chrono::Utc;
use openstack_service_framework::traits::{
    DispatchError, DispatchResponse, RequestContext, ServiceProvider,
};
use openstack_state::AccountRegionBundle;
use serde_json::{Value, json};

use crate::store::{KinesisStore, ShardIteratorState, ShardIteratorType};

pub struct KinesisProvider {
    store: Arc<AccountRegionBundle<KinesisStore>>,
}

impl KinesisProvider {
    pub fn new() -> Self {
        Self {
            store: Arc::new(AccountRegionBundle::new()),
        }
    }
}

impl Default for KinesisProvider {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn json_ok(body: Value) -> DispatchResponse {
    DispatchResponse {
        status_code: 200,
        body: Bytes::from(serde_json::to_vec(&body).unwrap()),
        content_type: "application/x-amz-json-1.1".to_string(),
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
        content_type: "application/x-amz-json-1.1".to_string(),
        headers: Vec::new(),
    }
}

fn str_param(ctx: &RequestContext, key: &str) -> Option<String> {
    ctx.request_body
        .get(key)
        .and_then(|v| v.as_str())
        .map(String::from)
}

fn encode_iterator(state: &ShardIteratorState) -> String {
    B64.encode(serde_json::to_string(state).unwrap().as_bytes())
}

fn decode_iterator(token: &str) -> Option<ShardIteratorState> {
    let bytes = B64.decode(token).ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// Simple hash of a string to u64 (for shard selection).
fn hash_string(s: &str) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut h = DefaultHasher::new();
    s.hash(&mut h);
    h.finish()
}

fn shard_json(shard: &crate::store::Shard, _stream_arn: &str) -> Value {
    json!({
        "ShardId": shard.shard_id,
        "ParentShardId": shard.parent_shard_id,
        "AdjacentParentShardId": shard.adjacent_parent_shard_id,
        "HashKeyRange": {
            "StartingHashKey": "0",
            "EndingHashKey": "340282366920938463463374607431768211455",
        },
        "SequenceNumberRange": {
            "StartingSequenceNumber": "1",
        },
    })
}

// ---------------------------------------------------------------------------
// ServiceProvider
// ---------------------------------------------------------------------------

#[async_trait]
impl ServiceProvider for KinesisProvider {
    fn service_name(&self) -> &str {
        "kinesis"
    }

    async fn dispatch(&self, ctx: &RequestContext) -> Result<DispatchResponse, DispatchError> {
        let op = ctx.operation.as_str();
        let account_id = &ctx.account_id;
        let region = &ctx.region;

        match op {
            // ---------------------------------------------------------------
            // Stream Management
            // ---------------------------------------------------------------
            "CreateStream" => {
                let stream_name = match str_param(ctx, "StreamName") {
                    Some(n) => n,
                    None => {
                        return Ok(json_error(
                            "ValidationException",
                            "StreamName is required",
                            400,
                        ));
                    }
                };
                let shard_count = ctx
                    .request_body
                    .get("ShardCount")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(1) as usize;

                let stream_arn =
                    format!("arn:aws:kinesis:{region}:{account_id}:stream/{stream_name}");
                let stream =
                    crate::store::KinesisStream::new(stream_name.clone(), stream_arn, shard_count);
                let mut store = self.store.get_or_create(account_id, region);
                if store.streams.contains_key(&stream_name) {
                    return Ok(json_error(
                        "ResourceInUseException",
                        &format!("Stream {stream_name} already exists"),
                        400,
                    ));
                }
                store.streams.insert(stream_name, stream);
                Ok(json_ok(json!({})))
            }

            "DeleteStream" => {
                let stream_name = match str_param(ctx, "StreamName") {
                    Some(n) => n,
                    None => {
                        return Ok(json_error(
                            "ValidationException",
                            "StreamName is required",
                            400,
                        ));
                    }
                };
                let mut store = self.store.get_or_create(account_id, region);
                if store.streams.remove(&stream_name).is_none() {
                    return Ok(json_error(
                        "ResourceNotFoundException",
                        &format!("Stream {stream_name} not found"),
                        400,
                    ));
                }
                Ok(json_ok(json!({})))
            }

            "DescribeStream" => {
                let stream_name = match str_param(ctx, "StreamName") {
                    Some(n) => n,
                    None => {
                        return Ok(json_error(
                            "ValidationException",
                            "StreamName is required",
                            400,
                        ));
                    }
                };
                let store = self.store.get_or_create(account_id, region);
                let stream = match store.streams.get(&stream_name) {
                    Some(s) => s,
                    None => {
                        return Ok(json_error(
                            "ResourceNotFoundException",
                            &format!("Stream {stream_name} not found"),
                            400,
                        ));
                    }
                };
                let shards: Vec<Value> = stream
                    .shards
                    .iter()
                    .map(|s| shard_json(s, &stream.stream_arn))
                    .collect();
                Ok(json_ok(json!({
                    "StreamDescription": {
                        "StreamName": stream.stream_name,
                        "StreamARN": stream.stream_arn,
                        "StreamStatus": stream.status.as_str(),
                        "StreamCreationTimestamp": stream.created.timestamp(),
                        "RetentionPeriodHours": stream.retention_period_hours,
                        "Shards": shards,
                        "HasMoreShards": false,
                        "EnhancedMonitoring": [],
                    }
                })))
            }

            "DescribeStreamSummary" => {
                let stream_name = match str_param(ctx, "StreamName") {
                    Some(n) => n,
                    None => {
                        return Ok(json_error(
                            "ValidationException",
                            "StreamName is required",
                            400,
                        ));
                    }
                };
                let store = self.store.get_or_create(account_id, region);
                let stream = match store.streams.get(&stream_name) {
                    Some(s) => s,
                    None => {
                        return Ok(json_error(
                            "ResourceNotFoundException",
                            &format!("Stream {stream_name} not found"),
                            400,
                        ));
                    }
                };
                Ok(json_ok(json!({
                    "StreamDescriptionSummary": {
                        "StreamName": stream.stream_name,
                        "StreamARN": stream.stream_arn,
                        "StreamStatus": stream.status.as_str(),
                        "StreamCreationTimestamp": stream.created.timestamp(),
                        "RetentionPeriodHours": stream.retention_period_hours,
                        "OpenShardCount": stream.shards.iter().filter(|s| s.is_open).count(),
                        "EnhancedMonitoring": [],
                    }
                })))
            }

            "ListStreams" => {
                let store = self.store.get_or_create(account_id, region);
                let names: Vec<&str> = store
                    .streams
                    .values()
                    .map(|s| s.stream_name.as_str())
                    .collect();
                Ok(json_ok(json!({
                    "StreamNames": names,
                    "HasMoreStreams": false,
                })))
            }

            "ListShards" => {
                let stream_name = match str_param(ctx, "StreamName") {
                    Some(n) => n,
                    None => {
                        return Ok(json_error(
                            "ValidationException",
                            "StreamName is required",
                            400,
                        ));
                    }
                };
                let store = self.store.get_or_create(account_id, region);
                let stream = match store.streams.get(&stream_name) {
                    Some(s) => s,
                    None => {
                        return Ok(json_error(
                            "ResourceNotFoundException",
                            &format!("Stream {stream_name} not found"),
                            400,
                        ));
                    }
                };
                let shards: Vec<Value> = stream
                    .shards
                    .iter()
                    .map(|s| shard_json(s, &stream.stream_arn))
                    .collect();
                Ok(json_ok(json!({
                    "Shards": shards,
                    "NextToken": null,
                })))
            }

            // ---------------------------------------------------------------
            // Retention
            // ---------------------------------------------------------------
            "IncreaseStreamRetentionPeriod" => {
                let stream_name = match str_param(ctx, "StreamName") {
                    Some(n) => n,
                    None => {
                        return Ok(json_error(
                            "ValidationException",
                            "StreamName is required",
                            400,
                        ));
                    }
                };
                let hours = ctx
                    .request_body
                    .get("RetentionPeriodHours")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(24);
                let mut store = self.store.get_or_create(account_id, region);
                match store.streams.get_mut(&stream_name) {
                    None => Ok(json_error(
                        "ResourceNotFoundException",
                        &format!("Stream {stream_name} not found"),
                        400,
                    )),
                    Some(s) => {
                        s.retention_period_hours = hours;
                        Ok(json_ok(json!({})))
                    }
                }
            }

            "DecreaseStreamRetentionPeriod" => {
                let stream_name = match str_param(ctx, "StreamName") {
                    Some(n) => n,
                    None => {
                        return Ok(json_error(
                            "ValidationException",
                            "StreamName is required",
                            400,
                        ));
                    }
                };
                let hours = ctx
                    .request_body
                    .get("RetentionPeriodHours")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(24);
                let mut store = self.store.get_or_create(account_id, region);
                match store.streams.get_mut(&stream_name) {
                    None => Ok(json_error(
                        "ResourceNotFoundException",
                        &format!("Stream {stream_name} not found"),
                        400,
                    )),
                    Some(s) => {
                        s.retention_period_hours = hours;
                        Ok(json_ok(json!({})))
                    }
                }
            }

            // ---------------------------------------------------------------
            // Data Operations
            // ---------------------------------------------------------------
            "PutRecord" => {
                let stream_name = match str_param(ctx, "StreamName") {
                    Some(n) => n,
                    None => {
                        return Ok(json_error(
                            "ValidationException",
                            "StreamName is required",
                            400,
                        ));
                    }
                };
                let partition_key = str_param(ctx, "PartitionKey").unwrap_or_default();
                let data = str_param(ctx, "Data").unwrap_or_default(); // base64

                let mut store = self.store.get_or_create(account_id, region);
                let stream = match store.streams.get_mut(&stream_name) {
                    Some(s) => s,
                    None => {
                        return Ok(json_error(
                            "ResourceNotFoundException",
                            &format!("Stream {stream_name} not found"),
                            400,
                        ));
                    }
                };
                let shard_count = stream.shards.len();
                if shard_count == 0 {
                    return Ok(json_error(
                        "ResourceNotFoundException",
                        "No shards available",
                        400,
                    ));
                }
                let shard_idx = (hash_string(&partition_key) % shard_count as u64) as usize;
                let shard = &mut stream.shards[shard_idx];
                let sequence_number = shard.next_sequence_number();
                let record = crate::store::KinesisRecord {
                    sequence_number: sequence_number.clone(),
                    partition_key,
                    data,
                    approximate_arrival_timestamp: Utc::now(),
                };
                let shard_id = shard.shard_id.clone();
                shard.records.push_back(record);

                Ok(json_ok(json!({
                    "ShardId": shard_id,
                    "SequenceNumber": sequence_number,
                })))
            }

            "PutRecords" => {
                let stream_name = match str_param(ctx, "StreamName") {
                    Some(n) => n,
                    None => {
                        return Ok(json_error(
                            "ValidationException",
                            "StreamName is required",
                            400,
                        ));
                    }
                };
                let records_val = match ctx.request_body.get("Records") {
                    Some(v) => v.clone(),
                    None => {
                        return Ok(json_error(
                            "ValidationException",
                            "Records is required",
                            400,
                        ));
                    }
                };
                let records_arr = match records_val.as_array() {
                    Some(a) => a.clone(),
                    None => {
                        return Ok(json_error(
                            "ValidationException",
                            "Records must be an array",
                            400,
                        ));
                    }
                };

                let mut store = self.store.get_or_create(account_id, region);
                let stream = match store.streams.get_mut(&stream_name) {
                    Some(s) => s,
                    None => {
                        return Ok(json_error(
                            "ResourceNotFoundException",
                            &format!("Stream {stream_name} not found"),
                            400,
                        ));
                    }
                };
                let shard_count = stream.shards.len();
                let mut results: Vec<Value> = Vec::new();

                for rec in &records_arr {
                    let partition_key = rec
                        .get("PartitionKey")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let data = rec
                        .get("Data")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let shard_idx = (hash_string(&partition_key) % shard_count as u64) as usize;
                    let shard = &mut stream.shards[shard_idx];
                    let sequence_number = shard.next_sequence_number();
                    let shard_id = shard.shard_id.clone();
                    shard.records.push_back(crate::store::KinesisRecord {
                        sequence_number: sequence_number.clone(),
                        partition_key,
                        data,
                        approximate_arrival_timestamp: Utc::now(),
                    });
                    results.push(json!({
                        "ShardId": shard_id,
                        "SequenceNumber": sequence_number,
                    }));
                }

                Ok(json_ok(json!({
                    "FailedRecordCount": 0,
                    "Records": results,
                })))
            }

            "GetShardIterator" => {
                let stream_name = match str_param(ctx, "StreamName") {
                    Some(n) => n,
                    None => {
                        return Ok(json_error(
                            "ValidationException",
                            "StreamName is required",
                            400,
                        ));
                    }
                };
                let shard_id = match str_param(ctx, "ShardId") {
                    Some(s) => s,
                    None => {
                        return Ok(json_error(
                            "ValidationException",
                            "ShardId is required",
                            400,
                        ));
                    }
                };
                let iterator_type_str = str_param(ctx, "ShardIteratorType")
                    .unwrap_or_else(|| "TRIM_HORIZON".to_string());
                let iterator_type = ShardIteratorType::parse(&iterator_type_str);
                let starting_sequence =
                    str_param(ctx, "StartingSequenceNumber").unwrap_or_default();

                let store = self.store.get_or_create(account_id, region);
                let stream = match store.streams.get(&stream_name) {
                    Some(s) => s,
                    None => {
                        return Ok(json_error(
                            "ResourceNotFoundException",
                            &format!("Stream {stream_name} not found"),
                            400,
                        ));
                    }
                };
                let shard = match stream.shards.iter().find(|s| s.shard_id == shard_id) {
                    Some(s) => s,
                    None => {
                        return Ok(json_error(
                            "ResourceNotFoundException",
                            &format!("Shard {shard_id} not found"),
                            400,
                        ));
                    }
                };

                let next_index = match iterator_type {
                    ShardIteratorType::TrimHorizon => 0,
                    ShardIteratorType::Latest => shard.records.len(),
                    ShardIteratorType::AtSequenceNumber => shard
                        .records
                        .iter()
                        .position(|r| r.sequence_number == starting_sequence)
                        .unwrap_or(shard.records.len()),
                    ShardIteratorType::AfterSequenceNumber => shard
                        .records
                        .iter()
                        .position(|r| r.sequence_number == starting_sequence)
                        .map(|i| i + 1)
                        .unwrap_or(shard.records.len()),
                    ShardIteratorType::AtTimestamp => 0,
                };

                let state = ShardIteratorState {
                    stream_name,
                    shard_id,
                    next_index,
                };
                let token = encode_iterator(&state);

                Ok(json_ok(json!({ "ShardIterator": token })))
            }

            "GetRecords" => {
                let shard_iterator = match str_param(ctx, "ShardIterator") {
                    Some(t) => t,
                    None => {
                        return Ok(json_error(
                            "ValidationException",
                            "ShardIterator is required",
                            400,
                        ));
                    }
                };
                let limit = ctx
                    .request_body
                    .get("Limit")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(10000) as usize;

                let state = match decode_iterator(&shard_iterator) {
                    Some(s) => s,
                    None => {
                        return Ok(json_error(
                            "InvalidArgumentException",
                            "Invalid ShardIterator",
                            400,
                        ));
                    }
                };

                let store = self.store.get_or_create(account_id, region);
                let stream = match store.streams.get(&state.stream_name) {
                    Some(s) => s,
                    None => {
                        return Ok(json_error(
                            "ResourceNotFoundException",
                            &format!("Stream {} not found", state.stream_name),
                            400,
                        ));
                    }
                };
                let shard = match stream.shards.iter().find(|s| s.shard_id == state.shard_id) {
                    Some(s) => s,
                    None => {
                        return Ok(json_error(
                            "ResourceNotFoundException",
                            &format!("Shard {} not found", state.shard_id),
                            400,
                        ));
                    }
                };

                let available: Vec<&crate::store::KinesisRecord> = shard
                    .records
                    .iter()
                    .skip(state.next_index)
                    .take(limit)
                    .collect();

                let next_index = state.next_index + available.len();
                let records_json: Vec<Value> = available
                    .iter()
                    .map(|r| {
                        json!({
                            "SequenceNumber": r.sequence_number,
                            "PartitionKey": r.partition_key,
                            "Data": r.data,
                            "ApproximateArrivalTimestamp": r.approximate_arrival_timestamp.timestamp(),
                        })
                    })
                    .collect();

                let next_state = ShardIteratorState {
                    stream_name: state.stream_name,
                    shard_id: state.shard_id,
                    next_index,
                };
                let next_iterator = encode_iterator(&next_state);

                Ok(json_ok(json!({
                    "Records": records_json,
                    "NextShardIterator": next_iterator,
                    "MillisBehindLatest": 0,
                })))
            }

            // ---------------------------------------------------------------
            // Shard Operations
            // ---------------------------------------------------------------
            "SplitShard" => {
                let stream_name = match str_param(ctx, "StreamName") {
                    Some(n) => n,
                    None => {
                        return Ok(json_error(
                            "ValidationException",
                            "StreamName is required",
                            400,
                        ));
                    }
                };
                let shard_to_split = match str_param(ctx, "ShardToSplit") {
                    Some(s) => s,
                    None => {
                        return Ok(json_error(
                            "ValidationException",
                            "ShardToSplit is required",
                            400,
                        ));
                    }
                };

                let mut store = self.store.get_or_create(account_id, region);
                let stream = match store.streams.get_mut(&stream_name) {
                    Some(s) => s,
                    None => {
                        return Ok(json_error(
                            "ResourceNotFoundException",
                            &format!("Stream {stream_name} not found"),
                            400,
                        ));
                    }
                };

                // Find and close the shard to split
                let parent_idx = match stream
                    .shards
                    .iter()
                    .position(|s| s.shard_id == shard_to_split)
                {
                    Some(i) => i,
                    None => {
                        return Ok(json_error(
                            "ResourceNotFoundException",
                            &format!("Shard {shard_to_split} not found"),
                            400,
                        ));
                    }
                };
                stream.shards[parent_idx].is_open = false;

                // Create two new child shards
                let counter = stream.shard_id_counter;
                stream.shard_id_counter += 2;
                let mut child1 = crate::store::Shard::new(format!("shardId-{:012}", counter));
                let mut child2 = crate::store::Shard::new(format!("shardId-{:012}", counter + 1));
                child1.parent_shard_id = Some(shard_to_split.clone());
                child2.parent_shard_id = Some(shard_to_split.clone());
                stream.shards.push(child1);
                stream.shards.push(child2);
                stream.shard_count = stream.shards.iter().filter(|s| s.is_open).count();

                Ok(json_ok(json!({})))
            }

            "MergeShards" => {
                let stream_name = match str_param(ctx, "StreamName") {
                    Some(n) => n,
                    None => {
                        return Ok(json_error(
                            "ValidationException",
                            "StreamName is required",
                            400,
                        ));
                    }
                };
                let shard_to_merge = str_param(ctx, "ShardToMerge").unwrap_or_default();
                let adjacent_shard = str_param(ctx, "AdjacentShardToMerge").unwrap_or_default();

                let mut store = self.store.get_or_create(account_id, region);
                let stream = match store.streams.get_mut(&stream_name) {
                    Some(s) => s,
                    None => {
                        return Ok(json_error(
                            "ResourceNotFoundException",
                            &format!("Stream {stream_name} not found"),
                            400,
                        ));
                    }
                };

                // Close both shards
                for shard in stream.shards.iter_mut() {
                    if shard.shard_id == shard_to_merge || shard.shard_id == adjacent_shard {
                        shard.is_open = false;
                    }
                }

                // Create merged child shard
                let counter = stream.shard_id_counter;
                stream.shard_id_counter += 1;
                let mut child = crate::store::Shard::new(format!("shardId-{:012}", counter));
                child.parent_shard_id = Some(shard_to_merge.clone());
                child.adjacent_parent_shard_id = Some(adjacent_shard.clone());
                stream.shards.push(child);
                stream.shard_count = stream.shards.iter().filter(|s| s.is_open).count();

                Ok(json_ok(json!({})))
            }

            // ---------------------------------------------------------------
            // Tagging
            // ---------------------------------------------------------------
            "AddTagsToStream" => {
                // No-op — we don't store tags on streams in this impl
                Ok(json_ok(json!({})))
            }

            "ListTagsForStream" => Ok(json_ok(json!({
                "Tags": [],
                "HasMoreTags": false,
            }))),

            "RemoveTagsFromStream" => Ok(json_ok(json!({}))),

            // ---------------------------------------------------------------
            // Fallback
            // ---------------------------------------------------------------
            _ => Ok(json_error(
                "NotImplementedException",
                &format!("Operation not implemented: {op}"),
                501,
            )),
        }
    }
}
