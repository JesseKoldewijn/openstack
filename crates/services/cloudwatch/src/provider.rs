use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use chrono::Utc;
use openstack_service_framework::traits::{
    DispatchError, DispatchResponse, RequestContext, ServiceProvider,
};
use openstack_state::AccountRegionBundle;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::store::{CloudWatchStore, LogEvent, LogGroup, LogStream, MetricAlarm, MetricDatum};

pub struct CloudWatchProvider {
    store: Arc<AccountRegionBundle<CloudWatchStore>>,
}

impl CloudWatchProvider {
    pub fn new() -> Self {
        Self {
            store: Arc::new(AccountRegionBundle::new()),
        }
    }
}

impl Default for CloudWatchProvider {
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

fn now_ms() -> i64 {
    Utc::now().timestamp_millis()
}

// ---------------------------------------------------------------------------
// ServiceProvider
// ---------------------------------------------------------------------------

#[async_trait]
impl ServiceProvider for CloudWatchProvider {
    fn service_name(&self) -> &str {
        "cloudwatch"
    }

    async fn dispatch(&self, ctx: &RequestContext) -> Result<DispatchResponse, DispatchError> {
        let region = &ctx.region;
        let account_id = &ctx.account_id;

        match ctx.operation.as_str() {
            // ----------------------------------------------------------------
            // PutMetricData
            // ----------------------------------------------------------------
            "PutMetricData" => {
                let namespace = match str_param(ctx, "Namespace") {
                    Some(n) => n,
                    None => return Ok(json_error("ValidationError", "Namespace is required", 400)),
                };
                let mut store = self.store.get_or_create(account_id, region);
                if let Some(data) = ctx
                    .request_body
                    .get("MetricData")
                    .and_then(|v| v.as_array())
                {
                    let now = Utc::now();
                    for datum in data {
                        let metric_name = datum
                            .get("MetricName")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        let value = datum.get("Value").and_then(|v| v.as_f64()).unwrap_or(0.0);
                        let unit = datum
                            .get("Unit")
                            .and_then(|v| v.as_str())
                            .unwrap_or("None")
                            .to_string();
                        let dimensions: Vec<(String, String)> = datum
                            .get("Dimensions")
                            .and_then(|v| v.as_array())
                            .map(|dims| {
                                dims.iter()
                                    .filter_map(|d| {
                                        let name = d.get("Name")?.as_str()?.to_string();
                                        let value = d.get("Value")?.as_str()?.to_string();
                                        Some((name, value))
                                    })
                                    .collect()
                            })
                            .unwrap_or_default();
                        store.metrics.push(MetricDatum {
                            namespace: namespace.clone(),
                            metric_name,
                            dimensions,
                            timestamp: now,
                            value,
                            unit,
                        });
                    }
                }
                Ok(json_ok(json!({})))
            }

            // ----------------------------------------------------------------
            // ListMetrics
            // ----------------------------------------------------------------
            "ListMetrics" => {
                let namespace_filter = str_param(ctx, "Namespace");
                let metric_name_filter = str_param(ctx, "MetricName");
                let store = self.store.get_or_create(account_id, region);

                let mut seen = std::collections::HashSet::new();
                let metrics: Vec<Value> = store
                    .metrics
                    .iter()
                    .filter(|m| {
                        namespace_filter.as_deref().map(|n| n == m.namespace).unwrap_or(true)
                            && metric_name_filter
                                .as_deref()
                                .map(|n| n == m.metric_name)
                                .unwrap_or(true)
                    })
                    .filter_map(|m| {
                        let key = format!("{}:{}", m.namespace, m.metric_name);
                        if seen.insert(key) {
                            Some(json!({
                                "Namespace": m.namespace,
                                "MetricName": m.metric_name,
                                "Dimensions": m.dimensions.iter().map(|(k, v)| json!({"Name": k, "Value": v})).collect::<Vec<_>>(),
                            }))
                        } else {
                            None
                        }
                    })
                    .collect();

                Ok(json_ok(json!({ "Metrics": metrics })))
            }

            // ----------------------------------------------------------------
            // GetMetricStatistics
            // ----------------------------------------------------------------
            "GetMetricStatistics" => {
                let namespace = str_param(ctx, "Namespace").unwrap_or_default();
                let metric_name = str_param(ctx, "MetricName").unwrap_or_default();
                let store = self.store.get_or_create(account_id, region);

                let values: Vec<f64> = store
                    .metrics
                    .iter()
                    .filter(|m| m.namespace == namespace && m.metric_name == metric_name)
                    .map(|m| m.value)
                    .collect();

                let count = values.len() as f64;
                let sum: f64 = values.iter().sum();
                let average = if count > 0.0 { sum / count } else { 0.0 };
                let min = values.iter().cloned().fold(f64::INFINITY, f64::min);
                let max = values.iter().cloned().fold(f64::NEG_INFINITY, f64::max);

                let datapoints = if count > 0.0 {
                    vec![json!({
                        "Timestamp": Utc::now().to_rfc3339(),
                        "SampleCount": count,
                        "Sum": sum,
                        "Average": average,
                        "Minimum": if min.is_finite() { min } else { 0.0 },
                        "Maximum": if max.is_finite() { max } else { 0.0 },
                        "Unit": "None",
                    })]
                } else {
                    vec![]
                };

                Ok(json_ok(json!({
                    "Label": metric_name,
                    "Datapoints": datapoints,
                })))
            }

            // ----------------------------------------------------------------
            // GetMetricData
            // ----------------------------------------------------------------
            "GetMetricData" => Ok(json_ok(json!({ "MetricDataResults": [], "Messages": [] }))),

            // ----------------------------------------------------------------
            // PutMetricAlarm
            // ----------------------------------------------------------------
            "PutMetricAlarm" => {
                let alarm_name = match str_param(ctx, "AlarmName") {
                    Some(n) => n,
                    None => return Ok(json_error("ValidationError", "AlarmName is required", 400)),
                };
                let now = Utc::now();
                let alarm = MetricAlarm {
                    alarm_name: alarm_name.clone(),
                    alarm_description: str_param(ctx, "AlarmDescription").unwrap_or_default(),
                    metric_name: str_param(ctx, "MetricName").unwrap_or_default(),
                    namespace: str_param(ctx, "Namespace").unwrap_or_default(),
                    statistic: str_param(ctx, "Statistic").unwrap_or_else(|| "Average".to_string()),
                    period: ctx
                        .request_body
                        .get("Period")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(60),
                    evaluation_periods: ctx
                        .request_body
                        .get("EvaluationPeriods")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(1),
                    threshold: ctx
                        .request_body
                        .get("Threshold")
                        .and_then(|v| v.as_f64())
                        .unwrap_or(0.0),
                    comparison_operator: str_param(ctx, "ComparisonOperator")
                        .unwrap_or_else(|| "GreaterThanThreshold".to_string()),
                    state_value: "INSUFFICIENT_DATA".to_string(),
                    state_reason: "Newly created alarm".to_string(),
                    actions_enabled: ctx
                        .request_body
                        .get("ActionsEnabled")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(true),
                    alarm_actions: ctx
                        .request_body
                        .get("AlarmActions")
                        .and_then(|v| v.as_array())
                        .map(|a| {
                            a.iter()
                                .filter_map(|v| v.as_str().map(String::from))
                                .collect()
                        })
                        .unwrap_or_default(),
                    ok_actions: ctx
                        .request_body
                        .get("OKActions")
                        .and_then(|v| v.as_array())
                        .map(|a| {
                            a.iter()
                                .filter_map(|v| v.as_str().map(String::from))
                                .collect()
                        })
                        .unwrap_or_default(),
                    insufficient_data_actions: vec![],
                    created: now,
                    updated: now,
                };
                let mut store = self.store.get_or_create(account_id, region);
                store.alarms.insert(alarm_name, alarm);
                Ok(json_ok(json!({})))
            }

            // ----------------------------------------------------------------
            // DescribeAlarms
            // ----------------------------------------------------------------
            "DescribeAlarms" => {
                let alarm_name_filter: Vec<String> = ctx
                    .request_body
                    .get("AlarmNames")
                    .and_then(|v| v.as_array())
                    .map(|a| {
                        a.iter()
                            .filter_map(|v| v.as_str().map(String::from))
                            .collect()
                    })
                    .unwrap_or_default();
                let store = self.store.get_or_create(account_id, region);
                let alarms: Vec<Value> = store
                    .alarms
                    .values()
                    .filter(|a| {
                        alarm_name_filter.is_empty() || alarm_name_filter.contains(&a.alarm_name)
                    })
                    .map(|a| {
                        json!({
                            "AlarmName": a.alarm_name,
                            "AlarmDescription": a.alarm_description,
                            "MetricName": a.metric_name,
                            "Namespace": a.namespace,
                            "Statistic": a.statistic,
                            "Period": a.period,
                            "EvaluationPeriods": a.evaluation_periods,
                            "Threshold": a.threshold,
                            "ComparisonOperator": a.comparison_operator,
                            "StateValue": a.state_value,
                            "StateReason": a.state_reason,
                            "ActionsEnabled": a.actions_enabled,
                            "AlarmActions": a.alarm_actions,
                            "OKActions": a.ok_actions,
                            "AlarmArn": format!("arn:aws:cloudwatch:{region}:{account_id}:alarm:{}", a.alarm_name),
                        })
                    })
                    .collect();
                Ok(json_ok(json!({ "MetricAlarms": alarms })))
            }

            // ----------------------------------------------------------------
            // DeleteAlarms
            // ----------------------------------------------------------------
            "DeleteAlarms" => {
                let alarm_names: Vec<String> = ctx
                    .request_body
                    .get("AlarmNames")
                    .and_then(|v| v.as_array())
                    .map(|a| {
                        a.iter()
                            .filter_map(|v| v.as_str().map(String::from))
                            .collect()
                    })
                    .unwrap_or_default();
                let mut store = self.store.get_or_create(account_id, region);
                for name in &alarm_names {
                    store.alarms.remove(name);
                }
                Ok(json_ok(json!({})))
            }

            // ----------------------------------------------------------------
            // SetAlarmState
            // ----------------------------------------------------------------
            "SetAlarmState" => {
                let alarm_name = match str_param(ctx, "AlarmName") {
                    Some(n) => n,
                    None => return Ok(json_error("ValidationError", "AlarmName is required", 400)),
                };
                let state_value = str_param(ctx, "StateValue").unwrap_or_else(|| "OK".to_string());
                let state_reason = str_param(ctx, "StateReason").unwrap_or_default();
                let mut store = self.store.get_or_create(account_id, region);
                if let Some(alarm) = store.alarms.get_mut(&alarm_name) {
                    alarm.state_value = state_value;
                    alarm.state_reason = state_reason;
                    alarm.updated = Utc::now();
                    Ok(json_ok(json!({})))
                } else {
                    Ok(json_error(
                        "ResourceNotFoundException",
                        &format!("Alarm {alarm_name} not found"),
                        404,
                    ))
                }
            }

            // ================================================================
            // CloudWatch LOGS operations
            // ================================================================

            // ----------------------------------------------------------------
            // CreateLogGroup
            // ----------------------------------------------------------------
            "CreateLogGroup" => {
                let log_group_name = match str_param(ctx, "logGroupName") {
                    Some(n) => n,
                    None => {
                        return Ok(json_error(
                            "ValidationError",
                            "logGroupName is required",
                            400,
                        ));
                    }
                };
                let retention = ctx
                    .request_body
                    .get("retentionInDays")
                    .and_then(|v| v.as_u64())
                    .map(|v| v as u32);
                let mut store = self.store.get_or_create(account_id, region);
                if store.log_groups.contains_key(&log_group_name) {
                    return Ok(json_error(
                        "ResourceAlreadyExistsException",
                        &format!("Log group {log_group_name} already exists"),
                        400,
                    ));
                }
                store.log_groups.insert(
                    log_group_name.clone(),
                    LogGroup {
                        log_group_name,
                        retention_in_days: retention,
                        created_at: now_ms(),
                    },
                );
                Ok(json_ok(json!({})))
            }

            // ----------------------------------------------------------------
            // DeleteLogGroup
            // ----------------------------------------------------------------
            "DeleteLogGroup" => {
                let log_group_name = match str_param(ctx, "logGroupName") {
                    Some(n) => n,
                    None => {
                        return Ok(json_error(
                            "ValidationError",
                            "logGroupName is required",
                            400,
                        ));
                    }
                };
                let mut store = self.store.get_or_create(account_id, region);
                if store.log_groups.remove(&log_group_name).is_none() {
                    return Ok(json_error(
                        "ResourceNotFoundException",
                        &format!("Log group {log_group_name} not found"),
                        404,
                    ));
                }
                // Remove associated streams and events
                store.log_streams.retain(|(g, _), _| g != &log_group_name);
                store.log_events.retain(|(g, _), _| g != &log_group_name);
                Ok(json_ok(json!({})))
            }

            // ----------------------------------------------------------------
            // DescribeLogGroups
            // ----------------------------------------------------------------
            "DescribeLogGroups" => {
                let prefix = str_param(ctx, "logGroupNamePrefix");
                let store = self.store.get_or_create(account_id, region);
                let groups: Vec<Value> = store
                    .log_groups
                    .values()
                    .filter(|g| {
                        prefix
                            .as_deref()
                            .map(|p| g.log_group_name.starts_with(p))
                            .unwrap_or(true)
                    })
                    .map(|g| {
                        let mut obj = json!({
                            "logGroupName": g.log_group_name,
                            "creationTime": g.created_at,
                            "arn": format!("arn:aws:logs:{region}:{account_id}:log-group:{}:*", g.log_group_name),
                        });
                        if let Some(r) = g.retention_in_days {
                            obj["retentionInDays"] = json!(r);
                        }
                        obj
                    })
                    .collect();
                Ok(json_ok(json!({ "logGroups": groups })))
            }

            // ----------------------------------------------------------------
            // CreateLogStream
            // ----------------------------------------------------------------
            "CreateLogStream" => {
                let log_group_name = match str_param(ctx, "logGroupName") {
                    Some(n) => n,
                    None => {
                        return Ok(json_error(
                            "ValidationError",
                            "logGroupName is required",
                            400,
                        ));
                    }
                };
                let log_stream_name = match str_param(ctx, "logStreamName") {
                    Some(n) => n,
                    None => {
                        return Ok(json_error(
                            "ValidationError",
                            "logStreamName is required",
                            400,
                        ));
                    }
                };
                let mut store = self.store.get_or_create(account_id, region);
                if !store.log_groups.contains_key(&log_group_name) {
                    return Ok(json_error(
                        "ResourceNotFoundException",
                        &format!("Log group {log_group_name} not found"),
                        404,
                    ));
                }
                let key = (log_group_name.clone(), log_stream_name.clone());
                if store.log_streams.contains_key(&key) {
                    return Ok(json_error(
                        "ResourceAlreadyExistsException",
                        &format!("Log stream {log_stream_name} already exists"),
                        400,
                    ));
                }
                store.log_streams.insert(
                    key,
                    LogStream {
                        log_stream_name,
                        log_group_name,
                        created_at: now_ms(),
                        first_event_timestamp: None,
                        last_event_timestamp: None,
                        upload_sequence_token: 0,
                    },
                );
                Ok(json_ok(json!({})))
            }

            // ----------------------------------------------------------------
            // DescribeLogStreams
            // ----------------------------------------------------------------
            "DescribeLogStreams" => {
                let log_group_name = match str_param(ctx, "logGroupName") {
                    Some(n) => n,
                    None => {
                        return Ok(json_error(
                            "ValidationError",
                            "logGroupName is required",
                            400,
                        ));
                    }
                };
                let prefix = str_param(ctx, "logStreamNamePrefix");
                let store = self.store.get_or_create(account_id, region);
                let streams: Vec<Value> = store
                    .log_streams
                    .values()
                    .filter(|s| {
                        s.log_group_name == log_group_name
                            && prefix
                                .as_deref()
                                .map(|p| s.log_stream_name.starts_with(p))
                                .unwrap_or(true)
                    })
                    .map(|s| {
                        json!({
                            "logStreamName": s.log_stream_name,
                            "creationTime": s.created_at,
                            "uploadSequenceToken": s.upload_sequence_token.to_string(),
                            "arn": format!("arn:aws:logs:{region}:{account_id}:log-group:{}:log-stream:{}", s.log_group_name, s.log_stream_name),
                        })
                    })
                    .collect();
                Ok(json_ok(json!({ "logStreams": streams })))
            }

            // ----------------------------------------------------------------
            // PutLogEvents
            // ----------------------------------------------------------------
            "PutLogEvents" => {
                let log_group_name = match str_param(ctx, "logGroupName") {
                    Some(n) => n,
                    None => {
                        return Ok(json_error(
                            "ValidationError",
                            "logGroupName is required",
                            400,
                        ));
                    }
                };
                let log_stream_name = match str_param(ctx, "logStreamName") {
                    Some(n) => n,
                    None => {
                        return Ok(json_error(
                            "ValidationError",
                            "logStreamName is required",
                            400,
                        ));
                    }
                };
                let key = (log_group_name.clone(), log_stream_name.clone());
                let mut store = self.store.get_or_create(account_id, region);
                if !store.log_streams.contains_key(&key) {
                    return Ok(json_error(
                        "ResourceNotFoundException",
                        &format!("Log stream {log_stream_name} not found"),
                        404,
                    ));
                }
                let ingestion_time = now_ms();
                if let Some(events) = ctx.request_body.get("logEvents").and_then(|v| v.as_array()) {
                    let new_events: Vec<LogEvent> = events
                        .iter()
                        .map(|e| LogEvent {
                            timestamp: e
                                .get("timestamp")
                                .and_then(|v| v.as_i64())
                                .unwrap_or(ingestion_time),
                            message: e
                                .get("message")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string(),
                            ingestion_time,
                        })
                        .collect();
                    let first_ts = new_events.first().map(|e| e.timestamp);
                    let last_ts = new_events.last().map(|e| e.timestamp);
                    store
                        .log_events
                        .entry(key.clone())
                        .or_default()
                        .extend(new_events);
                    if let Some(stream) = store.log_streams.get_mut(&key) {
                        stream.upload_sequence_token += 1;
                        if stream.first_event_timestamp.is_none() {
                            stream.first_event_timestamp = first_ts;
                        }
                        stream.last_event_timestamp = last_ts;
                    }
                }
                let next_token = Uuid::new_v4().to_string();
                Ok(json_ok(json!({ "nextSequenceToken": next_token })))
            }

            // ----------------------------------------------------------------
            // GetLogEvents
            // ----------------------------------------------------------------
            "GetLogEvents" => {
                let log_group_name = match str_param(ctx, "logGroupName") {
                    Some(n) => n,
                    None => {
                        return Ok(json_error(
                            "ValidationError",
                            "logGroupName is required",
                            400,
                        ));
                    }
                };
                let log_stream_name = match str_param(ctx, "logStreamName") {
                    Some(n) => n,
                    None => {
                        return Ok(json_error(
                            "ValidationError",
                            "logStreamName is required",
                            400,
                        ));
                    }
                };
                let key = (log_group_name, log_stream_name);
                let store = self.store.get_or_create(account_id, region);
                let events: Vec<Value> = store
                    .log_events
                    .get(&key)
                    .map(|evts| {
                        evts.iter()
                            .map(|e| {
                                json!({
                                    "timestamp": e.timestamp,
                                    "message": e.message,
                                    "ingestionTime": e.ingestion_time,
                                })
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                let token = Uuid::new_v4().to_string();
                Ok(json_ok(json!({
                    "events": events,
                    "nextForwardToken": token,
                    "nextBackwardToken": token,
                })))
            }

            // ----------------------------------------------------------------
            // FilterLogEvents
            // ----------------------------------------------------------------
            "FilterLogEvents" => {
                let log_group_name = match str_param(ctx, "logGroupName") {
                    Some(n) => n,
                    None => {
                        return Ok(json_error(
                            "ValidationError",
                            "logGroupName is required",
                            400,
                        ));
                    }
                };
                let filter_pattern = str_param(ctx, "filterPattern").unwrap_or_default();
                let store = self.store.get_or_create(account_id, region);
                let mut filtered_events: Vec<Value> = Vec::new();
                for ((group, stream_name), events) in &store.log_events {
                    if group != &log_group_name {
                        continue;
                    }
                    for e in events {
                        if filter_pattern.is_empty() || e.message.contains(&filter_pattern) {
                            filtered_events.push(json!({
                                "logStreamName": stream_name,
                                "timestamp": e.timestamp,
                                "message": e.message,
                                "ingestionTime": e.ingestion_time,
                                "eventId": Uuid::new_v4().to_string(),
                            }));
                        }
                    }
                }
                Ok(json_ok(json!({ "events": filtered_events })))
            }

            // ----------------------------------------------------------------
            // PutRetentionPolicy
            // ----------------------------------------------------------------
            "PutRetentionPolicy" => {
                let log_group_name = match str_param(ctx, "logGroupName") {
                    Some(n) => n,
                    None => {
                        return Ok(json_error(
                            "ValidationError",
                            "logGroupName is required",
                            400,
                        ));
                    }
                };
                let days = ctx
                    .request_body
                    .get("retentionInDays")
                    .and_then(|v| v.as_u64())
                    .map(|v| v as u32);
                let mut store = self.store.get_or_create(account_id, region);
                if let Some(group) = store.log_groups.get_mut(&log_group_name) {
                    group.retention_in_days = days;
                    Ok(json_ok(json!({})))
                } else {
                    Ok(json_error(
                        "ResourceNotFoundException",
                        &format!("Log group {log_group_name} not found"),
                        404,
                    ))
                }
            }

            _ => Err(DispatchError::NotImplemented(ctx.operation.clone())),
        }
    }
}
