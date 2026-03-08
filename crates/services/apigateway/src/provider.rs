use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use chrono::Utc;
use openstack_service_framework::traits::{
    DispatchError, DispatchResponse, RequestContext, ServiceProvider,
};
use openstack_state::AccountRegionBundle;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::store::{
    ApiDeployment, ApiGatewayStore, ApiIntegration, ApiMethod, ApiResource, ApiStage, RestApi,
};

pub struct ApiGatewayProvider {
    store: Arc<AccountRegionBundle<ApiGatewayStore>>,
}

impl ApiGatewayProvider {
    pub fn new() -> Self {
        Self {
            store: Arc::new(AccountRegionBundle::new()),
        }
    }
}

impl Default for ApiGatewayProvider {
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
        content_type: "application/json".to_string(),
        headers: Vec::new(),
    }
}

fn json_created(body: Value) -> DispatchResponse {
    DispatchResponse {
        status_code: 201,
        body: Bytes::from(serde_json::to_vec(&body).unwrap()),
        content_type: "application/json".to_string(),
        headers: Vec::new(),
    }
}

fn json_error(code: &str, message: &str, status: u16) -> DispatchResponse {
    DispatchResponse {
        status_code: status,
        body: Bytes::from(
            serde_json::to_vec(&json!({
                "message": message,
                "code": code,
            }))
            .unwrap(),
        ),
        content_type: "application/json".to_string(),
        headers: Vec::new(),
    }
}

fn str_param(ctx: &RequestContext, key: &str) -> Option<String> {
    ctx.request_body
        .get(key)
        .and_then(|v| v.as_str())
        .map(String::from)
}

/// Extract path segment from REST-style path like /restapis/{restApiId}/...
fn path_segment(ctx: &RequestContext, index: usize) -> Option<&str> {
    ctx.path.split('/').filter(|s| !s.is_empty()).nth(index)
}

fn short_id() -> String {
    Uuid::new_v4().to_string()[..8].to_string()
}

// ---------------------------------------------------------------------------
// ServiceProvider
// ---------------------------------------------------------------------------

#[async_trait]
impl ServiceProvider for ApiGatewayProvider {
    fn service_name(&self) -> &str {
        "apigateway"
    }

