use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use chrono::Utc;
use openstack_service_framework::traits::{
    DispatchError, DispatchResponse, RequestContext, ServiceProvider,
};
use openstack_state::AccountRegionBundle;
use serde_json::{Value, json};

use crate::store::{Parameter, ParameterType, SsmStore};

pub struct SsmProvider {
    store: Arc<AccountRegionBundle<SsmStore>>,
}

impl SsmProvider {
    pub fn new() -> Self {
        Self {
            store: Arc::new(AccountRegionBundle::new()),
        }
    }
}

impl Default for SsmProvider {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// JSON response helpers
// ---------------------------------------------------------------------------

fn json_ok(body: Value) -> DispatchResponse {
    DispatchResponse {
        status_code: 200,
        body: Bytes::from(serde_json::to_vec(&body).unwrap()),
        content_type: "application/x-amz-json-1.1".to_string(),
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

fn str_param(ctx: &RequestContext, key: &str) -> Option<String> {
    ctx.request_body
        .get(key)
        .and_then(|v| v.as_str())
        .map(String::from)
}

fn param_to_json(p: &Parameter) -> Value {
    json!({
        "Name": p.name,
        "Type": p.type_.as_str(),
        "Value": p.value,
        "Version": p.version,
        "LastModifiedDate": p.last_modified.timestamp(),
        "ARN": p.arn,
        "DataType": "text",
    })
}

// ---------------------------------------------------------------------------
// ServiceProvider
// ---------------------------------------------------------------------------

#[async_trait]
impl ServiceProvider for SsmProvider {
    fn service_name(&self) -> &str {
        "ssm"
    }

    async fn dispatch(&self, ctx: &RequestContext) -> Result<DispatchResponse, DispatchError> {
        let op = ctx.operation.as_str();
        let account_id = &ctx.account_id;
        let region = &ctx.region;

        match op {
            "PutParameter" => {
                let name = match str_param(ctx, "Name") {
                    Some(n) => n,
                    None => return Ok(json_error("ValidationException", "Name is required", 400)),
                };
                let value = match str_param(ctx, "Value") {
                    Some(v) => v,
                    None => return Ok(json_error("ValidationException", "Value is required", 400)),
                };
                let type_str = str_param(ctx, "Type").unwrap_or_else(|| "String".to_string());
                let type_ = type_str
                    .parse::<ParameterType>()
                    .unwrap_or(ParameterType::String);
                let description = str_param(ctx, "Description").unwrap_or_default();
                let overwrite = ctx
                    .request_body
                    .get("Overwrite")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);

                let mut store = self.store.get_or_create(account_id, region);
                if store.parameters.contains_key(&name) && !overwrite {
                    return Ok(json_error(
                        "ParameterAlreadyExists",
                        &format!("Parameter {name} already exists"),
                        400,
                    ));
                }
                let version = store
                    .parameters
                    .get(&name)
                    .map(|p| p.version + 1)
                    .unwrap_or(1);
                let arn = format!("arn:aws:ssm:{region}:{account_id}:parameter{name}");
                let param = Parameter {
                    name: name.clone(),
                    type_,
                    value,
                    description,
                    version,
                    last_modified: Utc::now(),
                    arn,
                    overwrite,
                };
                store.parameters.insert(name, param);
                Ok(json_ok(json!({ "Version": version, "Tier": "Standard" })))
            }

            "GetParameter" => {
                let name = match str_param(ctx, "Name") {
                    Some(n) => n,
                    None => return Ok(json_error("ValidationException", "Name is required", 400)),
                };
                let store = self.store.get_or_create(account_id, region);
                match store.parameters.get(&name) {
                    None => Ok(json_error(
                        "ParameterNotFound",
                        &format!("Parameter {name} not found"),
                        400,
                    )),
                    Some(p) => Ok(json_ok(json!({ "Parameter": param_to_json(p) }))),
                }
            }

            "GetParameters" => {
                let names = ctx
                    .request_body
                    .get("Names")
                    .and_then(|v| v.as_array())
                    .cloned()
                    .unwrap_or_default();
                let store = self.store.get_or_create(account_id, region);
                let mut parameters = Vec::new();
                let mut invalid_parameters = Vec::new();
                for n in &names {
                    if let Some(name) = n.as_str() {
                        match store.parameters.get(name) {
                            Some(p) => parameters.push(param_to_json(p)),
                            None => invalid_parameters.push(json!(name)),
                        }
                    }
                }
                Ok(json_ok(json!({
                    "Parameters": parameters,
                    "InvalidParameters": invalid_parameters,
                })))
            }

            "GetParametersByPath" => {
                let path = match str_param(ctx, "Path") {
                    Some(p) => p,
                    None => return Ok(json_error("ValidationException", "Path is required", 400)),
                };
                let recursive = ctx
                    .request_body
                    .get("Recursive")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let store = self.store.get_or_create(account_id, region);
                let params: Vec<Value> = store
                    .parameters
                    .values()
                    .filter(|p| {
                        if recursive {
                            p.name.starts_with(&path)
                        } else {
                            // Direct children only: path must be the parent
                            p.name.starts_with(&path)
                                && !p.name[path.len()..].trim_start_matches('/').contains('/')
                        }
                    })
                    .map(param_to_json)
                    .collect();
                Ok(json_ok(json!({ "Parameters": params })))
            }

            "DeleteParameter" => {
                let name = match str_param(ctx, "Name") {
                    Some(n) => n,
                    None => return Ok(json_error("ValidationException", "Name is required", 400)),
                };
                let mut store = self.store.get_or_create(account_id, region);
                if store.parameters.remove(&name).is_none() {
                    return Ok(json_error(
                        "ParameterNotFound",
                        &format!("Parameter {name} not found"),
                        400,
                    ));
                }
                Ok(json_ok(json!({})))
            }

            "DeleteParameters" => {
                let names = ctx
                    .request_body
                    .get("Names")
                    .and_then(|v| v.as_array())
                    .cloned()
                    .unwrap_or_default();
                let mut store = self.store.get_or_create(account_id, region);
                let mut deleted = Vec::new();
                let mut invalid = Vec::new();
                for n in &names {
                    if let Some(name) = n.as_str() {
                        if store.parameters.remove(name).is_some() {
                            deleted.push(json!(name));
                        } else {
                            invalid.push(json!(name));
                        }
                    }
                }
                Ok(json_ok(json!({
                    "DeletedParameters": deleted,
                    "InvalidParameters": invalid,
                })))
            }

            "DescribeParameters" => {
                let store = self.store.get_or_create(account_id, region);
                let params: Vec<Value> = store
                    .parameters
                    .values()
                    .map(|p| {
                        json!({
                            "Name": p.name,
                            "Type": p.type_.as_str(),
                            "Description": p.description,
                            "Version": p.version,
                            "LastModifiedDate": p.last_modified.timestamp(),
                            "ARN": p.arn,
                        })
                    })
                    .collect();
                Ok(json_ok(json!({ "Parameters": params })))
            }

            _ => Ok(json_error(
                "NotImplementedException",
                &format!("Operation not implemented: {op}"),
                501,
            )),
        }
    }
}
