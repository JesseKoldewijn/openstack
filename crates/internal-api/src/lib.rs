pub mod config_api;
pub mod diagnose;
pub mod health;
pub mod info;
pub mod init;
pub mod plugins;
pub mod router;
pub mod studio;

use std::sync::Arc;
use std::time::Instant;

use openstack_config::Config;
use openstack_service_framework::ServicePluginManager;
pub use router::internal_api_router;
use tokio::sync::broadcast;

/// Shared state injected into all internal API handlers.
#[derive(Clone)]
pub struct ApiState {
    pub config: Config,
    pub plugin_manager: ServicePluginManager,
    pub session_id: String,
    pub start_time: Arc<Instant>,
    /// Send to this channel to request a graceful shutdown / restart.
    pub shutdown_tx: broadcast::Sender<()>,
}

impl ApiState {
    pub fn new(
        config: Config,
        plugin_manager: ServicePluginManager,
        shutdown_tx: broadcast::Sender<()>,
    ) -> Self {
        Self {
            config,
            plugin_manager,
            session_id: uuid::Uuid::new_v4().to_string(),
            start_time: Arc::new(Instant::now()),
            shutdown_tx,
        }
    }
}
