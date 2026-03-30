use std::sync::atomic::{AtomicU64, AtomicU8, AtomicUsize, Ordering};
use std::sync::Arc;

use tokio::sync::{Mutex, RwLock};
use tracing::{error, info};

use crate::lifecycle::ServiceState;
use crate::traits::ServiceProvider;

// Atomic state constants — must mirror ServiceState discriminants
const ATOMIC_AVAILABLE: u8 = 0;
const ATOMIC_STARTING: u8 = 1;
const ATOMIC_RUNNING: u8 = 2;
const ATOMIC_STOPPING: u8 = 3;
const ATOMIC_STOPPED: u8 = 4;
const ATOMIC_ERROR: u8 = 5;

/// Wraps a `ServiceProvider` with lifecycle state tracking and a loading lock
/// to prevent concurrent initialization.
pub struct ServiceContainer {
    pub provider: Arc<dyn ServiceProvider>,
    state: Arc<RwLock<ServiceState>>,
    /// Atomic mirror of `state` for lock-free hot-path reads.
    ///
    /// Invariant: updated with `Ordering::Release` immediately *after* the
    /// corresponding `RwLock` write completes, so any thread that observes
    /// `ATOMIC_RUNNING` via `Ordering::Acquire` is guaranteed to see all
    /// side effects of the preceding `start()` call.
    atomic_state: AtomicU8,
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
            atomic_state: AtomicU8::new(ATOMIC_AVAILABLE),
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
        // Hot-path fast check: no lock acquisition when already running.
        // Acquire ordering pairs with the Release store set after start() succeeds.
        match self.atomic_state.load(Ordering::Acquire) {
            ATOMIC_RUNNING => return Ok(()),
            ATOMIC_ERROR => {
                // Fall through to get the error message from the RwLock.
            }
            ATOMIC_STOPPED => return Err(anyhow::anyhow!("Service has been stopped")),
            _ => {}
        }

        // Slow path: check RwLock for detailed state (error message, or not yet running)
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
            self.atomic_state.store(ATOMIC_STARTING, Ordering::Relaxed);
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
                {
                    let mut state = self.state.write().await;
                    *state = ServiceState::Running;
                }
                // Release store: pairs with Acquire load in the hot path.
                // All side effects of start() are visible to any thread that
                // subsequently observes ATOMIC_RUNNING via Acquire.
                self.atomic_state.store(ATOMIC_RUNNING, Ordering::Release);
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
                {
                    let mut state = self.state.write().await;
                    *state = ServiceState::Error(msg.clone());
                }
                self.atomic_state.store(ATOMIC_ERROR, Ordering::Release);
                Err(anyhow::anyhow!("{}", msg))
            }
        }
    }

    /// Stop the service.
    pub async fn stop(&self) -> Result<(), anyhow::Error> {
        {
            let mut state = self.state.write().await;
            *state = ServiceState::Stopping;
            self.atomic_state.store(ATOMIC_STOPPING, Ordering::Relaxed);
        }
        self.provider.stop().await?;
        {
            let mut state = self.state.write().await;
            *state = ServiceState::Stopped;
            self.atomic_state.store(ATOMIC_STOPPED, Ordering::Release);
        }
        Ok(())
    }
}
