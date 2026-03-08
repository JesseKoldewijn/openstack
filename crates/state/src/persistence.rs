use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tracing::info;

/// Serialize a store to a JSON file at the given path.
pub async fn save_store<S: Serialize>(store: &S, path: &Path) -> Result<(), anyhow::Error> {
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let json = serde_json::to_vec_pretty(store)?;
    tokio::fs::write(path, &json).await?;
    info!("Saved state to {:?}", path);
    Ok(())
}

/// Load a store from a JSON file. Returns the default if the file does not exist.
pub async fn load_store<S: for<'de> Deserialize<'de> + Default>(
    path: &Path,
) -> Result<S, anyhow::Error> {
    match tokio::fs::read(path).await {
        Ok(bytes) => {
            let store = serde_json::from_slice(&bytes)?;
            info!("Loaded state from {:?}", path);
            Ok(store)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(S::default()),
        Err(e) => Err(e.into()),
    }
}

/// Returns the canonical path for a service store snapshot.
///
/// Layout: `{data_dir}/state/{service}/{account_id}/{region}/store.json`
pub fn state_path(data_dir: &Path, service: &str, account_id: &str, region: &str) -> PathBuf {
    data_dir
        .join("state")
        .join(service)
        .join(account_id)
        .join(region)
        .join("store.json")
}

/// A trait for types that can persist themselves to/from disk.
///
/// Implement this on your service store struct to enable snapshot-based persistence.
#[async_trait::async_trait]
pub trait PersistableStore: Send + Sync {
    /// The service name (e.g. "s3", "sqs").
    fn service_name(&self) -> &str;

    /// Save all account+region stores to `{data_dir}/state/{service}/...`
    async fn save(&self, data_dir: &Path) -> Result<(), anyhow::Error>;

    /// Load all account+region stores from `{data_dir}/state/{service}/...`
    async fn load(&self, data_dir: &Path) -> Result<(), anyhow::Error>;

    /// Clear all in-memory state.
    fn reset(&self);
}
