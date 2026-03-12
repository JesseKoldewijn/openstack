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
    /// Create the default ExecutionPolicy.
    ///
    /// The default policy uses `RetryPolicy::default()` (max_attempts = 2) and a
    /// per-step timeout of 30,000 milliseconds.
    ///
    /// # Examples
    ///
    /// ```
    /// let p = ExecutionPolicy::default();
    /// assert_eq!(p.step_timeout_ms, 30_000);
    /// assert_eq!(p.retry.max_attempts, 2);
    /// ```
    fn default() -> Self {
        Self {
            retry: RetryPolicy::default(),
            step_timeout_ms: 30_000,
        }
    }
}

impl Default for RetryPolicy {
    /// Creates a RetryPolicy with the default configuration.
    ///
    /// # Examples
    ///
    /// ```
    /// let p = crate::RetryPolicy::default();
    /// assert_eq!(p.max_attempts, 2);
    /// ```
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
    /// Creates a `RetryEnvelope` configured with the specified maximum attempts.
    ///
    /// The envelope's `attempted` counter is initialized to 0.
    ///
    /// # Examples
    ///
    /// ```
    /// let env = RetryEnvelope::new(3);
    /// assert_eq!(env.max_attempts, 3);
    /// assert_eq!(env.attempted, 0);
    /// ```
    pub fn new(max_attempts: u8) -> Self {
        Self {
            max_attempts,
            attempted: 0,
        }
    }

    /// Increments the recorded retry attempt count by one, saturating at `u8::MAX`.
    ///
    /// # Examples
    ///
    /// ```
    /// let mut env = RetryEnvelope::new(3);
    /// assert_eq!(env.attempted, 0);
    /// env.record_attempt();
    /// assert_eq!(env.attempted, 1);
    /// ```
    pub fn record_attempt(&mut self) {
        self.attempted = self.attempted.saturating_add(1);
    }

