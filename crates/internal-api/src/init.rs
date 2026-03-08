use std::collections::HashMap;

use axum::Json;
use axum::extract::{Path, State};
use axum::response::IntoResponse;
use serde_json::json;
use tokio::process::Command;
use tracing::{error, info};

use crate::ApiState;

#[derive(Debug, Clone, serde::Serialize)]
pub struct ScriptStatus {
    pub name: String,
    pub state: String, // "UNKNOWN", "RUNNING", "SUCCESSFUL", "ERROR"
    pub return_code: Option<i32>,
}

pub async fn get_init(State(state): State<ApiState>) -> impl IntoResponse {
    let stages = ["boot", "start", "ready", "shutdown"];
    let mut result: HashMap<&str, Vec<ScriptStatus>> = HashMap::new();
    for stage in &stages {
        result.insert(stage, scan_scripts(stage, &state).await);
    }
    Json(json!({ "scripts": result }))
}

pub async fn get_init_stage(
    Path(stage): Path<String>,
    State(state): State<ApiState>,
) -> impl IntoResponse {
    let scripts = scan_scripts(&stage, &state).await;
    Json(json!({ "scripts": scripts }))
}

async fn stage_dir(stage: &str, state: &ApiState) -> std::path::PathBuf {
    // Respect the configured init directories from config
    let dirs = &state.config.directories;
    match stage {
        "boot" => dirs.init_boot.clone(),
        "start" => dirs.init_start.clone(),
        "ready" => dirs.init_ready.clone(),
        "shutdown" => dirs.init_shutdown.clone(),
        other => dirs.init.join(format!("{}.d", other)),
    }
}

async fn scan_scripts(stage: &str, state: &ApiState) -> Vec<ScriptStatus> {
    let dir = stage_dir(stage, state).await;
    let mut scripts = Vec::new();

    if let Ok(mut entries) = tokio::fs::read_dir(&dir).await {
        while let Ok(Some(entry)) = entries.next_entry().await {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.ends_with(".sh") {
                scripts.push(ScriptStatus {
                    name,
                    state: "UNKNOWN".to_string(),
                    return_code: None,
                });
            }
        }
    }
    scripts.sort_by(|a, b| a.name.cmp(&b.name));
    scripts
}

/// Run all scripts in a given init stage directory in alphabetical order.
pub async fn run_init_stage(stage: &str, state: &ApiState) {
    let dir = stage_dir(stage, state).await;
    let mut scripts = scan_scripts(stage, state).await;
    scripts.sort_by(|a, b| a.name.cmp(&b.name));

    for script in scripts {
        let script_path = dir.join(&script.name);
        info!("Running init script: {:?}", script_path);
        match Command::new("sh").arg(&script_path).status().await {
            Ok(status) => {
                if status.success() {
                    info!("Init script succeeded: {:?}", script_path);
                } else {
                    error!(
                        "Init script failed with code {:?}: {:?}",
                        status.code(),
                        script_path
                    );
                }
            }
            Err(e) => {
                error!("Failed to run init script {:?}: {}", script_path, e);
            }
        }
    }
}
