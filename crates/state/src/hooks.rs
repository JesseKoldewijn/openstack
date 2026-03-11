use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StateFailureClass {
    SaveFailure,
    LoadFailure,
    RecoveryInconsistency,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StateFailureDiagnostic {
    pub failure_class: StateFailureClass,
    pub service: String,
    pub operation: String,
    pub path: Option<PathBuf>,
    pub message: String,
}

/// Lifecycle hooks for state management.
///
/// Services can implement this trait to be notified before/after save, load, and reset.
#[async_trait::async_trait]
pub trait StateHooks: Send + Sync {
    /// Called before state is saved to disk.
    async fn on_before_state_save(&self) {}
    /// Called after state has been successfully saved.
    async fn on_after_state_save(&self) {}

    /// Called before state is loaded from disk.
    async fn on_before_state_load(&self) {}
    /// Called after state has been successfully loaded.
    async fn on_after_state_load(&self) {}

    /// Called before state is reset (cleared).
    async fn on_before_state_reset(&self) {}
    /// Called after state has been reset.
    async fn on_after_state_reset(&self) {}

    /// Called when a save operation fails.
    async fn on_state_save_error(&self, _diagnostic: &StateFailureDiagnostic) {}
    /// Called when a load operation fails.
    async fn on_state_load_error(&self, _diagnostic: &StateFailureDiagnostic) {}
    /// Called when recovery consistency checks fail.
    async fn on_state_recovery_error(&self, _diagnostic: &StateFailureDiagnostic) {}
}

/// A no-op hooks implementation — use as default.
pub struct NoopHooks;

#[async_trait::async_trait]
impl StateHooks for NoopHooks {}
