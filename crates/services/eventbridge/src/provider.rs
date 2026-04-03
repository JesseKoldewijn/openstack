use std::borrow::Cow;
use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use chrono::Utc;
use openstack_service_framework::traits::{
    CrossServiceDispatcher, DispatchError, DispatchResponse, RequestContext, ResponseBody,
    ServiceProvider,
};
use openstack_service_framework::xml::url_encode;
use openstack_state::AccountRegionBundle;
use serde_json::{Value, json};
use tracing::warn;

use crate::store::{EventBridgeStore, EventBus, EventRule, RuleTarget};

pub struct EventBridgeProvider {
    store: Arc<AccountRegionBundle<EventBridgeStore>>,
    dispatcher: Option<Arc<dyn CrossServiceDispatcher>>,
}

impl EventBridgeProvider {
    pub fn new() -> Self {
        Self {
            store: Arc::new(AccountRegionBundle::new()),
            dispatcher: None,
        }
    }

    pub fn new_with_dispatcher(dispatcher: Arc<dyn CrossServiceDispatcher>) -> Self {
        Self {
            store: Arc::new(AccountRegionBundle::new()),
            dispatcher: Some(dispatcher),
        }
    }
}

impl Default for EventBridgeProvider {
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

// ---------------------------------------------------------------------------
// Event pattern matching
// ---------------------------------------------------------------------------

/// Returns true if the event matches the rule's event_pattern.
///
/// Pattern matching follows the EventBridge pattern specification:
/// - Each field in the pattern must exist in the event
/// - Arrays in pattern mean "any of these values" (OR)
/// - Nested objects recurse
fn event_matches_pattern(event: &Value, pattern: &Option<Value>) -> bool {
    let Some(pat) = pattern else {
        // No pattern = schedule rule, doesn't match PutEvents
        return false;
    };
    matches_value(event, pat)
}

fn matches_value(event: &Value, pattern: &Value) -> bool {
    match (event, pattern) {
        (Value::Object(ev_map), Value::Object(pat_map)) => {
            for (key, pat_val) in pat_map {
                let ev_val = ev_map.get(key).unwrap_or(&Value::Null);
                if !matches_value(ev_val, pat_val) {
                    return false;
                }
            }
            true
        }
        (ev_val, Value::Array(alternatives)) => {
            // Pattern array = OR: event value must equal one of the alternatives.
            // When the event value is itself an array, check if any element matches.
            alternatives.iter().any(|alt| {
                if let Value::Object(condition) = alt {
                    // Condition like {"prefix": "foo"} or {"exists": true}
                    if let Some(prefix) = condition.get("prefix").and_then(|v| v.as_str()) {
                        return ev_val.as_str().is_some_and(|s| s.starts_with(prefix));
                    }
                    if let Some(exists) = condition.get("exists").and_then(|v| v.as_bool()) {
                        return if exists {
                            !matches!(ev_val, Value::Null)
                        } else {
                            matches!(ev_val, Value::Null)
                        };
                    }
                    // Warn on unrecognised condition keys so users know they won't be evaluated.
                    let unknown_keys: Vec<&str> = condition.keys().map(String::as_str).collect();
                    warn!(
                        keys = ?unknown_keys,
                        "EventBridge: unrecognised rule condition key(s) — condition will not match"
                    );
                    return false;
                }
                // If the event value is an array, check if any element matches the alternative
                if let Value::Array(ev_arr) = ev_val {
                    return ev_arr.iter().any(|elem| elem == alt);
                }
                ev_val == alt
            })
        }
        _ => event == pattern,
    }
}

// ---------------------------------------------------------------------------
// Simple URL encoding for form-encoded dispatch bodies
// ---------------------------------------------------------------------------

// simple_url_encode delegates to the shared helper in openstack_service_framework
#[inline(always)]
fn simple_url_encode(s: &str) -> String {
    url_encode(s)
}

// ---------------------------------------------------------------------------
// Target dispatch helper
// ---------------------------------------------------------------------------

async fn dispatch_to_target(
    dispatcher: &dyn CrossServiceDispatcher,
    target_arn: &str,
    account_id: &str,
    region: &str,
    input: Value,
) {
    // ARN format: arn:aws:<service>:<region>:<account>:<resource>
    let parts: Vec<&str> = target_arn.split(':').collect();
    if parts.len() < 6 {
        warn!(target_arn = %target_arn, "EventBridge: target ARN has fewer than 6 segments, skipping dispatch");
        return;
    }
    let service = parts[2]; // e.g. "sqs", "sns", "lambda"

    let ctx = match service {
        "sqs" => {
            // arn:aws:sqs:<region>:<account>:QueueName
            let queue_name = parts.last().unwrap_or(&"");
            let message_body = serde_json::to_string(&input).unwrap_or_default();
            // SQS provider reads from raw_body (form-encoded)
            let body = format!(
                "Action=SendMessage&QueueUrl=http%3A%2F%2Flocalhost%3A4566%2F{account_id}%2F{queue_name}&MessageBody={}",
                simple_url_encode(&message_body),
            );
            let mut ctx = RequestContext::new("sqs", "SendMessage", region, account_id);
            ctx.method = "POST".to_string();
            ctx.path = format!("/{account_id}/{queue_name}");
            ctx.raw_body = Some(Bytes::from(body.into_bytes()));
            ctx
        }
        "sns" => {
            // SNS provider reads from raw_body (form-encoded), not request_body JSON.
            let message = serde_json::to_string(&input).unwrap_or_default();
            let body = format!(
                "Action=Publish&TopicArn={}&Message={}",
                simple_url_encode(target_arn),
                simple_url_encode(&message),
            );
            let mut ctx = RequestContext::new("sns", "Publish", region, account_id);
            ctx.method = "POST".to_string();
            ctx.path = "/".to_string();
            ctx.raw_body = Some(Bytes::from(body.into_bytes()));
            ctx
        }
        "lambda" => {
            // arn:aws:lambda:<region>:<account>:function:<name>
            let function_name = parts.last().unwrap_or(&"");
            let payload = serde_json::to_string(&input).unwrap_or_default();
            let mut ctx = RequestContext::new("lambda", "Invoke", region, account_id);
            ctx.method = "POST".to_string();
            ctx.path = format!("/2015-03-31/functions/{function_name}/invocations");
            ctx.raw_body = Some(Bytes::from(payload.into_bytes()));
            ctx
        }
        _ => {
            warn!(service = %service, target_arn = %target_arn, "EventBridge: unsupported target service");
            return;
        }
    };

    if let Err(e) = dispatcher.dispatch_to(&ctx).await {
        warn!(err = %e, target_arn = %target_arn, "EventBridge: target dispatch failed");
    }
}

// ---------------------------------------------------------------------------
// ServiceProvider
// ---------------------------------------------------------------------------

#[async_trait]
impl ServiceProvider for EventBridgeProvider {
    fn service_name(&self) -> &str {
        "eventbridge"
    }

