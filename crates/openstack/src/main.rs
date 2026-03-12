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

async fn daemon_restart(config: &openstack_config::Config) -> Result<()> {
    daemon_stop(config).await?;
    daemon_start(config).await
}

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

async fn recover_stale_state(paths: &DaemonPaths) -> Result<()> {
    let pid = read_pid(&paths.pid).await;
    if pid.is_none() || !pid.is_some_and(is_pid_running) {
        remove_state_files(paths).await?;
    }
    Ok(())
}

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

async fn read_pid(path: &Path) -> Option<i32> {
    let content = tokio::fs::read_to_string(path).await.ok()?;
    content.trim().parse::<i32>().ok()
}

async fn read_meta(path: &Path) -> Result<DaemonMetadata> {
    let bytes = tokio::fs::read(path).await?;
    Ok(serde_json::from_slice::<DaemonMetadata>(&bytes)?)
}

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

/// Remove any `.tmp` files left in the spool directory from a previous crash.
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
