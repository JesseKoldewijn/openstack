use std::borrow::Cow;
use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use chrono::Utc;
use openstack_service_framework::traits::{
    DispatchError, DispatchResponse, RequestContext, ResponseBody, ServiceProvider,
};
use openstack_state::AccountRegionBundle;
use serde::Serialize;
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
        content_type: Cow::Borrowed("application/x-amz-json-1.1"),
        headers: Vec::new(),
    }
}

fn json_ok_bytes(bytes: Bytes) -> DispatchResponse {
    DispatchResponse {
        status_code: 200,
        body: ResponseBody::Buffered(bytes),
        content_type: Cow::Borrowed("application/x-amz-json-1.1"),
        headers: Vec::new(),
    }
}

fn json_error(code: &str, message: &str, status: u16) -> DispatchResponse {
    DispatchResponse {
        status_code: status,
        body: ResponseBody::Buffered(Bytes::from(
            serde_json::to_vec(&json!({
                "__type": code,
                "Message": message,
            }))
            .unwrap(),
        )),
        content_type: Cow::Borrowed("application/json"),
        headers: Vec::new(),
    }
}

/// Extract a string parameter from the request body as a borrowed `&str`.
/// Call `.map(str::to_owned)` at the call site when an owned `String` is needed.
fn str_param<'a>(ctx: &'a RequestContext, key: &str) -> Option<&'a str> {
    ctx.request_body.get(key).and_then(|v| v.as_str())
}

// ---------------------------------------------------------------------------
// Serialization structs for list responses
// ---------------------------------------------------------------------------

/// Owned summary of a secret for list responses.
///
/// Uses `String` fields so the DashMap read lock can be released before
/// `serde_json` serializes the response, preventing lock contention from
/// inflating P95 under concurrent load.
#[derive(Serialize)]
struct SecretSummary {
    #[serde(rename = "ARN")]
    arn: String,
    #[serde(rename = "Name")]
    name: String,
    #[serde(rename = "Description")]
    description: String,
    #[serde(rename = "CreatedDate")]
    created_date: i64,
    #[serde(rename = "LastChangedDate")]
    last_changed_date: i64,
    #[serde(rename = "DeletedDate", skip_serializing_if = "Option::is_none")]
    deleted_date: Option<i64>,
}

impl SecretSummary {
    fn from_secret(s: &Secret) -> Self {
        Self {
            arn: s.arn.clone(),
            name: s.name.clone(),
            description: s.description.clone(),
            created_date: s.created.timestamp(),
            last_changed_date: s.last_changed.timestamp(),
            deleted_date: s.deletion_date.map(|d| d.timestamp()),
        }
    }
}

/// Borrowed summary of a secret — avoids cloning `String` fields during
/// `ListSecrets` by holding `&str` references into the `DashMap` read guard.
#[derive(Serialize)]
struct SecretSummaryRef<'a> {
    #[serde(rename = "ARN")]
    arn: &'a str,
    #[serde(rename = "Name")]
    name: &'a str,
    #[serde(rename = "Description")]
    description: &'a str,
    #[serde(rename = "CreatedDate")]
    created_date: i64,
    #[serde(rename = "LastChangedDate")]
    last_changed_date: i64,
    #[serde(rename = "DeletedDate", skip_serializing_if = "Option::is_none")]
    deleted_date: Option<i64>,
}

