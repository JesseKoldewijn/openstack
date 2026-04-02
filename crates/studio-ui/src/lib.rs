pub mod api;
pub mod catalog;
pub mod console;
pub mod dashboard;
pub mod explorer;
pub mod guided_manifest;
pub mod guided_renderer;
pub mod guided_runtime;
pub mod history;
pub mod models;
pub mod navigation;
pub mod operation_catalog;
pub mod protocol_adapters;
pub mod service_detail;
pub mod slugs;
pub mod state;
pub mod storage_inspector;
pub mod transaction_log;
pub mod workflow;
pub mod workspace;

pub use api::StudioApiClient;
pub use api::StudioUrlResolution;
pub use api::{
    AllTransactionsResponse, OperationEntryDto, ServiceOperationsResponse,
    ServiceStorageResponse, ServiceTransactionsResponse, StudioCredentials,
    StudioPollingConfig, StudioRuntimeConfig, TransactionDto, TransactionSummaryDto,
};
pub use catalog::ServiceCatalog;
pub use console::RawConsoleState;
pub use dashboard::{DashboardHomeViewModel, DashboardServiceCard, build_dashboard_home_model};
pub use explorer::{
    ExplorerTab, OperationFilter, OperationsTabViewModel, ServiceExplorerViewModel,
    ServiceOverview, StorageSection, StorageTabViewModel, TransactionFilter, TransactionRow,
    TransactionsTabViewModel,
};
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
pub use operation_catalog::{OperationCatalog, OperationEntry, ServiceOperationSet};
pub use protocol_adapters::{
    AdapterError, AdapterExecError, AdapterRequest, AdapterResponse, AdapterResult,
    execute_protocol_adapter,
};
pub use service_detail::{PanelState, ServiceDetailLayout, build_service_detail_layout};
pub use state::{ThemeMode, ThemeStore};
pub use storage_inspector::{
    RuntimeStorageState, ServiceStorageSnapshot, StorageResource, ResourceAttribute,
};
pub use slugs::{alias_map, to_manifest_slug, to_provider_slug};
pub use transaction_log::{
    TransactionLog, TransactionOutcome, TransactionRecord, TransactionSummary,
};
pub use workflow::{GuidedWorkflow, GuidedWorkflowKind, WorkflowStep};
pub use workspace::{ServiceWorkspaceState, WorkspaceError};
