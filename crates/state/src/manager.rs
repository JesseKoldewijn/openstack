use std::sync::Arc;

use openstack_config::{Config, SnapshotLoadStrategy, SnapshotSaveStrategy};
use tokio::sync::RwLock;
use tokio::time;
use tracing::{error, info};

use crate::hooks::{NoopHooks, StateHooks};
use crate::persistence::PersistableStore;

/// Central manager that orchestrates snapshot save/load/reset across all service stores.
pub struct StateManager {
    config: Config,
    stores: Arc<RwLock<Vec<Arc<dyn PersistableStore>>>>,
    hooks: Arc<dyn StateHooks>,
}

impl StateManager {
    pub fn new(config: Config) -> Self {
        Self {
            config,
            stores: Arc::new(RwLock::new(Vec::new())),
            hooks: Arc::new(NoopHooks),
        }
    }

    /// Create a `StateManager` with a custom hooks implementation.
    pub fn with_hooks(config: Config, hooks: Arc<dyn StateHooks>) -> Self {
        Self {
            config,
            stores: Arc::new(RwLock::new(Vec::new())),
            hooks,
        }
    }

    /// Register a persistable store.  Services call this during initialisation.
    pub async fn register_store(&self, store: Arc<dyn PersistableStore>) {
        self.stores.write().await.push(store);
    }

    /// Load state from disk according to `SNAPSHOT_LOAD_STRATEGY`.
    pub async fn load_on_startup(&self) -> Result<(), anyhow::Error> {
        if !self.config.persistence {
            return Ok(());
        }
        if self.config.snapshot_load_strategy != SnapshotLoadStrategy::OnStartup {
            return Ok(());
        }
        self.load_all().await
    }

    /// Load state from disk on-demand (for `ON_REQUEST` strategy).
    pub async fn load_on_request(&self) -> Result<(), anyhow::Error> {
        if !self.config.persistence {
            return Ok(());
        }
        if self.config.snapshot_load_strategy != SnapshotLoadStrategy::OnRequest {
            return Ok(());
        }
        self.load_all().await
    }

    /// Save state to disk according to `SNAPSHOT_SAVE_STRATEGY` (called on shutdown).
    pub async fn save_on_shutdown(&self) -> Result<(), anyhow::Error> {
        if !self.config.persistence {
            return Ok(());
        }
        if self.config.snapshot_save_strategy != SnapshotSaveStrategy::OnShutdown {
            return Ok(());
        }
        self.save_all().await
    }

    /// Save state to disk on-demand (for `ON_REQUEST` strategy).
    pub async fn save_on_request(&self) -> Result<(), anyhow::Error> {
        if !self.config.persistence {
            return Ok(());
        }
        if self.config.snapshot_save_strategy != SnapshotSaveStrategy::OnRequest {
            return Ok(());
        }
        self.save_all().await
    }

    /// Force an immediate save of all stores (for `MANUAL` and internal use).
    pub async fn save_now(&self) -> Result<(), anyhow::Error> {
        if !self.config.persistence {
            return Ok(());
        }
        self.save_all().await
    }

    /// Force an immediate load of all stores.
    pub async fn load_now(&self) -> Result<(), anyhow::Error> {
        if !self.config.persistence {
            return Ok(());
        }
        self.load_all().await
    }

    /// Reset (clear) all in-memory state across all registered stores and invoke hooks.
    pub async fn reset_all(&self) {
        self.hooks.on_before_state_reset().await;
        let stores = self.stores.read().await;
        for store in stores.iter() {
            store.reset();
            info!("Reset state for service: {}", store.service_name());
        }
        self.hooks.on_after_state_reset().await;
    }

    /// Start the scheduled snapshot background task (if `SCHEDULED` strategy is active).
    /// Returns a `JoinHandle` that the caller must keep alive (or abort on shutdown).
    pub fn start_scheduled_snapshots(&self) -> Option<tokio::task::JoinHandle<()>> {
        if !self.config.persistence {
            return None;
        }
        if self.config.snapshot_save_strategy != SnapshotSaveStrategy::Scheduled {
            return None;
        }
        let interval = self.config.snapshot_flush_interval;
        let stores = Arc::clone(&self.stores);
        let data_dir = self.config.directories.data.clone();
        let handle = tokio::spawn(async move {
            let mut ticker = time::interval(interval);
            loop {
                ticker.tick().await;
                info!("Scheduled snapshot triggered");
                let stores_guard = stores.read().await;
                for store in stores_guard.iter() {
                    if let Err(e) = store.save(&data_dir).await {
                        error!(
                            "Scheduled snapshot failed for service '{}': {}",
                            store.service_name(),
                            e
                        );
                    }
                }
            }
        });
        Some(handle)
    }

    // ── Private helpers ──────────────────────────────────────────────────────

    async fn save_all(&self) -> Result<(), anyhow::Error> {
        self.hooks.on_before_state_save().await;
        let stores = self.stores.read().await;
        let data_dir = &self.config.directories.data;
        for store in stores.iter() {
            store.save(data_dir).await.map_err(|e| {
                anyhow::anyhow!("Failed to save state for '{}': {}", store.service_name(), e)
            })?;
        }
        self.hooks.on_after_state_save().await;
        info!("All state saved to {:?}", data_dir);
        Ok(())
    }

    async fn load_all(&self) -> Result<(), anyhow::Error> {
        self.hooks.on_before_state_load().await;
        let stores = self.stores.read().await;
        let data_dir = &self.config.directories.data;
        for store in stores.iter() {
            store.load(data_dir).await.map_err(|e| {
                anyhow::anyhow!("Failed to load state for '{}': {}", store.service_name(), e)
            })?;
        }
        self.hooks.on_after_state_load().await;
        info!("All state loaded from {:?}", data_dir);
        Ok(())
    }
}
