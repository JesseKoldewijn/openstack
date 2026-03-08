use tracing_subscriber::{EnvFilter, fmt};

use crate::{Config, LogLevel};

/// Initialize the tracing subscriber based on the config.
pub fn init(config: &Config) {
    let level_str = match config.log_level {
        LogLevel::Trace => "trace",
        LogLevel::Debug => "debug",
        LogLevel::Info => "info",
        LogLevel::Warn => "warn",
        LogLevel::Error => "error",
    };

    // Allow RUST_LOG to override
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(level_str));

    fmt()
        .with_env_filter(filter)
        .with_target(true)
        .with_thread_ids(false)
        .init();
}
