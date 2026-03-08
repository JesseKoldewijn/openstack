use std::path::PathBuf;

/// Directory paths for openstack data, matching LocalStack's layout.
#[derive(Debug, Clone)]
pub struct Directories {
    /// Root data directory (LOCALSTACK_DATA_DIR or /var/lib/localstack)
    pub data: PathBuf,
    /// State/persistence directory
    pub state: PathBuf,
    /// Cache directory
    pub cache: PathBuf,
    /// Temporary files directory
    pub tmp: PathBuf,
    /// Logs directory
    pub logs: PathBuf,
    /// Init scripts root (contains boot.d, start.d, ready.d, shutdown.d)
    pub init: PathBuf,
    /// Boot scripts
    pub init_boot: PathBuf,
    /// Start scripts
    pub init_start: PathBuf,
    /// Ready scripts
    pub init_ready: PathBuf,
    /// Shutdown scripts
    pub init_shutdown: PathBuf,
}

impl Directories {
    pub fn from_env() -> Self {
        let data = std::env::var("LOCALSTACK_DATA_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("/var/lib/localstack"));

        let state = std::env::var("PERSISTENCE_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| data.join("state"));

        let cache = data.join("cache");
        let tmp = data.join("tmp");
        let logs = data.join("logs");

        let init = PathBuf::from(
            std::env::var("LOCALSTACK_INIT_DIR")
                .unwrap_or_else(|_| "/etc/localstack/init".to_string()),
        );
        let init_boot = init.join("boot.d");
        let init_start = init.join("start.d");
        let init_ready = init.join("ready.d");
        let init_shutdown = init.join("shutdown.d");

        Self {
            data,
            state,
            cache,
            tmp,
            logs,
            init,
            init_boot,
            init_start,
            init_ready,
            init_shutdown,
        }
    }
}
