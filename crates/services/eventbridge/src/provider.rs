use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use chrono::Utc;
use openstack_service_framework::traits::{
    DispatchError, DispatchResponse, RequestContext, ResponseBody, ServiceProvider,
};
use openstack_state::AccountRegionBundle;
use serde_json::{Value, json};

use crate::store::{EventBridgeStore, EventBus, EventRule, RuleTarget};

pub struct EventBridgeProvider {
    store: Arc<AccountRegionBundle<EventBridgeStore>>,
}

impl EventBridgeProvider {
    pub fn new() -> Self {
        Self {
            store: Arc::new(AccountRegionBundle::new()),
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

/// Very simple event pattern matching.
/// Checks that each key in the pattern matches the event.
fn matches_pattern(event: &Value, pattern: &Value) -> bool {
    let pattern_obj = match pattern.as_object() {
        Some(o) => o,
        None => return true,
    };
    for (key, pattern_val) in pattern_obj {
        let event_val = event.get(key);
        if !matches_field(event_val, pattern_val) {
            return false;
        }
    }
    true
}

fn matches_field(event_val: Option<&Value>, pattern_val: &Value) -> bool {
    match pattern_val {
        Value::Array(items) => {
            // Pattern array = list of allowed values
            if let Some(ev) = event_val {
                items.iter().any(|item| item == ev)
            } else {
                false
            }
        }
        Value::Object(_) => {
            // Nested object pattern — recurse
            if let Some(ev) = event_val {
                matches_pattern(ev, pattern_val)
            } else {
                false
            }
        }
        _ => event_val.map(|ev| ev == pattern_val).unwrap_or(false),
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
                let store = self.store.get_or_create(account_id, region);
                let mut buses: Vec<Value> = store
                    .buses
                    .values()
                    .map(|b| json!({ "Name": b.name, "Arn": b.arn }))
                    .collect();
                // Always include default
                if !store.buses.contains_key("default") {
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
                let store = self.store.get_or_create(account_id, region);
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
                let store = self.store.get_or_create(account_id, region);
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
                let entries = ctx
                    .request_body
                    .get("Entries")
                    .and_then(|v| v.as_array())
                    .cloned()
                    .unwrap_or_default();

                let store = self.store.get_or_create(account_id, region);

                // For each event, find matching rules and (in-process) dispatch to targets
                // For simplicity: we record success for all entries
                let results: Vec<Value> = entries
                    .iter()
                    .map(|entry| {
                        let source = entry.get("Source").and_then(|v| v.as_str()).unwrap_or("");
                        let detail_type = entry
                            .get("DetailType")
                            .and_then(|v| v.as_str())
                            .unwrap_or("");
                        let detail: Value = entry
                            .get("Detail")
                            .and_then(|v| v.as_str())
                            .and_then(|s| serde_json::from_str(s).ok())
                            .unwrap_or(json!({}));

                        // Find matching rules
                        let event_obj = json!({
                            "source": source,
                            "detail-type": detail_type,
                            "detail": detail,
                        });

                        let _matched_rules: Vec<&str> = store
                            .rules
                            .values()
                            .filter(|r| {
                                r.state == "ENABLED"
                                    && r.event_pattern
                                        .as_ref()
                                        .map(|p| matches_pattern(&event_obj, p))
                                        .unwrap_or(false)
                            })
                            .map(|r| r.name.as_str())
                            .collect();

                        // Note: actual target dispatch (SQS, Lambda, SNS) would be done here
                        // For test compatibility we just record success
                        json!({
                            "EventId": uuid::Uuid::new_v4().to_string(),
                        })
                    })
                    .collect();

                Ok(json_ok(json!({
                    "FailedEntryCount": 0,
                    "Entries": results,
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
                let store = self.store.get_or_create(account_id, region);
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
}
