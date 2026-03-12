use std::collections::HashMap;

use crate::guided_manifest::{GuidedFlow, GuidedManifest, GuidedStep};
use crate::guided_runtime::{GuidedExecutionReport, GuidedExecutionState};
use crate::history::InteractionHistory;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GuidedUxState {
    Idle,
    Running,
    Succeeded,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimelineItem {
    pub step_id: String,
    pub title: String,
    pub status: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssertionsPanel {
    pub total: usize,
    pub passed: usize,
    pub failed: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CleanupPanel {
    pub total: usize,
    pub succeeded: usize,
    pub failed: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedGuidedFlow {
    pub flow_id: String,
    pub timeline: Vec<TimelineItem>,
    pub assertions: AssertionsPanel,
    pub cleanup: CleanupPanel,
    pub captures: HashMap<String, String>,
    pub error_guidance: Vec<String>,
}

/// Builds a RenderedGuidedFlow view for a guided flow using the manifest and an optional execution report.
///
/// The returned RenderedGuidedFlow contains the flow identifier, a timeline of steps with titles and statuses,
/// an assertions summary (total, passed, failed), a cleanup summary (total, succeeded, failed),
/// captured values taken from the report if present, and any error guidance for steps that failed in the report.
///
/// # Examples
///
/// ```
/// // assuming `manifest`, `flow`, and `report` are available in scope
/// let rendered = render_guided_flow(&manifest, &flow, report.as_ref());
/// assert_eq!(rendered.flow_id, flow.id);
/// assert_eq!(rendered.timeline.len(), flow.steps.len());
/// ```
pub fn render_guided_flow(
    manifest: &GuidedManifest,
    flow: &GuidedFlow,
    report: Option<&GuidedExecutionReport>,
) -> RenderedGuidedFlow {
    let timeline = flow
        .steps
        .iter()
        .map(|step| TimelineItem {
            step_id: step.id.clone(),
            title: format!("{}: {}", manifest.service, step.title),
            status: step_status(step, report),
        })
        .collect::<Vec<_>>();

    let total_assertions = flow
        .steps
        .iter()
        .map(|step| step.assertions.len())
        .sum::<usize>();
    let failed_assertions = report
        .map(|item| {
            item.outcomes
                .iter()
                .filter(|outcome| !outcome.success)
                .count()
        })
        .unwrap_or(0);

    let cleanup_total = flow.cleanup.len();
    let cleanup_succeeded = report
        .map(|item| {
            item.cleanup
                .iter()
                .filter(|outcome| outcome.success)
                .count()
        })
        .unwrap_or(0);
    let cleanup_failed = cleanup_total.saturating_sub(cleanup_succeeded);
    let captures = report.map(|item| item.captures.clone()).unwrap_or_default();
    let error_guidance = flow
        .steps
        .iter()
        .filter_map(|step| {
            let failed = report
                .map(|item| {
                    item.outcomes
                        .iter()
                        .any(|outcome| outcome.step_id == step.id && !outcome.success)
                })
                .unwrap_or(false);
            if failed {
                step.error_guidance.clone()
            } else {
                None
            }
        })
        .collect::<Vec<_>>();

    RenderedGuidedFlow {
        flow_id: flow.id.clone(),
        timeline,
        assertions: AssertionsPanel {
            total: total_assertions,
            passed: total_assertions.saturating_sub(failed_assertions),
            failed: failed_assertions,
        },
        cleanup: CleanupPanel {
            total: cleanup_total,
            succeeded: cleanup_succeeded,
            failed: cleanup_failed,
        },
        captures,
        error_guidance,
    }
}

/// Checks that all required input fields are present and have non-empty values in the provided map.
///
/// # Parameters
///
/// - `required_fields`: slice of field names that must be present and non-empty.
/// - `values`: map from field name to its provided string value.
///
/// # Returns
///
/// `Ok(())` if every required field exists in `values` and its value is not empty, `Err(Vec<String>)` containing the names of fields that are missing or have empty values otherwise.
///
/// # Examples
///
/// ```
/// use std::collections::HashMap;
///
/// let required = vec!["username".to_string(), "email".to_string()];
/// let mut values = HashMap::new();
/// values.insert("username".to_string(), "alice".to_string());
/// values.insert("email".to_string(), "".to_string());
///
/// let res = validate_guided_inputs(&required, &values);
/// assert!(res.is_err());
/// assert_eq!(res.unwrap_err(), vec!["email".to_string()]);
/// ```
pub fn validate_guided_inputs(
    required_fields: &[String],
    values: &HashMap<String, String>,
) -> Result<(), Vec<String>> {
    let missing = required_fields
        .iter()
        .filter(|field| values.get(*field).map(String::is_empty).unwrap_or(true))
        .cloned()
        .collect::<Vec<_>>();

    if missing.is_empty() {
        Ok(())
    } else {
        Err(missing)
    }
}

/// Map an optional guided execution report to the corresponding UX state.
///
/// # Returns
/// `GuidedUxState` where:
/// - `Idle` when `report` is `None` or its state is `Pending`.
/// - `Running` when the state is `Running`.
/// - `Succeeded` when the state is `Succeeded`.
/// - `Failed` when the state is `Failed` or `Canceled`.
///
/// # Examples
///
/// ```
/// let ux = map_ux_state(None);
/// assert_eq!(ux, GuidedUxState::Idle);
/// ```
pub fn map_ux_state(report: Option<&GuidedExecutionReport>) -> GuidedUxState {
    match report.map(|r| r.state) {
        None | Some(GuidedExecutionState::Pending) => GuidedUxState::Idle,
        Some(GuidedExecutionState::Running) => GuidedUxState::Running,
        Some(GuidedExecutionState::Succeeded) => GuidedUxState::Succeeded,
        Some(GuidedExecutionState::Failed) | Some(GuidedExecutionState::Canceled) => {
            GuidedUxState::Failed
        }
    }
}

/// Attempts to replay a previously recorded interaction and returns its raw request if found.
///
/// # Parameters
///
/// - `interaction_id`: The identifier of the interaction to replay.
///
/// # Returns
///
/// `Some(RawRequest)` containing the recorded request for the given `interaction_id` if present, `None` otherwise.
///
/// # Examples
///
/// ```no_run
/// use crate::history::InteractionHistory;
/// use crate::ui::replay_from_history;
///
/// let history: InteractionHistory = /* obtain interaction history */ unimplemented!();
/// let interaction_id = 42;
/// if let Some(raw_req) = replay_from_history(&history, interaction_id) {
///     // inspect or resend `raw_req`
///     println!("{:?}", raw_req);
/// }
/// ```
pub fn replay_from_history(
    history: &InteractionHistory,
    interaction_id: u64,
) -> Option<crate::api::RawRequest> {
    history.replay_request(interaction_id)
}

/// Determines the display status string for a guided step given an optional execution report.
///
/// Returns `"success"` if the report contains a successful outcome for the step, `"failed"` if the report
/// contains a failed outcome for the step, and `"pending"` if no report is provided or the step has no outcome.
///
/// # Examples
///
/// ```
/// // Assuming `GuidedStep` and `GuidedExecutionReport` are in scope and `GuidedStep` implements `Default`.
/// let step = GuidedStep { id: "step-1".to_string(), ..Default::default() };
/// assert_eq!(step_status(&step, None), "pending".to_string());
/// ```
fn step_status(step: &GuidedStep, report: Option<&GuidedExecutionReport>) -> String {
    let Some(report) = report else {
        return "pending".to_string();
    };

    report
        .outcomes
        .iter()
        .find(|outcome| outcome.step_id == step.id)
        .map(|outcome| if outcome.success { "success" } else { "failed" })
        .unwrap_or("pending")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::guided_manifest::{
        CaptureBinding, FlowAssertion, GuidedManifest, NormalizedOperation, ProtocolClass,
    };
    use crate::guided_runtime::{GuidedExecutionReport, GuidedExecutionState, StepOutcome};

    /// Creates a minimal GuidedManifest with one flow ("flow-a") and one step ("step-a") useful for tests.
    ///
    /// # Examples
    ///
    /// ```
    /// let manifest = sample_manifest(ProtocolClass::Http);
    /// assert_eq!(manifest.flows[0].id, "flow-a");
    /// assert_eq!(manifest.flows[0].steps[0].id, "step-a");
    /// ```
    fn sample_manifest(protocol: ProtocolClass) -> GuidedManifest {
        GuidedManifest {
            schema_version: "1.2".to_string(),
            service: "svc".to_string(),
            protocol,
            flows: vec![GuidedFlow {
                id: "flow-a".to_string(),
                level: "L1".to_string(),
                steps: vec![GuidedStep {
                    id: "step-a".to_string(),
                    title: "Step A".to_string(),
                    operation: NormalizedOperation {
                        method: "GET".to_string(),
                        path: "/x".to_string(),
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
                        name: "id".to_string(),
                        source: "Id".to_string(),
                    }],
                    error_guidance: None,
                }],
                cleanup: vec![],
            }],
        }
    }

    #[test]
    fn renderer_handles_all_protocol_classes() {
        for protocol in [
            ProtocolClass::Query,
            ProtocolClass::JsonTarget,
            ProtocolClass::RestXml,
            ProtocolClass::RestJson,
        ] {
            let manifest = sample_manifest(protocol);
            let rendered = render_guided_flow(&manifest, &manifest.flows[0], None);
            assert_eq!(rendered.flow_id, "flow-a");
            assert_eq!(rendered.timeline.len(), 1);
            assert_eq!(rendered.timeline[0].status, "pending");
        }
    }

    #[test]
    fn input_validation_returns_missing_fields() {
        let required = vec!["bucket".to_string(), "key".to_string()];
        let values = HashMap::from([(String::from("bucket"), String::from("b"))]);
        let result = validate_guided_inputs(&required, &values).expect_err("should fail");
        assert_eq!(result, vec!["key".to_string()]);
    }

    #[test]
    fn renderer_includes_captures_and_failed_step_guidance() {
        let mut manifest = sample_manifest(ProtocolClass::RestXml);
        manifest.flows[0].steps[0].error_guidance = Some("Check bucket policy".to_string());

        let report = GuidedExecutionReport {
            state: GuidedExecutionState::Failed,
            outcomes: vec![StepOutcome {
                step_id: "step-a".to_string(),
                success: false,
                attempts: 1,
                status_code: Some(403),
                error: None,
            }],
            cleanup: vec![],
            captures: HashMap::from([(String::from("bucket"), String::from("demo"))]),
        };

        let rendered = render_guided_flow(&manifest, &manifest.flows[0], Some(&report));
        assert_eq!(rendered.captures.get("bucket"), Some(&"demo".to_string()));
        assert_eq!(
            rendered.error_guidance,
            vec!["Check bucket policy".to_string()]
        );
    }
}
