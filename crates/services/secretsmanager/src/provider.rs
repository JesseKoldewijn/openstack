use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use chrono::Utc;
use openstack_service_framework::traits::{
    DispatchError, DispatchResponse, RequestContext, ResponseBody, ServiceProvider,
};
use openstack_state::AccountRegionBundle;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::store::{Secret, SecretVersion, SecretsManagerStore};

pub struct SecretsManagerProvider {
    store: Arc<AccountRegionBundle<SecretsManagerStore>>,
}

impl SecretsManagerProvider {
    pub fn new() -> Self {
        Self {
            store: Arc::new(AccountRegionBundle::new()),
        }
    }
}

impl Default for SecretsManagerProvider {
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
        body: ResponseBody::Buffered(Bytes::from(serde_json::to_vec(&body).unwrap())),
        content_type: "application/x-amz-json-1.1".to_string(),
        headers: Vec::new(),
    }
}

fn json_error(code: &str, message: &str, status: u16) -> DispatchResponse {
    DispatchResponse {
        status_code: status,
        body: ResponseBody::Buffered(Bytes::from(
            serde_json::to_vec(&json!({
                "__type": code,
                "message": message,
            }))
            .unwrap(),
        )),
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

fn secret_summary(s: &Secret, include_secret: bool) -> Value {
    let mut v = json!({
        "ARN": s.arn,
        "Name": s.name,
        "Description": s.description,
        "CreatedDate": s.created.timestamp(),
        "LastChangedDate": s.last_changed.timestamp(),
        "DeletedDate": s.deletion_date.map(|d| d.timestamp()),
    });
    if include_secret && let Some(cv) = s.current_version() {
        if let Some(ss) = &cv.secret_string {
            v["SecretString"] = json!(ss);
        }
        v["VersionId"] = json!(cv.version_id);
    }
    v
}

// ---------------------------------------------------------------------------
// ServiceProvider
// ---------------------------------------------------------------------------

#[async_trait]
impl ServiceProvider for SecretsManagerProvider {
    fn service_name(&self) -> &str {
        "secretsmanager"
    }

    async fn dispatch(&self, ctx: &RequestContext) -> Result<DispatchResponse, DispatchError> {
        let op = ctx.operation.as_str();
        let account_id = &ctx.account_id;
        let region = &ctx.region;

        match op {
            "CreateSecret" => {
                let name = match str_param(ctx, "Name") {
                    Some(n) => n,
                    None => return Ok(json_error("ValidationException", "Name is required", 400)),
                };
                let description = str_param(ctx, "Description").unwrap_or_default();
                let secret_string = str_param(ctx, "SecretString");
                let arn = format!(
                    "arn:aws:secretsmanager:{region}:{account_id}:secret:{name}-{}",
                    &Uuid::new_v4().to_string()[..6]
                );
                let version_id = Uuid::new_v4().to_string();

                {
                    let store = self.store.get_or_create(account_id, region);
                    if store
                        .secrets
                        .get(&name)
                        .map(|s| !s.deleted)
                        .unwrap_or(false)
                    {
                        return Ok(json_error(
                            "ResourceExistsException",
                            &format!("Secret {name} already exists"),
                            400,
                        ));
                    }
                }

                let version = SecretVersion {
                    version_id: version_id.clone(),
                    secret_string: secret_string.clone(),
                    secret_binary: None,
                    created: Utc::now(),
                    version_stages: vec!["AWSCURRENT".to_string()],
                };
                let secret = Secret {
                    arn: arn.clone(),
                    name: name.clone(),
                    description,
                    created: Utc::now(),
                    last_changed: Utc::now(),
                    deleted: false,
                    deletion_date: None,
                    versions: vec![version],
                    tags: Default::default(),
                };
                let mut store = self.store.get_or_create(account_id, region);
                store.secrets.insert(name.clone(), secret);
                Ok(json_ok(json!({
                    "ARN": arn,
                    "Name": name,
                    "VersionId": version_id,
                })))
            }

            "GetSecretValue" => {
                let secret_id = match str_param(ctx, "SecretId") {
                    Some(s) => s,
                    None => {
                        return Ok(json_error(
                            "ValidationException",
                            "SecretId is required",
                            400,
                        ));
                    }
                };
                let store = self.store.get_or_create(account_id, region);
                let secret = store.secrets.get(&secret_id).or_else(|| {
                    // Try ARN lookup
                    store.secrets.values().find(|s| s.arn == secret_id)
                });
                match secret {
                    None => Ok(json_error(
                        "ResourceNotFoundException",
                        &format!("Secret {secret_id} not found"),
                        400,
                    )),
                    Some(s) if s.deleted => Ok(json_error(
                        "InvalidRequestException",
                        "Secret is scheduled for deletion",
                        400,
                    )),
                    Some(s) => match s.current_version() {
                        None => Ok(json_error(
                            "ResourceNotFoundException",
                            "No current version",
                            400,
                        )),
                        Some(v) => {
                            let mut resp = json!({
                                "ARN": s.arn,
                                "Name": s.name,
                                "VersionId": v.version_id,
                                "VersionStages": v.version_stages,
                                "CreatedDate": v.created.timestamp(),
                            });
                            if let Some(ss) = &v.secret_string {
                                resp["SecretString"] = json!(ss);
                            }
                            Ok(json_ok(resp))
                        }
                    },
                }
            }

            "PutSecretValue" => {
                let secret_id = match str_param(ctx, "SecretId") {
                    Some(s) => s,
                    None => {
                        return Ok(json_error(
                            "ValidationException",
                            "SecretId is required",
                            400,
                        ));
                    }
                };
                let secret_string = str_param(ctx, "SecretString");
                let version_id = Uuid::new_v4().to_string();
                let new_version = SecretVersion {
                    version_id: version_id.clone(),
                    secret_string,
                    secret_binary: None,
                    created: Utc::now(),
                    version_stages: vec!["AWSCURRENT".to_string()],
                };
                let mut store = self.store.get_or_create(account_id, region);
                // Resolve by name or ARN to the canonical name key
                let resolved_name: Option<String> = if store.secrets.contains_key(&secret_id) {
                    Some(secret_id.clone())
                } else {
                    store
                        .secrets
                        .values()
                        .find(|s| s.arn == secret_id)
                        .map(|s| s.name.clone())
                };
                let secret = resolved_name
                    .as_ref()
                    .and_then(|n| store.secrets.get_mut(n));
                match secret {
                    None => Ok(json_error(
                        "ResourceNotFoundException",
                        &format!("Secret {secret_id} not found"),
                        400,
                    )),
                    Some(s) => {
                        // Demote old AWSCURRENT to AWSPREVIOUS
                        for v in &mut s.versions {
                            if v.version_stages.contains(&"AWSCURRENT".to_string()) {
                                v.version_stages.retain(|st| st != "AWSCURRENT");
                                v.version_stages.push("AWSPREVIOUS".to_string());
                            }
                        }
                        let arn = s.arn.clone();
                        let name = s.name.clone();
                        s.versions.push(new_version);
                        s.last_changed = Utc::now();
                        Ok(json_ok(json!({
                            "ARN": arn,
                            "Name": name,
                            "VersionId": version_id,
                            "VersionStages": ["AWSCURRENT"],
                        })))
                    }
                }
            }

            "UpdateSecret" => {
                let secret_id = match str_param(ctx, "SecretId") {
                    Some(s) => s,
                    None => {
                        return Ok(json_error(
                            "ValidationException",
                            "SecretId is required",
                            400,
                        ));
                    }
                };
                let description = str_param(ctx, "Description");
                let secret_string = str_param(ctx, "SecretString");
                let mut store = self.store.get_or_create(account_id, region);
                let secret = store.secrets.get_mut(&secret_id);
                match secret {
                    None => Ok(json_error(
                        "ResourceNotFoundException",
                        &format!("Secret {secret_id} not found"),
                        400,
                    )),
                    Some(s) => {
                        if let Some(d) = description {
                            s.description = d;
                        }
                        if let Some(ss) = secret_string {
                            let version_id = Uuid::new_v4().to_string();
                            for v in &mut s.versions {
                                v.version_stages.retain(|st| st != "AWSCURRENT");
                            }
                            s.versions.push(SecretVersion {
                                version_id,
                                secret_string: Some(ss),
                                secret_binary: None,
                                created: Utc::now(),
                                version_stages: vec!["AWSCURRENT".to_string()],
                            });
                        }
                        s.last_changed = Utc::now();
                        let arn = s.arn.clone();
                        let name = s.name.clone();
                        Ok(json_ok(json!({ "ARN": arn, "Name": name })))
                    }
                }
            }

            "DescribeSecret" => {
                let secret_id = match str_param(ctx, "SecretId") {
                    Some(s) => s,
                    None => {
                        return Ok(json_error(
                            "ValidationException",
                            "SecretId is required",
                            400,
                        ));
                    }
                };
                let store = self.store.get_or_create(account_id, region);
                let secret = store
                    .secrets
                    .get(&secret_id)
                    .or_else(|| store.secrets.values().find(|s| s.arn == secret_id));
                match secret {
                    None => Ok(json_error(
                        "ResourceNotFoundException",
                        &format!("Secret {secret_id} not found"),
                        400,
                    )),
                    Some(s) => Ok(json_ok(secret_summary(s, false))),
                }
            }

            "ListSecrets" => {
                let store = self.store.get_or_create(account_id, region);
                let secrets: Vec<Value> = store
                    .secrets
                    .values()
                    .filter(|s| !s.deleted)
                    .map(|s| secret_summary(s, false))
                    .collect();
                Ok(json_ok(json!({ "SecretList": secrets })))
            }

            "DeleteSecret" => {
                let secret_id = match str_param(ctx, "SecretId") {
                    Some(s) => s,
                    None => {
                        return Ok(json_error(
                            "ValidationException",
                            "SecretId is required",
                            400,
                        ));
                    }
                };
                let force_delete = ctx
                    .request_body
                    .get("ForceDeleteWithoutRecovery")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let mut store = self.store.get_or_create(account_id, region);
                let secret = store.secrets.get_mut(&secret_id);
                match secret {
                    None => Ok(json_error(
                        "ResourceNotFoundException",
                        &format!("Secret {secret_id} not found"),
                        400,
                    )),
                    Some(s) => {
                        let arn = s.arn.clone();
                        let name = s.name.clone();
                        if force_delete {
                            store.secrets.remove(&secret_id);
                            Ok(json_ok(json!({ "ARN": arn, "Name": name })))
                        } else {
                            let deletion_date = Utc::now() + chrono::Duration::days(30);
                            s.deleted = true;
                            s.deletion_date = Some(deletion_date);
                            Ok(json_ok(json!({
                                "ARN": arn,
                                "Name": name,
                                "DeletionDate": deletion_date.timestamp(),
                            })))
                        }
                    }
                }
            }

            "RestoreSecret" => {
                let secret_id = match str_param(ctx, "SecretId") {
                    Some(s) => s,
                    None => {
                        return Ok(json_error(
                            "ValidationException",
                            "SecretId is required",
                            400,
                        ));
                    }
                };
                let mut store = self.store.get_or_create(account_id, region);
                match store.secrets.get_mut(&secret_id) {
                    None => Ok(json_error(
                        "ResourceNotFoundException",
                        &format!("Secret {secret_id} not found"),
                        400,
                    )),
                    Some(s) => {
                        s.deleted = false;
                        s.deletion_date = None;
                        let arn = s.arn.clone();
                        let name = s.name.clone();
                        Ok(json_ok(json!({ "ARN": arn, "Name": name })))
                    }
                }
            }

            _ => Ok(json_error(
                "NotImplementedException",
                &format!("Operation not implemented: {op}"),
                501,
            )),
        }
    }
}
