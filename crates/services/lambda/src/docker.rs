//! Docker executor for Lambda function invocation.
//!
//! Implements the AWS Lambda Runtime Interface Client (RIC) protocol:
//!   GET  /2018-06-01/runtime/invocation/next          → deliver event to container
//!   POST /2018-06-01/runtime/invocation/{id}/response → receive result
//!   POST /2018-06-01/runtime/invocation/{id}/error    → receive function error
//!   POST /2018-06-01/runtime/init/error               → receive init error
//!
//! Each container gets its own RIC server bound to a random port on the host.
//! `AWS_LAMBDA_RUNTIME_API` is set to `host.docker.internal:<port>` so that the
//! container can reach us even on Docker Desktop (Mac/Windows) or Linux with
//! `--add-host=host.docker.internal:host-gateway`.

use std::{collections::HashMap, path::PathBuf, sync::Arc, time::Duration};

use axum::{
    Router,
    body::Bytes,
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
};
use bollard::{
    Docker,
    models::{ContainerCreateBody as ContainerConfig, HostConfig},
    query_parameters::{CreateContainerOptions, RemoveContainerOptions, StartContainerOptions},
};
use dashmap::DashMap;
use serde_json::Value;
use tokio::{
    sync::{Mutex, mpsc, oneshot},
    time::{Instant, timeout},
};
use tracing::{debug, info, warn};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Invocation message types
// ---------------------------------------------------------------------------

pub struct InvocationRequest {
    pub request_id: String,
    pub payload: String,
    pub response_tx: oneshot::Sender<InvocationResult>,
}

#[derive(Debug)]
pub enum InvocationResult {
    Success(String),
    FunctionError {
        error_type: String,
        error_message: String,
    },
    Timeout,
    ContainerError(String),
}

// ---------------------------------------------------------------------------
// RIC server state (shared across axum handlers)
// ---------------------------------------------------------------------------

#[derive(Clone)]
struct RicState {
    invocation_rx: Arc<Mutex<mpsc::UnboundedReceiver<InvocationRequest>>>,
    current_response_tx: Arc<Mutex<Option<oneshot::Sender<InvocationResult>>>>,
}

// ---------------------------------------------------------------------------
// RIC server axum handlers
// ---------------------------------------------------------------------------

async fn ric_next(State(state): State<RicState>) -> impl IntoResponse {
    let mut rx = state.invocation_rx.lock().await;
    match rx.recv().await {
        Some(req) => {
            let request_id = req.request_id.clone();
            *state.current_response_tx.lock().await = Some(req.response_tx);
            (
                StatusCode::OK,
                [
                    ("Lambda-Runtime-Aws-Request-Id", request_id),
                    ("Content-Type", "application/json".to_string()),
                ],
                req.payload,
            )
                .into_response()
        }
        None => StatusCode::GONE.into_response(),
    }
}

async fn ric_response(
    State(state): State<RicState>,
    Path(_req_id): Path<String>,
    body: Bytes,
) -> impl IntoResponse {
    let payload = String::from_utf8_lossy(&body).to_string();
    if let Some(tx) = state.current_response_tx.lock().await.take() {
        let _ = tx.send(InvocationResult::Success(payload));
    }
    StatusCode::ACCEPTED
}

async fn ric_error(
    State(state): State<RicState>,
    Path(_req_id): Path<String>,
    body: Bytes,
) -> impl IntoResponse {
    let payload = String::from_utf8_lossy(&body).to_string();
    let (error_type, error_message) = if let Ok(v) = serde_json::from_str::<Value>(&payload) {
        (
            v.get("errorType")
                .and_then(|s| s.as_str())
                .unwrap_or("UnknownError")
                .to_string(),
            v.get("errorMessage")
                .and_then(|s| s.as_str())
                .unwrap_or(&payload)
                .to_string(),
        )
    } else {
        ("UnknownError".to_string(), payload)
    };
    if let Some(tx) = state.current_response_tx.lock().await.take() {
        let _ = tx.send(InvocationResult::FunctionError {
            error_type,
            error_message,
        });
    }
    StatusCode::ACCEPTED
}

