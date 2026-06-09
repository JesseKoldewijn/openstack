/// Service explorer layout model for the Studio UI.
///
/// Drives the tab-based service detail view:
///   Overview  |  Operations  |  Storage  |  Transactions
///
/// Each tab has its own view model built from the underlying catalog,
/// storage state, and transaction log.  All types are plain data — no
/// async, no HTTP — so they can be unit-tested in isolation.
use crate::catalog::ServiceCatalog;
use crate::operation_catalog::{OperationCatalog, OperationEntry};
use crate::storage_inspector::{RuntimeStorageState, ServiceStorageSnapshot, StorageResource};
use crate::transaction_log::{TransactionLog, TransactionOutcome, TransactionRecord};

// ---------------------------------------------------------------------------
// Active tab
// ---------------------------------------------------------------------------

/// Which tab is currently visible in the explorer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ExplorerTab {
    #[default]
    Overview,
    Operations,
    Storage,
    Transactions,
}

impl ExplorerTab {
    pub fn label(self) -> &'static str {
        match self {
            Self::Overview => "Overview",
            Self::Operations => "Operations",
            Self::Storage => "Storage",
            Self::Transactions => "Transactions",
        }
    }

    pub fn all() -> &'static [ExplorerTab] {
        &[
            ExplorerTab::Overview,
            ExplorerTab::Operations,
            ExplorerTab::Storage,
            ExplorerTab::Transactions,
        ]
    }
}

// ---------------------------------------------------------------------------
// Overview tab
// ---------------------------------------------------------------------------

/// High-level summary shown on the Overview tab.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceOverview {
    pub service: String,
    pub status: String,
    pub support_tier: String,
    pub protocol: String,
    pub total_operations: usize,
    pub guided_flow_count: usize,
    pub storage_resource_count: usize,
    pub transaction_count: usize,
    pub error_rate_pct: u8,
}

// ---------------------------------------------------------------------------
// Operations tab
// ---------------------------------------------------------------------------

/// Operation search/filter state.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct OperationFilter {
    /// Case-insensitive substring match against operation name.
    pub query: String,
    /// When `true`, only show operations that have a guided flow.
    pub guided_only: bool,
}

/// The full Operations tab view model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationsTabViewModel {
    pub service: String,
    /// Filtered + sorted list of operations to display.
    pub visible: Vec<OperationEntry>,
    pub total: usize,
    pub guided_count: usize,
    pub filter: OperationFilter,
}

impl OperationsTabViewModel {
    pub fn build(service: &str, catalog: &OperationCatalog, filter: OperationFilter) -> Self {
        let set = catalog.for_service(service);
        let total = set.map(|s| s.total()).unwrap_or(0);
        let guided_count = set.map(|s| s.guided_count()).unwrap_or(0);

        let visible = set
            .map(|s| {
                let q = filter.query.to_lowercase();
                s.operations
                    .iter()
                    .filter(|op| {
                        let name_matches = q.is_empty() || op.name.to_lowercase().contains(&q);
                        let guided_ok = !filter.guided_only || op.has_guided_flow;
                        name_matches && guided_ok
                    })
                    .cloned()
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        Self {
            service: service.to_string(),
            visible,
            total,
            guided_count,
            filter,
        }
    }
}

// ---------------------------------------------------------------------------
// Storage tab
// ---------------------------------------------------------------------------

/// Kind of resource section within the storage tab.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StorageSection {
    /// Section heading, e.g. `"Buckets"`, `"Queues"`.
    pub heading: String,
    pub resources: Vec<StorageResource>,
}

/// The full Storage tab view model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StorageTabViewModel {
    pub service: String,
    pub sections: Vec<StorageSection>,
    pub total_resources: usize,
    pub has_data: bool,
}

impl StorageTabViewModel {
    pub fn build(service: &str, storage: &RuntimeStorageState) -> Self {
        let snapshot = storage.get(service);
        let sections = snapshot.map(sections_from_snapshot).unwrap_or_default();
        let total_resources = sections.iter().map(|s| s.resources.len()).sum();

        Self {
            service: service.to_string(),
            has_data: !sections.is_empty(),
            total_resources,
            sections,
        }
    }
}

