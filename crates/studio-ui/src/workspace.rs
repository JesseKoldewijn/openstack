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

    pub fn select_flow(&mut self, manifest: &GuidedManifest, flow_id: Option<&str>) {
        let target_flow = flow_id
            .and_then(|id| manifest.flows.iter().find(|flow| flow.id == id))
            .or_else(|| manifest.flows.first());

        self.selected_flow_id = target_flow.map(|flow| flow.id.clone());
        self.guided_required_fields = target_flow.map_or_else(Vec::new, required_inputs);
    }

    pub fn set_guided_input(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.guided_inputs.insert(key.into(), value.into());
    }

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

    pub fn apply_raw_response(&mut self, response: RawResponse) {
        self.raw_console.apply_response(response);
    }

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

        let next_id = history.next_id();
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
