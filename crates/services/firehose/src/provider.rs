use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use chrono::Utc;
use openstack_service_framework::traits::{
    DispatchError, DispatchResponse, RequestContext, ResponseBody, ServiceProvider,
};
use openstack_state::AccountRegionBundle;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::store::{
    DeliveryStreamStatus, DestinationType, FirehoseDeliveryStream, FirehoseRecord, FirehoseStore,
};

pub struct FirehoseProvider {
    store: Arc<AccountRegionBundle<FirehoseStore>>,
}

impl FirehoseProvider {
    pub fn new() -> Self {
        Self {
            store: Arc::new(AccountRegionBundle::new()),
        }
    }
}

impl Default for FirehoseProvider {
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
        body: ResponseBody::Buffered(Bytes::from(serde_json::to_vec(&body).unwrap())),
        content_type: "application/x-amz-json-1.1".to_string(),
        headers: Vec::new(),
    }
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

fn stream_summary(s: &FirehoseDeliveryStream) -> Value {
    let _destination_type = match &s.destination {
        DestinationType::S3 { .. } | DestinationType::ExtendedS3 { .. } => "S3",
        DestinationType::Other => "OTHER",
    };
    let bucket_arn = match &s.destination {
        DestinationType::S3 { bucket_arn } | DestinationType::ExtendedS3 { bucket_arn } => {
            bucket_arn.as_str()
        }
        DestinationType::Other => "",
    };
    json!({
        "DeliveryStreamName": s.name,
        "DeliveryStreamARN": s.arn,
        "DeliveryStreamStatus": s.status.as_str(),
        "DeliveryStreamType": "DirectPut",
        "CreateTimestamp": s.created.timestamp(),
        "Destinations": [
            {
                "DestinationId": "destinationId-000000000001",
                "S3DestinationDescription": {
                    "BucketARN": bucket_arn,
                },
            }
        ],
        "HasMoreDestinations": false,
    })
}

// ---------------------------------------------------------------------------
// ServiceProvider
// ---------------------------------------------------------------------------

#[async_trait]
impl ServiceProvider for FirehoseProvider {
    fn service_name(&self) -> &str {
        "firehose"
    }

