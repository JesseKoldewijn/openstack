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
    /// S3 object data directory (S3_OBJECT_STORE_DIR or {data}/s3/objects)
    pub s3_objects: PathBuf,
    /// Spool directory for temporary body spooling
    pub spool: PathBuf,
}

impl Directories {
    /// Create a `Directories` layout rooted at `data`, suitable for tests.
    pub fn from_root(data: impl Into<PathBuf>) -> Self {
        let data = data.into();
        let state = data.join("state");
        let cache = data.join("cache");
        let tmp = data.join("tmp");
        let logs = data.join("logs");
        let init = data.join("init");
        let init_boot = init.join("boot.d");
        let init_start = init.join("start.d");
        let init_ready = init.join("ready.d");
        let init_shutdown = init.join("shutdown.d");
        let s3_objects = data.join("s3").join("objects");
        let spool = data.join("spool");
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
            s3_objects,
            spool,
        }
    }

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

        let s3_objects = std::env::var("S3_OBJECT_STORE_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| data.join("s3").join("objects"));

        let spool = data.join("spool");

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
            s3_objects,
            spool,
        }
    }
}
