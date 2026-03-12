use std::collections::{BTreeSet, HashMap};

use crate::api::{RawRequest, RawResponse};
use crate::console::RawConsoleState;
use crate::guided_manifest::{GuidedFlow, GuidedManifest};
use crate::guided_renderer::{
    GuidedUxState, RenderedGuidedFlow, map_ux_state, render_guided_flow, replay_from_history,
    validate_guided_inputs,
};
use crate::guided_runtime::{
    BindingContext, GuidedExecutionReport, GuidedExecutor, RetryPolicy, run_guided_flow,
};
use crate::history::InteractionHistory;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceWorkspaceState {
    pub service: String,
    pub selected_flow_id: Option<String>,
    pub guided_inputs: HashMap<String, String>,
    pub guided_required_fields: Vec<String>,
    pub guided_state: GuidedUxState,
    pub guided_report: Option<GuidedExecutionReport>,
    pub guided_render: Option<RenderedGuidedFlow>,
    pub raw_console: RawConsoleState,
    pub replay_interaction_id: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkspaceError {
    ServiceMismatch,
    FlowNotFound,
    MissingGuidedInputs(Vec<String>),
}

impl ServiceWorkspaceState {
    /// Creates a new ServiceWorkspaceState for the specified service with all fields set to their defaults.
    ///
    /// The returned state has no selected flow, no guided inputs, UI state set to `Idle`, no execution report or render,
    /// a default `RawConsoleState`, and no replay interaction id.
    ///
    /// # Examples
    ///
    /// ```
    /// let _state = ServiceWorkspaceState::new("my-service");
    /// ```
    pub fn new(service: impl Into<String>) -> Self {
        Self {
            service: service.into(),
            selected_flow_id: None,
            guided_inputs: HashMap::new(),
            guided_required_fields: Vec::new(),
            guided_state: GuidedUxState::Idle,
            guided_report: None,
            guided_render: None,
            raw_console: RawConsoleState::default(),
            replay_interaction_id: None,
        }
    }

    /// Selects a guided flow from the provided manifest and updates the workspace's required input fields.
    ///
    /// If `flow_id` is Some and matches a flow in `manifest`, that flow is selected. If `flow_id` is None
    /// or no matching flow is found, the first flow in `manifest` (if any) is selected. If the manifest
    /// contains no flows, the selection is cleared and required inputs are set to an empty list.
    ///
    /// # Parameters
    ///
    /// - `manifest`: the manifest containing available guided flows.
    /// - `flow_id`: optional identifier of the flow to select; when omitted, the first flow in the manifest is used.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// let mut state = ServiceWorkspaceState::new("service-a");
    /// let manifest = /* obtain a GuidedManifest with at least one flow */ unimplemented!();
    /// state.select_flow(&manifest, None); // selects the first flow and derives required inputs
    /// assert!(state.selected_flow_id.is_some());
    /// ```
    pub fn select_flow(&mut self, manifest: &GuidedManifest, flow_id: Option<&str>) {
        let target_flow = flow_id
            .and_then(|id| manifest.flows.iter().find(|flow| flow.id == id))
            .or_else(|| manifest.flows.first());

        self.selected_flow_id = target_flow.map(|flow| flow.id.clone());
        self.guided_required_fields = target_flow.map_or_else(Vec::new, required_inputs);
    }

    /// Sets or updates a guided input value for the workspace.
    ///
    /// # Examples
    ///
    /// ```
    /// let mut state = ServiceWorkspaceState::new("my-service");
    /// state.set_guided_input("bucket", "demo-bucket");
    /// assert_eq!(state.guided_inputs.get("bucket").map(String::as_str), Some("demo-bucket"));
    /// ```
    pub fn set_guided_input(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.guided_inputs.insert(key.into(), value.into());
    }

    /// Executes the selected guided flow from `manifest`, runs it through `executor`, updates the workspace UX state and rendered flow, and stores the execution report.
    ///
    /// This function validates that the manifest's service matches the workspace, chooses the active flow (the currently selected flow if present, otherwise the manifest's first flow), verifies that all required guided inputs are present, then runs the flow. On success the workspace's `guided_state`, `guided_render`, `guided_report`, and `selected_flow_id` are updated to reflect the execution result.
    ///
    /// Error cases:
    /// - `WorkspaceError::ServiceMismatch` if `manifest.service` does not equal this workspace's `service`.
    /// - `WorkspaceError::FlowNotFound` if no flow can be selected from the manifest.
    /// - `WorkspaceError::MissingGuidedInputs(Vec<String>)` if required input fields are missing; the vector contains the missing field names.
    ///
    /// # Returns
    ///
    /// A reference to the `GuidedExecutionReport` produced by the executed flow.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use crate::workspace::ServiceWorkspaceState;
    /// # use crate::guided_manifest::GuidedManifest;
    /// # use crate::guided_runtime::{GuidedExecutor, RetryPolicy};
    /// # use crate::history::InteractionHistory;
    /// // Assume `state`, `manifest`, `executor`, and `history` are initialized.
    /// // let mut state = ServiceWorkspaceState::new("my-service");
    /// // let mut executor: Box<dyn GuidedExecutor> = ...;
    /// // let mut history = InteractionHistory::new();
    /// // let retry = RetryPolicy::default();
    /// // state.select_flow(&manifest, None);
    /// // match state.execute_guided(&manifest, executor.as_mut(), &mut history, retry) {
    /// //     Ok(report) => println!("Execution succeeded: {:?}", report),
    /// //     Err(err) => eprintln!("Execution failed: {:?}", err),
    /// // }
    /// ```
    pub fn execute_guided(
        &mut self,
        manifest: &GuidedManifest,
        executor: &mut dyn GuidedExecutor,
        history: &mut InteractionHistory,
        retry_policy: RetryPolicy,
    ) -> Result<&GuidedExecutionReport, WorkspaceError> {
        if manifest.service != self.service {
            return Err(WorkspaceError::ServiceMismatch);
        }

        let flow = self
            .selected_flow_id
            .as_deref()
            .and_then(|id| manifest.flows.iter().find(|item| item.id == id))
            .or_else(|| manifest.flows.first())
            .ok_or(WorkspaceError::FlowNotFound)?;

        let required = required_inputs(flow);
        if let Err(missing) = validate_guided_inputs(&required, &self.guided_inputs) {
            return Err(WorkspaceError::MissingGuidedInputs(missing));
        }

        self.guided_state = GuidedUxState::Running;
        self.guided_required_fields = required;

        let mut bindings = BindingContext {
            inputs: self.guided_inputs.clone(),
            context: HashMap::new(),
            captures: HashMap::new(),
        };

        let report = run_guided_flow(
            flow,
            manifest.protocol.clone(),
            executor,
            history,
            &mut bindings,
            retry_policy,
        );

        self.guided_state = map_ux_state(Some(&report));
        self.guided_render = Some(render_guided_flow(manifest, flow, Some(&report)));
        self.guided_report = Some(report);
        self.selected_flow_id = Some(flow.id.clone());

        Ok(self
            .guided_report
            .as_ref()
            .expect("guided report should exist after execution"))
    }

    /// Loads an interaction from history into the raw console and records the replay ID.
    ///
    /// If an interaction with `interaction_id` exists in `history`, applies its request to the service workspace's raw console, sets `replay_interaction_id` to `Some(interaction_id)`, and returns the applied `RawRequest`. Returns `None` when the interaction cannot be found.
    ///
    /// # Examples
    ///
    /// ```
    /// // Setup (pseudo-constructors shown for brevity)
    /// let mut state = ServiceWorkspaceState::new("service-a");
    /// let history = InteractionHistory::default(); // assume populated with interactions
    ///
    /// // Attempt to replay interaction 42 into the raw console
    /// if let Some(req) = state.replay_into_raw(&history, 42) {
    ///     assert_eq!(state.replay_interaction_id, Some(42));
    ///     // raw_console now reflects the replayed request
    ///     assert_eq!(state.raw_console.path, req.path);
    /// }
    /// ```
    pub fn replay_into_raw(
        &mut self,
        history: &InteractionHistory,
        interaction_id: u64,
    ) -> Option<RawRequest> {
        let replay = replay_from_history(history, interaction_id)?;
        self.raw_console.apply_request(&replay);
        self.replay_interaction_id = Some(interaction_id);
        Some(replay)
    }

    /// Apply a `RawResponse` to the workspace's raw console state.
    ///
    /// This updates the internal `raw_console` with the provided response (e.g., setting the last response).
    ///
    /// # Examples
    ///
    /// ```
    /// use std::collections::HashMap;
    ///
    /// let mut state = ServiceWorkspaceState::new("service");
    /// let resp = RawResponse { status: 200, headers: HashMap::new(), body: b"ok".to_vec() };
    /// state.apply_raw_response(resp);
    /// assert_eq!(state.raw_console.last_response.status, 200);
    /// ```
    pub fn apply_raw_response(&mut self, response: RawResponse) {
        self.raw_console.apply_response(response);
    }

    /// Performs the current raw-console request using the provided executor and records the interaction.
    ///
    /// The request is taken from the workspace's `raw_console`, passed to the `execute` closure, and the returned response is appended to `history` and applied to the `raw_console` state.
    ///
    /// # Returns
    ///
    /// The `RawResponse` produced by `execute`.
    ///
    /// # Examples
    ///
    /// ```
    /// # use crate::workspace::ServiceWorkspaceState;
    /// # use crate::api::RawResponse;
    /// # use crate::history::InteractionHistory;
    /// let mut state = ServiceWorkspaceState::new("service-a");
    /// // prepare raw_console on `state` as needed...
    /// let mut history = InteractionHistory::default();
    /// let response = state.execute_raw(&mut history, |req| RawResponse {
    ///     status: 200,
    ///     headers: vec![],
    ///     body: Vec::new(),
    /// }).unwrap();
    /// assert_eq!(response.status, 200);
    /// ```
    pub fn execute_raw<F>(
        &mut self,
        history: &mut InteractionHistory,
        mut execute: F,
    ) -> Result<RawResponse, WorkspaceError>
    where
        F: FnMut(&RawRequest) -> RawResponse,
    {
        let request = self.raw_console.to_request();
        let response = execute(&request);

        let next_id = history.list().next().map(|entry| entry.id + 1).unwrap_or(1);
        history.push(crate::history::InteractionEntry {
            id: next_id,
            timestamp_unix_ms: 0,
            service: self.service.clone(),
            status: response.status,
            request,
        });

        self.raw_console.apply_response(response.clone());
        Ok(response)
    }
}

/// Extracts the distinct input field names referenced by a guided flow.
///
/// Scans each step and cleanup step of `flow` for tokens of the form `{{inputs.NAME}}`
/// appearing in the operation path, headers, query values, and body, and returns a
/// sorted list of unique `NAME` values.
///
/// # Examples
///
/// ```
/// use std::collections::HashMap;
///
/// // Construct a minimal GuidedFlow with one step referencing `{{inputs.bucket}}`.
/// let flow = GuidedFlow {
///     steps: vec![Step {
///         operation: Operation {
///             path: "/{{inputs.bucket}}".into(),
///             headers: HashMap::new(),
///             query: HashMap::new(),
///             body: None,
///         },
///         assertions: vec![],
///         bindings: vec![],
///     }],
///     cleanup: vec![],
///     id: "example".into(),
///     binds: vec![],
/// };
///
/// let required = required_inputs(&flow);
/// assert_eq!(required, vec!["bucket".to_string()]);
/// ```
fn required_inputs(flow: &GuidedFlow) -> Vec<String> {
    let mut fields = BTreeSet::new();

    for step in flow.steps.iter().chain(flow.cleanup.iter()) {
        collect_from_text(&step.operation.path, &mut fields);
        for value in step.operation.headers.values() {
            collect_from_text(value, &mut fields);
        }
        for value in step.operation.query.values() {
            collect_from_text(value, &mut fields);
        }
        if let Some(body) = step.operation.body.as_deref() {
            collect_from_text(body, &mut fields);
        }
    }

    fields.into_iter().collect()
}

/// Extracts input field names referenced as `{{inputs.<name>}}` from a string and inserts them into `fields`.
///
/// The function scans `value` for `{{ ... }}` tokens, and for each token whose trimmed contents
/// start with `inputs.` and have a non-empty path after that prefix, the path (the portion after
/// `inputs.`) is inserted into the provided `BTreeSet`.
///
/// # Examples
///
/// ```
/// use std::collections::BTreeSet;
/// let mut fields = BTreeSet::new();
/// collect_from_text("PUT /{{inputs.bucket}} with owner {{ inputs.owner }}", &mut fields);
/// assert!(fields.contains("bucket"));
/// assert!(fields.contains("owner"));
/// ```
fn collect_from_text(value: &str, fields: &mut BTreeSet<String>) {
    let mut cursor = 0usize;
    while let Some(start_rel) = value[cursor..].find("{{") {
        let start = cursor + start_rel + 2;
        let Some(end_rel) = value[start..].find("}}") else {
            return;
        };
        let end = start + end_rel;
        let token = value[start..end].trim();
        if let Some(path) = token.strip_prefix("inputs.")
            && !path.is_empty()
        {
            fields.insert(path.to_string());
        }
        cursor = end + 2;
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;
    use crate::guided_manifest::{
        CaptureBinding, FlowAssertion, GuidedStep, NormalizedOperation, ProtocolClass,
    };
    use crate::protocol_adapters::{AdapterExecError, AdapterResponse};

    #[derive(Default)]
    struct FakeExecutor {
        responses: Vec<AdapterResponse>,
        cursor: usize,
    }

    impl GuidedExecutor for FakeExecutor {
        /// Returns the next predefined adapter response and advances the internal cursor.
        ///
        /// The `_operation` argument is ignored. If no predefined response exists at the current cursor,
        /// a default `500` response with body `"error"` is returned.
        ///
        /// # Examples
        ///
        /// ```
        /// // Construct a fake executor with one predefined response and call `execute`.
        /// use std::collections::HashMap;
        ///
        /// let mut exec = FakeExecutor {
        ///     responses: vec![AdapterResponse { status: 200, headers: HashMap::new(), body: "ok".into() }],
        ///     cursor: 0,
        /// };
        /// let op = NormalizedOperation::default();
        /// let resp = exec.execute(&op).unwrap();
        /// assert_eq!(resp.status, 200);
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
                    body: "error".to_string(),
                });
            self.cursor += 1;
            Ok(response)
        }
    }

    /// Builds a sample `GuidedManifest` containing a single L1 flow that creates a bucket.
    ///
    /// The manifest uses the `RestXml` protocol and defines one flow (`"l1-basic"`) with a
    /// single `PUT /{{inputs.bucket}}` step, a `status == 200` assertion, and a capture
    /// binding named `"bucket_name"`.
    ///
    /// # Examples
    ///
    /// ```
    /// let m = test_manifest();
    /// assert_eq!(m.service, "s3");
    /// assert_eq!(m.flows.len(), 1);
    /// assert_eq!(m.flows[0].id, "l1-basic");
    /// ```
    fn test_manifest() -> GuidedManifest {
        GuidedManifest {
            schema_version: "1.2".to_string(),
            service: "s3".to_string(),
            protocol: ProtocolClass::RestXml,
            flows: vec![GuidedFlow {
                id: "l1-basic".to_string(),
                level: "L1".to_string(),
                steps: vec![GuidedStep {
                    id: "create".to_string(),
                    title: "Create".to_string(),
                    operation: NormalizedOperation {
                        method: "PUT".to_string(),
                        path: "/{{inputs.bucket}}".to_string(),
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
                        name: "bucket_name".to_string(),
                        source: "Name".to_string(),
                    }],
                    error_guidance: None,
                }],
                cleanup: vec![],
            }],
        }
    }

    #[test]
    fn selecting_flow_derives_required_inputs() {
        let manifest = test_manifest();
        let mut state = ServiceWorkspaceState::new("s3");
        state.select_flow(&manifest, Some("l1-basic"));
        assert_eq!(state.selected_flow_id.as_deref(), Some("l1-basic"));
        assert_eq!(state.guided_required_fields, vec!["bucket".to_string()]);
    }

    #[test]
    fn guided_execution_transitions_runtime_state_and_render_output() {
        let manifest = test_manifest();
        let mut state = ServiceWorkspaceState::new("s3");
        state.select_flow(&manifest, None);
        state.set_guided_input("bucket", "demo-bucket");

        let mut executor = FakeExecutor {
            responses: vec![AdapterResponse {
                status: 200,
                headers: HashMap::new(),
                body: "<Name>demo-bucket</Name>".to_string(),
            }],
            cursor: 0,
        };
        let mut history = InteractionHistory::new(5);

        let report = state
            .execute_guided(
                &manifest,
                &mut executor,
                &mut history,
                RetryPolicy::default(),
            )
            .expect("guided execution should pass");

        assert!(
            report
                .outcomes
                .first()
                .map(|outcome| outcome.success)
                .unwrap_or(false)
        );
        assert_eq!(state.guided_state, GuidedUxState::Succeeded);
        assert_eq!(
            state.guided_render.as_ref().map(|r| r.flow_id.as_str()),
            Some("l1-basic")
        );
        assert!(history.replay_request(1).is_some());
    }

    #[test]
    fn guided_execution_reports_missing_required_inputs() {
        let manifest = test_manifest();
        let mut state = ServiceWorkspaceState::new("s3");
        state.select_flow(&manifest, None);

        let mut executor = FakeExecutor::default();
        let mut history = InteractionHistory::new(5);
        let err = state
            .execute_guided(
                &manifest,
                &mut executor,
                &mut history,
                RetryPolicy::default(),
            )
            .expect_err("missing inputs should fail");

        assert_eq!(
            err,
            WorkspaceError::MissingGuidedInputs(vec!["bucket".to_string()])
        );
        assert_eq!(state.guided_state, GuidedUxState::Idle);
    }

    #[test]
    fn replay_moves_history_request_into_raw_console() {
        let mut history = InteractionHistory::new(5);
        history.push(crate::history::InteractionEntry {
            id: 44,
            timestamp_unix_ms: 0,
            service: "s3".to_string(),
            status: 200,
            request: RawRequest {
                method: "GET".to_string(),
                path: "/_localstack/health".to_string(),
                query: HashMap::new(),
                headers: HashMap::new(),
                body: None,
            },
        });

        let mut state = ServiceWorkspaceState::new("s3");
        let replay = state
            .replay_into_raw(&history, 44)
            .expect("replay request should be present");

        assert_eq!(replay.path, "/_localstack/health");
        assert_eq!(state.raw_console.path, "/_localstack/health");
        assert_eq!(state.replay_interaction_id, Some(44));
    }

    #[test]
    fn raw_execution_records_history_and_updates_console_response() {
        let mut state = ServiceWorkspaceState::new("s3");
        state.raw_console.method = "POST".to_string();
        state.raw_console.path = "/_localstack/info".to_string();
        state.raw_console.body = Some("{}".to_string());

        let mut history = InteractionHistory::new(10);
        let response = state
            .execute_raw(&mut history, |_| RawResponse {
                status: 200,
                headers: HashMap::new(),
                body: "ok".to_string(),
            })
            .expect("raw execution should succeed");

        assert_eq!(response.status, 200);
        assert_eq!(
            state.raw_console.last_response.as_ref().map(|r| r.status),
            Some(200)
        );
        assert_eq!(history.list().count(), 1);
        assert_eq!(
            history
                .list()
                .next()
                .map(|entry| entry.request.path.as_str()),
            Some("/_localstack/info")
        );
    }
}