fn sections_from_snapshot(snapshot: &ServiceStorageSnapshot) -> Vec<StorageSection> {
    use crate::storage_inspector::ServiceStorageSnapshot::*;

    match snapshot {
        S3(s) => vec![StorageSection {
            heading: "Buckets".to_string(),
            resources: s.buckets.clone(),
        }],
        Sqs(s) => vec![StorageSection {
            heading: "Queues".to_string(),
            resources: s.queues.clone(),
        }],
        Sns(s) => vec![StorageSection {
            heading: "Topics".to_string(),
            resources: s.topics.clone(),
        }],
        DynamoDb(s) => vec![StorageSection {
            heading: "Tables".to_string(),
            resources: s.tables.clone(),
        }],
        Lambda(s) => vec![StorageSection {
            heading: "Functions".to_string(),
            resources: s.functions.clone(),
        }],
        SecretsManager(s) => vec![StorageSection {
            heading: "Secrets".to_string(),
            resources: s.secrets.clone(),
        }],
        Kms(s) => vec![StorageSection {
            heading: "Keys".to_string(),
            resources: s.keys.clone(),
        }],
        Kinesis(s) => vec![StorageSection {
            heading: "Streams".to_string(),
            resources: s.streams.clone(),
        }],
        StepFunctions(s) => vec![StorageSection {
            heading: "State Machines".to_string(),
            resources: s.state_machines.clone(),
        }],
        Ecr(s) => vec![StorageSection {
            heading: "Repositories".to_string(),
            resources: s.repositories.clone(),
        }],
        EventBridge(s) => vec![StorageSection {
            heading: "Event Buses".to_string(),
            resources: s.buses.clone(),
        }],
        Ssm(s) => vec![StorageSection {
            heading: "Parameters".to_string(),
            resources: s.parameters.clone(),
        }],
        Route53(s) => vec![StorageSection {
            heading: "Hosted Zones".to_string(),
            resources: s.hosted_zones.clone(),
        }],
        Iam(s) => vec![
            StorageSection {
                heading: "Users".to_string(),
                resources: s.users.clone(),
            },
            StorageSection {
                heading: "Roles".to_string(),
                resources: s.roles.clone(),
            },
            StorageSection {
                heading: "Policies".to_string(),
                resources: s.policies.clone(),
            },
        ],
        Generic { resources, .. } => vec![StorageSection {
            heading: "Resources".to_string(),
            resources: resources.clone(),
        }],
    }
}

// ---------------------------------------------------------------------------
// Transactions tab
// ---------------------------------------------------------------------------

/// A single row in the Transactions tab table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransactionRow {
    pub id: u64,
    pub method: String,
    pub path: String,
    pub operation: Option<String>,
    pub status: u16,
    pub duration_ms: Option<u64>,
    pub outcome_label: &'static str,
    pub from_guided_flow: bool,
}

impl From<&TransactionRecord> for TransactionRow {
    fn from(r: &TransactionRecord) -> Self {
        Self {
            id: r.id,
            method: r.method.clone(),
            path: r.path.clone(),
            operation: r.operation.clone(),
            status: r.status,
            duration_ms: r.duration_ms,
            outcome_label: r.outcome.label(),
            from_guided_flow: r.from_guided_flow,
        }
    }
}

/// Active filter on the Transactions tab.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TransactionFilter {
    pub outcome: Option<TransactionOutcome>,
    pub guided_only: bool,
}

/// The full Transactions tab view model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransactionsTabViewModel {
    pub service: String,
    pub rows: Vec<TransactionRow>,
    pub total: usize,
    pub success_count: usize,
    pub error_count: usize,
    pub filter: TransactionFilter,
}