async fn ric_init_error(State(state): State<RicState>, body: Bytes) -> impl IntoResponse {
    let payload = String::from_utf8_lossy(&body).to_string();
    warn!("Lambda init error: {}", payload);
    if let Some(tx) = state.current_response_tx.lock().await.take() {
        let _ = tx.send(InvocationResult::ContainerError(format!(
            "Init error: {payload}"
        )));
    }
    StatusCode::ACCEPTED
}

// ---------------------------------------------------------------------------
// Warm container entry
// ---------------------------------------------------------------------------

pub struct WarmContainer {
    pub container_id: String,
    pub invocation_tx: mpsc::UnboundedSender<InvocationRequest>,
    pub last_used: Instant,
    pub _ric_handle: tokio::task::JoinHandle<()>,
}

// ---------------------------------------------------------------------------
// DockerExecutor
// ---------------------------------------------------------------------------

pub struct DockerExecutor {
    docker: Option<Docker>,
    keepalive_ms: u64,
    #[allow(dead_code)]
    remove_containers: bool,
    /// function_arn → WarmContainer
    warm_pool: Arc<DashMap<String, WarmContainer>>,
    /// code_sha256 → extracted temp dir path
    #[allow(dead_code)]
    code_cache: Arc<DashMap<String, PathBuf>>,
}

impl DockerExecutor {
    pub fn new(keepalive_ms: u64, remove_containers: bool) -> Self {
        let docker = Docker::connect_with_local_defaults().ok();
        if docker.is_none() {
            warn!("Docker not available — Lambda invocations will fail");
        }
        Self {
            docker,
            keepalive_ms,
            remove_containers,
            warm_pool: Arc::new(DashMap::new()),
            code_cache: Arc::new(DashMap::new()),
        }
    }

    pub fn is_available(&self) -> bool {
        self.docker.is_some()
    }

    // -----------------------------------------------------------------------
    // Runtime image mapping
    // -----------------------------------------------------------------------

    fn runtime_image(runtime: &str) -> String {
        match runtime {
            "python3.12" => "public.ecr.aws/lambda/python:3.12".to_string(),
            "python3.11" => "public.ecr.aws/lambda/python:3.11".to_string(),
            "python3.10" => "public.ecr.aws/lambda/python:3.10".to_string(),
            "python3.9" => "public.ecr.aws/lambda/python:3.9".to_string(),
            "nodejs20.x" => "public.ecr.aws/lambda/nodejs:20".to_string(),
            "nodejs18.x" => "public.ecr.aws/lambda/nodejs:18".to_string(),
            "java21" => "public.ecr.aws/lambda/java:21".to_string(),
            "java17" => "public.ecr.aws/lambda/java:17".to_string(),
            "java11" => "public.ecr.aws/lambda/java:11".to_string(),
            "provided.al2023" => "public.ecr.aws/lambda/provided:al2023".to_string(),
            "provided.al2" => "public.ecr.aws/lambda/provided:al2".to_string(),
            other => format!("public.ecr.aws/lambda/provided:{other}"),
        }
    }

    // -----------------------------------------------------------------------
    // Code extraction
    // -----------------------------------------------------------------------

    fn extract_code(code_zip_b64: &str, sha256: &str) -> anyhow::Result<PathBuf> {
        let zip_bytes =
            base64::Engine::decode(&base64::engine::general_purpose::STANDARD, code_zip_b64)?;
        let dir = std::env::temp_dir().join(format!("lambda-code-{sha256}"));
        if dir.exists() {
            return Ok(dir);
        }
        std::fs::create_dir_all(&dir)?;
        let cursor = std::io::Cursor::new(zip_bytes);
        let mut archive = zip::ZipArchive::new(cursor)?;
        for i in 0..archive.len() {
            let mut file = archive.by_index(i)?;
            let outpath = dir.join(file.name());
            if file.name().ends_with('/') {
                std::fs::create_dir_all(&outpath)?;
            } else {
                if let Some(parent) = outpath.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                let mut outfile = std::fs::File::create(&outpath)?;
                std::io::copy(&mut file, &mut outfile)?;
            }
        }
        Ok(dir)
    }

    // -----------------------------------------------------------------------
    // Start RIC server
    // -----------------------------------------------------------------------

