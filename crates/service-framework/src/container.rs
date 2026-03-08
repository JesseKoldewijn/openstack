use std::sync::Arc;

use tokio::sync::{Mutex, RwLock};
use tracing::{error, info};

use crate::lifecycle::ServiceState;
use crate::traits::ServiceProvider;

/// Wraps a `ServiceProvider` with lifecycle state tracking and a loading lock
/// to prevent concurrent initialization.
pub struct ServiceContainer {
    pub provider: Arc<dyn ServiceProvider>,
    state: Arc<RwLock<ServiceState>>,
    init_lock: Arc<Mutex<()>>,
}

impl ServiceContainer {
    pub fn new(provider: Arc<dyn ServiceProvider>) -> Self {
        Self {
            provider,
            state: Arc::new(RwLock::new(ServiceState::Available)),
            init_lock: Arc::new(Mutex::new(())),
        }
    }

    pub async fn current_state(&self) -> ServiceState {
        self.state.read().await.clone()
    }

    /// Ensure the service is running, starting it if necessary.
    /// Thread-safe: only one start() call will proceed; others wait.
    pub async fn ensure_running(&self) -> Result<(), anyhow::Error> {
        // Fast path: already running
        {
            let state = self.state.read().await;
            if *state == ServiceState::Running {
                return Ok(());
            }
            if let ServiceState::Error(msg) = &*state {
                return Err(anyhow::anyhow!("Service in error state: {}", msg));
            }
            if *state == ServiceState::Stopped {
                return Err(anyhow::anyhow!("Service has been stopped"));
            }
        }

        // Acquire the init lock to prevent double-initialization
        let _lock = self.init_lock.lock().await;

        // Re-check after acquiring lock
        {
            let state = self.state.read().await;
            if *state == ServiceState::Running {
                return Ok(());
            }
        }

        // Transition to Starting
        {
            let mut state = self.state.write().await;
            *state = ServiceState::Starting;
        }

        info!("Starting service: {}", self.provider.service_name());

        match self.provider.start().await {
            Ok(()) => {
                let mut state = self.state.write().await;
                *state = ServiceState::Running;
                info!("Service started: {}", self.provider.service_name());
                Ok(())
            }
            Err(e) => {
                let msg = e.to_string();
                error!(
                    "Service failed to start {}: {}",
                    self.provider.service_name(),
                    msg
                );
                let mut state = self.state.write().await;
                *state = ServiceState::Error(msg.clone());
                Err(anyhow::anyhow!("{}", msg))
            }
        }
    }

    /// Stop the service.
    pub async fn stop(&self) -> Result<(), anyhow::Error> {
        {
            let mut state = self.state.write().await;
            *state = ServiceState::Stopping;
        }
        self.provider.stop().await?;
        {
            let mut state = self.state.write().await;
            *state = ServiceState::Stopped;
        }
        Ok(())
    }
}
