use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use base64::{Engine as _, engine::general_purpose::STANDARD as B64};
use bytes::Bytes;
use chrono::Utc;
use openstack_service_framework::traits::{
    DispatchError, DispatchResponse, RequestContext, ServiceProvider,
};
use openstack_state::AccountRegionBundle;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::docker::{DockerExecutor, InvocationResult};
use crate::store::{
    EventSourceMapping, FunctionState, LambdaAlias, LambdaFunction, LambdaLayerVersion,
    LambdaStore, LambdaVersion,
};

pub struct LambdaProvider {
    store: Arc<AccountRegionBundle<LambdaStore>>,
    executor: Arc<DockerExecutor>,
}

impl LambdaProvider {
    pub fn new() -> Self {
        let keepalive_ms = std::env::var("LAMBDA_KEEPALIVE_MS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(600_000);
        let remove_containers = std::env::var("LAMBDA_REMOVE_CONTAINERS")
            .map(|v| v == "1" || v == "true")
            .unwrap_or(true);
        Self {
            store: Arc::new(AccountRegionBundle::new()),
            executor: Arc::new(DockerExecutor::new(keepalive_ms, remove_containers)),
        }
    }
}

impl Default for LambdaProvider {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn json_ok(body: Value) -> DispatchResponse {
    DispatchResponse {
        status_code: 200,
        body: Bytes::from(serde_json::to_vec(&body).unwrap()),
        content_type: "application/x-amz-json-1.1".to_string(),
        headers: Vec::new(),
    }
}

fn json_ok_raw(body: String) -> DispatchResponse {
    DispatchResponse {
        status_code: 200,
        body: Bytes::from(body.into_bytes()),
        content_type: "application/json".to_string(),
        headers: Vec::new(),
    }
}

fn json_error(code: &str, message: &str, status: u16) -> DispatchResponse {
    DispatchResponse {
        status_code: status,
        body: Bytes::from(
            serde_json::to_vec(&json!({
                "__type": code,
                "message": message,
            }))
            .unwrap(),
        ),
        content_type: "application/x-amz-json-1.1".to_string(),
        headers: Vec::new(),
    }
}

fn str_field(body: &Value, key: &str) -> Option<String> {
    body.get(key)
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

fn i64_field(body: &Value, key: &str, default: i64) -> i64 {
    body.get(key).and_then(|v| v.as_i64()).unwrap_or(default)
}

fn compute_sha256_b64(zip_b64: &str) -> String {
    match B64.decode(zip_b64) {
        Ok(bytes) => {
            let hash = Sha256::digest(&bytes);
            B64.encode(hash)
        }
        Err(_) => String::new(),
    }
}

fn make_arn(region: &str, account_id: &str, function_name: &str) -> String {
    format!("arn:aws:lambda:{region}:{account_id}:function:{function_name}")
}

fn make_layer_arn(region: &str, account_id: &str, layer_name: &str) -> String {
    format!("arn:aws:lambda:{region}:{account_id}:layer:{layer_name}")
}

fn make_layer_version_arn(
    region: &str,
    account_id: &str,
    layer_name: &str,
    version: i64,
) -> String {
    format!("arn:aws:lambda:{region}:{account_id}:layer:{layer_name}:{version}")
}

fn function_to_json(f: &LambdaFunction) -> Value {
    json!({
        "FunctionName": f.function_name,
        "FunctionArn": f.function_arn,
        "Runtime": f.runtime,
        "Handler": f.handler,
        "Role": f.role,
        "Description": f.description,
        "Timeout": f.timeout,
        "MemorySize": f.memory_size,
        "Version": f.version,
        "CodeSha256": f.code_sha256,
        "State": f.state.as_str(),
        "LastModified": f.modified.to_rfc3339(),
        "Environment": {
            "Variables": f.environment,
        },
        "Layers": f.layers.iter().map(|arn| json!({"Arn": arn})).collect::<Vec<_>>(),
    })
}

fn layer_version_to_json(lv: &LambdaLayerVersion) -> Value {
    json!({
        "LayerArn": lv.layer_arn,
        "LayerVersionArn": lv.layer_version_arn,
        "Description": lv.description,
        "Version": lv.version,
        "CompatibleRuntimes": lv.compatible_runtimes,
        "CreatedDate": lv.created.to_rfc3339(),
        "Content": {
            "CodeSize": 0,
        }
    })
}

fn esm_to_json(esm: &EventSourceMapping) -> Value {
    json!({
        "UUID": esm.uuid,
        "FunctionArn": esm.function_arn,
        "EventSourceArn": esm.event_source_arn,
        "State": esm.state,
        "BatchSize": esm.batch_size,
        "StartingPosition": esm.starting_position,
        "LastModified": esm.created.timestamp(),
    })
}

/// Extract function name from a path like /2015-03-31/functions/{name}/invocations
/// or /2015-03-31/functions/{name}
fn function_name_from_path(path: &str) -> Option<String> {
    let parts: Vec<&str> = path.trim_matches('/').split('/').collect();
    let idx = parts.iter().position(|&p| p == "functions")?;
    parts.get(idx + 1).map(|s| s.to_string())
}

fn alias_from_path(path: &str) -> Option<String> {
    // /2015-03-31/functions/{name}/aliases/{alias}
    let parts: Vec<&str> = path.trim_matches('/').split('/').collect();
    let idx = parts.iter().position(|&p| p == "aliases")?;
    parts.get(idx + 1).map(|s| s.to_string())
}

fn layer_name_from_path(path: &str) -> Option<String> {
    // /2015-03-31/layers/{name}[/...]
    let parts: Vec<&str> = path.trim_matches('/').split('/').collect();
    let idx = parts.iter().position(|&p| p == "layers")?;
    parts.get(idx + 1).map(|s| s.to_string())
}

fn layer_version_from_path(path: &str) -> Option<i64> {
    // /2015-03-31/layers/{name}/versions/{version}
    let parts: Vec<&str> = path.trim_matches('/').split('/').collect();
    let idx = parts.iter().position(|&p| p == "versions")?;
    parts.get(idx + 1).and_then(|s| s.parse().ok())
}

fn esm_uuid_from_path(path: &str) -> Option<String> {
    // /2015-03-31/event-source-mappings/{uuid}
    let parts: Vec<&str> = path.trim_matches('/').split('/').collect();
    let idx = parts.iter().position(|&p| p == "event-source-mappings")?;
    parts.get(idx + 1).map(|s| s.to_string())
}

// ---------------------------------------------------------------------------
// ServiceProvider implementation
// ---------------------------------------------------------------------------

#[async_trait]
impl ServiceProvider for LambdaProvider {
    fn service_name(&self) -> &str {
        "lambda"
    }

    async fn dispatch(&self, ctx: &RequestContext) -> Result<DispatchResponse, DispatchError> {
        let body = &ctx.request_body;
        let region = &ctx.region;
        let account_id = &ctx.account_id;
        let path = &ctx.path;

        match ctx.operation.as_str() {
            // ----------------------------------------------------------------
            // CreateFunction
            // ----------------------------------------------------------------
            "CreateFunction" => {
                let function_name = match str_field(body, "FunctionName") {
                    Some(n) => n,
                    None => {
                        return Ok(json_error(
                            "ValidationException",
                            "FunctionName is required",
                            400,
                        ));
                    }
                };
                let runtime = str_field(body, "Runtime").unwrap_or_default();
                let handler = str_field(body, "Handler").unwrap_or_default();
                let role = str_field(body, "Role").unwrap_or_default();
                let description = str_field(body, "Description").unwrap_or_default();
                let timeout = i64_field(body, "Timeout", 3);
                let memory_size = i64_field(body, "MemorySize", 128);

                let code_zip = body
                    .get("Code")
                    .and_then(|c| c.get("ZipFile"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let code_sha256 = compute_sha256_b64(&code_zip);

                let env_vars: HashMap<String, String> = body
                    .get("Environment")
                    .and_then(|e| e.get("Variables"))
                    .and_then(|v| serde_json::from_value(v.clone()).ok())
                    .unwrap_or_default();

                let layers: Vec<String> = body
                    .get("Layers")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str().map(|s| s.to_string()))
                            .collect()
                    })
                    .unwrap_or_default();

                let function_arn = make_arn(region, account_id, &function_name);
                let now = Utc::now();

                let func = LambdaFunction {
                    function_name: function_name.clone(),
                    function_arn: function_arn.clone(),
                    runtime,
                    handler,
                    code_zip,
                    environment: env_vars,
                    timeout,
                    memory_size,
                    role,
                    description,
                    state: FunctionState::Active,
                    version: "$LATEST".to_string(),
                    code_sha256,
                    layers,
                    created: now,
                    modified: now,
                };

                {
                    let mut store = self.store.get_or_create(account_id, region);
                    if store.functions.contains_key(&function_name) {
                        return Ok(json_error(
                            "ResourceConflictException",
                            &format!("Function already exists: {function_name}"),
                            409,
                        ));
                    }
                    store.functions.insert(function_name.clone(), func.clone());
                }

                Ok(DispatchResponse {
                    status_code: 201,
                    body: Bytes::from(serde_json::to_vec(&function_to_json(&func)).unwrap()),
                    content_type: "application/json".to_string(),
                    headers: Vec::new(),
                })
            }

            // ----------------------------------------------------------------
            // GetFunction
            // ----------------------------------------------------------------
            "GetFunction" => {
                let function_name = function_name_from_path(path)
                    .or_else(|| str_field(body, "FunctionName"))
                    .ok_or_else(|| {
                        DispatchError::NotImplemented("FunctionName required".to_string())
                    })?;

                let store = self.store.get_or_create(account_id, region);
                match store.functions.get(&function_name) {
                    Some(f) => Ok(json_ok(json!({
                        "Configuration": function_to_json(f),
                        "Code": { "Location": "" },
                    }))),
                    None => Ok(json_error(
                        "ResourceNotFoundException",
                        &format!("Function not found: {function_name}"),
                        404,
                    )),
                }
            }

            // ----------------------------------------------------------------
            // GetFunctionConfiguration
            // ----------------------------------------------------------------
            "GetFunctionConfiguration" => {
                let function_name = function_name_from_path(path)
                    .or_else(|| str_field(body, "FunctionName"))
                    .ok_or_else(|| {
                        DispatchError::NotImplemented("FunctionName required".to_string())
                    })?;

                let store = self.store.get_or_create(account_id, region);
                match store.functions.get(&function_name) {
                    Some(f) => Ok(json_ok(function_to_json(f))),
                    None => Ok(json_error(
                        "ResourceNotFoundException",
                        &format!("Function not found: {function_name}"),
                        404,
                    )),
                }
            }

            // ----------------------------------------------------------------
            // ListFunctions
            // ----------------------------------------------------------------
            "ListFunctions" => {
                let store = self.store.get_or_create(account_id, region);
                let functions: Vec<Value> =
                    store.functions.values().map(function_to_json).collect();
                Ok(json_ok(json!({
                    "Functions": functions,
                })))
            }

            // ----------------------------------------------------------------
            // DeleteFunction
            // ----------------------------------------------------------------
            "DeleteFunction" => {
                let function_name = function_name_from_path(path)
                    .or_else(|| str_field(body, "FunctionName"))
                    .ok_or_else(|| {
                        DispatchError::NotImplemented("FunctionName required".to_string())
                    })?;

                let mut store = self.store.get_or_create(account_id, region);
                if store.functions.remove(&function_name).is_none() {
                    return Ok(json_error(
                        "ResourceNotFoundException",
                        &format!("Function not found: {function_name}"),
                        404,
                    ));
                }
                store.aliases.remove(&function_name);
                store.versions.remove(&function_name);

                Ok(DispatchResponse {
                    status_code: 204,
                    body: Bytes::new(),
                    content_type: "application/json".to_string(),
                    headers: Vec::new(),
                })
            }

            // ----------------------------------------------------------------
            // UpdateFunctionCode
            // ----------------------------------------------------------------
            "UpdateFunctionCode" => {
                let function_name = function_name_from_path(path)
                    .or_else(|| str_field(body, "FunctionName"))
                    .ok_or_else(|| {
                        DispatchError::NotImplemented("FunctionName required".to_string())
                    })?;

                let code_zip = body
                    .get("ZipFile")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let code_sha256 = compute_sha256_b64(&code_zip);

                let mut store = self.store.get_or_create(account_id, region);
                match store.functions.get_mut(&function_name) {
                    Some(f) => {
                        f.code_zip = code_zip;
                        f.code_sha256 = code_sha256;
                        f.modified = Utc::now();
                        Ok(json_ok(function_to_json(f)))
                    }
                    None => Ok(json_error(
                        "ResourceNotFoundException",
                        &format!("Function not found: {function_name}"),
                        404,
                    )),
                }
            }

            // ----------------------------------------------------------------
            // UpdateFunctionConfiguration
            // ----------------------------------------------------------------
            "UpdateFunctionConfiguration" => {
                let function_name = function_name_from_path(path)
                    .or_else(|| str_field(body, "FunctionName"))
                    .ok_or_else(|| {
                        DispatchError::NotImplemented("FunctionName required".to_string())
                    })?;

                let mut store = self.store.get_or_create(account_id, region);
                match store.functions.get_mut(&function_name) {
                    Some(f) => {
                        if let Some(runtime) = str_field(body, "Runtime") {
                            f.runtime = runtime;
                        }
                        if let Some(handler) = str_field(body, "Handler") {
                            f.handler = handler;
                        }
                        if let Some(role) = str_field(body, "Role") {
                            f.role = role;
                        }
                        if let Some(desc) = str_field(body, "Description") {
                            f.description = desc;
                        }
                        if let Some(t) = body.get("Timeout").and_then(|v| v.as_i64()) {
                            f.timeout = t;
                        }
                        if let Some(m) = body.get("MemorySize").and_then(|v| v.as_i64()) {
                            f.memory_size = m;
                        }
                        if let Some(env) = body.get("Environment").and_then(|e| e.get("Variables"))
                            && let Ok(vars) =
                                serde_json::from_value::<HashMap<String, String>>(env.clone())
                        {
                            f.environment = vars;
                        }
                        f.modified = Utc::now();
                        Ok(json_ok(function_to_json(f)))
                    }
                    None => Ok(json_error(
                        "ResourceNotFoundException",
                        &format!("Function not found: {function_name}"),
                        404,
                    )),
                }
            }

            // ----------------------------------------------------------------
            // PublishVersion
            // ----------------------------------------------------------------
            "PublishVersion" => {
                let function_name = function_name_from_path(path)
                    .or_else(|| str_field(body, "FunctionName"))
                    .ok_or_else(|| {
                        DispatchError::NotImplemented("FunctionName required".to_string())
                    })?;
                let description = str_field(body, "Description").unwrap_or_default();

                let mut store = self.store.get_or_create(account_id, region);
                if !store.functions.contains_key(&function_name) {
                    return Ok(json_error(
                        "ResourceNotFoundException",
                        &format!("Function not found: {function_name}"),
                        404,
                    ));
                }

                let existing_count = store
                    .versions
                    .get(&function_name)
                    .map(|v| v.len())
                    .unwrap_or(0);
                let version_num = (existing_count + 1).to_string();

                let (func_arn, code_sha256) = {
                    let f = store.functions.get(&function_name).unwrap();
                    (f.function_arn.clone(), f.code_sha256.clone())
                };

                let lv = LambdaVersion {
                    version: version_num.clone(),
                    function_name: function_name.clone(),
                    code_sha256: code_sha256.clone(),
                    description,
                    created: Utc::now(),
                };
                store
                    .versions
                    .entry(function_name.clone())
                    .or_default()
                    .push(lv);

                Ok(json_ok(json!({
                    "Version": version_num,
                    "FunctionArn": func_arn,
                    "FunctionName": function_name,
                    "CodeSha256": code_sha256,
                })))
            }

            // ----------------------------------------------------------------
            // Invoke
            // ----------------------------------------------------------------
            "Invoke" => {
                let function_name = match function_name_from_path(path) {
                    Some(n) => n,
                    None => {
                        return Ok(json_error(
                            "ValidationException",
                            "FunctionName not found in path",
                            400,
                        ));
                    }
                };

                let invocation_type = ctx
                    .headers
                    .get("x-amz-invocation-type")
                    .or_else(|| ctx.headers.get("X-Amz-Invocation-Type"))
                    .map(|s| s.as_str())
                    .unwrap_or("RequestResponse");

                let payload = String::from_utf8_lossy(&ctx.raw_body).to_string();

                // Clone function data while holding the store lock briefly
                let func_data = {
                    let store = self.store.get_or_create(account_id, region);
                    store.functions.get(&function_name).map(|f| {
                        (
                            f.function_arn.clone(),
                            f.function_name.clone(),
                            f.runtime.clone(),
                            f.handler.clone(),
                            f.code_zip.clone(),
                            f.code_sha256.clone(),
                            f.environment.clone(),
                            f.timeout,
                        )
                    })
                };

                let (
                    function_arn,
                    func_name,
                    runtime,
                    handler,
                    code_zip,
                    code_sha256,
                    env_vars,
                    timeout_secs,
                ) = match func_data {
                    Some(d) => d,
                    None => {
                        return Ok(json_error(
                            "ResourceNotFoundException",
                            &format!("Function not found: {function_name}"),
                            404,
                        ));
                    }
                };

                // Async (Event) invocation — fire and forget
                if invocation_type == "Event" {
                    let executor = self.executor.clone();
                    let payload_clone = payload.clone();
                    tokio::spawn(async move {
                        let _ = executor
                            .invoke(
                                &function_arn,
                                &func_name,
                                &runtime,
                                &handler,
                                &code_zip,
                                &code_sha256,
                                &env_vars,
                                timeout_secs,
                                &payload_clone,
                            )
                            .await;
                    });
                    return Ok(DispatchResponse {
                        status_code: 202,
                        body: Bytes::new(),
                        content_type: "application/json".to_string(),
                        headers: Vec::new(),
                    });
                }

                // DryRun
                if invocation_type == "DryRun" {
                    return Ok(DispatchResponse {
                        status_code: 204,
                        body: Bytes::new(),
                        content_type: "application/json".to_string(),
                        headers: Vec::new(),
                    });
                }

                // Synchronous RequestResponse
                let result = self
                    .executor
                    .invoke(
                        &function_arn,
                        &func_name,
                        &runtime,
                        &handler,
                        &code_zip,
                        &code_sha256,
                        &env_vars,
                        timeout_secs,
                        &payload,
                    )
                    .await;

                match result {
                    InvocationResult::Success(resp) => Ok(json_ok_raw(resp)),
                    InvocationResult::FunctionError {
                        error_type,
                        error_message,
                    } => Ok(DispatchResponse {
                        status_code: 200,
                        body: Bytes::from(
                            serde_json::to_vec(&json!({
                                "errorType": error_type,
                                "errorMessage": error_message,
                            }))
                            .unwrap(),
                        ),
                        content_type: "application/json".to_string(),
                        headers: vec![(
                            "X-Amz-Function-Error".to_string(),
                            "Unhandled".to_string(),
                        )],
                    }),
                    InvocationResult::Timeout => Ok(json_error(
                        "TooManyRequestsException",
                        "Function execution timed out",
                        429,
                    )),
                    InvocationResult::ContainerError(e) => {
                        Ok(json_error("ServiceException", &e, 500))
                    }
                }
            }

            // ----------------------------------------------------------------
            // PublishLayerVersion
            // ----------------------------------------------------------------
            "PublishLayerVersion" => {
                let layer_name = layer_name_from_path(path)
                    .or_else(|| str_field(body, "LayerName"))
                    .ok_or_else(|| {
                        DispatchError::NotImplemented("LayerName required".to_string())
                    })?;

                let description = str_field(body, "Description").unwrap_or_default();
                let compatible_runtimes: Vec<String> = body
                    .get("CompatibleRuntimes")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str().map(|s| s.to_string()))
                            .collect()
                    })
                    .unwrap_or_default();

                let code_zip = body
                    .get("Content")
                    .and_then(|c| c.get("ZipFile"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();

                let layer_arn = make_layer_arn(region, account_id, &layer_name);

                let mut store = self.store.get_or_create(account_id, region);
                let versions = store.layers.entry(layer_name.clone()).or_default();
                let version_num = (versions.len() as i64) + 1;
                let layer_version_arn =
                    make_layer_version_arn(region, account_id, &layer_name, version_num);

                let lv = LambdaLayerVersion {
                    layer_name: layer_name.clone(),
                    version: version_num,
                    layer_arn,
                    layer_version_arn,
                    description,
                    code_zip,
                    compatible_runtimes,
                    created: Utc::now(),
                };
                let lv_json = layer_version_to_json(&lv);
                versions.push(lv);

                Ok(DispatchResponse {
                    status_code: 201,
                    body: Bytes::from(serde_json::to_vec(&lv_json).unwrap()),
                    content_type: "application/json".to_string(),
                    headers: Vec::new(),
                })
            }

            // ----------------------------------------------------------------
            // GetLayerVersion
            // ----------------------------------------------------------------
            "GetLayerVersion" => {
                let layer_name = layer_name_from_path(path)
                    .or_else(|| str_field(body, "LayerName"))
                    .ok_or_else(|| {
                        DispatchError::NotImplemented("LayerName required".to_string())
                    })?;
                let version_num = layer_version_from_path(path)
                    .or_else(|| body.get("VersionNumber").and_then(|v| v.as_i64()))
                    .ok_or_else(|| {
                        DispatchError::NotImplemented("VersionNumber required".to_string())
                    })?;

                let store = self.store.get_or_create(account_id, region);
                match store.layers.get(&layer_name) {
                    Some(versions) => match versions.iter().find(|lv| lv.version == version_num) {
                        Some(lv) => Ok(json_ok(layer_version_to_json(lv))),
                        None => Ok(json_error(
                            "ResourceNotFoundException",
                            &format!("Layer version not found: {layer_name}:{version_num}"),
                            404,
                        )),
                    },
                    None => Ok(json_error(
                        "ResourceNotFoundException",
                        &format!("Layer not found: {layer_name}"),
                        404,
                    )),
                }
            }

            // ----------------------------------------------------------------
            // ListLayerVersions
            // ----------------------------------------------------------------
            "ListLayerVersions" => {
                let layer_name = layer_name_from_path(path)
                    .or_else(|| str_field(body, "LayerName"))
                    .ok_or_else(|| {
                        DispatchError::NotImplemented("LayerName required".to_string())
                    })?;

                let store = self.store.get_or_create(account_id, region);
                let versions = store
                    .layers
                    .get(&layer_name)
                    .map(|v| v.iter().map(layer_version_to_json).collect::<Vec<_>>())
                    .unwrap_or_default();
                Ok(json_ok(json!({ "LayerVersions": versions })))
            }

            // ----------------------------------------------------------------
            // ListLayers
            // ----------------------------------------------------------------
            "ListLayers" => {
                let store = self.store.get_or_create(account_id, region);
                let layers: Vec<Value> = store
                    .layers
                    .iter()
                    .map(|(name, versions)| {
                        let latest = versions.last();
                        json!({
                            "LayerName": name,
                            "LayerArn": make_layer_arn(region, account_id, name),
                            "LatestMatchingVersion": latest.map(layer_version_to_json),
                        })
                    })
                    .collect();
                Ok(json_ok(json!({ "Layers": layers })))
            }

            // ----------------------------------------------------------------
            // CreateEventSourceMapping
            // ----------------------------------------------------------------
            "CreateEventSourceMapping" => {
                let function_name = match str_field(body, "FunctionName") {
                    Some(n) => n,
                    None => {
                        return Ok(json_error(
                            "ValidationException",
                            "FunctionName is required",
                            400,
                        ));
                    }
                };
                let event_source_arn = match str_field(body, "EventSourceArn") {
                    Some(a) => a,
                    None => {
                        return Ok(json_error(
                            "ValidationException",
                            "EventSourceArn is required",
                            400,
                        ));
                    }
                };
                let batch_size = i64_field(body, "BatchSize", 10);
                let starting_position =
                    str_field(body, "StartingPosition").unwrap_or_else(|| "LATEST".to_string());

                let function_arn = make_arn(region, account_id, &function_name);
                let uuid = Uuid::new_v4().to_string();

                let esm = EventSourceMapping {
                    uuid: uuid.clone(),
                    function_arn,
                    event_source_arn,
                    state: "Enabled".to_string(),
                    batch_size,
                    starting_position,
                    created: Utc::now(),
                };

                let mut store = self.store.get_or_create(account_id, region);
                store.event_source_mappings.insert(uuid, esm.clone());

                Ok(DispatchResponse {
                    status_code: 202,
                    body: Bytes::from(serde_json::to_vec(&esm_to_json(&esm)).unwrap()),
                    content_type: "application/json".to_string(),
                    headers: Vec::new(),
                })
            }

            // ----------------------------------------------------------------
            // GetEventSourceMapping
            // ----------------------------------------------------------------
            "GetEventSourceMapping" => {
                let uuid = esm_uuid_from_path(path)
                    .or_else(|| str_field(body, "UUID"))
                    .ok_or_else(|| DispatchError::NotImplemented("UUID required".to_string()))?;

                let store = self.store.get_or_create(account_id, region);
                match store.event_source_mappings.get(&uuid) {
                    Some(esm) => Ok(json_ok(esm_to_json(esm))),
                    None => Ok(json_error(
                        "ResourceNotFoundException",
                        &format!("ESM not found: {uuid}"),
                        404,
                    )),
                }
            }

            // ----------------------------------------------------------------
            // ListEventSourceMappings
            // ----------------------------------------------------------------
            "ListEventSourceMappings" => {
                let store = self.store.get_or_create(account_id, region);
                let mappings: Vec<Value> = store
                    .event_source_mappings
                    .values()
                    .map(esm_to_json)
                    .collect();
                Ok(json_ok(json!({ "EventSourceMappings": mappings })))
            }

            // ----------------------------------------------------------------
            // DeleteEventSourceMapping
            // ----------------------------------------------------------------
            "DeleteEventSourceMapping" => {
                let uuid = esm_uuid_from_path(path)
                    .or_else(|| str_field(body, "UUID"))
                    .ok_or_else(|| DispatchError::NotImplemented("UUID required".to_string()))?;

                let mut store = self.store.get_or_create(account_id, region);
                match store.event_source_mappings.remove(&uuid) {
                    Some(mut esm) => {
                        esm.state = "Deleting".to_string();
                        Ok(json_ok(esm_to_json(&esm)))
                    }
                    None => Ok(json_error(
                        "ResourceNotFoundException",
                        &format!("ESM not found: {uuid}"),
                        404,
                    )),
                }
            }

            // ----------------------------------------------------------------
            // UpdateEventSourceMapping
            // ----------------------------------------------------------------
            "UpdateEventSourceMapping" => {
                let uuid = esm_uuid_from_path(path)
                    .or_else(|| str_field(body, "UUID"))
                    .ok_or_else(|| DispatchError::NotImplemented("UUID required".to_string()))?;

                let mut store = self.store.get_or_create(account_id, region);
                match store.event_source_mappings.get_mut(&uuid) {
                    Some(esm) => {
                        if let Some(bs) = body.get("BatchSize").and_then(|v| v.as_i64()) {
                            esm.batch_size = bs;
                        }
                        Ok(json_ok(esm_to_json(esm)))
                    }
                    None => Ok(json_error(
                        "ResourceNotFoundException",
                        &format!("ESM not found: {uuid}"),
                        404,
                    )),
                }
            }

            // ----------------------------------------------------------------
            // CreateAlias
            // ----------------------------------------------------------------
            "CreateAlias" => {
                let function_name = function_name_from_path(path)
                    .or_else(|| str_field(body, "FunctionName"))
                    .ok_or_else(|| {
                        DispatchError::NotImplemented("FunctionName required".to_string())
                    })?;
                let alias_name = match str_field(body, "Name") {
                    Some(n) => n,
                    None => return Ok(json_error("ValidationException", "Name is required", 400)),
                };
                let function_version =
                    str_field(body, "FunctionVersion").unwrap_or_else(|| "$LATEST".to_string());
                let description = str_field(body, "Description").unwrap_or_default();

                let function_arn = make_arn(region, account_id, &function_name);
                let alias_arn = format!("{function_arn}:{alias_name}");

                let alias = LambdaAlias {
                    name: alias_name.clone(),
                    function_name: function_name.clone(),
                    function_version,
                    description,
                    arn: alias_arn,
                };

                let mut store = self.store.get_or_create(account_id, region);
                if !store.functions.contains_key(&function_name) {
                    return Ok(json_error(
                        "ResourceNotFoundException",
                        &format!("Function not found: {function_name}"),
                        404,
                    ));
                }
                let aliases = store.aliases.entry(function_name).or_default();
                if aliases.iter().any(|a| a.name == alias_name) {
                    return Ok(json_error(
                        "ResourceConflictException",
                        &format!("Alias already exists: {alias_name}"),
                        409,
                    ));
                }
                aliases.push(alias.clone());

                Ok(DispatchResponse {
                    status_code: 201,
                    body: Bytes::from(
                        serde_json::to_vec(&json!({
                            "Name": alias.name,
                            "AliasArn": alias.arn,
                            "FunctionVersion": alias.function_version,
                            "Description": alias.description,
                        }))
                        .unwrap(),
                    ),
                    content_type: "application/json".to_string(),
                    headers: Vec::new(),
                })
            }

            // ----------------------------------------------------------------
            // GetAlias
            // ----------------------------------------------------------------
            "GetAlias" => {
                let function_name = function_name_from_path(path).ok_or_else(|| {
                    DispatchError::NotImplemented("FunctionName not found in path".to_string())
                })?;
                let alias_name = alias_from_path(path)
                    .or_else(|| str_field(body, "Name"))
                    .ok_or_else(|| {
                        DispatchError::NotImplemented("Alias name required".to_string())
                    })?;

                let store = self.store.get_or_create(account_id, region);
                let aliases = store
                    .aliases
                    .get(&function_name)
                    .map(|v| v.as_slice())
                    .unwrap_or(&[]);
                match aliases.iter().find(|a| a.name == alias_name) {
                    Some(a) => Ok(json_ok(json!({
                        "Name": a.name,
                        "AliasArn": a.arn,
                        "FunctionVersion": a.function_version,
                        "Description": a.description,
                    }))),
                    None => Ok(json_error(
                        "ResourceNotFoundException",
                        &format!("Alias not found: {alias_name}"),
                        404,
                    )),
                }
            }

            // ----------------------------------------------------------------
            // ListAliases
            // ----------------------------------------------------------------
            "ListAliases" => {
                let function_name = function_name_from_path(path)
                    .or_else(|| str_field(body, "FunctionName"))
                    .ok_or_else(|| {
                        DispatchError::NotImplemented("FunctionName required".to_string())
                    })?;

                let store = self.store.get_or_create(account_id, region);
                let aliases = store
                    .aliases
                    .get(&function_name)
                    .map(|v| {
                        v.iter()
                            .map(|a| {
                                json!({
                                    "Name": a.name,
                                    "AliasArn": a.arn,
                                    "FunctionVersion": a.function_version,
                                    "Description": a.description,
                                })
                            })
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                Ok(json_ok(json!({ "Aliases": aliases })))
            }

            // ----------------------------------------------------------------
            // DeleteAlias
            // ----------------------------------------------------------------
            "DeleteAlias" => {
                let function_name = function_name_from_path(path).ok_or_else(|| {
                    DispatchError::NotImplemented("FunctionName not found in path".to_string())
                })?;
                let alias_name = alias_from_path(path)
                    .or_else(|| str_field(body, "Name"))
                    .ok_or_else(|| {
                        DispatchError::NotImplemented("Alias name required".to_string())
                    })?;

                let mut store = self.store.get_or_create(account_id, region);
                if let Some(aliases) = store.aliases.get_mut(&function_name) {
                    let before = aliases.len();
                    aliases.retain(|a| a.name != alias_name);
                    if aliases.len() == before {
                        return Ok(json_error(
                            "ResourceNotFoundException",
                            &format!("Alias not found: {alias_name}"),
                            404,
                        ));
                    }
                }
                Ok(DispatchResponse {
                    status_code: 204,
                    body: Bytes::new(),
                    content_type: "application/json".to_string(),
                    headers: Vec::new(),
                })
            }

            // ----------------------------------------------------------------
            // UpdateAlias
            // ----------------------------------------------------------------
            "UpdateAlias" => {
                let function_name = function_name_from_path(path).ok_or_else(|| {
                    DispatchError::NotImplemented("FunctionName not found in path".to_string())
                })?;
                let alias_name = alias_from_path(path)
                    .or_else(|| str_field(body, "Name"))
                    .ok_or_else(|| {
                        DispatchError::NotImplemented("Alias name required".to_string())
                    })?;

                let mut store = self.store.get_or_create(account_id, region);
                match store.aliases.get_mut(&function_name) {
                    Some(aliases) => match aliases.iter_mut().find(|a| a.name == alias_name) {
                        Some(a) => {
                            if let Some(fv) = str_field(body, "FunctionVersion") {
                                a.function_version = fv;
                            }
                            if let Some(desc) = str_field(body, "Description") {
                                a.description = desc;
                            }
                            Ok(json_ok(json!({
                                "Name": a.name,
                                "AliasArn": a.arn,
                                "FunctionVersion": a.function_version,
                                "Description": a.description,
                            })))
                        }
                        None => Ok(json_error(
                            "ResourceNotFoundException",
                            &format!("Alias not found: {alias_name}"),
                            404,
                        )),
                    },
                    None => Ok(json_error(
                        "ResourceNotFoundException",
                        &format!("Alias not found: {alias_name}"),
                        404,
                    )),
                }
            }

            // ----------------------------------------------------------------
            // AddPermission / GetPolicy / RemovePermission (stubs)
            // ----------------------------------------------------------------
            "AddPermission" => Ok(json_ok(json!({ "Statement": "" }))),
            "GetPolicy" => Ok(json_ok(json!({ "Policy": "{}", "RevisionId": "" }))),
            "RemovePermission" => Ok(DispatchResponse {
                status_code: 204,
                body: Bytes::new(),
                content_type: "application/json".to_string(),
                headers: Vec::new(),
            }),

            // ----------------------------------------------------------------
            // Catch-all
            // ----------------------------------------------------------------
            _ => Err(DispatchError::NotImplemented(ctx.operation.clone())),
        }
    }
}
