use std::sync::Arc;

use dashmap::DashMap;
use openstack_config::Config;
use tracing::{error, warn};

use crate::container::ServiceContainer;
use crate::lifecycle::ServiceState;
use crate::traits::{DispatchError, DispatchResponse, RequestContext, ServiceProvider};

#[derive(Debug, Clone)]
pub struct ServiceManagerMetrics {
    pub service: String,
    pub state: ServiceState,
    pub startup_attempts: usize,
    pub startup_wait_count: usize,
    pub last_startup_duration_ms: u64,
}

/// Central registry and dispatcher for service providers.
#[derive(Clone)]
pub struct ServicePluginManager {
    config: Config,
    containers: Arc<DashMap<String, Arc<ServiceContainer>>>,
}

impl ServicePluginManager {
    pub fn new(config: Config) -> Self {
        Self {
            config,
            containers: Arc::new(DashMap::new()),
        }
    }

    /// Register a service provider.
    pub fn register(&self, service_name: &str, provider: impl ServiceProvider + 'static) {
        let name = service_name.to_lowercase();

        // Check for provider override
        let provider: Arc<dyn ServiceProvider> = if let Some(override_name) =
            self.config.services.get_override(&name)
        {
            warn!(
                "Provider override for '{}' requested: '{}' (not yet implemented, using default)",
                name, override_name
            );
            Arc::new(provider)
        } else {
            Arc::new(provider)
        };

        let container = Arc::new(ServiceContainer::new(provider));
        self.containers.insert(name, container);
    }

    /// Dispatch a request to the appropriate service provider.
    pub async fn dispatch(&self, ctx: &RequestContext) -> Result<DispatchResponse, DispatchError> {
        let service = ctx.service.to_lowercase();

        let container = self
            .containers
            .get(&service)
            .ok_or_else(|| DispatchError::ServiceNotFound(service.clone()))?
            .clone();

        // Ensure the service is running (lazy start)
        container
            .ensure_running()
            .await
            .map_err(|e| DispatchError::ServiceUnavailable(e.to_string()))?;

        container.provider.dispatch(ctx).await
    }

    /// Returns the current state of all registered services.
    pub async fn service_states(&self) -> Vec<(String, ServiceState)> {
        let mut states = Vec::new();
        for entry in self.containers.iter() {
            let state = entry.value().current_state().await;
            states.push((entry.key().clone(), state));
        }
        states
    }

    /// Start all registered services eagerly (for EAGER_SERVICE_LOADING).
    pub async fn start_all(&self) {
        for entry in self.containers.iter() {
            let container = entry.value().clone();
            tokio::spawn(async move {
                if let Err(e) = container.ensure_running().await {
                    error!("Failed to eagerly start service: {}", e);
                }
            });
        }
    }

    pub async fn service_runtime_metrics(&self) -> Vec<ServiceManagerMetrics> {
        let mut metrics = Vec::new();
        for entry in self.containers.iter() {
            let container = entry.value();
            let runtime = container.runtime_metrics();
            metrics.push(ServiceManagerMetrics {
                service: entry.key().clone(),
                state: container.current_state().await,
                startup_attempts: runtime.startup_attempts,
                startup_wait_count: runtime.startup_wait_count,
                last_startup_duration_ms: runtime.last_startup_duration_ms,
            });
        }
        metrics
    }

    /// Stop all registered services.
    pub async fn stop_all(&self) {
        for entry in self.containers.iter() {
            let container = entry.value().clone();
            let name = entry.key().clone();
            if let Err(e) = container.stop().await {
                error!("Failed to stop service '{}': {}", name, e);
            }
        }
    }
}