    async fn start_ric_server(
        invocation_rx: mpsc::UnboundedReceiver<InvocationRequest>,
    ) -> anyhow::Result<(u16, tokio::task::JoinHandle<()>)> {
        let state = RicState {
            invocation_rx: Arc::new(Mutex::new(invocation_rx)),
            current_response_tx: Arc::new(Mutex::new(None)),
        };

        let app = Router::new()
            .route("/2018-06-01/runtime/invocation/next", get(ric_next))
            .route(
                "/2018-06-01/runtime/invocation/{req_id}/response",
                post(ric_response),
            )
            .route(
                "/2018-06-01/runtime/invocation/{req_id}/error",
                post(ric_error),
            )
            .route("/2018-06-01/runtime/init/error", post(ric_init_error))
            .with_state(state);

        let listener = tokio::net::TcpListener::bind("0.0.0.0:0").await?;
        let port = listener.local_addr()?.port();

        let handle = tokio::spawn(async move {
            axum::serve(listener, app).await.ok();
        });

        Ok((port, handle))
    }

    // -----------------------------------------------------------------------
    // Invoke
    // -----------------------------------------------------------------------

    /// Invoke a Lambda function. Uses warm pool if available, cold starts otherwise.
    #[allow(clippy::too_many_arguments)]
    pub async fn invoke(
        &self,
        function_arn: &str,
        function_name: &str,
        runtime: &str,
        handler: &str,
        code_zip_b64: &str,
        code_sha256: &str,
        env_vars: &HashMap<String, String>,
        timeout_secs: i64,
        payload: &str,
    ) -> InvocationResult {
        let docker = match &self.docker {
            Some(d) => d.clone(),
            None => return InvocationResult::ContainerError("Docker not available".to_string()),
        };

        // Check warm pool
        if self.keepalive_ms > 0
            && let Some(mut entry) = self.warm_pool.get_mut(function_arn)
        {
            let elapsed_ms = entry.last_used.elapsed().as_millis() as u64;
            if elapsed_ms < self.keepalive_ms {
                // Reuse warm container
                let (response_tx, response_rx) = oneshot::channel();
                let request_id = Uuid::new_v4().to_string();
                if entry
                    .invocation_tx
                    .send(InvocationRequest {
                        request_id,
                        payload: payload.to_string(),
                        response_tx,
                    })
                    .is_ok()
                {
                    entry.last_used = Instant::now();
                    drop(entry); // release lock
                    return self.await_response(response_rx, timeout_secs).await;
                }
                // Channel closed — fall through to cold start
            }
            drop(entry);
            self.warm_pool.remove(function_arn);
        }

        // Cold start
        self.cold_start(
            docker,
            function_arn,
            function_name,
            runtime,
            handler,
            code_zip_b64,
            code_sha256,
            env_vars,
            timeout_secs,
            payload,
        )
        .await
    }

