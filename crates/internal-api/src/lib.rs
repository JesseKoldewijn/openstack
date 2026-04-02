pub mod config_api;
pub mod diagnose;
pub mod health;
pub mod info;
pub mod init;
pub mod plugins;
pub mod router;
pub mod studio;
pub mod studio_operations;
pub mod studio_runtime_config;
pub mod studio_storage;
pub mod studio_transactions;

use std::sync::Arc;
use std::time::Instant;

use openstack_config::Config;
use openstack_service_framework::ServicePluginManager;
pub use router::internal_api_router;
use tokio::sync::{Mutex, broadcast};

/// Shared state injected into all internal API handlers.
#[derive(Clone)]
pub struct ApiState {
    pub config: Config,
    pub plugin_manager: ServicePluginManager,
    pub session_id: String,
    pub start_time: Arc<Instant>,
    /// Send to this channel to request a graceful shutdown / restart.
    pub shutdown_tx: broadcast::Sender<()>,
    pub(crate) guided_service_matrix: std::collections::HashSet<String>,
    pub(crate) guided_manifest_inventory:
        std::collections::HashMap<String, crate::studio::GuidedManifestFile>,
    /// Live transaction log — only allocated when Studio is enabled.
    ///
    /// `None` when running in headless/benchmark mode (no `STUDIO=1` env var
    /// and no `--debug` flag).  All Studio TX endpoints silently return empty
    /// results when this is `None`, so the binary stays lean by default.
    pub transaction_log: Option<Arc<Mutex<openstack_studio_ui::TransactionLog>>>,
    /// Whether the Studio UI subsystem (TX log, operation catalog) is active.
    pub studio_enabled: bool,
}

impl ApiState {
    pub fn new(
        config: Config,
        plugin_manager: ServicePluginManager,
        shutdown_tx: broadcast::Sender<()>,
    ) -> Self {
        Self::new_with_studio(config, plugin_manager, shutdown_tx, false)
    }

    /// Create an `ApiState` with the Studio subsystem explicitly enabled or
    /// disabled.  Pass `studio_enabled = true` when the process was launched
    /// with `openstack start --studio` or `STUDIO=1`.
    pub fn new_with_studio(
        config: Config,
        plugin_manager: ServicePluginManager,
        shutdown_tx: broadcast::Sender<()>,
        studio_enabled: bool,
    ) -> Self {
        // Also enable via STUDIO env var or DEBUG mode (matches existing pattern).
        let studio_active = studio_enabled
            || config.debug
            || std::env::var("STUDIO").is_ok_and(|v| v == "1" || v == "true");

        let (guided_service_matrix, guided_manifest_inventory, transaction_log) = if studio_active {
            (
                crate::studio::load_service_matrix_services(),
                crate::studio::load_manifest_inventory(),
                Some(Arc::new(Mutex::new(
                    openstack_studio_ui::TransactionLog::new(2000),
                ))),
            )
        } else {
            (
                std::collections::HashSet::new(),
                std::collections::HashMap::new(),
                None,
            )
        };

        Self {
            config,
            plugin_manager,
            session_id: uuid::Uuid::new_v4().to_string(),
            start_time: Arc::new(Instant::now()),
            shutdown_tx,
            guided_service_matrix,
            guided_manifest_inventory,
            transaction_log,
            studio_enabled: studio_active,
        }
    }
}