impl TransactionsTabViewModel {
    pub fn build(service: &str, log: &TransactionLog, filter: TransactionFilter) -> Self {
        let all: Vec<&TransactionRecord> = log.for_service(service).collect();
        let total = all.len();
        let success_count = all
            .iter()
            .filter(|r| r.outcome == TransactionOutcome::Success)
            .count();
        let error_count = all
            .iter()
            .filter(|r| {
                matches!(
                    r.outcome,
                    TransactionOutcome::ClientError | TransactionOutcome::ServerError
                )
            })
            .count();

        let rows = all
            .iter()
            .filter(|r| {
                let outcome_ok = filter.outcome.map(|o| r.outcome == o).unwrap_or(true);
                let guided_ok = !filter.guided_only || r.from_guided_flow;
                outcome_ok && guided_ok
            })
            .map(|r| TransactionRow::from(*r))
            .collect();

        Self {
            service: service.to_string(),
            rows,
            total,
            success_count,
            error_count,
            filter,
        }
    }
}

// ---------------------------------------------------------------------------
// Top-level explorer layout
// ---------------------------------------------------------------------------

/// The complete view model for one service's explorer page.
pub struct ServiceExplorerViewModel {
    pub overview: ServiceOverview,
    pub operations: OperationsTabViewModel,
    pub storage: StorageTabViewModel,
    pub transactions: TransactionsTabViewModel,
    pub active_tab: ExplorerTab,
}

