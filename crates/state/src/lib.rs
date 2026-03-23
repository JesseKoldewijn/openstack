pub mod bundle;
pub mod hooks;
pub mod manager;
pub mod persistence;
pub mod scoping;

pub use bundle::{AccountBundle, AccountRegionBundle};
pub use hooks::{NoopHooks, StateFailureClass, StateFailureDiagnostic, StateHooks};
pub use manager::StateManager;
pub use persistence::{PersistableStore, load_store, save_store, state_path};
pub use scoping::{
    AccountId, AccountRegionKey, CrossAccountAttribute, CrossRegionAttribute, LocalAttribute,
    Region,
};
