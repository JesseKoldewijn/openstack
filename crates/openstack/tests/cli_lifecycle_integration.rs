use std::fs;
use std::path::Path;
use std::process::Command;

fn daemon_dir(root: &Path) -> std::path::PathBuf {
    root.join("daemon")
}

fn write_pid(root: &Path, pid: i32) {
    let dir = daemon_dir(root);
    fs::create_dir_all(&dir).expect("failed to create daemon test directory for pid");
    fs::write(dir.join("openstack.pid"), pid.to_string()).expect("failed to write pid file");
}

fn write_lock(root: &Path) {
    let dir = daemon_dir(root);
    fs::create_dir_all(&dir).expect("failed to create daemon test directory for lock");
    fs::write(dir.join("openstack.lock"), "lock").expect("failed to write lock file");
}

fn write_meta(root: &Path, pid: i32, health_url: &str) {
    let dir = daemon_dir(root);
    fs::create_dir_all(&dir).expect("failed to create daemon test directory for metadata");
    let meta = serde_json::json!({
        "pid": pid,
        "started_at_utc": "2026-01-01T00:00:00Z",
        "health_url": health_url,
        "log_path": dir.join("openstack.log").to_string_lossy().to_string(),
        "command": "openstack"
    });
    let meta_text =
        serde_json::to_string_pretty(&meta).expect("failed to serialize daemon metadata json");
    fs::write(dir.join("openstack.meta.json"), meta_text)
        .expect("failed to write daemon metadata file");
}

fn run_openstack_with_data_dir(data_dir: &Path, args: &[&str]) -> std::process::Output {
    let exe = assert_cmd::cargo::cargo_bin("openstack");
    Command::new(exe)
        .args(args)
        .env("LOCALSTACK_DATA_DIR", data_dir)
        .output()
        .expect("failed to run openstack command with given data dir")
}

#[test]
fn status_reports_not_running_without_metadata() {
    let tmp = tempfile::tempdir().unwrap();
    let output = run_openstack_with_data_dir(tmp.path(), &["status"]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success());
    assert!(stdout.contains("status: not-running"));
}

#[test]
fn status_reports_degraded_when_pid_exists_but_health_fails() {
    let tmp = tempfile::tempdir().unwrap();
    write_meta(tmp.path(), 1, "http://127.0.0.1:9/_localstack/health");
    write_pid(tmp.path(), 1);
    write_lock(tmp.path());

    let output = run_openstack_with_data_dir(tmp.path(), &["status"]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success());
    assert!(stdout.contains("status: degraded"));
}

#[test]
fn stop_when_not_running_succeeds_with_message() {
    let tmp = tempfile::tempdir().unwrap();
    let output = run_openstack_with_data_dir(tmp.path(), &["stop"]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success());
    assert!(stdout.contains("openstack daemon is not running"));
}

#[test]
fn unknown_command_exits_non_zero() {
    let tmp = tempfile::tempdir().unwrap();
    let output = run_openstack_with_data_dir(tmp.path(), &["unknown-command"]);
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Unknown command"));
}

#[test]
fn logs_tail_prints_existing_file() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = daemon_dir(tmp.path());
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("openstack.log"), "line-a\nline-b\n").unwrap();

    let output = run_openstack_with_data_dir(tmp.path(), &["logs"]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success());
    assert!(stdout.contains("line-a"));
    assert!(stdout.contains("line-b"));
}

#[test]
fn start_with_existing_running_pid_prevents_duplicate_start() {
    let tmp = tempfile::tempdir().unwrap();
    write_lock(tmp.path());
    let current_pid = std::process::id() as i32;
    write_pid(tmp.path(), current_pid);
    write_meta(
        tmp.path(),
        current_pid,
        "http://127.0.0.1:9/_localstack/health",
    );

    let output = run_openstack_with_data_dir(tmp.path(), &["start", "--daemon"]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success());
    assert!(stdout.contains("openstack daemon already running"));
}

#[test]
fn studio_print_url_outputs_dashboard_url() {
    let tmp = tempfile::tempdir().unwrap();
    let output = run_openstack_with_data_dir(tmp.path(), &["studio", "--print-url"]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success());
    assert!(stdout.contains("/_localstack/studio"));
}

#[test]
fn studio_unknown_option_fails() {
    let tmp = tempfile::tempdir().unwrap();
    let output = run_openstack_with_data_dir(tmp.path(), &["studio", "--bad-opt"]);
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Unknown studio option"));
}

#[test]
fn stop_cleans_stale_metadata_for_non_running_pid() {
    let tmp = tempfile::tempdir().unwrap();
    write_lock(tmp.path());
    write_pid(tmp.path(), 999_999);
    write_meta(tmp.path(), 999_999, "http://127.0.0.1:9/_localstack/health");

    let output = run_openstack_with_data_dir(tmp.path(), &["stop"]);
    assert!(output.status.success());

    let dir = daemon_dir(tmp.path());
    assert!(!dir.join("openstack.lock").exists());
    assert!(!dir.join("openstack.pid").exists());
    assert!(!dir.join("openstack.meta.json").exists());
}
