use std::fs::OpenOptions;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};

const DAEMON_CHILD_ENV: &str = "OPENSTACK_DAEMON_CHILD";

#[derive(Debug, Clone, PartialEq, Eq)]
enum CliCommand {
    RunForeground,
    Start { daemon: bool },
    Stop,
    Status,
    Restart,
    Logs { follow: bool },
    Studio { print_url: bool },
    Help,
    Version,
}

#[derive(Debug, Clone)]
struct DaemonPaths {
    dir: PathBuf,
    lock: PathBuf,
    pid: PathBuf,
    meta: PathBuf,
    log: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DaemonMetadata {
    pid: i32,
    started_at_utc: String,
    health_url: String,
    log_path: String,
    command: String,
}

/// Entrypoint for the CLI: parses command-line arguments, initializes configuration and logging, and dispatches the requested command.
///
/// This function:
/// - Parses CLI arguments into a CliCommand.
/// - Handles Help and Version flags immediately.
/// - Loads configuration from the environment and initializes tracing/logging.
/// - Dispatches to the appropriate handler (run server, daemon lifecycle commands, logs, studio, etc.).
///
/// # Examples
///
/// ```no_run
/// // Run the CLI binary (examples assume running from project root):
/// // cargo run --release -- --help
/// ```
#[tokio::main]
async fn main() -> Result<()> {
    let command = parse_cli_command(std::env::args().skip(1).collect::<Vec<_>>().as_slice())?;

    if command == CliCommand::Help {
        print_help();
        return Ok(());
    }
    if command == CliCommand::Version {
        println!("openstack {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }

    // Initialize configuration from environment
    let config = openstack_config::Config::from_env()?;

    // Initialize tracing/logging
    openstack_config::logging::init(&config);

    match command {
        CliCommand::RunForeground => run_server(config).await,
        CliCommand::Start { daemon } => {
            if daemon {
                daemon_start(&config).await
            } else {
                run_server(config).await
            }
        }
        CliCommand::Stop => daemon_stop(&config).await,
        CliCommand::Status => daemon_status(&config).await,
        CliCommand::Restart => daemon_restart(&config).await,
        CliCommand::Logs { follow } => daemon_logs(&config, follow).await,
        CliCommand::Studio { print_url } => studio_open(&config, print_url).await,
        CliCommand::Help | CliCommand::Version => Ok(()),
    }
}

/// Starts and runs the gateway and associated background services according to `config`.
///
/// This initializes the service plugin manager and state manager (registering persistable stores),
/// cleans up any stale spool temp files, optionally starts the DNS server, and runs the gateway until shutdown.
///
/// # Returns
///
/// `Ok(())` on clean shutdown, or an error if startup or runtime operations fail.
///
/// # Examples
///
/// ```no_run
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// let config = openstack_config::Config::default();
/// // This call runs until the gateway shuts down.
/// run_server(config).await?;
/// # Ok(()) }
/// ```
async fn run_server(config: openstack_config::Config) -> Result<()> {
    info!("Starting openstack v{}", env!("CARGO_PKG_VERSION"));
    info!("Gateway listening on: {:?}", config.gateway_listen);

    // Initialize the service plugin manager
    let plugin_manager = openstack_service_framework::ServicePluginManager::new(config.clone());

    // Register all built-in service providers and collect persistable stores
    let persistable_stores = register_services(&plugin_manager, &config);

    // Initialize state and register persistable stores before loading
    let state_manager = openstack_state::StateManager::new(config.clone());
    for store in persistable_stores {
        state_manager.register_store(store).await;
    }
    state_manager.load_on_startup().await?;

    // Clean up orphaned spool temp files from previous crashes
    cleanup_spool_dir(&config.directories.spool).await;

    // Start internal API server and gateway
    let gateway = openstack_gateway::Gateway::new(config.clone(), plugin_manager.clone());

    // Start DNS server if configured
    let dns_handle = if config.dns_enabled() {
        let dns = openstack_dns::DnsServer::new(config.clone());
        Some(tokio::spawn(async move { dns.run().await }))
    } else {
        None
    };

    // Run the gateway (blocks until shutdown)
    gateway.run(state_manager).await?;

    // Shutdown DNS if running
    if let Some(handle) = dns_handle {
        handle.abort();
    }

    info!("openstack shut down cleanly");
    Ok(())
}

/// Parse a slice of CLI arguments into a `CliCommand`.
///
/// The `args` slice should contain only the command and its options (it does not need to include the program name).
/// When `args` is empty, returns `CliCommand::RunForeground`. Recognized commands and options:
/// - `-h`, `--help` → `Help`
/// - `-V`, `--version` → `Version`
/// - `start [--daemon]` → `Start { daemon: bool }`
/// - `stop` → `Stop`
/// - `status` → `Status`
/// - `restart` → `Restart`
/// - `logs [--follow | -f]` → `Logs { follow: bool }`
/// - `studio [--print-url]` → `Studio { print_url: bool }`
///
/// Unknown commands or unrecognized options for a known command produce an `Err`.
///
/// # Examples
///
/// ```
/// # use anyhow::Result;
/// # use crate::CliCommand;
/// # fn example() -> Result<()> {
/// let cmd = crate::parse_cli_command(&[String::from("start"), String::from("--daemon")])?;
/// assert!(matches!(cmd, CliCommand::Start { daemon: true }));
///
/// let cmd = crate::parse_cli_command(&[String::from("logs")])?;
/// assert!(matches!(cmd, CliCommand::Logs { follow: false }));
/// # Ok(()) }
/// ```
fn parse_cli_command(args: &[String]) -> Result<CliCommand> {
    if args.is_empty() {
        return Ok(CliCommand::RunForeground);
    }

    match args[0].as_str() {
        "-h" | "--help" => Ok(CliCommand::Help),
        "-V" | "--version" => Ok(CliCommand::Version),
        "start" => {
            let daemon = args.iter().skip(1).any(|a| a == "--daemon");
            for arg in args.iter().skip(1) {
                if arg != "--daemon" {
                    return Err(anyhow!("Unknown start option: {arg}"));
                }
            }
            Ok(CliCommand::Start { daemon })
        }
        "stop" => Ok(CliCommand::Stop),
        "status" => Ok(CliCommand::Status),
        "restart" => Ok(CliCommand::Restart),
        "logs" => {
            let follow = args.iter().skip(1).any(|a| a == "--follow" || a == "-f");
            for arg in args.iter().skip(1) {
                if arg != "--follow" && arg != "-f" {
                    return Err(anyhow!("Unknown logs option: {arg}"));
                }
            }
            Ok(CliCommand::Logs { follow })
        }
        "studio" => {
            let print_url = args.iter().skip(1).any(|a| a == "--print-url");
            for arg in args.iter().skip(1) {
                if arg != "--print-url" {
                    return Err(anyhow!("Unknown studio option: {arg}"));
                }
            }
            Ok(CliCommand::Studio { print_url })
        }
        unknown => Err(anyhow!("Unknown command: {unknown}")),
    }
}

/// Prints the command-line usage and help text for the `openstack` binary.
///
/// # Examples
///
/// ```no_run
/// // Display help to stdout
/// print_help();
/// ```
fn print_help() {
    println!("openstack {}", env!("CARGO_PKG_VERSION"));
    println!("Usage: openstack [command] [options]");
    println!();
    println!("Commands:");
    println!("  start [--daemon]   Start openstack (foreground by default)");
    println!("  stop               Stop managed daemon instance");
    println!("  status             Show managed daemon status");
    println!("  restart            Restart managed daemon instance");
    println!("  logs [--follow]    Show daemon logs");
    println!("  studio [--print-url]  Print/open Studio dashboard URL");
    println!();
    println!("Flags:");
    println!("  -h, --help         Show help");
    println!("  -V, --version      Show version");
}

/// Open the Studio web UI URL resolved for the current configuration and optionally open it in the system browser.
///
/// This resolves a Studio URL using the daemon's health endpoint (if daemon metadata exists) and the configured base URL.
/// If `print_url` is true, the resolved URL is printed and the function returns.
/// If the resolved runtime is not ready, the function prints an informational message and the URL.
/// If the runtime is ready, the function attempts to open the URL in the user's default browser and prints the outcome.
///
/// # Parameters
///
/// - `config`: application configuration used to resolve the Studio URL.
/// - `print_url`: when `true`, print the resolved URL and do not attempt to open a browser.
///
/// # Examples
///
/// ```
/// # use anyhow::Result;
/// # async fn example(config: &openstack_config::Config) -> Result<()> {
/// studio_open(config, true).await?; // prints the resolved Studio URL
/// # Ok(())
/// # }
/// ```
async fn studio_open(config: &openstack_config::Config, print_url: bool) -> Result<()> {
    let paths = daemon_paths(config);

    let daemon_health = if paths.meta.exists() {
        read_meta(&paths.meta)
            .await
            .ok()
            .map(|meta| meta.health_url)
    } else {
        None
    };

    let resolution = openstack_studio_ui::api::resolve_studio_url(
        None,
        daemon_health.as_deref(),
        &config.base_url(),
    )
    .await;

    if print_url {
        println!("{}", resolution.url);
        return Ok(());
    }

    if !resolution.daemon_ready {
        println!("Studio runtime is not ready yet.");
        println!("URL: {}", resolution.url);
        println!("Tip: start daemon with 'openstack start --daemon' and retry.");
        return Ok(());
    }

    if try_open_browser(&resolution.url).is_ok() {
        println!("Opened Studio: {}", resolution.url);
    } else {
        println!("Could not open browser automatically.");
        println!("Studio URL: {}", resolution.url);
    }

    Ok(())
}

/// Attempts to open the provided URL in the system's default web browser.
///
/// # Errors
/// Returns an error if the platform's mechanism for launching a browser fails.
///
/// # Examples
///
/// ```
/// # use anyhow::Result;
/// # fn doc() -> Result<()> {
/// try_open_browser("https://example.com")?;
/// # Ok(())
/// # }
/// ```
fn try_open_browser(url: &str) -> Result<()> {
    #[cfg(target_os = "linux")]
    {
        if Command::new("xdg-open")
            .arg(url)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
        {
            return Ok(());
        }
    }

    #[cfg(target_os = "macos")]
    {
        if Command::new("open")
            .arg(url)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
        {
            return Ok(());
        }
    }

    #[cfg(target_os = "windows")]
    {
        if Command::new("cmd")
            .args(["/C", "start", "", url])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
        {
            return Ok(());
        }
    }

    Err(anyhow!("failed to open browser"))
}

/// Builds daemon-related filesystem paths inside the configured data directory.
///
/// The returned `DaemonPaths` contains the directory plus paths for the daemon lock,
/// pid file, metadata JSON, and log file (named `openstack.lock`, `openstack.pid`,
/// `openstack.meta.json`, and `openstack.log` respectively).
///
/// # Examples
///
/// ```
/// // Given a Config with a data directory, the daemon paths live under `<data>/daemon`.
/// let cfg = openstack_config::Config {
///     directories: openstack_config::Directories { data: std::path::PathBuf::from("/var/lib/openstack"), ..Default::default() },
///     ..Default::default()
/// };
/// let paths = daemon_paths(&cfg);
/// assert!(paths.dir.ends_with("daemon"));
/// assert!(paths.pid.ends_with("openstack.pid"));
/// assert!(paths.meta.ends_with("openstack.meta.json"));
/// ```
fn daemon_paths(config: &openstack_config::Config) -> DaemonPaths {
    let dir = config.directories.data.join("daemon");
    DaemonPaths {
        lock: dir.join("openstack.lock"),
        pid: dir.join("openstack.pid"),
        meta: dir.join("openstack.meta.json"),
        log: dir.join("openstack.log"),
        dir,
    }
}

/// Starts the application as a background daemon, creating and publishing daemon state files.
///
/// Attempts to create a daemon lock, spawn a child process of the current executable with
/// stdout/stderr redirected to the daemon log, write PID and metadata files, and wait for the
/// daemon health endpoint to become ready. On failure the function removes any created state
/// files and returns an error.
///
/// On success, prints startup information (PID, health URL, log path) to stdout.
///
/// # Errors
///
/// Returns an error if any filesystem operations, process spawn, metadata serialization, or the
/// health check fail.
///
/// # Examples
///
/// ```no_run
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// // Build or obtain a `openstack_config::Config` appropriate for your environment.
/// let config = /* construct config */;
/// openstack::daemon_start(&config).await?;
/// # Ok(())
/// # }
/// ```
async fn daemon_start(config: &openstack_config::Config) -> Result<()> {
    let paths = daemon_paths(config);
    tokio::fs::create_dir_all(&paths.dir).await?;
    recover_stale_state(&paths).await?;

    if paths.lock.exists() {
        let existing = read_pid(&paths.pid).await;
        if existing.is_some_and(is_pid_running) {
            println!(
                "openstack daemon already running (pid {})",
                existing.unwrap_or_default()
            );
            return Ok(());
        }
        remove_state_files(&paths).await?;
    }

    OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&paths.lock)
        .context("failed to create daemon lock file")?;

    let exe = std::env::current_exe().context("failed to resolve current executable")?;
    let log = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&paths.log)
        .context("failed to open daemon log file")?;
    let err = log
        .try_clone()
        .context("failed to clone daemon log handle")?;

    let mut cmd = Command::new(exe);
    cmd.env(DAEMON_CHILD_ENV, "1")
        .stdin(Stdio::null())
        .stdout(Stdio::from(log))
        .stderr(Stdio::from(err));

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // SAFETY: setsid has no Rust-level invariants and is called in child before exec.
        unsafe {
            cmd.pre_exec(|| {
                if libc::setsid() < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
    }

    let child = cmd.spawn().context("failed to spawn daemon process")?;
    let pid = child.id() as i32;

    tokio::fs::write(&paths.pid, pid.to_string()).await?;
    let meta = DaemonMetadata {
        pid,
        started_at_utc: chrono::Utc::now().to_rfc3339(),
        health_url: format!("{}/_localstack/health", config.base_url()),
        log_path: paths.log.display().to_string(),
        command: "openstack".to_string(),
    };
    tokio::fs::write(&paths.meta, serde_json::to_vec_pretty(&meta)?).await?;

    let started = wait_for_health(&meta.health_url, Duration::from_secs(15)).await;
    if !started {
        let _ = send_signal(pid, libc::SIGKILL);
        remove_state_files(&paths).await?;
        return Err(anyhow!(
            "daemon failed startup validation (health endpoint not ready)"
        ));
    }

    println!("openstack daemon started (pid {pid})");
    println!("health: {}", meta.health_url);
    println!("logs: {}", meta.log_path);
    Ok(())
}

/// Prints the daemon's current status and associated metadata.
///
/// Reads the stored daemon metadata, evaluates whether the recorded PID is running and whether the daemon's health endpoint is responding, then prints the status ("running", "degraded", or "not-running"), PID, health URL, and log path to stdout.
///
/// # Errors
/// Returns an error if reading or parsing the metadata file or performing the health check fails.
///
/// # Examples
///
/// ```no_run
/// # use openstack_config::Config;
/// # async fn example(config: Config) -> anyhow::Result<()> {
/// daemon_status(&config).await?;
/// # Ok(())
/// # }
/// ```
async fn daemon_status(config: &openstack_config::Config) -> Result<()> {
    let paths = daemon_paths(config);
    if !paths.meta.exists() {
        println!("status: not-running");
        return Ok(());
    }

    let meta = read_meta(&paths.meta).await?;
    let process_running = is_pid_running(meta.pid);
    let health_ok = is_health_ok(&meta.health_url).await;

    let status = if process_running && health_ok {
        "running"
    } else if process_running {
        "degraded"
    } else {
        "not-running"
    };

    println!("status: {status}");
    println!("pid: {}", meta.pid);
    println!("health: {}", meta.health_url);
    println!("log: {}", meta.log_path);
    Ok(())
}

/// Stops the managed daemon if it is running, attempting a graceful shutdown and falling back to forced termination.
///
/// This will:
/// - Return immediately if no daemon metadata is present (daemon not running).
/// - Request the daemon to shut down via its health URL, wait for the process to exit, and on success remove daemon state files.
/// - If the daemon does not exit, send `SIGTERM` and wait; if still running, send `SIGKILL` and remove state files.
///
/// Errors from filesystem operations, HTTP client construction, or signal delivery are returned.
///
/// # Examples
///
/// ```
/// # use anyhow::Result;
/// # #[tokio::main]
/// # async fn main() -> Result<()> {
/// let config = openstack_config::Config::default();
/// daemon_stop(&config).await?;
/// # Ok(())
/// # }
/// ```
async fn daemon_stop(config: &openstack_config::Config) -> Result<()> {
    let paths = daemon_paths(config);
    if !paths.meta.exists() {
        println!("openstack daemon is not running");
        return Ok(());
    }
    let meta = read_meta(&paths.meta).await?;

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()?;
    let _ = client
        .post(&meta.health_url)
        .json(&serde_json::json!({"action":"kill"}))
        .send()
        .await;

    if wait_for_exit(meta.pid, Duration::from_secs(8)).await {
        remove_state_files(&paths).await?;
        println!("openstack daemon stopped");
        return Ok(());
    }

    let _ = send_signal(meta.pid, libc::SIGTERM);
    if wait_for_exit(meta.pid, Duration::from_secs(4)).await {
        remove_state_files(&paths).await?;
        println!("openstack daemon stopped");
        return Ok(());
    }

    let _ = send_signal(meta.pid, libc::SIGKILL);
    let _ = wait_for_exit(meta.pid, Duration::from_secs(2)).await;
    remove_state_files(&paths).await?;
    println!("openstack daemon stopped (forced)");
    Ok(())
}

/// Restarts the daemon by stopping it and then starting it again.
///
/// Returns `Ok(())` on success, or an error if stopping or starting the daemon fails.
///
/// # Examples
///
/// ```no_run
/// # async fn try_example() -> anyhow::Result<()> {
/// let config = openstack_config::Config::from_env()?; // load your config appropriately
/// daemon_restart(&config).await?;
/// # Ok(())
/// # }
/// ```
async fn daemon_restart(config: &openstack_config::Config) -> Result<()> {
    daemon_stop(config).await?;
    daemon_start(config).await
}

/// Prints recent daemon log lines to stdout and, if `follow` is true, tails the log.
///
/// Reads the daemon log file, prints the last 200 lines (or fewer if the file is smaller),
/// and when `follow` is enabled continues printing new appended data until the log file is removed.
///
/// # Returns
///
/// `Ok(())` on success; returns an error if reading the log file fails for reasons other than the file being removed while following.
///
/// # Examples
///
/// ```ignore
/// # use anyhow::Result;
/// # use tokio;
/// # async fn example() -> Result<()> {
/// let config = openstack_config::Config::default();
/// // Print last 200 lines and follow new entries
/// daemon_logs(&config, true).await?;
/// # Ok(())
/// # }
/// ```
async fn daemon_logs(config: &openstack_config::Config, follow: bool) -> Result<()> {
    let paths = daemon_paths(config);
    if !paths.log.exists() {
        println!("No daemon log file at {}", paths.log.display());
        return Ok(());
    }

    let initial = tokio::fs::read_to_string(&paths.log)
        .await
        .unwrap_or_default();
    let lines: Vec<&str> = initial.lines().collect();
    let start = lines.len().saturating_sub(200);
    for line in &lines[start..] {
        println!("{line}");
    }

    if !follow {
        return Ok(());
    }

    let mut offset = initial.len();
    loop {
        tokio::time::sleep(Duration::from_millis(500)).await;
        match tokio::fs::read_to_string(&paths.log).await {
            Ok(content) => {
                if content.len() > offset {
                    print!("{}", &content[offset..]);
                    offset = content.len();
                }
            }
            Err(e) if e.kind() == ErrorKind::NotFound => break,
            Err(e) => return Err(e.into()),
        }
    }
    Ok(())
}

/// Removes stale daemon state files when the recorded PID is absent or no longer running.
///
/// Reads the PID from `paths.pid`. If the PID is missing or the process is not running,
/// the function removes the lock, pid, and meta files referenced by `paths`.
///
/// Returns `Ok(())` on success, or an error if file operations fail.
///
/// # Examples
///
/// ```
/// // Given a prepared `paths: DaemonPaths`, remove stale state if present.
/// # async fn _example(paths: &DaemonPaths) -> anyhow::Result<()> {
/// recover_stale_state(paths).await?;
/// # Ok(())
/// # }
/// ```
async fn recover_stale_state(paths: &DaemonPaths) -> Result<()> {
    let pid = read_pid(&paths.pid).await;
    if pid.is_none() || !pid.is_some_and(is_pid_running) {
        remove_state_files(paths).await?;
    }
    Ok(())
}

/// Removes daemon state files (lock, pid, and meta) if they exist.
///
/// This function attempts to delete the lock, pid, and meta files referenced by
/// `paths`. Missing files are ignored; any other I/O error is returned.
///
/// # Parameters
///
/// `paths` — DaemonPaths containing the `lock`, `pid`, and `meta` file paths to remove.
///
/// # Returns
///
/// `Ok(())` on success; an error if any file removal fails for reasons other than the file not being found.
///
/// # Examples
///
/// ```
/// use std::fs::File;
/// use tempfile::tempdir;
/// use tokio::runtime::Runtime;
/// // construct DaemonPaths similar to your module's definition
/// struct DaemonPaths { lock: std::path::PathBuf, pid: std::path::PathBuf, meta: std::path::PathBuf }
///
/// let rt = Runtime::new().unwrap();
/// let dir = tempdir().unwrap();
/// let lock = dir.path().join("daemon.lock");
/// let pid = dir.path().join("daemon.pid");
/// let meta = dir.path().join("daemon.meta");
/// File::create(&lock).unwrap();
/// File::create(&pid).unwrap();
/// File::create(&meta).unwrap();
/// let paths = DaemonPaths { lock, pid, meta };
///
/// // call the async function
/// rt.block_on(async {
///     remove_state_files(&paths).await.unwrap();
/// });
///
/// assert!(!paths.lock.exists());
/// assert!(!paths.pid.exists());
/// assert!(!paths.meta.exists());
/// ```
async fn remove_state_files(paths: &DaemonPaths) -> Result<()> {
    for path in [&paths.lock, &paths.pid, &paths.meta] {
        match tokio::fs::remove_file(path).await {
            Ok(_) => {}
            Err(e) if e.kind() == ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
    }
    Ok(())
}

/// Reads a PID from the given file and parses it as a 32-bit integer.
///
/// # Returns
///
/// `Some(pid)` if the file exists and contains a parsable `i32` PID (whitespace is ignored), `None` otherwise.
///
/// # Examples
///
/// ```
/// # tokio::runtime::Runtime::new().unwrap().block_on(async {
/// use std::env;
/// use std::path::PathBuf;
///
/// let mut p: PathBuf = env::temp_dir();
/// p.push("read_pid_example.pid");
/// tokio::fs::write(&p, "123\n").await.unwrap();
///
/// assert_eq!(read_pid(&p).await, Some(123));
///
/// let _ = std::fs::remove_file(&p);
/// # });
/// ```
async fn read_pid(path: &Path) -> Option<i32> {
    let content = tokio::fs::read_to_string(path).await.ok()?;
    content.trim().parse::<i32>().ok()
}

/// Reads and deserializes daemon metadata from a JSON file at the given path.

///

/// On success, returns the deserialized `DaemonMetadata`. The function fails if the file

/// cannot be read or if the contents are not valid JSON for `DaemonMetadata`.

///

/// # Examples

///

/// ```

/// use std::fs::File;

/// use std::io::Write;

/// use tempfile::tempdir;

/// use tokio::runtime::Runtime;

///

/// // Prepare a temporary file containing valid DaemonMetadata JSON.

/// let dir = tempdir().unwrap();

/// let path = dir.path().join("meta.json");

/// let mut f = File::create(&path).unwrap();

/// write!(f, r#"{{"pid":1234,"started_at_utc":"2026-01-01T00:00:00Z","health_url":null,"log_path":null,"command":"run"}}"#).unwrap();

///

/// let rt = Runtime::new().unwrap();

/// rt.block_on(async {

///     let meta = read_meta(&path).await.unwrap();

///     assert_eq!(meta.pid, 1234);

/// });

/// ```
async fn read_meta(path: &Path) -> Result<DaemonMetadata> {
    let bytes = tokio::fs::read(path).await?;
    Ok(serde_json::from_slice::<DaemonMetadata>(&bytes)?)
}

/// Checks whether an HTTP endpoint responds with a successful status within one second.
///
/// # Examples
///
/// ```
/// #[tokio::test]
/// async fn health_check_example() {
///     let healthy = is_health_ok("https://example.com/health").await;
///     // `healthy` is `true` when the endpoint returns a 2xx status
///     let _ = healthy;
/// }
/// ```
///
/// # Returns
/// `true` if the request returns an HTTP success status within one second, `false` otherwise.
async fn is_health_ok(url: &str) -> bool {
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(1))
        .build()
    {
        Ok(c) => c,
        Err(_) => return false,
    };

    match client.get(url).send().await {
        Ok(resp) => resp.status().is_success(),
        Err(_) => false,
    }
}

/// Polls a health endpoint until it responds with success or the timeout is reached.
///
/// # Examples
///
/// ```
/// use std::time::Duration;
/// // Synchronously run the async helper for the example.
/// let ok = tokio::runtime::Runtime::new()
///     .unwrap()
///     .block_on(async { crate::wait_for_health("http://127.0.0.1:8080/health", Duration::from_secs(5)).await });
/// // `ok` will be `true` if the endpoint became healthy within 5 seconds.
/// ```
///
/// # Returns
///
/// `true` if the health endpoint returned a successful response before the timeout, `false` otherwise.
async fn wait_for_health(url: &str, timeout: Duration) -> bool {
    let deadline = std::time::Instant::now() + timeout;
    while std::time::Instant::now() < deadline {
        if is_health_ok(url).await {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    false
}

/// Waits until the process with the given PID exits or the timeout elapses.
///
/// Checks the process liveness periodically and returns whether the process
/// terminated before the provided timeout.
///
/// # Examples
///
/// ```no_run
/// use std::time::Duration;
///
/// #[tokio::main]
/// async fn main() {
///     // Spawn a short-lived process (platform-dependent example)
///     let mut child = std::process::Command::new("sleep")
///         .arg("1")
///         .spawn()
///         .expect("spawn failed");
///     let pid = child.id() as i32;
///
///     // Wait up to 2 seconds for the child to exit
///     let exited = wait_for_exit(pid, Duration::from_secs(2)).await;
///     assert!(exited);
/// }
/// ```
async fn wait_for_exit(pid: i32, timeout: Duration) -> bool {
    let deadline = std::time::Instant::now() + timeout;
    while std::time::Instant::now() < deadline {
        if !is_pid_running(pid) {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    false
}

/// Checks whether a process with the given PID is currently running on the host.
///
/// On Unix platforms this performs an liveness probe for the PID. On non-Unix platforms this
/// always returns `false`.
///
/// # Examples
///
/// ```
/// # fn example() {
/// let pid = std::process::id() as i32;
/// #[cfg(unix)]
/// {
///     // current process should be reported as running on Unix
///     assert!(crate::is_pid_running(pid));
/// }
/// #[cfg(not(unix))]
/// {
///     // non-Unix implementations always return false
///     assert!(!crate::is_pid_running(pid));
/// }
/// # }
/// ```
fn is_pid_running(pid: i32) -> bool {
    #[cfg(unix)]
    {
        // SAFETY: kill with signal 0 is a standard liveness probe and does not mutate memory.
        let rc = unsafe { libc::kill(pid, 0) };
        if rc == 0 {
            return true;
        }
        std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
        false
    }
}

/// Sends a signal to the process identified by `pid`.
///
/// On Unix, this invokes the underlying `kill(2)` syscall with the provided signal number.
/// On non-Unix platforms, this returns an error indicating signals are unsupported.
///
/// # Parameters
///
/// - `pid`: Process identifier to receive the signal.
/// - `signal`: Platform signal number to send (POSIX signal numbers on Unix, e.g., `libc::SIGTERM`).
///
/// # Returns
///
/// `Ok(())` if the signal was successfully delivered; `Err` if the syscall failed (returns the OS error on Unix)
/// or if signals are unsupported on the current platform.
///
/// # Examples
///
/// ```no_run
/// // Attempt to send SIGTERM to PID 12345 and handle potential errors.
/// let res = send_signal(12345, libc::SIGTERM);
/// match res {
///     Ok(()) => println!("signal sent"),
///     Err(e) => eprintln!("failed to send signal: {}", e),
/// }
/// ```
fn send_signal(pid: i32, signal: i32) -> Result<()> {
    #[cfg(unix)]
    {
        // SAFETY: kill sends OS signal; no Rust memory invariants affected.
        let rc = unsafe { libc::kill(pid, signal) };
        if rc == 0 {
            return Ok(());
        }
        Err(std::io::Error::last_os_error().into())
    }
    #[cfg(not(unix))]
    {
        let _ = (pid, signal);
        Err(anyhow!("signals are unsupported on this platform"))
    }
}

fn register_services(
    manager: &openstack_service_framework::ServicePluginManager,
    config: &openstack_config::Config,
) -> Vec<std::sync::Arc<dyn openstack_state::PersistableStore>> {
    let services = &config.services;
    let mut persistable_stores: Vec<std::sync::Arc<dyn openstack_state::PersistableStore>> =
        Vec::new();

    // S3 is special: it has a persistable store that shares state with the provider
    if services.is_enabled("s3") {
        let s3_provider = openstack_s3::S3Provider::new(&config.directories.s3_objects);
        persistable_stores.push(s3_provider.persistable_store());
        manager.register("s3", s3_provider);
    }

    macro_rules! register {
        ($name:literal, $provider:expr) => {
            if services.is_enabled($name) {
                manager.register($name, $provider);
            }
        };
    }

    register!("sqs", openstack_sqs::SqsProvider::new());
    register!("sns", openstack_sns::SnsProvider::new());
    register!("dynamodb", openstack_dynamodb::DynamoDbProvider::new());
    register!("lambda", openstack_lambda::LambdaProvider::new());
    register!("iam", openstack_iam::IamProvider::new());
    register!("sts", openstack_sts::StsProvider::new());
    register!("kms", openstack_kms::KmsProvider::new());
    register!(
        "cloudformation",
        openstack_cloudformation::CloudFormationProvider::new()
    );
    register!(
        "cloudwatch",
        openstack_cloudwatch::CloudWatchProvider::new()
    );
    register!("kinesis", openstack_kinesis::KinesisProvider::new());
    register!("firehose", openstack_firehose::FirehoseProvider::new());
    register!("events", openstack_eventbridge::EventBridgeProvider::new());
    register!(
        "states",
        openstack_stepfunctions::StepFunctionsProvider::new()
    );
    register!(
        "apigateway",
        openstack_apigateway::ApiGatewayProvider::new()
    );
    register!("ec2", openstack_ec2::Ec2Provider::new());
    register!("route53", openstack_route53::Route53Provider::new());
    register!("ses", openstack_ses::SesProvider::new());
    register!("ssm", openstack_ssm::SsmProvider::new());
    register!(
        "secretsmanager",
        openstack_secretsmanager::SecretsManagerProvider::new()
    );
    register!("acm", openstack_acm::AcmProvider::new());
    register!("ecr", openstack_ecr::EcrProvider::new());
    register!(
        "opensearch",
        openstack_opensearch::OpenSearchProvider::new()
    );
    register!("redshift", openstack_redshift::RedshiftProvider::new());

    persistable_stores
}

/// Removes leftover `.tmp` files from the given spool directory.
///
/// If the directory does not exist the function returns quietly. Any failures
/// to remove individual files are logged but do not prevent continuing to
/// attempt removal of other files.
///
/// # Examples
///
/// ```
/// use std::path::Path;
/// use tempfile::tempdir;
///
/// // Create a temporary directory with a `.tmp` file and run the cleaner.
/// let dir = tempdir().unwrap();
/// let tmp_path = dir.path().join("orphan.tmp");
/// std::fs::write(&tmp_path, b"data").unwrap();
///
/// // Run the async cleanup using a tokio runtime.
/// let rt = tokio::runtime::Runtime::new().unwrap();
/// rt.block_on(async {
///     cleanup_spool_dir(dir.path()).await;
/// });
///
/// assert!(!tmp_path.exists());
/// ```
async fn cleanup_spool_dir(spool_dir: &std::path::Path) {
    let Ok(mut rd) = tokio::fs::read_dir(spool_dir).await else {
        // Directory may not exist yet — that's fine.
        return;
    };
    let mut cleaned = 0u64;
    while let Ok(Some(entry)) = rd.next_entry().await {
        let path = entry.path();
        if path.extension().is_some_and(|ext| ext == "tmp") {
            if let Err(e) = tokio::fs::remove_file(&path).await {
                warn!("Failed to remove orphaned spool file {:?}: {}", path, e);
            } else {
                cleaned += 1;
            }
        }
    }
    if cleaned > 0 {
        debug!("Cleaned up {} orphaned spool temp files", cleaned);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_command_defaults_to_foreground() {
        assert_eq!(parse_cli_command(&[]).unwrap(), CliCommand::RunForeground);
    }

    #[test]
    fn parse_start_daemon() {
        let args = vec!["start".to_string(), "--daemon".to_string()];
        assert_eq!(
            parse_cli_command(&args).unwrap(),
            CliCommand::Start { daemon: true }
        );
    }

    #[test]
    fn parse_logs_follow() {
        let args = vec!["logs".to_string(), "--follow".to_string()];
        assert_eq!(
            parse_cli_command(&args).unwrap(),
            CliCommand::Logs { follow: true }
        );
    }

    #[test]
    fn parse_studio_print_url() {
        let args = vec!["studio".to_string(), "--print-url".to_string()];
        assert_eq!(
            parse_cli_command(&args).unwrap(),
            CliCommand::Studio { print_url: true }
        );
    }

    #[tokio::test]
    async fn stale_state_cleanup_removes_files() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = DaemonPaths {
            dir: tmp.path().to_path_buf(),
            lock: tmp.path().join("openstack.lock"),
            pid: tmp.path().join("openstack.pid"),
            meta: tmp.path().join("openstack.meta.json"),
            log: tmp.path().join("openstack.log"),
        };
        tokio::fs::write(&paths.lock, "lock").await.unwrap();
        tokio::fs::write(&paths.pid, "999999").await.unwrap();
        tokio::fs::write(&paths.meta, "{}").await.unwrap();

        recover_stale_state(&paths).await.unwrap();

        assert!(!paths.lock.exists());
        assert!(!paths.pid.exists());
        assert!(!paths.meta.exists());
    }
}