    async fn dispatch(&self, ctx: &RequestContext) -> Result<DispatchResponse, DispatchError> {
        let region = &ctx.region;
        let account_id = &ctx.account_id;

        match ctx.operation.as_str() {
            // ----------------------------------------------------------------
            // CreateEventBus
            // ----------------------------------------------------------------
            "CreateEventBus" => {
                let name = match str_param(ctx, "Name") {
                    Some(n) => n,
                    None => return Ok(json_error("ValidationError", "Name is required", 400)),
                };
                let arn = format!("arn:aws:events:{region}:{account_id}:event-bus/{name}");
                let mut store = self.store.get_or_create(account_id, region);
                store.buses.insert(
                    name.clone(),
                    EventBus {
                        name: name.clone(),
                        arn: arn.clone(),
                    },
                );
                Ok(json_ok(json!({ "EventBusArn": arn })))
            }

            // ----------------------------------------------------------------
            // DeleteEventBus
            // ----------------------------------------------------------------
            "DeleteEventBus" => {
                let name = match str_param(ctx, "Name") {
                    Some(n) => n,
                    None => return Ok(json_error("ValidationError", "Name is required", 400)),
                };
                if name == "default" {
                    return Ok(json_error(
                        "ValidationError",
                        "Cannot delete default event bus",
                        400,
                    ));
                }
                let mut store = self.store.get_or_create(account_id, region);
                store.buses.remove(&name);
                Ok(json_ok(json!({})))
            }

            // ----------------------------------------------------------------
            // ListEventBuses
            // ----------------------------------------------------------------
            "ListEventBuses" => {
                let store_opt = self.store.get(account_id, region);
                let mut buses: Vec<Value> = store_opt
                    .as_ref()
                    .map(|s| {
                        s.buses
                            .values()
                            .map(|b| json!({ "Name": b.name, "Arn": b.arn }))
                            .collect()
                    })
                    .unwrap_or_default();
                // Always include default
                let has_default = store_opt
                    .as_ref()
                    .map(|s| s.buses.contains_key("default"))
                    .unwrap_or(false);
                if !has_default {
                    buses.push(json!({
                        "Name": "default",
                        "Arn": format!("arn:aws:events:{region}:{account_id}:event-bus/default"),
                    }));
                }
                Ok(json_ok(json!({ "EventBuses": buses })))
            }

            // ----------------------------------------------------------------
            // PutRule
            // ----------------------------------------------------------------
            "PutRule" => {
                let name = match str_param(ctx, "Name") {
                    Some(n) => n,
                    None => return Ok(json_error("ValidationError", "Name is required", 400)),
                };
                let event_bus_name =
                    str_param(ctx, "EventBusName").unwrap_or_else(|| "default".to_string());
                let event_pattern: Option<Value> = ctx
                    .request_body
                    .get("EventPattern")
                    .and_then(|v| v.as_str())
                    .and_then(|s| serde_json::from_str(s).ok());
                let schedule_expression = str_param(ctx, "ScheduleExpression");
                let state = str_param(ctx, "State").unwrap_or_else(|| "ENABLED".to_string());
                let description = str_param(ctx, "Description").unwrap_or_default();
                let arn = format!("arn:aws:events:{region}:{account_id}:rule/{name}");

                let mut store = self.store.get_or_create(account_id, region);
                let rule = store
                    .rules
                    .entry(name.clone())
                    .or_insert_with(|| EventRule {
                        name: name.clone(),
                        event_bus_name: event_bus_name.clone(),
                        event_pattern: None,
                        schedule_expression: None,
                        state: "ENABLED".to_string(),
                        description: String::new(),
                        targets: Default::default(),
                        arn: arn.clone(),
                        created: Utc::now(),
                    });
                rule.event_pattern = event_pattern;
                rule.schedule_expression = schedule_expression;
                rule.state = state;
                rule.description = description;
                rule.event_bus_name = event_bus_name;

                Ok(json_ok(json!({ "RuleArn": arn })))
            }

            // ----------------------------------------------------------------
            // DeleteRule
            // ----------------------------------------------------------------
            "DeleteRule" => {
                let name = match str_param(ctx, "Name") {
                    Some(n) => n,
                    None => return Ok(json_error("ValidationError", "Name is required", 400)),
                };
                let mut store = self.store.get_or_create(account_id, region);
                store.rules.remove(&name);
                Ok(json_ok(json!({})))
            }

            // ----------------------------------------------------------------
            // ListRules
            // ----------------------------------------------------------------
            "ListRules" => {
                let event_bus_name =
                    str_param(ctx, "EventBusName").unwrap_or_else(|| "default".to_string());
                let Some(store) = self.store.get(account_id, region) else {
                    return Ok(json_ok(json!({ "Rules": [] })));
                };
                let rules: Vec<Value> = store
                    .rules
                    .values()
                    .filter(|r| r.event_bus_name == event_bus_name)
                    .map(|r| {
                        let mut obj = json!({
                            "Name": r.name,
                            "Arn": r.arn,
                            "State": r.state,
                            "Description": r.description,
                            "EventBusName": r.event_bus_name,
                        });
                        if let Some(ep) = &r.event_pattern {
                            obj["EventPattern"] =
                                Value::String(serde_json::to_string(ep).unwrap_or_default());
                        }
                        if let Some(se) = &r.schedule_expression {
                            obj["ScheduleExpression"] = json!(se);
                        }
                        obj
                    })
                    .collect();
                Ok(json_ok(json!({ "Rules": rules })))
            }

            // ----------------------------------------------------------------
            // PutTargets
            // ----------------------------------------------------------------
            "PutTargets" => {
                let rule = match str_param(ctx, "Rule") {
                    Some(n) => n,
                    None => return Ok(json_error("ValidationError", "Rule is required", 400)),
                };
                let targets = ctx
                    .request_body
                    .get("Targets")
                    .and_then(|v| v.as_array())
                    .cloned()
                    .unwrap_or_default();

                let mut store = self.store.get_or_create(account_id, region);
                if let Some(r) = store.rules.get_mut(&rule) {
                    for target in &targets {
                        let id = target
                            .get("Id")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        let arn = target
                            .get("Arn")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        let input = target
                            .get("Input")
                            .and_then(|v| v.as_str())
                            .map(String::from);
                        let input_path = target
                            .get("InputPath")
                            .and_then(|v| v.as_str())
                            .map(String::from);
                        r.targets.insert(
                            id.clone(),
                            RuleTarget {
                                id,
                                arn,
                                input,
                                input_path,
                            },
                        );
                    }
                    Ok(json_ok(
                        json!({ "FailedEntryCount": 0, "FailedEntries": [] }),
                    ))
                } else {
                    Ok(json_error(
                        "ResourceNotFoundException",
                        &format!("Rule {rule} not found"),
                        404,
                    ))
                }
            }

            // ----------------------------------------------------------------
            // RemoveTargets
            // ----------------------------------------------------------------
            "RemoveTargets" => {
                let rule = match str_param(ctx, "Rule") {
                    Some(n) => n,
                    None => return Ok(json_error("ValidationError", "Rule is required", 400)),
                };
                let ids: Vec<String> = ctx
                    .request_body
                    .get("Ids")
                    .and_then(|v| v.as_array())
                    .map(|a| {
                        a.iter()
                            .filter_map(|v| v.as_str().map(String::from))
                            .collect()
                    })
                    .unwrap_or_default();
                let mut store = self.store.get_or_create(account_id, region);
                if let Some(r) = store.rules.get_mut(&rule) {
                    for id in &ids {
                        r.targets.remove(id);
                    }
                }
                Ok(json_ok(
                    json!({ "FailedEntryCount": 0, "FailedEntries": [] }),
                ))
            }

            // ----------------------------------------------------------------
            // ListTargetsByRule
            // ----------------------------------------------------------------
            "ListTargetsByRule" => {
                let rule = match str_param(ctx, "Rule") {
                    Some(n) => n,
                    None => return Ok(json_error("ValidationError", "Rule is required", 400)),
                };
                let Some(store) = self.store.get(account_id, region) else {
                    return Ok(json_ok(json!({ "Targets": [] })));
                };
                let targets: Vec<Value> = store
                    .rules
                    .get(&rule)
                    .map(|r| {
                        r.targets
                            .values()
                            .map(|t| json!({ "Id": t.id, "Arn": t.arn }))
                            .collect()
                    })
                    .unwrap_or_default();
                Ok(json_ok(json!({ "Targets": targets })))
            }

            // ----------------------------------------------------------------
            // PutEvents
            // ----------------------------------------------------------------
            "PutEvents" => {
                let entries = match ctx.request_body.get("Entries").and_then(|v| v.as_array()) {
                    Some(e) => e.clone(),
                    None => return Ok(json_error("ValidationError", "Entries is required", 400)),
                };

                let mut result_entries: Vec<Value> = Vec::new();

                // Gather matching rules + targets up front so we can async-dispatch after
                let mut dispatch_targets: Vec<(String, String, Value)> = Vec::new(); // (target_arn, target_id, input)

                {
                    let Some(store) = self.store.get(account_id, region) else {
                        // No rules registered — accept but no routing
                        for _ in &entries {
                            result_entries
                                .push(json!({ "EventId": uuid::Uuid::new_v4().to_string() }));
                        }
                        return Ok(json_ok(json!({
                            "FailedEntryCount": 0,
                            "Entries": result_entries,
                        })));
                    };

                    for entry in &entries {
                        let event_id = uuid::Uuid::new_v4().to_string();
                        result_entries.push(json!({ "EventId": event_id }));

                        // Determine which bus this event targets
                        let bus_name = entry
                            .get("EventBusName")
                            .and_then(|v| v.as_str())
                            .unwrap_or("default")
                            .to_string();
                        let source = entry.get("Source").and_then(|v| v.as_str()).unwrap_or("");
                        let detail_type = entry
                            .get("DetailType")
                            .and_then(|v| v.as_str())
                            .unwrap_or("");

                        // Detail arrives as a JSON string in PutEvents; parse it so
                        // rules matching detail.* fields can fire correctly.
                        let detail = entry
                            .get("Detail")
                            .and_then(|v| v.as_str())
                            .and_then(|s| serde_json::from_str(s).ok())
                            .unwrap_or(Value::Object(Default::default()));

                        // Build the full CloudWatch event envelope
                        let event_envelope = json!({
                            "id": event_id,
                            "version": "0",
                            "account": account_id,
                            "region": region,
                            "source": source,
                            "detail-type": detail_type,
                            "detail": detail,
                            "time": Utc::now().to_rfc3339(),
                        });

                        // Match against enabled rules on this bus (exact bus match only)
                        for rule in store.rules.values() {
                            if rule.state != "ENABLED" {
                                continue;
                            }
                            if rule.event_bus_name != bus_name {
                                continue;
                            }
                            if !event_matches_pattern(&event_envelope, &rule.event_pattern) {
                                continue;
                            }
                            for target in rule.targets.values() {
                                let input = if let Some(literal) = &target.input {
                                    serde_json::from_str(literal).unwrap_or_else(|_| json!(literal))
                                } else {
                                    event_envelope.clone()
                                };
                                dispatch_targets.push((
                                    target.arn.clone(),
                                    target.id.clone(),
                                    input,
                                ));
                            }
                        }
                    }
                }

                // Dispatch to targets via cross-service dispatcher (fire-and-forget)
                if let Some(dispatcher) = &self.dispatcher {
                    for (target_arn, _target_id, input) in dispatch_targets {
                        let dispatcher = Arc::clone(dispatcher);
                        let account_id = account_id.to_string();
                        let region = region.to_string();
                        tokio::spawn(async move {
                            dispatch_to_target(
                                &*dispatcher,
                                &target_arn,
                                &account_id,
                                &region,
                                input,
                            )
                            .await;
                        });
                    }
                }

                Ok(json_ok(json!({
                    "FailedEntryCount": 0u32,
                    "Entries": result_entries,
                })))
            }

            // ----------------------------------------------------------------
            // DescribeRule
            // ----------------------------------------------------------------
            "DescribeRule" => {
                let name = match str_param(ctx, "Name") {
                    Some(n) => n,
                    None => return Ok(json_error("ValidationError", "Name is required", 400)),
                };
                let Some(store) = self.store.get(account_id, region) else {
                    return Ok(json_error(
                        "ResourceNotFoundException",
                        &format!("Rule {name} not found"),
                        404,
                    ));
                };
                match store.rules.get(&name) {
                    Some(r) => {
                        let mut obj = json!({
                            "Name": r.name,
                            "Arn": r.arn,
                            "State": r.state,
                            "Description": r.description,
                            "EventBusName": r.event_bus_name,
                        });
                        if let Some(ep) = &r.event_pattern {
                            obj["EventPattern"] =
                                Value::String(serde_json::to_string(ep).unwrap_or_default());
                        }
                        if let Some(se) = &r.schedule_expression {
                            obj["ScheduleExpression"] = json!(se);
                        }
                        Ok(json_ok(obj))
                    }
                    None => Ok(json_error(
                        "ResourceNotFoundException",
                        &format!("Rule {name} not found"),
                        404,
                    )),
                }
            }

            // ----------------------------------------------------------------
            // EnableRule / DisableRule
            // ----------------------------------------------------------------
            "EnableRule" => {
                let name = str_param(ctx, "Name").unwrap_or_default();
                let mut store = self.store.get_or_create(account_id, region);
                if let Some(r) = store.rules.get_mut(&name) {
                    r.state = "ENABLED".to_string();
                }
                Ok(json_ok(json!({})))
            }

            "DisableRule" => {
                let name = str_param(ctx, "Name").unwrap_or_default();
                let mut store = self.store.get_or_create(account_id, region);
                if let Some(r) = store.rules.get_mut(&name) {
                    r.state = "DISABLED".to_string();
                }
                Ok(json_ok(json!({})))
            }

            _ => Err(DispatchError::NotImplemented(ctx.operation.clone())),
        }
    }

    async fn storage_snapshot(&self) -> Option<serde_json::Value> {
        use serde_json::json;
        let mut buses = Vec::new();
        for entry in self.store.iter() {
            let store = entry.value();
            for bus in store.buses.values() {
                let rule_count = store
                    .rules
                    .values()
                    .filter(|r| r.event_bus_name == bus.name)
                    .count();
                buses.push(json!({
                    "id": bus.arn.clone(),
                    "kind": "event_bus",
                    "attributes": [
                        {"key": "name", "value": bus.name.clone()},
                        {"key": "rule_count", "value": rule_count.to_string()},
                    ]
                }));
            }
        }
        Some(json!({ "kind": "event_bridge", "buses": buses }))
    }
}
