use std::collections::HashMap;

use serde_json::Value;

use crate::guided_manifest::{CaptureBinding, FlowAssertion, GuidedFlow, NormalizedOperation};
use crate::history::{InteractionEntry, InteractionHistory};
use crate::protocol_adapters::{
    AdapterError, AdapterExecError, AdapterResponse, execute_protocol_adapter, normalize_error,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GuidedExecutionState {
    Pending,
    Running,
    Succeeded,
    Failed,
    Canceled,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StepOutcome {
    pub step_id: String,
    pub success: bool,
    pub attempts: u8,
    pub status_code: Option<u16>,
    pub error: Option<AdapterError>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CleanupOutcome {
    pub step_id: String,
    pub success: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuidedExecutionReport {
    pub state: GuidedExecutionState,
    pub outcomes: Vec<StepOutcome>,
    pub cleanup: Vec<CleanupOutcome>,
    pub captures: HashMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetryPolicy {
    pub max_attempts: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionPolicy {
    pub retry: RetryPolicy,
    pub step_timeout_ms: u64,
}

impl Default for ExecutionPolicy {
    fn default() -> Self {
        Self {
            retry: RetryPolicy::default(),
            step_timeout_ms: 30_000,
        }
    }
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self { max_attempts: 2 }
    }
}

pub trait GuidedExecutor {
    fn execute(
        &mut self,
        operation: &NormalizedOperation,
    ) -> Result<AdapterResponse, AdapterExecError>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetryEnvelope {
    pub max_attempts: u8,
    pub attempted: u8,
}

impl RetryEnvelope {
    pub fn new(max_attempts: u8) -> Self {
        Self {
            max_attempts,
            attempted: 0,
        }
    }

    pub fn record_attempt(&mut self) {
        self.attempted = self.attempted.saturating_add(1);
    }

    pub fn can_retry(&self) -> bool {
        self.attempted < self.max_attempts
    }
}

#[derive(Debug, Default)]
pub struct BindingContext {
    pub inputs: HashMap<String, String>,
    pub context: HashMap<String, String>,
    pub captures: HashMap<String, String>,
}

pub fn run_guided_flow(
    flow: &GuidedFlow,
    protocol: crate::guided_manifest::ProtocolClass,
    executor: &mut dyn GuidedExecutor,
    history: &mut InteractionHistory,
    binding_ctx: &mut BindingContext,
    retry_policy: RetryPolicy,
) -> GuidedExecutionReport {
    let mut state = GuidedExecutionState::Running;
    let mut outcomes = Vec::new();
    let mut cleanup = Vec::new();
    let mut next_entry_id = 1u64;

    for step in &flow.steps {
        let resolved_operation = resolve_operation(&step.operation, binding_ctx);
        let mut attempts = 0u8;
        let mut final_response = None;
        let mut final_error = None;

        while attempts < retry_policy.max_attempts {
            attempts += 1;
            let response = match executor.execute(&resolved_operation) {
                Ok(response) => response,
                Err(_) => {
                    final_error = Some(AdapterError {
                        code: "execution_error".to_string(),
                        message: "executor failed before response".to_string(),
                        retryable: false,
                    });
                    break;
                }
            };

            let normalized_error = normalize_error(protocol.clone(), &response);
            final_response = Some(response.clone());
            if let Some(error) = normalized_error {
                if error.retryable && attempts < retry_policy.max_attempts {
                    continue;
                }
                final_error = Some(error);
                break;
            }

            let capture_sources = step
                .captures
                .iter()
                .map(|capture| (capture.name.clone(), capture.source.clone()))
                .collect::<HashMap<_, _>>();

            match execute_protocol_adapter(
                protocol.clone(),
                &resolved_operation,
                &response,
                &capture_sources,
            ) {
                Ok(result) => {
                    apply_capture_bindings(&step.captures, &result.captures, binding_ctx);

                    if let Some(assertion_error) = evaluate_assertions(&step.assertions, &response)
                    {
                        final_error = Some(assertion_error);
                    }
                }
                Err(_) => {
                    final_error = Some(AdapterError {
                        code: "adapter_error".to_string(),
                        message: "protocol adapter failed".to_string(),
                        retryable: false,
                    });
                }
            }

            break;
        }

        if let Some(response) = final_response.as_ref() {
            history.push(InteractionEntry {
                id: next_entry_id,
                timestamp_unix_ms: 0,
                service: "guided".to_string(),
                status: response.status,
                request: crate::api::RawRequest {
                    method: resolved_operation.method.clone(),
                    path: resolved_operation.path.clone(),
                    query: resolved_operation.query.clone(),
                    headers: resolved_operation.headers.clone(),
                    body: resolved_operation.body.clone(),
                },
            });
            next_entry_id += 1;
        }

        let success = final_error.is_none();
        outcomes.push(StepOutcome {
            step_id: step.id.clone(),
            success,
            attempts,
            status_code: final_response.as_ref().map(|r| r.status),
            error: final_error.clone(),
        });

        if !success {
            state = GuidedExecutionState::Failed;
            break;
        }
    }

    if !flow.cleanup.is_empty() {
        let cleanup_items = run_cleanup(flow, protocol, executor, binding_ctx);
        cleanup.extend(cleanup_items);
    }

    if state == GuidedExecutionState::Running {
        state = GuidedExecutionState::Succeeded;
    }

    GuidedExecutionReport {
        state,
        outcomes,
        cleanup,
        captures: binding_ctx.captures.clone(),
    }
}

pub fn run_guided_flow_with_policy(
    flow: &GuidedFlow,
    protocol: crate::guided_manifest::ProtocolClass,
    executor: &mut dyn GuidedExecutor,
    history: &mut InteractionHistory,
    binding_ctx: &mut BindingContext,
    policy: ExecutionPolicy,
) -> GuidedExecutionReport {
    let _timeout_ms = policy.step_timeout_ms;
    run_guided_flow(flow, protocol, executor, history, binding_ctx, policy.retry)
}

fn run_cleanup(
    flow: &GuidedFlow,
    protocol: crate::guided_manifest::ProtocolClass,
    executor: &mut dyn GuidedExecutor,
    binding_ctx: &BindingContext,
) -> Vec<CleanupOutcome> {
    let mut outcomes = Vec::new();

    for step in &flow.cleanup {
        let operation = resolve_operation(&step.operation, binding_ctx);
        let success = executor
            .execute(&operation)
            .ok()
            .and_then(|resp| {
                normalize_error(protocol.clone(), &resp)
                    .map(|_| false)
                    .or(Some(true))
            })
            .unwrap_or(false);
        outcomes.push(CleanupOutcome {
            step_id: step.id.clone(),
            success,
        });
    }

    outcomes
}

fn evaluate_assertions(
    assertions: &[FlowAssertion],
    response: &AdapterResponse,
) -> Option<AdapterError> {
    for assertion in assertions {
        let matches = match assertion.kind.as_str() {
            "status" => assertion.expected.parse::<u16>().ok() == Some(response.status),
            "header" => {
                let header_value = response.headers.get(&assertion.target).map(String::as_str);
                header_value == Some(assertion.expected.as_str())
            }
            "body" => response.body.contains(&assertion.expected),
            "json-path" => match serde_json::from_str::<Value>(&response.body) {
                Ok(value) => match extract_json_path(&value, &assertion.target) {
                    Some(found) => found == assertion.expected,
                    None => false,
                },
                Err(_) => false,
            },
            "xml-path" => response.body.contains(&assertion.expected),
            "resource" => response.body.contains(&assertion.expected),
            _ => false,
        };

        if !matches {
            return Some(AdapterError {
                code: "assertion_failed".to_string(),
                message: format!(
                    "assertion '{}' on '{}' expected '{}'",
                    assertion.kind, assertion.target, assertion.expected
                ),
                retryable: false,
            });
        }
    }

    None
}

fn resolve_operation(
    operation: &NormalizedOperation,
    binding_ctx: &BindingContext,
) -> NormalizedOperation {
    NormalizedOperation {
        method: operation.method.clone(),
        path: interpolate_value(&operation.path, binding_ctx),
        headers: operation
            .headers
            .iter()
            .map(|(k, v)| (k.clone(), interpolate_value(v, binding_ctx)))
            .collect(),
        query: operation
            .query
            .iter()
            .map(|(k, v)| (k.clone(), interpolate_value(v, binding_ctx)))
            .collect(),
        body: operation
            .body
            .as_ref()
            .map(|body| interpolate_value(body, binding_ctx)),
    }
}

pub fn apply_capture_bindings(
    captures: &[CaptureBinding],
    extracted: &HashMap<String, String>,
    binding_ctx: &mut BindingContext,
) {
    for capture in captures {
        if let Some(value) = extracted.get(&capture.name) {
            binding_ctx
                .captures
                .insert(capture.name.clone(), value.clone());
        }
    }
}

pub fn validate_expression(expr: &str) -> Result<(), String> {
    if expr.is_empty() {
        return Err("expression must not be empty".to_string());
    }

    if expr.contains(';') || expr.contains('`') || expr.contains("${") {
        return Err("expression contains unsafe token".to_string());
    }

    if expr == "rand8()" || expr == "timestamp()" {
        return Ok(());
    }

    if expr.starts_with("inputs.") || expr.starts_with("context.") || expr.starts_with("captures.")
    {
        return Ok(());
    }

    Err("unsupported expression source".to_string())
}

pub fn interpolate_value(input: &str, binding_ctx: &BindingContext) -> String {
    let mut output = String::new();
    let mut cursor = 0usize;

    while let Some(start_rel) = input[cursor..].find("{{") {
        let start = cursor + start_rel;
        output.push_str(&input[cursor..start]);

        let expr_start = start + 2;
        let Some(end_rel) = input[expr_start..].find("}}") else {
            output.push_str(&input[start..]);
            return output;
        };
        let end = expr_start + end_rel;
        let expr = input[expr_start..end].trim();

        let value = resolve_expression(expr, binding_ctx).unwrap_or_default();
        output.push_str(&value);

        cursor = end + 2;
    }

    output.push_str(&input[cursor..]);
    output
}

fn resolve_expression(expr: &str, binding_ctx: &BindingContext) -> Option<String> {
    if expr == "rand8()" {
        return Some("aaaaaaaa".to_string());
    }

    if expr == "timestamp()" {
        return Some("0".to_string());
    }

    if let Some(path) = expr.strip_prefix("inputs.") {
        return binding_ctx.inputs.get(path).cloned();
    }
    if let Some(path) = expr.strip_prefix("context.") {
        return binding_ctx.context.get(path).cloned();
    }
    if let Some(path) = expr.strip_prefix("captures.") {
        return binding_ctx.captures.get(path).cloned();
    }

    None
}

fn extract_json_path(value: &Value, path: &str) -> Option<String> {
    let mut current = value;
    for segment in path.split('.').filter(|segment| !segment.is_empty()) {
        match current {
            Value::Object(map) => {
                current = map.get(segment)?;
            }
            _ => return None,
        }
    }

    Some(match current {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::guided_manifest::GuidedStep;

    struct FakeExecutor {
        responses: Vec<AdapterResponse>,
        cursor: usize,
    }

    impl GuidedExecutor for FakeExecutor {
        fn execute(
            &mut self,
            _operation: &NormalizedOperation,
        ) -> Result<AdapterResponse, AdapterExecError> {
            let response = self
                .responses
                .get(self.cursor)
                .cloned()
                .unwrap_or(AdapterResponse {
                    status: 500,
                    headers: HashMap::new(),
                    body: "fallback".to_string(),
                });
            self.cursor += 1;
            Ok(response)
        }
    }

    fn simple_flow() -> GuidedFlow {
        GuidedFlow {
            id: "basic".to_string(),
            level: "L1".to_string(),
            steps: vec![GuidedStep {
                id: "create".to_string(),
                title: "Create".to_string(),
                operation: NormalizedOperation {
                    method: "GET".to_string(),
                    path: "/resource/{{inputs.id}}".to_string(),
                    headers: HashMap::new(),
                    query: HashMap::new(),
                    body: None,
                },
                assertions: vec![FlowAssertion {
                    kind: "status".to_string(),
                    target: "status".to_string(),
                    expected: "200".to_string(),
                }],
                captures: vec![CaptureBinding {
                    name: "resource_id".to_string(),
                    source: "Id".to_string(),
                }],
                error_guidance: Some("check id".to_string()),
            }],
            cleanup: vec![GuidedStep {
                id: "cleanup".to_string(),
                title: "Cleanup".to_string(),
                operation: NormalizedOperation {
                    method: "DELETE".to_string(),
                    path: "/resource/{{captures.resource_id}}".to_string(),
                    headers: HashMap::new(),
                    query: HashMap::new(),
                    body: None,
                },
                assertions: vec![],
                captures: vec![],
                error_guidance: None,
            }],
        }
    }

    #[test]
    fn expression_validator_rejects_unsupported_sources() {
        assert!(validate_expression("inputs.bucket").is_ok());
        assert!(validate_expression("captures.id").is_ok());
        assert!(validate_expression("evil()").is_err());
        assert!(validate_expression("inputs.a;rm -rf /").is_err());
    }

    #[test]
    fn interpolation_resolves_inputs_and_captures() {
        let mut ctx = BindingContext::default();
        ctx.inputs
            .insert("bucket".to_string(), "my-bucket".to_string());
        ctx.captures.insert("id".to_string(), "abc123".to_string());

        let value = interpolate_value("/{{inputs.bucket}}/{{captures.id}}", &ctx);
        assert_eq!(value, "/my-bucket/abc123");
    }

    #[test]
    fn guided_flow_runs_and_records_success_and_cleanup() {
        let mut executor = FakeExecutor {
            responses: vec![
                AdapterResponse {
                    status: 200,
                    headers: HashMap::new(),
                    body: "<Id>r-1</Id>".to_string(),
                },
                AdapterResponse {
                    status: 204,
                    headers: HashMap::new(),
                    body: String::new(),
                },
            ],
            cursor: 0,
        };
        let mut history = InteractionHistory::new(10);
        let mut ctx = BindingContext::default();
        ctx.inputs.insert("id".to_string(), "r-1".to_string());

        let report = run_guided_flow(
            &simple_flow(),
            crate::guided_manifest::ProtocolClass::Query,
            &mut executor,
            &mut history,
            &mut ctx,
            RetryPolicy::default(),
        );

        assert_eq!(report.state, GuidedExecutionState::Succeeded);
        assert_eq!(report.outcomes.len(), 1);
        assert!(report.outcomes[0].success);
        assert_eq!(report.cleanup.len(), 1);
        assert!(report.cleanup[0].success);
        assert!(history.replay_request(1).is_some());
    }

    #[test]
    fn guided_flow_marks_failed_assertion_and_runs_cleanup() {
        let mut executor = FakeExecutor {
            responses: vec![
                AdapterResponse {
                    status: 500,
                    headers: HashMap::new(),
                    body: "boom".to_string(),
                },
                AdapterResponse {
                    status: 204,
                    headers: HashMap::new(),
                    body: String::new(),
                },
            ],
            cursor: 0,
        };
        let mut history = InteractionHistory::new(10);
        let mut ctx = BindingContext::default();
        ctx.inputs.insert("id".to_string(), "r-1".to_string());

        let report = run_guided_flow(
            &simple_flow(),
            crate::guided_manifest::ProtocolClass::RestJson,
            &mut executor,
            &mut history,
            &mut ctx,
            RetryPolicy::default(),
        );

        assert_eq!(report.state, GuidedExecutionState::Failed);
        assert_eq!(report.cleanup.len(), 1);
    }
}