    async fn await_response(
        &self,
        response_rx: oneshot::Receiver<InvocationResult>,
        timeout_secs: i64,
    ) -> InvocationResult {
        match timeout(Duration::from_secs(timeout_secs as u64), response_rx).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => InvocationResult::ContainerError("Response channel dropped".to_string()),
            Err(_) => InvocationResult::Timeout,
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn cold_start(
        &self,
        docker: Docker,
        function_arn: &str,
        function_name: &str,
        runtime: &str,
        handler: &str,
        code_zip_b64: &str,
        code_sha256: &str,
        env_vars: &HashMap<String, String>,
        timeout_secs: i64,
        payload: &str,
    ) -> InvocationResult {
        // Extract code
        let code_dir = match Self::extract_code(code_zip_b64, code_sha256) {
            Ok(d) => d,
            Err(e) => {
                return InvocationResult::ContainerError(format!("Code extraction failed: {e}"));
            }
        };

        // Start RIC server
        let (invocation_tx, invocation_rx) = mpsc::unbounded_channel();
        let (ric_port, ric_handle) = match Self::start_ric_server(invocation_rx).await {
            Ok(v) => v,
            Err(e) => {
                return InvocationResult::ContainerError(format!("RIC server start failed: {e}"));
            }
        };

        // Build env vars for container
        let request_id = Uuid::new_v4().to_string();
        let mut container_env: Vec<String> = vec![
            format!("AWS_LAMBDA_RUNTIME_API=host.docker.internal:{ric_port}"),
            format!("AWS_LAMBDA_FUNCTION_NAME={function_name}"),
            format!("AWS_LAMBDA_FUNCTION_VERSION=$LATEST"),
            format!("AWS_LAMBDA_FUNCTION_MEMORY_SIZE=128"),
            format!("AWS_DEFAULT_REGION=us-east-1"),
            format!("AWS_REGION=us-east-1"),
            format!("AWS_ACCESS_KEY_ID=test"),
            format!("AWS_SECRET_ACCESS_KEY=test"),
        ];
        for (k, v) in env_vars {
            container_env.push(format!("{k}={v}"));
        }

        let image = Self::runtime_image(runtime);
        let container_name = format!("openstack-lambda-{}-{}", function_name, &request_id[..8]);

        debug!(
            "Starting Lambda container {} with image {}",
            container_name, image
        );

        // Create container
        let host_config = HostConfig {
            binds: Some(vec![format!("{}:/var/task:ro", code_dir.display())]),
            extra_hosts: Some(vec!["host.docker.internal:host-gateway".to_string()]),
            ..Default::default()
        };

        let container_config = ContainerConfig {
            image: Some(image),
            cmd: Some(vec![handler.to_string()]),
            env: Some(container_env),
            host_config: Some(host_config),
            ..Default::default()
        };

        let container_id = match docker
            .create_container(
                Some(CreateContainerOptions {
                    name: Some(container_name.clone()),
                    platform: String::new(),
                }),
                container_config,
            )
            .await
        {
            Ok(resp) => resp.id,
            Err(e) => {
                ric_handle.abort();
                return InvocationResult::ContainerError(format!("Container creation failed: {e}"));
            }
        };

        // Start container
        if let Err(e) = docker
            .start_container(&container_id, None::<StartContainerOptions>)
            .await
        {
            let _ = docker
                .remove_container(
                    &container_id,
                    Some(RemoveContainerOptions {
                        force: true,
                        ..Default::default()
                    }),
                )
                .await;
            ric_handle.abort();
            return InvocationResult::ContainerError(format!("Container start failed: {e}"));
        }

        info!("Lambda container {} started", container_name);

        // Send the invocation to the RIC server
        let (response_tx, response_rx) = oneshot::channel();
        if invocation_tx
            .send(InvocationRequest {
                request_id: request_id.clone(),
                payload: payload.to_string(),
                response_tx,
            })
            .is_err()
        {
            let _ = self.cleanup_container(&docker, &container_id).await;
            ric_handle.abort();
            return InvocationResult::ContainerError("Failed to queue invocation".to_string());
        }

        // Await response with timeout
        let result = self.await_response(response_rx, timeout_secs).await;

        // Warm pool management or cleanup
        if self.keepalive_ms > 0
            && !matches!(
                result,
                InvocationResult::Timeout | InvocationResult::ContainerError(_)
            )
        {
            self.warm_pool.insert(
                function_arn.to_string(),
                WarmContainer {
                    container_id: container_id.clone(),
                    invocation_tx,
                    last_used: Instant::now(),
                    _ric_handle: ric_handle,
                },
            );
        } else {
            let _ = self.cleanup_container(&docker, &container_id).await;
            ric_handle.abort();
        }

        result
    }

    async fn cleanup_container(&self, docker: &Docker, container_id: &str) {
        let _ = docker
            .remove_container(
                container_id,
                Some(RemoveContainerOptions {
                    force: true,
                    ..Default::default()
                }),
            )
            .await;
    }

    /// Evict containers from warm pool that have exceeded keepalive_ms.
    pub async fn evict_expired(&self) {
        if self.keepalive_ms == 0 {
            return;
        }
        let docker = match &self.docker {
            Some(d) => d.clone(),
            None => return,
        };
        let expired: Vec<String> = self
            .warm_pool
            .iter()
            .filter(|e| e.last_used.elapsed().as_millis() as u64 >= self.keepalive_ms)
            .map(|e| e.key().clone())
            .collect();
        for key in expired {
            if let Some((_, wc)) = self.warm_pool.remove(&key) {
                self.cleanup_container(&docker, &wc.container_id).await;
            }
        }
    }
}
