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
}

/// A no-op hooks implementation — use as default.
pub struct NoopHooks;

#[async_trait::async_trait]
impl StateHooks for NoopHooks {}
