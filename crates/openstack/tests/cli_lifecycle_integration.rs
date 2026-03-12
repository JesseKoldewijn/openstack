use std::fs;
use std::path::Path;
use std::process::Command;

/// Get the path to the daemon subdirectory under the given root.
///
/// # Examples
///
/// ```
/// use std::path::Path;
/// let root = Path::new("/tmp/data");
/// let daemon = crate::daemon_dir(root);
/// assert_eq!(daemon, root.join("daemon"));
/// ```
fn daemon_dir(root: &Path) -> std::path::PathBuf {
    root.join("daemon")
}

/// Ensures the daemon directory exists and writes the given PID to `openstack.pid`.
///
/// The function creates the daemon subdirectory under `root` if needed and overwrites
/// `openstack.pid` with the decimal representation of `pid`.
///
/// # Examples
///
/// ```
/// use std::fs;
/// use std::path::Path;
/// // prepare a temporary directory (use tempfile in real tests)
/// let tmp = std::env::temp_dir().join("example_write_pid");
/// let _ = std::fs::remove_dir_all(&tmp);
/// std::fs::create_dir_all(&tmp).unwrap();
/// write_pid(&tmp, 12345);
/// let contents = fs::read_to_string(tmp.join("daemon").join("openstack.pid")).unwrap();
/// assert_eq!(contents, "12345");
/// ```
fn write_pid(root: &Path, pid: i32) {
    let dir = daemon_dir(root);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("openstack.pid"), pid.to_string()).unwrap();
}

/// Writes a lock file named `openstack.lock` inside the daemon subdirectory of `root`,
/// creating the daemon directory if it does not exist.
///
/// # Examples
///
/// ```
/// use std::fs;
/// use tempfile::tempdir;
///
/// let tmp = tempdir().unwrap();
/// let root = tmp.path();
/// write_lock(root);
/// let contents = fs::read_to_string(root.join("daemon").join("openstack.lock")).unwrap();
/// assert_eq!(contents, "lock");
/// ```
fn write_lock(root: &Path) {
    let dir = daemon_dir(root);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("openstack.lock"), "lock").unwrap();
}

/// Writes a daemon metadata file `openstack.meta.json` into the daemon subdirectory of `root`.
///
/// The file contains JSON with `pid`, a fixed `started_at_utc` timestamp ("2026-01-01T00:00:00Z"),
/// the provided `health_url`, a `log_path` pointing to `openstack.log` in the same directory,
/// and `command` set to `"openstack"`. The daemon directory is created if it does not exist.
///
/// # Arguments
///
/// * `root` - Base data directory under which the daemon subdirectory will be created.
/// * `pid` - Process id to record in the metadata.
/// * `health_url` - Health-check URL to record in the metadata.
///
/// # Panics
///
/// Panics if creating the daemon directory or writing the metadata file fails.
///
/// # Examples
///
/// ```
/// use std::path::Path;
/// use tempfile::tempdir;
///
/// let tmp = tempdir().unwrap();
/// let root = tmp.path();
/// write_meta(root, 12345, "http://127.0.0.1:4566/health");
/// let meta_path = root.join("daemon").join("openstack.meta.json");
/// let contents = std::fs::read_to_string(meta_path).unwrap();
/// assert!(contents.contains("\"pid\": 12345"));
/// assert!(contents.contains("\"health_url\": \"http://127.0.0.1:4566/health\""));
/// ```
fn write_meta(root: &Path, pid: i32, health_url: &str) {
    let dir = daemon_dir(root);
    fs::create_dir_all(&dir).unwrap();
    let meta = format!(
        "{{\n  \"pid\": {},\n  \"started_at_utc\": \"2026-01-01T00:00:00Z\",\n  \"health_url\": \"{}\",\n  \"log_path\": \"{}\",\n  \"command\": \"openstack\"\n}}",
        pid,
        health_url,
        dir.join("openstack.log").display()
    );
    fs::write(dir.join("openstack.meta.json"), meta).unwrap();
}

/// Run the `openstack` binary with `LOCALSTACK_DATA_DIR` set to the provided directory.
///
/// The binary is located using Cargo's test helper and invoked with the given arguments,
/// returning the captured process output.
///
/// # Examples
///
/// ```
/// use tempfile::tempdir;
///
/// let dir = tempdir().unwrap();
/// let output = run_openstack_with_data_dir(dir.path(), &["status"]);
/// assert!(output.status.success() || output.status.code().is_some());
/// ```
///
/// # Returns
///
/// The captured `std::process::Output` from the spawned process.
fn run_openstack_with_data_dir(data_dir: &Path, args: &[&str]) -> std::process::Output {
    let exe = assert_cmd::cargo::cargo_bin("openstack");
    Command::new(exe)
        .args(args)
        .env("LOCALSTACK_DATA_DIR", data_dir)
        .output()
        .unwrap()
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

/// Ensures the `stop` command removes stale daemon files when the recorded PID is not running.
///
/// Creates a lock, a large non-existent PID, and corresponding metadata, runs `openstack stop`,
/// and verifies that `openstack.lock`, `openstack.pid`, and `openstack.meta.json` are deleted.
///
/// # Examples
///
/// ```
/// let tmp = tempfile::tempdir().unwrap();
/// write_lock(tmp.path());
/// write_pid(tmp.path(), 999_999);
/// write_meta(tmp.path(), 999_999, "http://127.0.0.1:9/_localstack/health");
///
/// let output = run_openstack_with_data_dir(tmp.path(), &["stop"]);
/// assert!(output.status.success());
///
/// let dir = daemon_dir(tmp.path());
/// assert!(!dir.join("openstack.lock").exists());
/// assert!(!dir.join("openstack.pid").exists());
/// assert!(!dir.join("openstack.meta.json").exists());
/// ```
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