#[derive(Serialize)]
struct ListSecretsResponseRef<'a> {
    #[serde(rename = "SecretList")]
    secret_list: Vec<SecretSummaryRef<'a>>,
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
                    Some(n) => n.to_owned(),
                    None => return Ok(json_error("ValidationException", "Name is required", 400)),
                };
                let description = str_param(ctx, "Description").unwrap_or("").to_owned();
                let secret_string = str_param(ctx, "SecretString").map(str::to_owned);
                let arn = format!(
                    "arn:aws:secretsmanager:{region}:{account_id}:secret:{name}-{}",
                    &Uuid::new_v4().to_string()[..6]
                );
                let version_id = Uuid::new_v4().to_string();

                {
                    if let Some(store) = self.store.get(account_id, region)
                        && store
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
                    Some(s) => s.to_owned(),
                    None => {
                        return Ok(json_error(
                            "ValidationException",
                            "SecretId is required",
                            400,
                        ));
                    }
                };
                let Some(store) = self.store.get(account_id, region) else {
                    return Ok(json_error(
                        "ResourceNotFoundException",
                        "Secrets Manager can't find the specified secret.",
                        400,
                    ));
                };
                let secret = store.secrets.get(&secret_id).or_else(|| {
                    // Try ARN lookup
                    store.secrets.values().find(|s| s.arn == secret_id)
                });
                match secret {
                    None => Ok(json_error(
                        "ResourceNotFoundException",
                        "Secrets Manager can't find the specified secret.",
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
                    Some(s) => s.to_owned(),
                    None => {
                        return Ok(json_error(
                            "ValidationException",
                            "SecretId is required",
                            400,
                        ));
                    }
                };
                let secret_string = str_param(ctx, "SecretString").map(str::to_owned);
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
                        // Demote old AWSCURRENT to AWSPREVIOUS (Change F: no heap alloc)
                        for v in &mut s.versions {
                            if v.version_stages.iter().any(|st| st == "AWSCURRENT") {
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
                    Some(s) => s.to_owned(),
                    None => {
                        return Ok(json_error(
                            "ValidationException",
                            "SecretId is required",
                            400,
                        ));
                    }
                };
                let description = str_param(ctx, "Description").map(str::to_owned);
                let secret_string = str_param(ctx, "SecretString").map(str::to_owned);
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
                            // Change F: no heap alloc for "AWSCURRENT" comparison
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
                    Some(s) => s.to_owned(),
                    None => {
                        return Ok(json_error(
                            "ValidationException",
                            "SecretId is required",
                            400,
                        ));
                    }
                };
                let Some(store) = self.store.get(account_id, region) else {
                    return Ok(json_error(
                        "ResourceNotFoundException",
                        &format!("Secret {secret_id} not found"),
                        400,
                    ));
                };
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
                    // Change E: serialize via struct, not Value DOM tree
                    Some(s) => {
                        let summary = SecretSummary::from_secret(s);
                        Ok(json_ok_bytes(Bytes::from(
                            serde_json::to_vec(&summary).unwrap(),
                        )))
                    }
                }
            }

            "ListSecrets" => {
                // Serialize inside the DashMap read lock using borrowed refs
                // to avoid cloning String fields. The read lock is shared so
                // concurrent readers are not blocked.
                match self.store.get(account_id, region) {
                    None => Ok(json_ok_bytes(Bytes::from_static(b"{\"SecretList\":[]}"))),
                    Some(store) => {
                        let secret_list: Vec<SecretSummaryRef<'_>> = store
                            .secrets
                            .values()
                            .filter(|s| !s.deleted)
                            .map(|s| SecretSummaryRef {
                                arn: &s.arn,
                                name: &s.name,
                                description: &s.description,
                                created_date: s.created.timestamp(),
                                last_changed_date: s.last_changed.timestamp(),
                                deleted_date: s.deletion_date.map(|d| d.timestamp()),
                            })
                            .collect();
                        let resp = ListSecretsResponseRef { secret_list };
                        let mut buf = Vec::with_capacity(64 + store.secrets.len() * 200);
                        serde_json::to_writer(&mut buf, &resp).unwrap();
                        Ok(json_ok_bytes(Bytes::from(buf)))
                        // DashMap read lock released here after serialization
                    }
                }
            }

            "DeleteSecret" => {
                let secret_id = match str_param(ctx, "SecretId") {
                    Some(s) => s.to_owned(),
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
                    Some(s) => s.to_owned(),
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