    async fn dispatch(&self, ctx: &RequestContext) -> Result<DispatchResponse, DispatchError> {
        let region = &ctx.region;
        let account_id = &ctx.account_id;

        // API Gateway uses REST paths. The operation is derived from method + path.
        // The gateway layer sets ctx.operation based on path pattern matching.
        // We support the standard API Gateway V1 management API operations.

        match ctx.operation.as_str() {
            // ----------------------------------------------------------------
            // CreateRestApi  POST /restapis
            // ----------------------------------------------------------------
            "CreateRestApi" => {
                let name = match str_param(ctx, "name") {
                    Some(n) => n,
                    None => return Ok(json_error("BadRequestException", "name is required", 400)),
                };
                let api_id = short_id();
                let now = Utc::now();
                let api = RestApi {
                    id: api_id.clone(),
                    name: name.clone(),
                    description: str_param(ctx, "description").unwrap_or_default(),
                    created: now,
                };

                // Create root resource "/"
                let root_id = short_id();
                let root_resource = ApiResource {
                    id: root_id.clone(),
                    api_id: api_id.clone(),
                    parent_id: None,
                    path_part: "/".to_string(),
                    path: "/".to_string(),
                    methods: Default::default(),
                };

                let mut store = self.store.get_or_create(account_id, region);
                store.apis.insert(api_id.clone(), api);
                store.resources.insert(root_id.clone(), root_resource);

                Ok(json_created(json!({
                    "id": api_id,
                    "name": name,
                    "description": str_param(ctx, "description").unwrap_or_default(),
                    "rootResourceId": root_id,
                    "createdDate": now.timestamp(),
                })))
            }

            // ----------------------------------------------------------------
            // GetRestApis  GET /restapis
            // ----------------------------------------------------------------
            "GetRestApis" => {
                let store = self.store.get_or_create(account_id, region);
                let apis: Vec<Value> = store
                    .apis
                    .values()
                    .map(|a| json!({ "id": a.id, "name": a.name, "description": a.description, "createdDate": a.created.timestamp() }))
                    .collect();
                Ok(json_ok(json!({ "items": apis })))
            }

            // ----------------------------------------------------------------
            // GetRestApi  GET /restapis/{restApiId}
            // ----------------------------------------------------------------
            "GetRestApi" => {
                let api_id = match path_segment(ctx, 1) {
                    Some(id) => id.to_string(),
                    None => {
                        return Ok(json_error("BadRequestException", "restApiId required", 400));
                    }
                };
                let store = self.store.get_or_create(account_id, region);
                match store.apis.get(&api_id) {
                    Some(a) => Ok(json_ok(
                        json!({ "id": a.id, "name": a.name, "createdDate": a.created.timestamp() }),
                    )),
                    None => Ok(json_error(
                        "NotFoundException",
                        "Invalid API identifier specified",
                        404,
                    )),
                }
            }

            // ----------------------------------------------------------------
            // DeleteRestApi  DELETE /restapis/{restApiId}
            // ----------------------------------------------------------------
            "DeleteRestApi" => {
                let api_id = match path_segment(ctx, 1) {
                    Some(id) => id.to_string(),
                    None => {
                        return Ok(json_error("BadRequestException", "restApiId required", 400));
                    }
                };
                let mut store = self.store.get_or_create(account_id, region);
                store.apis.remove(&api_id);
                store.resources.retain(|_, r| r.api_id != api_id);
                store.deployments.retain(|_, d| d.api_id != api_id);
                store.stages.retain(|(aid, _), _| aid != &api_id);
                Ok(DispatchResponse {
                    status_code: 202,
                    body: Bytes::new(),
                    content_type: "application/json".to_string(),
                    headers: Vec::new(),
                })
            }

            // ----------------------------------------------------------------
            // CreateResource  POST /restapis/{restApiId}/resources/{parentId}
            // ----------------------------------------------------------------
            "CreateResource" => {
                let api_id = match path_segment(ctx, 1) {
                    Some(id) => id.to_string(),
                    None => {
                        return Ok(json_error("BadRequestException", "restApiId required", 400));
                    }
                };
                let parent_id = path_segment(ctx, 3).map(String::from);
                let path_part = match str_param(ctx, "pathPart") {
                    Some(p) => p,
                    None => {
                        return Ok(json_error(
                            "BadRequestException",
                            "pathPart is required",
                            400,
                        ));
                    }
                };

                let resource_id = short_id();
                let store_ref = self.store.get_or_create(account_id, region);
                let parent_path = parent_id
                    .as_deref()
                    .and_then(|pid| store_ref.resources.get(pid))
                    .map(|r| r.path.clone())
                    .unwrap_or_else(|| "/".to_string());
                drop(store_ref);

                let path = if parent_path == "/" {
                    format!("/{path_part}")
                } else {
                    format!("{parent_path}/{path_part}")
                };

                let resource = ApiResource {
                    id: resource_id.clone(),
                    api_id: api_id.clone(),
                    parent_id: parent_id.clone(),
                    path_part: path_part.clone(),
                    path: path.clone(),
                    methods: Default::default(),
                };

                let mut store = self.store.get_or_create(account_id, region);
                if !store.apis.contains_key(&api_id) {
                    return Ok(json_error(
                        "NotFoundException",
                        "Invalid API identifier specified",
                        404,
                    ));
                }
                store.resources.insert(resource_id.clone(), resource);

                Ok(json_created(json!({
                    "id": resource_id,
                    "parentId": parent_id,
                    "pathPart": path_part,
                    "path": path,
                })))
            }

            // ----------------------------------------------------------------
            // GetResources  GET /restapis/{restApiId}/resources
            // ----------------------------------------------------------------
            "GetResources" => {
                let api_id = match path_segment(ctx, 1) {
                    Some(id) => id.to_string(),
                    None => {
                        return Ok(json_error("BadRequestException", "restApiId required", 400));
                    }
                };
                let store = self.store.get_or_create(account_id, region);
                let resources: Vec<Value> = store
                    .resources
                    .values()
                    .filter(|r| r.api_id == api_id)
                    .map(|r| {
                        json!({
                            "id": r.id,
                            "parentId": r.parent_id,
                            "pathPart": r.path_part,
                            "path": r.path,
                        })
                    })
                    .collect();
                Ok(json_ok(json!({ "items": resources })))
            }

            // ----------------------------------------------------------------
            // PutMethod  PUT /restapis/{restApiId}/resources/{resourceId}/methods/{httpMethod}
            // ----------------------------------------------------------------
            "PutMethod" => {
                let resource_id = match path_segment(ctx, 3) {
                    Some(id) => id.to_string(),
                    None => {
                        return Ok(json_error(
                            "BadRequestException",
                            "resourceId required",
                            400,
                        ));
                    }
                };
                let http_method = match path_segment(ctx, 5) {
                    Some(m) => m.to_uppercase(),
                    None => {
                        return Ok(json_error(
                            "BadRequestException",
                            "httpMethod required",
                            400,
                        ));
                    }
                };
                let authorization_type =
                    str_param(ctx, "authorizationType").unwrap_or_else(|| "NONE".to_string());

                let method = ApiMethod {
                    http_method: http_method.clone(),
                    authorization_type,
                    integration: None,
                };

                let mut store = self.store.get_or_create(account_id, region);
                if let Some(resource) = store.resources.get_mut(&resource_id) {
                    resource.methods.insert(http_method.clone(), method);
                    Ok(json_ok(json!({
                        "httpMethod": http_method,
                        "authorizationType": "NONE",
                    })))
                } else {
                    Ok(json_error(
                        "NotFoundException",
                        "Invalid resource identifier specified",
                        404,
                    ))
                }
            }

            // ----------------------------------------------------------------
            // PutIntegration  PUT /restapis/{restApiId}/resources/{resourceId}/methods/{httpMethod}/integration
            // ----------------------------------------------------------------
            "PutIntegration" => {
                let resource_id = match path_segment(ctx, 3) {
                    Some(id) => id.to_string(),
                    None => {
                        return Ok(json_error(
                            "BadRequestException",
                            "resourceId required",
                            400,
                        ));
                    }
                };
                let http_method = match path_segment(ctx, 5) {
                    Some(m) => m.to_uppercase(),
                    None => {
                        return Ok(json_error(
                            "BadRequestException",
                            "httpMethod required",
                            400,
                        ));
                    }
                };
                let integration_type =
                    str_param(ctx, "type").unwrap_or_else(|| "AWS_PROXY".to_string());
                let uri = str_param(ctx, "uri").unwrap_or_default();
                let integration_http_method =
                    str_param(ctx, "httpMethod").unwrap_or_else(|| "POST".to_string());

                let integration = ApiIntegration {
                    integration_type: integration_type.clone(),
                    uri: uri.clone(),
                    http_method: integration_http_method,
                };

                let mut store = self.store.get_or_create(account_id, region);
                if let Some(resource) = store.resources.get_mut(&resource_id) {
                    if let Some(method) = resource.methods.get_mut(&http_method) {
                        method.integration = Some(integration);
                        Ok(json_ok(json!({
                            "type": integration_type,
                            "uri": uri,
                        })))
                    } else {
                        Ok(json_error(
                            "NotFoundException",
                            "Invalid method identifier specified",
                            404,
                        ))
                    }
                } else {
                    Ok(json_error(
                        "NotFoundException",
                        "Invalid resource identifier specified",
                        404,
                    ))
                }
            }

            // ----------------------------------------------------------------
            // CreateDeployment  POST /restapis/{restApiId}/deployments
            // ----------------------------------------------------------------
            "CreateDeployment" => {
                let api_id = match path_segment(ctx, 1) {
                    Some(id) => id.to_string(),
                    None => {
                        return Ok(json_error("BadRequestException", "restApiId required", 400));
                    }
                };
                let stage_name = str_param(ctx, "stageName").unwrap_or_else(|| "prod".to_string());
                let description = str_param(ctx, "description").unwrap_or_default();
                let deployment_id = short_id();
                let now = Utc::now();

                let deployment = ApiDeployment {
                    id: deployment_id.clone(),
                    api_id: api_id.clone(),
                    description: description.clone(),
                    created: now,
                };
                let stage = ApiStage {
                    api_id: api_id.clone(),
                    stage_name: stage_name.clone(),
                    deployment_id: deployment_id.clone(),
                    description: description.clone(),
                    created: now,
                };

                let mut store = self.store.get_or_create(account_id, region);
                if !store.apis.contains_key(&api_id) {
                    return Ok(json_error(
                        "NotFoundException",
                        "Invalid API identifier specified",
                        404,
                    ));
                }
                store.deployments.insert(deployment_id.clone(), deployment);
                store
                    .stages
                    .insert((api_id.clone(), stage_name.clone()), stage);

                Ok(json_created(json!({
                    "id": deployment_id,
                    "description": description,
                    "createdDate": now.timestamp(),
                })))
            }

            // ----------------------------------------------------------------
            // GetDeployments  GET /restapis/{restApiId}/deployments
            // ----------------------------------------------------------------
            "GetDeployments" => {
                let api_id = match path_segment(ctx, 1) {
                    Some(id) => id.to_string(),
                    None => {
                        return Ok(json_error("BadRequestException", "restApiId required", 400));
                    }
                };
                let store = self.store.get_or_create(account_id, region);
                let deployments: Vec<Value> = store
                    .deployments
                    .values()
                    .filter(|d| d.api_id == api_id)
                    .map(|d| json!({ "id": d.id, "description": d.description, "createdDate": d.created.timestamp() }))
                    .collect();
                Ok(json_ok(json!({ "items": deployments })))
            }

            // ----------------------------------------------------------------
            // GetStages  GET /restapis/{restApiId}/stages
            // ----------------------------------------------------------------
            "GetStages" => {
                let api_id = match path_segment(ctx, 1) {
                    Some(id) => id.to_string(),
                    None => {
                        return Ok(json_error("BadRequestException", "restApiId required", 400));
                    }
                };
                let store = self.store.get_or_create(account_id, region);
                let stages: Vec<Value> = store
                    .stages
                    .iter()
                    .filter(|((aid, _), _)| aid == &api_id)
                    .map(|((_, sname), s)| {
                        json!({
                            "stageName": sname,
                            "deploymentId": s.deployment_id,
                            "description": s.description,
                            "createdDate": s.created.timestamp(),
                        })
                    })
                    .collect();
                Ok(json_ok(json!({ "item": stages })))
            }

            // ----------------------------------------------------------------
            // GetMethod  GET /restapis/{restApiId}/resources/{resourceId}/methods/{httpMethod}
            // ----------------------------------------------------------------
            "GetMethod" => {
                let resource_id = match path_segment(ctx, 3) {
                    Some(id) => id.to_string(),
                    None => {
                        return Ok(json_error(
                            "BadRequestException",
                            "resourceId required",
                            400,
                        ));
                    }
                };
                let http_method = match path_segment(ctx, 5) {
                    Some(m) => m.to_uppercase(),
                    None => {
                        return Ok(json_error(
                            "BadRequestException",
                            "httpMethod required",
                            400,
                        ));
                    }
                };
                let store = self.store.get_or_create(account_id, region);
                if let Some(resource) = store.resources.get(&resource_id) {
                    if let Some(method) = resource.methods.get(&http_method) {
                        Ok(json_ok(json!({
                            "httpMethod": method.http_method,
                            "authorizationType": method.authorization_type,
                        })))
                    } else {
                        Ok(json_error(
                            "NotFoundException",
                            "Invalid method identifier specified",
                            404,
                        ))
                    }
                } else {
                    Ok(json_error(
                        "NotFoundException",
                        "Invalid resource identifier specified",
                        404,
                    ))
                }
            }

            _ => Err(DispatchError::NotImplemented(ctx.operation.clone())),
        }
    }
}
