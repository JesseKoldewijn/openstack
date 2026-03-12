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

use crate::store::{Execution, ExecutionStatus, StateMachine, StepFunctionsStore};

pub struct StepFunctionsProvider {
    store: Arc<AccountRegionBundle<StepFunctionsStore>>,
}

impl StepFunctionsProvider {
    pub fn new() -> Self {
        Self {
            store: Arc::new(AccountRegionBundle::new()),
        }
    }
}

impl Default for StepFunctionsProvider {
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

// ---------------------------------------------------------------------------
// ASL interpreter (synchronous, in-process)
// ---------------------------------------------------------------------------

/// Execute an ASL state machine synchronously and return (output_json, error, cause).
fn run_asl(definition: &Value, input_str: &str) -> (String, Option<String>, Option<String>) {
    let input: Value = serde_json::from_str(input_str).unwrap_or(json!({}));

    let states = match definition.get("States").and_then(|v| v.as_object()) {
        Some(s) => s,
        None => return (input_str.to_string(), Some("NoStates".to_string()), None),
    };

    let start_at = match definition.get("StartAt").and_then(|v| v.as_str()) {
        Some(s) => s.to_string(),
        None => return (input_str.to_string(), Some("NoStartAt".to_string()), None),
    };

    let mut current_state_name = start_at;
    let mut current_input = input;
    let mut iterations = 0;
    const MAX_ITER: usize = 1000;

    loop {
        if iterations >= MAX_ITER {
            return (
                serde_json::to_string(&current_input).unwrap_or_default(),
                Some("TooManyIterations".to_string()),
                None,
            );
        }
        iterations += 1;

        let state = match states.get(&current_state_name) {
            Some(s) => s,
            None => {
                return (
                    serde_json::to_string(&current_input).unwrap_or_default(),
                    Some("StateNotFound".to_string()),
                    Some(format!("State {} not found", current_state_name)),
                );
            }
        };

        let state_type = state.get("Type").and_then(|v| v.as_str()).unwrap_or("Pass");
        let is_end = state.get("End").and_then(|v| v.as_bool()).unwrap_or(false);
        let next = state.get("Next").and_then(|v| v.as_str()).map(String::from);

        match state_type {
            "Pass" => {
                // Apply Result if present, otherwise pass input through
                if let Some(result) = state.get("Result") {
                    current_input = result.clone();
                }
                // Apply ResultPath
                if let Some(result_path) = state.get("ResultPath").and_then(|v| v.as_str())
                    && result_path != "$"
                {
                    // Merge into input at path (simplified: only top-level $.key)
                    if let Some(key) = result_path.strip_prefix("$.")
                        && let Some(obj) = current_input.as_object_mut()
                        && let Some(result) = state.get("Result")
                    {
                        obj.insert(key.to_string(), result.clone());
                    }
                }
            }
            "Succeed" => {
                return (
                    serde_json::to_string(&current_input).unwrap_or_default(),
                    None,
                    None,
                );
            }
            "Fail" => {
                let error = state
                    .get("Error")
                    .and_then(|v| v.as_str())
                    .unwrap_or("States.TaskFailed")
                    .to_string();
                let cause = state
                    .get("Cause")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                return (
                    serde_json::to_string(&current_input).unwrap_or_default(),
                    Some(error),
                    Some(cause),
                );
            }
            "Wait" => {
                // In local execution we skip actual waiting
            }
            "Task" => {
                // In local execution Task states just pass through input as output
                // Real implementation would invoke Lambda/Activity
            }
            "Choice" => {
                // Evaluate choices
                let choices = state
                    .get("Choices")
                    .and_then(|v| v.as_array())
                    .cloned()
                    .unwrap_or_default();
                let mut matched = false;
                for choice in &choices {
                    if eval_choice_rule(choice, &current_input)
                        && let Some(next_state) = choice.get("Next").and_then(|v| v.as_str())
                    {
                        current_state_name = next_state.to_string();
                        matched = true;
                        break;
                    }
                }
                if matched {
                    continue;
                }
                // Default
                if let Some(default_state) = state.get("Default").and_then(|v| v.as_str()) {
                    current_state_name = default_state.to_string();
                    continue;
                }
                return (
                    serde_json::to_string(&current_input).unwrap_or_default(),
                    Some("NoChoiceMatched".to_string()),
                    None,
                );
            }
            "Parallel" => {
                // Run branches sequentially in local mode, collect results
                let branches = state
                    .get("Branches")
                    .and_then(|v| v.as_array())
                    .cloned()
                    .unwrap_or_default();
                let mut results = Vec::new();
                for branch in &branches {
                    let (out, err, _cause) = run_asl(
                        branch,
                        &serde_json::to_string(&current_input).unwrap_or_default(),
                    );
                    if err.is_some() {
                        return (out, err, _cause);
                    }
                    let out_val: Value = serde_json::from_str(&out).unwrap_or(json!(null));
                    results.push(out_val);
                }
                current_input = Value::Array(results);
            }
            "Map" => {
                // Iterate over input array
                let items_path = state
                    .get("ItemsPath")
                    .and_then(|v| v.as_str())
                    .unwrap_or("$");
                let iterator = state.get("Iterator").cloned().unwrap_or(json!({}));
                let items = if items_path == "$" {
                    current_input.as_array().cloned().unwrap_or_default()
                } else if let Some(key) = items_path.strip_prefix("$.") {
                    current_input
                        .get(key)
                        .and_then(|v| v.as_array())
                        .cloned()
                        .unwrap_or_default()
                } else {
                    vec![]
                };
                let mut results = Vec::new();
                for item in &items {
                    let item_str = serde_json::to_string(item).unwrap_or_default();
                    let (out, err, cause) = run_asl(&iterator, &item_str);
                    if err.is_some() {
                        return (out, err, cause);
                    }
                    let out_val: Value = serde_json::from_str(&out).unwrap_or(json!(null));
                    results.push(out_val);
                }
                current_input = Value::Array(results);
            }
            _ => {
                // Unknown state type — pass through
            }
        }

        if is_end {
            break;
        }

        match next {
            Some(n) => current_state_name = n,
            None => break,
        }
    }

    (
        serde_json::to_string(&current_input).unwrap_or_default(),
        None,
        None,
    )
}

/// Evaluate a Choice state rule against the current input.
fn eval_choice_rule(rule: &Value, input: &Value) -> bool {
    if let Some(var) = rule.get("Variable").and_then(|v| v.as_str()) {
        let field_val = get_json_path(input, var);

        if let Some(eq) = rule.get("StringEquals") {
            return field_val.as_ref().and_then(|v| v.as_str()) == eq.as_str();
        }
        if let Some(eq) = rule.get("NumericEquals") {
            return field_val.as_ref().and_then(|v| v.as_f64()) == eq.as_f64();
        }
        if let Some(gt) = rule.get("NumericGreaterThan")
            && let (Some(a), Some(b)) = (field_val.as_ref().and_then(|v| v.as_f64()), gt.as_f64())
        {
            return a > b;
        }
        if let Some(lt) = rule.get("NumericLessThan")
            && let (Some(a), Some(b)) = (field_val.as_ref().and_then(|v| v.as_f64()), lt.as_f64())
        {
            return a < b;
        }
        if let Some(expected) = rule.get("BooleanEquals") {
            return field_val.as_ref().and_then(|v| v.as_bool()) == expected.as_bool();
        }
        if let Some(prefix) = rule.get("StringMatches").and_then(|v| v.as_str())
            && let Some(s) = field_val.as_ref().and_then(|v| v.as_str())
        {
            // Simple prefix/suffix/contains matching using * wildcard
            if prefix.starts_with('*') && prefix.ends_with('*') {
                return s.contains(&prefix[1..prefix.len() - 1]);
            } else if let Some(stripped) = prefix.strip_suffix('*') {
                return s.starts_with(stripped);
            } else if let Some(stripped) = prefix.strip_prefix('*') {
                return s.ends_with(stripped);
            } else {
                return s == prefix;
            }
        }
    }

    // Logical combinators
    if let Some(and_rules) = rule.get("And").and_then(|v| v.as_array()) {
        return and_rules.iter().all(|r| eval_choice_rule(r, input));
    }
    if let Some(or_rules) = rule.get("Or").and_then(|v| v.as_array()) {
        return or_rules.iter().any(|r| eval_choice_rule(r, input));
    }
    if let Some(not_rule) = rule.get("Not") {
        return !eval_choice_rule(not_rule, input);
    }

    false
}

/// Simple JSON path resolver (supports $ and $.key and $.key.subkey).
fn get_json_path<'a>(value: &'a Value, path: &str) -> Option<&'a Value> {
    if path == "$" {
        return Some(value);
    }
    let path = path.strip_prefix("$.")?;
    let mut current = value;
    for part in path.split('.') {
        current = current.get(part)?;
    }
    Some(current)
}

