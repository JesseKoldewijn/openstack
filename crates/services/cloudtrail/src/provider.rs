use std::borrow::Cow;
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

use crate::store::{CloudTrailStore, Trail};

pub struct CloudTrailProvider {
    store: Arc<AccountRegionBundle<CloudTrailStore>>,
}

impl CloudTrailProvider {
    pub fn new() -> Self {
        Self {
            store: Arc::new(AccountRegionBundle::new()),
        }
    }
}

impl Default for CloudTrailProvider {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Helpers — CloudTrail uses JSON protocol
// ---------------------------------------------------------------------------

fn json_ok(body: Value) -> DispatchResponse {
    DispatchResponse {
        status_code: 200,
        body: ResponseBody::Buffered(Bytes::from(serde_json::to_vec(&body).unwrap())),
        content_type: Cow::Borrowed("application/x-amz-json-1.1"),
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
        content_type: Cow::Borrowed("application/x-amz-json-1.1"),
        headers: Vec::new(),
    }
}

fn str_param(ctx: &RequestContext, key: &str) -> Option<String> {
    ctx.request_body
        .get(key)
        .and_then(|v| v.as_str())
        .map(String::from)
}

fn trail_arn(account_id: &str, region: &str, name: &str) -> String {
    format!("arn:aws:cloudtrail:{region}:{account_id}:trail/{name}")
}

fn trail_to_json(t: &Trail) -> Value {
    json!({
        "Name": t.name,
        "TrailARN": t.trail_arn,
        "S3BucketName": t.s3_bucket_name,
        "S3KeyPrefix": t.s3_key_prefix,
        "SnsTopicName": t.sns_topic_name,
        "IncludeGlobalServiceEvents": t.include_global_service_events,
        "IsMultiRegionTrail": t.is_multi_region_trail,
        "LogFileValidationEnabled": t.log_file_validation_enabled,
        "HomeRegion": t.home_region,
    })
}

// ---------------------------------------------------------------------------
// ServiceProvider
// ---------------------------------------------------------------------------

#[async_trait]
impl ServiceProvider for CloudTrailProvider {
    fn service_name(&self) -> &str {
        "cloudtrail"
    }

    async fn dispatch(&self, ctx: &RequestContext) -> Result<DispatchResponse, DispatchError> {
        let region = &ctx.region;
        let account_id = &ctx.account_id;

        match ctx.operation.as_str() {
            // ----------------------------------------------------------------
            // CreateTrail
            // ----------------------------------------------------------------
            "CreateTrail" => {
                let name = match str_param(ctx, "Name") {
                    Some(n) => n,
                    None => {
                        return Ok(json_error(
                            "InvalidTrailNameException",
                            "Name is required",
                            400,
                        ));
                    }
                };
                let s3_bucket = match str_param(ctx, "S3BucketName") {
                    Some(b) => b,
                    None => {
                        return Ok(json_error(
                            "InvalidS3BucketNameException",
                            "S3BucketName is required",
                            400,
                        ));
                    }
                };
                let s3_prefix = str_param(ctx, "S3KeyPrefix");
                let sns_topic = str_param(ctx, "SnsTopicName");
                let include_global = ctx
                    .request_body
                    .get("IncludeGlobalServiceEvents")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(true);
                let multi_region = ctx
                    .request_body
                    .get("IsMultiRegionTrail")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let log_validation = ctx
                    .request_body
                    .get("EnableLogFileValidation")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);

                let arn = trail_arn(account_id, region, &name);

                let mut store = self.store.get_or_create(account_id, region);
                if store.trails.contains_key(&name) {
                    return Ok(json_error(
                        "TrailAlreadyExistsException",
                        &format!("Trail {name} already exists"),
                        400,
                    ));
                }
                let trail = Trail {
                    name: name.clone(),
                    trail_arn: arn,
                    s3_bucket_name: s3_bucket,
                    s3_key_prefix: s3_prefix,
                    sns_topic_name: sns_topic,
                    include_global_service_events: include_global,
                    is_multi_region_trail: multi_region,
                    log_file_validation_enabled: log_validation,
                    home_region: region.clone(),
                    logging_enabled: false,
                    created: Utc::now(),
                    tags: Default::default(),
                };
                store.trails.insert(name, trail.clone());
                Ok(json_ok(trail_to_json(&trail)))
            }

            // ----------------------------------------------------------------
            // DeleteTrail
            // ----------------------------------------------------------------
            "DeleteTrail" => {
                let name = match str_param(ctx, "Name") {
                    Some(n) => n,
                    None => {
                        return Ok(json_error(
                            "InvalidTrailNameException",
                            "Name is required",
                            400,
                        ));
                    }
                };
                // Accept both name and ARN
                let key = if name.starts_with("arn:") {
                    name.rsplit('/').next().unwrap_or(&name).to_string()
                } else {
                    name.clone()
                };
                let mut store = self.store.get_or_create(account_id, region);
                if store.trails.remove(&key).is_none() {
                    return Ok(json_error(
                        "TrailNotFoundException",
                        &format!("Trail {name} not found"),
                        400,
                    ));
                }
                Ok(json_ok(json!({})))
            }

            // ----------------------------------------------------------------
            // DescribeTrails
            // ----------------------------------------------------------------
            "DescribeTrails" => {
                let trail_name_list: Vec<String> = ctx
                    .request_body
                    .get("trailNameList")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str().map(String::from))
                            .collect()
                    })
                    .unwrap_or_default();

