pub mod api;
pub mod catalog;
pub mod console;
pub mod dashboard;
pub mod guided_manifest;
pub mod guided_renderer;
pub mod guided_runtime;
pub mod history;
pub mod models;
pub mod navigation;
pub mod protocol_adapters;
pub mod service_detail;
pub mod state;
pub mod workflow;
pub mod workspace;

pub use api::StudioApiClient;
pub use api::StudioUrlResolution;
pub use catalog::ServiceCatalog;
pub use console::RawConsoleState;
pub use dashboard::{DashboardHomeViewModel, DashboardServiceCard, build_dashboard_home_model};
pub use guided_manifest::{
    CaptureBinding, FlowAssertion, GuidedFlow, GuidedManifest, GuidedStep, ManifestError,
    NormalizedOperation, ProtocolClass, SUPPORTED_SCHEMA_VERSION,
};
pub use guided_renderer::{
    AssertionsPanel, CleanupPanel, GuidedUxState, RenderedGuidedFlow, TimelineItem, map_ux_state,
    render_guided_flow, replay_from_history, validate_guided_inputs,
};
pub use guided_runtime::{
    BindingContext, CleanupOutcome, ExecutionPolicy, GuidedExecutionReport, GuidedExecutionState,
    RetryEnvelope, RetryPolicy, StepOutcome, run_guided_flow, run_guided_flow_with_policy,
};
pub use history::{InteractionEntry, InteractionHistory};
pub use models::{
    ApiField, FlowCatalogEntry, FlowCatalogResponse, FlowCoverageEntry, FlowCoverageResponse,
    FlowDefinitionResponse, GuidedInputField, InteractionSchema, ServiceEntry,
    StudioServicesResponse,
};
pub use navigation::{DashboardNavigationState, DashboardRoute};
pub use protocol_adapters::{
    AdapterError, AdapterExecError, AdapterRequest, AdapterResponse, AdapterResult,
    execute_protocol_adapter,
};
pub use service_detail::{PanelState, ServiceDetailLayout, build_service_detail_layout};
pub use state::{ThemeMode, ThemeStore};
pub use workflow::{GuidedWorkflow, GuidedWorkflowKind, WorkflowStep};
pub use workspace::{ServiceWorkspaceState, WorkspaceError};