    async fn dispatch(&self, ctx: &RequestContext) -> Result<DispatchResponse, DispatchError> {
        let op = ctx.operation.as_str();
        let account_id = &ctx.account_id;
        let region = &ctx.region;

        match op {
            "CreateDeliveryStream" => {
                let stream_name = match str_param(ctx, "DeliveryStreamName") {
                    Some(n) => n,
                    None => {
                        return Ok(json_error(
                            "ValidationException",
                            "DeliveryStreamName is required",
                            400,
                        ));
                    }
                };

                // Determine destination from request
                let destination = if let Some(ext_s3) =
                    ctx.request_body.get("ExtendedS3DestinationConfiguration")
                {
                    let bucket_arn = ext_s3
                        .get("BucketARN")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    DestinationType::ExtendedS3 { bucket_arn }
                } else if let Some(s3) = ctx.request_body.get("S3DestinationConfiguration") {
                    let bucket_arn = s3
                        .get("BucketARN")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    DestinationType::S3 { bucket_arn }
                } else {
                    DestinationType::Other
                };

                let stream_arn =
                    format!("arn:aws:firehose:{region}:{account_id}:deliverystream/{stream_name}");
                let delivery_stream = FirehoseDeliveryStream {
                    name: stream_name.clone(),
                    arn: stream_arn,
                    status: DeliveryStreamStatus::Active,
                    destination,
                    records: Vec::new(),
                    created: Utc::now(),
                };

                let mut store = self.store.get_or_create(account_id, region);
                if store.streams.contains_key(&stream_name) {
                    return Ok(json_error(
                        "ResourceInUseException",
                        &format!("Delivery stream {stream_name} already exists"),
                        400,
                    ));
                }
                let stream_arn_copy = delivery_stream.arn.clone();
                store.streams.insert(stream_name, delivery_stream);
                Ok(json_ok(json!({ "DeliveryStreamARN": stream_arn_copy })))
            }

            "DeleteDeliveryStream" => {
                let stream_name = match str_param(ctx, "DeliveryStreamName") {
                    Some(n) => n,
                    None => {
                        return Ok(json_error(
                            "ValidationException",
                            "DeliveryStreamName is required",
                            400,
                        ));
                    }
                };
                let mut store = self.store.get_or_create(account_id, region);
                if store.streams.remove(&stream_name).is_none() {
                    return Ok(json_error(
                        "ResourceNotFoundException",
                        &format!("Delivery stream {stream_name} not found"),
                        400,
                    ));
                }
                Ok(json_ok(json!({})))
            }

            "DescribeDeliveryStream" => {
                let stream_name = match str_param(ctx, "DeliveryStreamName") {
                    Some(n) => n,
                    None => {
                        return Ok(json_error(
                            "ValidationException",
                            "DeliveryStreamName is required",
                            400,
                        ));
                    }
                };
                let store = self.store.get_or_create(account_id, region);
                match store.streams.get(&stream_name) {
                    None => Ok(json_error(
                        "ResourceNotFoundException",
                        &format!("Delivery stream {stream_name} not found"),
                        400,
                    )),
                    Some(s) => Ok(json_ok(json!({
                        "DeliveryStreamDescription": stream_summary(s),
                    }))),
                }
            }

            "ListDeliveryStreams" => {
                let store = self.store.get_or_create(account_id, region);
                let names: Vec<&str> = store.streams.values().map(|s| s.name.as_str()).collect();
                Ok(json_ok(json!({
                    "DeliveryStreamNames": names,
                    "HasMoreDeliveryStreams": false,
                })))
            }

            "PutRecord" => {
                let stream_name = match str_param(ctx, "DeliveryStreamName") {
                    Some(n) => n,
                    None => {
                        return Ok(json_error(
                            "ValidationException",
                            "DeliveryStreamName is required",
                            400,
                        ));
                    }
                };
                let data = ctx
                    .request_body
                    .get("Record")
                    .and_then(|r| r.get("Data"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();

                let record_id = Uuid::new_v4().to_string();
                let record = FirehoseRecord {
                    record_id: record_id.clone(),
                    data,
                    arrival: Utc::now(),
                };

                let mut store = self.store.get_or_create(account_id, region);
                match store.streams.get_mut(&stream_name) {
                    None => Ok(json_error(
                        "ResourceNotFoundException",
                        &format!("Delivery stream {stream_name} not found"),
                        400,
                    )),
                    Some(s) => {
                        s.records.push(record);
                        Ok(json_ok(json!({ "RecordId": record_id })))
                    }
                }
            }

            "PutRecordBatch" => {
                let stream_name = match str_param(ctx, "DeliveryStreamName") {
                    Some(n) => n,
                    None => {
                        return Ok(json_error(
                            "ValidationException",
                            "DeliveryStreamName is required",
                            400,
                        ));
                    }
                };
                let records_val = ctx
                    .request_body
                    .get("Records")
                    .cloned()
                    .unwrap_or(json!([]));
                let records_arr = records_val.as_array().cloned().unwrap_or_default();

                let mut store = self.store.get_or_create(account_id, region);
                match store.streams.get_mut(&stream_name) {
                    None => Ok(json_error(
                        "ResourceNotFoundException",
                        &format!("Delivery stream {stream_name} not found"),
                        400,
                    )),
                    Some(s) => {
                        let mut results: Vec<Value> = Vec::new();
                        for rec in &records_arr {
                            let data = rec
                                .get("Data")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string();
                            let record_id = Uuid::new_v4().to_string();
                            s.records.push(FirehoseRecord {
                                record_id: record_id.clone(),
                                data,
                                arrival: Utc::now(),
                            });
                            results.push(json!({ "RecordId": record_id }));
                        }
                        Ok(json_ok(json!({
                            "FailedPutCount": 0,
                            "RequestResponses": results,
                        })))
                    }
                }
            }

            _ => Ok(json_error(
                "NotImplementedException",
                &format!("Operation not implemented: {op}"),
                501,
            )),
        }
    }
}