                let Some(store) = self.store.get(account_id, region) else {
                    return Ok(json_ok(json!({ "trailList": [] })));
                };

                let trails: Vec<Value> = store
                    .trails
                    .values()
                    .filter(|t| {
                        trail_name_list.is_empty()
                            || trail_name_list.contains(&t.name)
                            || trail_name_list.contains(&t.trail_arn)
                    })
                    .map(trail_to_json)
                    .collect();

                Ok(json_ok(json!({ "trailList": trails })))
            }

            // ----------------------------------------------------------------
            // GetTrail
            // ----------------------------------------------------------------
            "GetTrail" => {
                let name = match str_param(ctx, "Name") {
                    Some(n) => n,
                    None => {
                        return Ok(json_error(
                            "InvalidTrailNameException",
                            "Name is required",
                            400,
                        ));
                    }
                };
                let key = if name.starts_with("arn:") {
                    name.rsplit('/').next().unwrap_or(&name).to_string()
                } else {
                    name.clone()
                };
                let Some(store) = self.store.get(account_id, region) else {
                    return Ok(json_error(
                        "TrailNotFoundException",
                        &format!("Trail {name} not found"),
                        404,
                    ));
                };
                match store.trails.get(&key) {
                    Some(t) => Ok(json_ok(json!({ "Trail": trail_to_json(t) }))),
                    None => Ok(json_error(
                        "TrailNotFoundException",
                        &format!("Trail {name} not found"),
                        404,
                    )),
                }
            }

            // ----------------------------------------------------------------
            // GetTrailStatus
            // ----------------------------------------------------------------
            "GetTrailStatus" => {
                let name = match str_param(ctx, "Name") {
                    Some(n) => n,
                    None => {
                        return Ok(json_error(
                            "InvalidTrailNameException",
                            "Name is required",
                            400,
                        ));
                    }
                };
                let key = if name.starts_with("arn:") {
                    name.rsplit('/').next().unwrap_or(&name).to_string()
                } else {
                    name.clone()
                };
                let Some(store) = self.store.get(account_id, region) else {
                    return Ok(json_error(
                        "TrailNotFoundException",
                        &format!("Trail {name} not found"),
                        404,
                    ));
                };
                match store.trails.get(&key) {
                    Some(t) => Ok(json_ok(json!({
                        "IsLogging": t.logging_enabled,
                        "LatestDeliveryTime": t.created.to_rfc3339(),
                        "StartLoggingTime": t.created.to_rfc3339(),
                    }))),
                    None => Ok(json_error(
                        "TrailNotFoundException",
                        &format!("Trail {name} not found"),
                        404,
                    )),
                }
            }

            // ----------------------------------------------------------------
            // StartLogging
            // ----------------------------------------------------------------
            "StartLogging" => {
                let name = match str_param(ctx, "Name") {
                    Some(n) => n,
                    None => {
                        return Ok(json_error(
                            "InvalidTrailNameException",
                            "Name is required",
                            400,
                        ));
                    }
                };
                let key = if name.starts_with("arn:") {
                    name.rsplit('/').next().unwrap_or(&name).to_string()
                } else {
                    name.clone()
                };
                let mut store = self.store.get_or_create(account_id, region);
                match store.trails.get_mut(&key) {
                    Some(t) => {
                        t.logging_enabled = true;
                        Ok(json_ok(json!({})))
                    }
                    None => Ok(json_error(
                        "TrailNotFoundException",
                        &format!("Trail {name} not found"),
                        404,
                    )),
                }
            }

            // ----------------------------------------------------------------
            // StopLogging
            // ----------------------------------------------------------------
            "StopLogging" => {
                let name = match str_param(ctx, "Name") {
                    Some(n) => n,
                    None => {
                        return Ok(json_error(
                            "InvalidTrailNameException",
                            "Name is required",
                            400,
                        ));
                    }
                };
                let key = if name.starts_with("arn:") {
                    name.rsplit('/').next().unwrap_or(&name).to_string()
                } else {
                    name.clone()
                };
                let mut store = self.store.get_or_create(account_id, region);
                match store.trails.get_mut(&key) {
                    Some(t) => {
                        t.logging_enabled = false;
                        Ok(json_ok(json!({})))
                    }
                    None => Ok(json_error(
                        "TrailNotFoundException",
                        &format!("Trail {name} not found"),
                        404,
                    )),
                }
            }

            // ----------------------------------------------------------------
            // UpdateTrail
            // ----------------------------------------------------------------
            "UpdateTrail" => {
                let name = match str_param(ctx, "Name") {
                    Some(n) => n,
                    None => {
                        return Ok(json_error(
                            "InvalidTrailNameException",
                            "Name is required",
                            400,
                        ));
                    }
                };
                let key = if name.starts_with("arn:") {
                    name.rsplit('/').next().unwrap_or(&name).to_string()
                } else {
                    name.clone()
                };
                let mut store = self.store.get_or_create(account_id, region);
                match store.trails.get_mut(&key) {
                    Some(t) => {
                        if let Some(bucket) = str_param(ctx, "S3BucketName") {
                            t.s3_bucket_name = bucket;
                        }
                        if let Some(prefix) = str_param(ctx, "S3KeyPrefix") {
                            t.s3_key_prefix = Some(prefix);
                        }
                        if let Some(multi) = ctx
                            .request_body
                            .get("IsMultiRegionTrail")
                            .and_then(|v| v.as_bool())
                        {
                            t.is_multi_region_trail = multi;
                        }
                        if let Some(validation) = ctx
                            .request_body
                            .get("EnableLogFileValidation")
                            .and_then(|v| v.as_bool())
                        {
                            t.log_file_validation_enabled = validation;
                        }
                        Ok(json_ok(trail_to_json(t)))
                    }
                    None => Ok(json_error(
                        "TrailNotFoundException",
                        &format!("Trail {name} not found"),
                        404,
                    )),
                }
            }

            // ----------------------------------------------------------------
            // LookupEvents — return stored events or empty list
            // ----------------------------------------------------------------
            "LookupEvents" => {
                let max_results: usize = ctx
                    .request_body
                    .get("MaxResults")
                    .and_then(|v| v.as_u64())
                    .map(|n| n as usize)
                    .unwrap_or(50)
                    .min(50);

                let events: Vec<Value> = self
                    .store
                    .get(account_id, region)
                    .map(|store| {
                        store
                            .events
                            .iter()
                            .rev()
                            .take(max_results)
                            .map(|e| {
                                json!({
                                    "EventId": e.event_id,
                                    "EventName": e.event_name,
                                    "EventTime": e.event_time.to_rfc3339(),
                                    "EventSource": e.event_source,
                                    "Username": e.username,
                                    "CloudTrailEvent": e.cloud_trail_event,
                                })
                            })
                            .collect()
                    })
                    .unwrap_or_default();

                Ok(json_ok(json!({
                    "Events": events,
                    "NextToken": null,
                })))
            }

            // ----------------------------------------------------------------
            // AddTags / ListTags / RemoveTags
            // ----------------------------------------------------------------
            "AddTags" => {
                let resource_id = match str_param(ctx, "ResourceId") {
                    Some(id) => id,
                    None => {
                        return Ok(json_error(
                            "InvalidParameterCombinationException",
                            "ResourceId is required",
                            400,
                        ));
                    }
                };
                let tag_list = ctx
                    .request_body
                    .get("TagsList")
                    .and_then(|v| v.as_array())
                    .cloned()
                    .unwrap_or_default();

                let key = resource_id.rsplit('/').next().unwrap_or(&resource_id).to_string();
                let mut store = self.store.get_or_create(account_id, region);
                if let Some(trail) = store.trails.get_mut(&key) {
                    for tag in &tag_list {
                        if let (Some(k), Some(v)) = (
                            tag.get("Key").and_then(|v| v.as_str()),
                            tag.get("Value").and_then(|v| v.as_str()),
                        ) {
                            trail.tags.insert(k.to_string(), v.to_string());
                        }
                    }
                }
                Ok(json_ok(json!({})))
            }

            "ListTags" => {
                let resource_id_list: Vec<String> = ctx
                    .request_body
                    .get("ResourceIdList")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str().map(String::from))
                            .collect()
                    })
                    .unwrap_or_default();

                let resource_tag_list: Vec<Value> = self
                    .store
                    .get(account_id, region)
                    .map(|store| {
                        resource_id_list
                            .iter()
                            .filter_map(|rid| {
                                let key = rid.rsplit('/').next().unwrap_or(rid).to_string();
                                store.trails.get(&key).map(|t| {
                                    let tags: Vec<Value> = t
                                        .tags
                                        .iter()
                                        .map(|(k, v)| json!({"Key": k, "Value": v}))
                                        .collect();
                                    json!({ "ResourceId": t.trail_arn, "TagsList": tags })
                                })
                            })
                            .collect()
                    })
                    .unwrap_or_default();

                Ok(json_ok(json!({ "ResourceTagList": resource_tag_list })))
            }

            "RemoveTags" => {
                let resource_id = match str_param(ctx, "ResourceId") {
                    Some(id) => id,
                    None => {
                        return Ok(json_error(
                            "InvalidParameterCombinationException",
                            "ResourceId is required",
                            400,
                        ));
                    }
                };
                let tag_list = ctx
                    .request_body
                    .get("TagsList")
                    .and_then(|v| v.as_array())
                    .cloned()
                    .unwrap_or_default();
                let keys: Vec<&str> = tag_list
                    .iter()
                    .filter_map(|t| t.get("Key").and_then(|v| v.as_str()))
                    .collect();

                let key = resource_id.rsplit('/').next().unwrap_or(&resource_id).to_string();
                let mut store = self.store.get_or_create(account_id, region);
                if let Some(trail) = store.trails.get_mut(&key) {
                    for k in &keys {
                        trail.tags.remove(*k);
                    }
                }
                Ok(json_ok(json!({})))
            }

            _ => Err(DispatchError::NotImplemented(ctx.operation.clone())),
        }
    }

    async fn storage_snapshot(&self) -> Option<serde_json::Value> {
        use serde_json::json;
        let mut trails = Vec::new();
        for entry in self.store.iter() {
            for t in entry.value().trails.values() {
                trails.push(json!({
                    "id": t.trail_arn, "kind": "trail",
                    "attributes": [
                        {"key": "name", "value": t.name.clone()},
                        {"key": "logging", "value": t.logging_enabled.to_string()},
                        {"key": "bucket", "value": t.s3_bucket_name.clone()},
                    ]
                }));
            }
        }
        Some(json!({ "kind": "cloudtrail", "trails": trails }))
    }
}

// Suppress unused import from uuid (used in req_id)
fn _use_uuid() -> String {
    Uuid::new_v4().to_string()
}