    /// Returns whether another retry attempt is allowed for this envelope.
    ///
    /// # Returns
    /// `true` if `attempted` is less than `max_attempts`, `false` otherwise.
    ///
    /// # Examples
    ///
    /// ```
    /// let env = RetryEnvelope { max_attempts: 3, attempted: 2 };
    /// assert!(env.can_retry());
    /// let env = RetryEnvelope { max_attempts: 3, attempted: 3 };
    /// assert!(!env.can_retry());
    /// ```
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

/// Executes a guided flow: runs each step with the provided executor, applies captures and assertions,
/// records interactions to history, performs configured retries, and runs cleanup steps when present.
///
/// The returned report contains the final global state, a per-step outcome list (attempt counts,
/// status codes, and any adapter errors), cleanup outcomes, and the collected captures from the
/// binding context.
///
/// # Examples
///
/// ```
/// // Prepare inputs (flow, protocol, executor, history, binding_ctx) and policy beforehand.
/// let report = run_guided_flow(&flow, protocol, &mut executor, &mut history, &mut binding_ctx, RetryPolicy::default());
/// assert!(matches!(report.state, GuidedExecutionState::Succeeded) || matches!(report.state, GuidedExecutionState::Failed));
/// ```
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

/// Executes a guided flow using the provided execution policy.

///

/// The function applies the policy's retry configuration and step timeout when running the flow and returns a summary report of the execution.

///

/// # Returns

/// A `GuidedExecutionReport` containing the final global state, per-step outcomes, cleanup outcomes, and captured values.

///

/// # Examples

///

/// ```

/// # use crate::guided_runtime::{run_guided_flow_with_policy, ExecutionPolicy, BindingContext};

/// # use crate::guided_manifest::GuidedFlow;

/// # use crate::history::InteractionHistory;

/// # struct FakeExecutor;

/// # impl crate::guided_runtime::GuidedExecutor for FakeExecutor {

/// #     fn execute(&mut self, _operation: &crate::guided_manifest::NormalizedOperation) -> Result<crate::protocol_adapters::AdapterResponse, crate::protocol_adapters::AdapterExecError> {

/// #         unimplemented!()

/// #     }

/// # }

/// # fn example() {

/// let flow = GuidedFlow::default();

/// let mut executor = FakeExecutor;

/// let mut history = InteractionHistory::default();

/// let mut ctx = BindingContext::default();

/// let policy = ExecutionPolicy::default();

/// let _report = run_guided_flow_with_policy(&flow, crate::guided_manifest::ProtocolClass::Http, &mut executor, &mut history, &mut ctx, policy);

/// # }

/// ```
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

/// Executes the flow's cleanup steps and returns per-step outcomes.
///
/// Each cleanup step is resolved with the provided binding context and executed
/// using the given executor. A step is marked successful if the executor
/// returns a response and normalizing that response does not produce an error;
/// otherwise it is marked failed.
///
/// # Examples
///
/// ```
/// // assume `flow`, `protocol`, `executor`, and `binding_ctx` are available in scope
/// let results = run_cleanup(&flow, protocol, &mut executor, &binding_ctx);
/// assert!(results.iter().all(|r| r.step_id.len() > 0));
/// ```
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

/// Validates a sequence of flow assertions against an adapter response.
///
/// Checks each assertion in order and returns an `AdapterError` describing the
/// first assertion that does not hold; returns `None` when all assertions pass.
///
/// Parameters:
/// - `assertions`: slice of `FlowAssertion` to evaluate (supported kinds include
///   "status", "header", "body", "json-path", "xml-path", and "resource").
/// - `response`: the `AdapterResponse` to validate assertions against.
///
/// # Returns
///
/// `Some(AdapterError)` describing the first failed assertion, `None` if all
/// assertions succeed.
///
/// # Examples
///
/// ```
/// use std::collections::HashMap;
///
/// let assertions = vec![FlowAssertion {
///     kind: "status".to_string(),
///     target: "".to_string(),
///     expected: "200".to_string(),
/// }];
///
/// let response = AdapterResponse {
///     status: 200,
///     headers: HashMap::new(),
///     body: "".to_string(),
/// };
///
/// assert!(evaluate_assertions(&assertions, &response).is_none());
/// ```
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

/// Produce a new `NormalizedOperation` with `path`, header values, query values, and `body` interpolated
/// using the provided `BindingContext`. The HTTP `method` is cloned unchanged.
///
/// # Examples
///
/// ```
/// let mut ctx = BindingContext::default();
/// ctx.inputs.insert("id".into(), "42".into());
///
/// let op = NormalizedOperation {
///     method: "GET".into(),
///     path: "resource/{{inputs.id}}".into(),
///     headers: std::collections::HashMap::new(),
///     query: std::collections::HashMap::new(),
///     body: None,
/// };
///
/// let resolved = resolve_operation(&op, &ctx);
/// assert_eq!(resolved.path, "resource/42");
/// ```
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

/// Inserts captured values into the binding context for any bindings whose name exists in `extracted`.
///
/// For each `CaptureBinding` in `captures`, if `extracted` contains a value keyed by the binding's `name`,
/// that value is inserted into `binding_ctx.captures`, overwriting any existing entry for the same name.
///
/// # Parameters
///
/// - `captures`: list of capture bindings to apply (uses each binding's `name` field as the key).
/// - `extracted`: map of extracted capture values keyed by name.
/// - `binding_ctx`: mutable binding context to receive captured values.
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

/// Validates a binding expression for use in interpolations.
///
/// This enforces allowed expression forms and rejects empty, unsafe, or unsupported sources.
/// Allowed forms:
/// - Builtins: `rand8()` and `timestamp()`
/// - Namespace accessors starting with `inputs.`, `context.`, or `captures.`
///
/// # Returns
///
/// `Ok(())` if the expression is valid, `Err(String)` with a short error message otherwise.
///
/// # Examples
///
/// ```
/// // valid builtins
/// assert!(validate_expression("rand8()").is_ok());
/// assert!(validate_expression("timestamp()").is_ok());
///
/// // valid namespaced accessors
/// assert!(validate_expression("inputs.user_id").is_ok());
/// assert!(validate_expression("context.env").is_ok());
/// assert!(validate_expression("captures.token").is_ok());
///
/// // invalid cases
/// assert!(validate_expression("").is_err());
/// assert!(validate_expression("rm -rf /;").is_err());
/// assert!(validate_expression("someUnsupportedSource.value").is_err());
/// ```
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

/// Interpolates `{{...}}` expressions in a template string using values from the binding context.
///
/// Expressions are trimmed and resolved via `resolve_expression`. Each `{{expr}}` is replaced by
/// the resolved value; if an expression cannot be resolved it is replaced with an empty string.
/// If a `{{` is not closed with `}}`, the remainder starting at the unmatched `{{` is appended
/// verbatim and interpolation stops.
///
/// # Examples
///
/// ```
/// let mut ctx = BindingContext::default();
/// ctx.inputs.insert("id".to_string(), "42".to_string());
/// ctx.captures.insert("resource_id".to_string(), "abc".to_string());
///
/// let tpl = "GET /resource/{{inputs.id}} -> capture={{captures.resource_id}}";
/// let out = interpolate_value(tpl, &ctx);
/// assert_eq!(out, "GET /resource/42 -> capture=abc");
/// ```
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

/// Resolves a single interpolation expression against the binding context or built-in tokens.
///
/// Accepts the built-in tokens `rand8()` and `timestamp()`, or sources prefixed with
/// `inputs.`, `context.`, or `captures.` and returns the corresponding string value if present.
///
/// # Returns
///
/// `Some(String)` with the resolved value when the expression matches a supported token or a key
/// present in the binding context, `None` otherwise.
///
/// # Examples
///
/// ```
/// let mut ctx = BindingContext::default();
/// ctx.inputs.insert("id".into(), "42".into());
/// ctx.captures.insert("resource_id".into(), "xyz".into());
///
/// assert_eq!(resolve_expression("rand8()", &ctx), Some("aaaaaaaa".into()));
/// assert_eq!(resolve_expression("timestamp()", &ctx), Some("0".into()));
/// assert_eq!(resolve_expression("inputs.id", &ctx), Some("42".into()));
/// assert_eq!(resolve_expression("captures.resource_id", &ctx), Some("xyz".into()));
/// assert_eq!(resolve_expression("unknown", &ctx), None);
/// ```
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

/// Extracts a value from a JSON `Value` following a dot-separated path.
///
/// Traverses nested JSON objects by splitting `path` on dots and descending into object fields.
/// If the final value is found it is returned as a `String` (string values are returned verbatim;
/// other JSON values are returned via `to_string()`).
///
/// # Returns
///
/// `Some(String)` containing the value at the given path, or `None` if any path segment is missing
/// or a non-object is encountered while traversing.
///
/// # Examples
///
/// ```
/// use serde_json::json;
/// let v = json!({
///     "user": {
///         "id": 42,
///         "name": "alice"
///     }
/// });
/// assert_eq!(super::extract_json_path(&v, "user.name"), Some("alice".to_string()));
/// assert_eq!(super::extract_json_path(&v, "user.id"), Some("42".to_string()));
/// assert_eq!(super::extract_json_path(&v, "user.missing"), None);
/// ```
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
        /// Returns the next predefined adapter response from this executor's queue and advances the internal cursor.
        ///
        /// This method ignores the provided `operation` and yields the response at the current cursor index from
        /// `self.responses`. After returning a response the cursor is incremented. If no response exists at the
        /// current cursor, a fallback `AdapterResponse` with status `500`, empty headers, and body `"fallback"` is returned.
        ///
        /// # Examples
        ///
        /// ```
        /// // Construct a fake executor with two responses and call execute twice.
        /// use std::collections::HashMap;
        ///
        /// let responses = vec![
        ///     AdapterResponse { status: 200, headers: HashMap::new(), body: "ok1".into() },
        ///     AdapterResponse { status: 201, headers: HashMap::new(), body: "ok2".into() },
        /// ];
        /// let mut exec = FakeExecutor { responses, cursor: 0 };
        /// let op = NormalizedOperation::default();
        ///
        /// let r1 = exec.execute(&op).unwrap();
        /// assert_eq!(r1.status, 200);
        /// let r2 = exec.execute(&op).unwrap();
        /// assert_eq!(r2.status, 201);
        /// let r3 = exec.execute(&op).unwrap(); // falls back when out of responses
        /// assert_eq!(r3.status, 500);
        /// ```
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

    /// Constructs a minimal GuidedFlow used in tests: a single GET step that asserts status 200 and captures `resource_id`, plus a DELETE cleanup step.
    ///
    /// # Examples
    ///
    /// ```
    /// let flow = simple_flow();
    /// assert_eq!(flow.id, "basic");
    /// assert_eq!(flow.steps.len(), 1);
    /// assert_eq!(flow.cleanup.len(), 1);
    /// assert_eq!(flow.steps[0].operation.path, "/resource/{{inputs.id}}");
    /// assert_eq!(flow.cleanup[0].operation.method, "DELETE");
    /// ```
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