// ---------------------------------------------------------------------------
// ServiceProvider
// ---------------------------------------------------------------------------

#[async_trait]
impl ServiceProvider for StepFunctionsProvider {
    fn service_name(&self) -> &str {
        "stepfunctions"
    }

    async fn dispatch(&self, ctx: &RequestContext) -> Result<DispatchResponse, DispatchError> {
        let region = &ctx.region;
        let account_id = &ctx.account_id;

        match ctx.operation.as_str() {
            // ----------------------------------------------------------------
            // CreateStateMachine
            // ----------------------------------------------------------------
            "CreateStateMachine" => {
                let name = match str_param(ctx, "name") {
                    Some(n) => n,
                    None => return Ok(json_error("ValidationException", "name is required", 400)),
                };
                let definition_str = match str_param(ctx, "definition") {
                    Some(d) => d,
                    None => {
                        return Ok(json_error(
                            "ValidationException",
                            "definition is required",
                            400,
                        ));
                    }
                };
                let definition: Value = match serde_json::from_str(&definition_str) {
                    Ok(v) => v,
                    Err(_) => {
                        return Ok(json_error(
                            "InvalidDefinition",
                            "Invalid JSON in definition",
                            400,
                        ));
                    }
                };
                let role_arn = str_param(ctx, "roleArn").unwrap_or_default();
                let machine_type = str_param(ctx, "type").unwrap_or_else(|| "STANDARD".to_string());
                let arn = format!("arn:aws:states:{region}:{account_id}:stateMachine:{name}");
                let now = Utc::now();

                let mut store = self.store.get_or_create(account_id, region);
                store.state_machines.insert(
                    arn.clone(),
                    StateMachine {
                        state_machine_arn: arn.clone(),
                        name,
                        definition,
                        role_arn,
                        status: "ACTIVE".to_string(),
                        machine_type,
                        created: now,
                    },
                );

                Ok(json_ok(json!({
                    "stateMachineArn": arn,
                    "creationDate": now.timestamp(),
                })))
            }

            // ----------------------------------------------------------------
            // DeleteStateMachine
            // ----------------------------------------------------------------
            "DeleteStateMachine" => {
                let arn = match str_param(ctx, "stateMachineArn") {
                    Some(a) => a,
                    None => {
                        return Ok(json_error(
                            "ValidationException",
                            "stateMachineArn is required",
                            400,
                        ));
                    }
                };
                let mut store = self.store.get_or_create(account_id, region);
                store.state_machines.remove(&arn);
                Ok(json_ok(json!({})))
            }

            // ----------------------------------------------------------------
            // DescribeStateMachine
            // ----------------------------------------------------------------
            "DescribeStateMachine" => {
                let arn = match str_param(ctx, "stateMachineArn") {
                    Some(a) => a,
                    None => {
                        return Ok(json_error(
                            "ValidationException",
                            "stateMachineArn is required",
                            400,
                        ));
                    }
                };
                let store = self.store.get_or_create(account_id, region);
                match store.state_machines.get(&arn) {
                    Some(sm) => Ok(json_ok(json!({
                        "stateMachineArn": sm.state_machine_arn,
                        "name": sm.name,
                        "status": sm.status,
                        "definition": serde_json::to_string(&sm.definition).unwrap_or_default(),
                        "roleArn": sm.role_arn,
                        "type": sm.machine_type,
                        "creationDate": sm.created.timestamp(),
                    }))),
                    None => Ok(json_error(
                        "StateMachineDoesNotExist",
                        &format!("State machine {arn} not found"),
                        404,
                    )),
                }
            }

            // ----------------------------------------------------------------
            // ListStateMachines
            // ----------------------------------------------------------------
            "ListStateMachines" => {
                let store = self.store.get_or_create(account_id, region);
                let machines: Vec<Value> = store
                    .state_machines
                    .values()
                    .map(|sm| {
                        json!({
                            "stateMachineArn": sm.state_machine_arn,
                            "name": sm.name,
                            "type": sm.machine_type,
                            "creationDate": sm.created.timestamp(),
                        })
                    })
                    .collect();
                Ok(json_ok(json!({ "stateMachines": machines })))
            }

            // ----------------------------------------------------------------
            // UpdateStateMachine
            // ----------------------------------------------------------------
            "UpdateStateMachine" => {
                let arn = match str_param(ctx, "stateMachineArn") {
                    Some(a) => a,
                    None => {
                        return Ok(json_error(
                            "ValidationException",
                            "stateMachineArn is required",
                            400,
                        ));
                    }
                };
                let mut store = self.store.get_or_create(account_id, region);
                if let Some(sm) = store.state_machines.get_mut(&arn) {
                    if let Some(def_str) = str_param(ctx, "definition")
                        && let Ok(def) = serde_json::from_str(&def_str)
                    {
                        sm.definition = def;
                    }
                    if let Some(role) = str_param(ctx, "roleArn") {
                        sm.role_arn = role;
                    }
                    Ok(json_ok(json!({ "updateDate": Utc::now().timestamp() })))
                } else {
                    Ok(json_error(
                        "StateMachineDoesNotExist",
                        &format!("State machine {arn} not found"),
                        404,
                    ))
                }
            }

            // ----------------------------------------------------------------
            // StartExecution
            // ----------------------------------------------------------------
            "StartExecution" => {
                let sm_arn = match str_param(ctx, "stateMachineArn") {
                    Some(a) => a,
                    None => {
                        return Ok(json_error(
                            "ValidationException",
                            "stateMachineArn is required",
                            400,
                        ));
                    }
                };
                let input_str = str_param(ctx, "input").unwrap_or_else(|| "{}".to_string());
                let exec_name =
                    str_param(ctx, "name").unwrap_or_else(|| Uuid::new_v4().to_string());

                let definition = {
                    let store = self.store.get_or_create(account_id, region);
                    match store.state_machines.get(&sm_arn) {
                        Some(sm) => sm.definition.clone(),
                        None => {
                            return Ok(json_error(
                                "StateMachineDoesNotExist",
                                &format!("State machine {sm_arn} not found"),
                                404,
                            ));
                        }
                    }
                };

                let exec_arn = format!(
                    "arn:aws:states:{region}:{account_id}:execution:{}:{exec_name}",
                    sm_arn.split(':').next_back().unwrap_or("unknown")
                );

                // Run ASL synchronously
                let (output, error, cause) = run_asl(&definition, &input_str);
                let status = if error.is_some() {
                    ExecutionStatus::Failed
                } else {
                    ExecutionStatus::Succeeded
                };
                let now = Utc::now();

                let execution = Execution {
                    execution_arn: exec_arn.clone(),
                    state_machine_arn: sm_arn,
                    name: exec_name,
                    status,
                    input: input_str,
                    output: Some(output),
                    error,
                    cause,
                    started_at: now,
                    stopped_at: Some(now),
                };

                let mut store = self.store.get_or_create(account_id, region);
                store.executions.insert(exec_arn.clone(), execution);

                Ok(json_ok(json!({
                    "executionArn": exec_arn,
                    "startDate": now.timestamp(),
                })))
            }

            // ----------------------------------------------------------------
            // DescribeExecution
            // ----------------------------------------------------------------
            "DescribeExecution" => {
                let exec_arn = match str_param(ctx, "executionArn") {
                    Some(a) => a,
                    None => {
                        return Ok(json_error(
                            "ValidationException",
                            "executionArn is required",
                            400,
                        ));
                    }
                };
                let store = self.store.get_or_create(account_id, region);
                match store.executions.get(&exec_arn) {
                    Some(e) => {
                        let mut obj = json!({
                            "executionArn": e.execution_arn,
                            "stateMachineArn": e.state_machine_arn,
                            "name": e.name,
                            "status": e.status.as_str(),
                            "input": e.input,
                            "startDate": e.started_at.timestamp(),
                        });
                        if let Some(out) = &e.output {
                            obj["output"] = json!(out);
                        }
                        if let Some(err) = &e.error {
                            obj["error"] = json!(err);
                        }
                        if let Some(cause) = &e.cause {
                            obj["cause"] = json!(cause);
                        }
                        if let Some(stopped) = &e.stopped_at {
                            obj["stopDate"] = json!(stopped.timestamp());
                        }
                        Ok(json_ok(obj))
                    }
                    None => Ok(json_error(
                        "ExecutionDoesNotExist",
                        &format!("Execution {exec_arn} not found"),
                        404,
                    )),
                }
            }

            // ----------------------------------------------------------------
            // ListExecutions
            // ----------------------------------------------------------------
            "ListExecutions" => {
                let sm_arn = str_param(ctx, "stateMachineArn");
                let status_filter = str_param(ctx, "statusFilter");
                let store = self.store.get_or_create(account_id, region);
                let executions: Vec<Value> = store
                    .executions
                    .values()
                    .filter(|e| {
                        sm_arn
                            .as_deref()
                            .map(|a| a == e.state_machine_arn)
                            .unwrap_or(true)
                            && status_filter
                                .as_deref()
                                .map(|s| s == e.status.as_str())
                                .unwrap_or(true)
                    })
                    .map(|e| {
                        json!({
                            "executionArn": e.execution_arn,
                            "stateMachineArn": e.state_machine_arn,
                            "name": e.name,
                            "status": e.status.as_str(),
                            "startDate": e.started_at.timestamp(),
                        })
                    })
                    .collect();
                Ok(json_ok(json!({ "executions": executions })))
            }

            // ----------------------------------------------------------------
            // StopExecution
            // ----------------------------------------------------------------
            "StopExecution" => {
                let exec_arn = match str_param(ctx, "executionArn") {
                    Some(a) => a,
                    None => {
                        return Ok(json_error(
                            "ValidationException",
                            "executionArn is required",
                            400,
                        ));
                    }
                };
                let mut store = self.store.get_or_create(account_id, region);
                if let Some(e) = store.executions.get_mut(&exec_arn) {
                    e.status = ExecutionStatus::Aborted;
                    e.stopped_at = Some(Utc::now());
                    Ok(json_ok(json!({ "stopDate": Utc::now().timestamp() })))
                } else {
                    Ok(json_error(
                        "ExecutionDoesNotExist",
                        &format!("Execution {exec_arn} not found"),
                        404,
                    ))
                }
            }

            _ => Err(DispatchError::NotImplemented(ctx.operation.clone())),
        }
    }
}
