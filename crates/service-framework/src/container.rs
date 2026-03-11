use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
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
    startup_attempts: AtomicUsize,
    startup_wait_count: AtomicUsize,
    last_startup_duration_ms: AtomicU64,
}

#[derive(Debug, Clone)]
pub struct ServiceRuntimeMetrics {
    pub startup_attempts: usize,
    pub startup_wait_count: usize,
    pub last_startup_duration_ms: u64,
}

impl ServiceContainer {
    pub fn new(provider: Arc<dyn ServiceProvider>) -> Self {
        Self {
            provider,
            state: Arc::new(RwLock::new(ServiceState::Available)),
            init_lock: Arc::new(Mutex::new(())),
            startup_attempts: AtomicUsize::new(0),
            startup_wait_count: AtomicUsize::new(0),
            last_startup_duration_ms: AtomicU64::new(0),
        }
    }

    pub fn runtime_metrics(&self) -> ServiceRuntimeMetrics {
        ServiceRuntimeMetrics {
            startup_attempts: self.startup_attempts.load(Ordering::Relaxed),
            startup_wait_count: self.startup_wait_count.load(Ordering::Relaxed),
            last_startup_duration_ms: self.last_startup_duration_ms.load(Ordering::Relaxed),
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
        self.startup_wait_count.fetch_add(1, Ordering::Relaxed);
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
        self.startup_attempts.fetch_add(1, Ordering::Relaxed);
        let startup_started = std::time::Instant::now();

        match self.provider.start().await {
            Ok(()) => {
                self.last_startup_duration_ms.store(
                    startup_started.elapsed().as_millis() as u64,
                    Ordering::Relaxed,
                );
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