impl ServiceExplorerViewModel {
    /// Build all tab view models from the current state.
    ///
    /// `active_tab` controls which tab is rendered as selected; callers should
    /// persist this in their own UI state and pass it back on rebuild.
    pub fn build(
        service: &str,
        catalog: &ServiceCatalog,
        op_catalog: &OperationCatalog,
        storage: &RuntimeStorageState,
        log: &TransactionLog,
        active_tab: ExplorerTab,
    ) -> Self {
        let service_entry = catalog.by_name(service);
        let guided_summary = catalog.guided_summary(service);

        let total_ops = op_catalog
            .for_service(service)
            .map(|s| s.total())
            .unwrap_or(0);
        let guided_count = op_catalog
            .for_service(service)
            .map(|s| s.guided_count())
            .unwrap_or(0);

        let storage_count = storage
            .get(service)
            .map(|s| s.resource_count())
            .unwrap_or(0);
        let tx_summary = log.summary();
        let service_tx_count = log.for_service(service).count();

        let error_rate_pct = {
            let errors = tx_summary.client_error + tx_summary.server_error;
            errors
                .checked_mul(100)
                .and_then(|value| value.checked_div(tx_summary.total))
                .unwrap_or(0)
                .min(100) as u8
        };

        let overview = ServiceOverview {
            service: service.to_string(),
            status: service_entry
                .map(|e| e.status.clone())
                .unwrap_or_else(|| "unknown".to_string()),
            support_tier: service_entry
                .map(|e| e.support_tier.clone())
                .unwrap_or_else(|| "unknown".to_string()),
            protocol: guided_summary
                .map(|s| s.protocol.clone())
                .unwrap_or_else(|| "unknown".to_string()),
            total_operations: total_ops,
            guided_flow_count: guided_count,
            storage_resource_count: storage_count,
            transaction_count: service_tx_count,
            error_rate_pct,
        };

        let operations =
            OperationsTabViewModel::build(service, op_catalog, OperationFilter::default());
        let storage_vm = StorageTabViewModel::build(service, storage);
        let transactions =
            TransactionsTabViewModel::build(service, log, TransactionFilter::default());

        Self {
            overview,
            operations,
            storage: storage_vm,
            transactions,
            active_tab,
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{
        FlowCatalogEntry, FlowCatalogResponse, FlowCoverageEntry, FlowCoverageResponse,
        ServiceEntry, StudioServicesResponse,
    };
    use crate::storage_inspector::{S3StorageSnapshot, ServiceStorageSnapshot, StorageResource};
    use crate::transaction_log::TransactionRecord;

    fn base_catalog() -> ServiceCatalog {
        let services = StudioServicesResponse {
            services: vec![ServiceEntry {
                name: "s3".to_string(),
                status: "running".to_string(),
                support_tier: "guided".to_string(),
            }],
        };
        let flow_catalog = FlowCatalogResponse {
            services: vec![FlowCatalogEntry {
                service: "s3".to_string(),
                manifest_version: "1.2".to_string(),
                protocol: "rest_xml".to_string(),
                flow_count: 1,
                maturity: "l1".to_string(),
            }],
        };
        let coverage = FlowCoverageResponse {
            schema_version: "1.2".to_string(),
            summary: "ok".to_string(),
            services: vec![FlowCoverageEntry {
                service: "s3".to_string(),
                has_manifest: true,
                l1_flows: 1,
                total_flows: 1,
                quality: "meets_l1".to_string(),
            }],
        };
        ServiceCatalog::from_response(services).with_guided_metadata(flow_catalog, coverage)
    }

    #[test]
    fn explorer_tab_labels_are_consistent() {
        assert_eq!(ExplorerTab::Overview.label(), "Overview");
        assert_eq!(ExplorerTab::Operations.label(), "Operations");
        assert_eq!(ExplorerTab::Storage.label(), "Storage");
        assert_eq!(ExplorerTab::Transactions.label(), "Transactions");
        assert_eq!(ExplorerTab::all().len(), 4);
    }

    #[test]
    fn operations_tab_filters_by_query() {
        let catalog = OperationCatalog::build(&[]);
        let filter = OperationFilter {
            query: "put".to_string(),
            guided_only: false,
        };
        let vm = OperationsTabViewModel::build("s3", &catalog, filter);
        assert!(!vm.visible.is_empty());
        for op in &vm.visible {
            assert!(op.name.to_lowercase().contains("put"));
        }
    }

    #[test]
    fn storage_tab_shows_sections_from_snapshot() {
        let mut storage = RuntimeStorageState::new();
        storage.update(ServiceStorageSnapshot::S3(S3StorageSnapshot {
            buckets: vec![
                StorageResource::new("bucket-a", "bucket"),
                StorageResource::new("bucket-b", "bucket"),
            ],
        }));

        let vm = StorageTabViewModel::build("s3", &storage);
        assert!(vm.has_data);
        assert_eq!(vm.total_resources, 2);
        assert_eq!(vm.sections[0].heading, "Buckets");
    }

    #[test]
    fn storage_tab_empty_when_no_snapshot() {
        let storage = RuntimeStorageState::new();
        let vm = StorageTabViewModel::build("sqs", &storage);
        assert!(!vm.has_data);
        assert_eq!(vm.total_resources, 0);
    }

    #[test]
    fn transactions_tab_filters_by_outcome() {
        let mut log = TransactionLog::new(50);
        for status in [200u16, 200, 404, 500] {
            log.push(TransactionRecord::new(0, "s3", "POST", "/", 0).complete(status, "", 10));
        }

        let filter = TransactionFilter {
            outcome: Some(TransactionOutcome::Success),
            guided_only: false,
        };
        let vm = TransactionsTabViewModel::build("s3", &log, filter);
        assert_eq!(vm.rows.len(), 2);
        assert_eq!(vm.success_count, 2);
        assert_eq!(vm.error_count, 2);
    }

    #[test]
    fn transactions_tab_guided_only_filter() {
        let mut log = TransactionLog::new(20);
        log.push(
            TransactionRecord::new(0, "s3", "GET", "/", 0)
                .with_guided()
                .complete(200, "", 5),
        );
        log.push(TransactionRecord::new(0, "s3", "PUT", "/", 0).complete(200, "", 5));

        let filter = TransactionFilter {
            outcome: None,
            guided_only: true,
        };
        let vm = TransactionsTabViewModel::build("s3", &log, filter);
        assert_eq!(vm.rows.len(), 1);
        assert!(vm.rows[0].from_guided_flow);
    }

    #[test]
    fn service_overview_composes_all_sources() {
        let catalog = base_catalog();
        let op_catalog = OperationCatalog::build(&[]);
        let storage = RuntimeStorageState::new();
        let log = TransactionLog::new(50);

        let vm = ServiceExplorerViewModel::build(
            "s3",
            &catalog,
            &op_catalog,
            &storage,
            &log,
            ExplorerTab::Overview,
        );

        assert_eq!(vm.overview.service, "s3");
        assert_eq!(vm.overview.status, "running");
        assert!(vm.overview.total_operations > 0);
        assert_eq!(vm.active_tab, ExplorerTab::Overview);
    }
}
