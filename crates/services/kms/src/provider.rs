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
use uuid::Uuid;

use crate::store::{KeyState, KmsKey, KmsStore};

pub struct KmsProvider {
    store: Arc<AccountRegionBundle<KmsStore>>,
}

impl KmsProvider {
    pub fn new() -> Self {
        Self {
            store: Arc::new(AccountRegionBundle::new()),
        }
    }
}

impl Default for KmsProvider {
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

fn rand_hex(bytes: usize) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    // Deterministic-ish random using time + uuid
    let seed = Uuid::new_v4().to_string();
    let mut out = String::with_capacity(bytes * 2);
    let mut remaining = bytes;
    let mut s = seed;
    while remaining > 0 {
        let mut h = DefaultHasher::new();
        s.hash(&mut h);
        let val = h.finish();
        let chunk = format!("{val:016x}");
        let take = remaining.min(8);
        out.push_str(&chunk[..take * 2]);
        remaining -= take;
        s = chunk;
    }
    out
}

fn key_metadata(k: &KmsKey) -> Value {
    json!({
        "KeyId": k.key_id,
        "Arn": k.arn,
        "Description": k.description,
        "KeyState": k.key_state.to_string(),
        "CreationDate": k.created.timestamp(),
        "Enabled": k.key_state == KeyState::Enabled,
        "KeyUsage": "ENCRYPT_DECRYPT",
        "KeySpec": "SYMMETRIC_DEFAULT",
        "Origin": "AWS_KMS",
        "MultiRegion": false,
    })
}

// ---------------------------------------------------------------------------
// ServiceProvider
// ---------------------------------------------------------------------------

#[async_trait]
impl ServiceProvider for KmsProvider {
    fn service_name(&self) -> &str {
        "kms"
    }

    async fn dispatch(&self, ctx: &RequestContext) -> Result<DispatchResponse, DispatchError> {
        let op = ctx.operation.as_str();
        let account_id = &ctx.account_id;
        let region = &ctx.region;

        match op {
            "CreateKey" => {
                let description = str_param(ctx, "Description").unwrap_or_default();
                let key_id = Uuid::new_v4().to_string();
                let arn = format!("arn:aws:kms:{region}:{account_id}:key/{key_id}");
                let key = KmsKey {
                    key_id: key_id.clone(),
                    arn: arn.clone(),
                    description: description.clone(),
                    key_state: KeyState::Enabled,
                    created: Utc::now(),
                    key_material: rand_hex(32),
                    aliases: Vec::new(),
                    tags: Default::default(),
                };
                let meta = key_metadata(&key);
                let mut store = self.store.get_or_create(account_id, region);
                store.keys.insert(key_id, key);
                Ok(json_ok(json!({ "KeyMetadata": meta })))
            }

            "DescribeKey" => {
                let key_id = match str_param(ctx, "KeyId") {
                    Some(k) => k,
                    None => return Ok(json_error("ValidationException", "KeyId is required", 400)),
                };
                let store = self.store.get_or_create(account_id, region);
                match store.resolve_key(&key_id) {
                    None => Ok(json_error(
                        "NotFoundException",
                        &format!("Invalid keyId {key_id}"),
                        404,
                    )),
                    Some(k) => Ok(json_ok(json!({ "KeyMetadata": key_metadata(k) }))),
                }
            }

            "ListKeys" => {
                let store = self.store.get_or_create(account_id, region);
                let keys: Vec<Value> = store
                    .keys
                    .values()
                    .map(|k| json!({ "KeyId": k.key_id, "KeyArn": k.arn }))
                    .collect();
                Ok(json_ok(json!({ "Keys": keys, "Truncated": false })))
            }

            "EnableKey" => {
                let key_id = match str_param(ctx, "KeyId") {
                    Some(k) => k,
                    None => return Ok(json_error("ValidationException", "KeyId is required", 400)),
                };
                let mut store = self.store.get_or_create(account_id, region);
                match store.resolve_key_mut(&key_id) {
                    None => Ok(json_error(
                        "NotFoundException",
                        &format!("Invalid keyId {key_id}"),
                        404,
                    )),
                    Some(k) => {
                        k.key_state = KeyState::Enabled;
                        Ok(json_ok(json!({})))
                    }
                }
            }

            "DisableKey" => {
                let key_id = match str_param(ctx, "KeyId") {
                    Some(k) => k,
                    None => return Ok(json_error("ValidationException", "KeyId is required", 400)),
                };
                let mut store = self.store.get_or_create(account_id, region);
                match store.resolve_key_mut(&key_id) {
                    None => Ok(json_error(
                        "NotFoundException",
                        &format!("Invalid keyId {key_id}"),
                        404,
                    )),
                    Some(k) => {
                        k.key_state = KeyState::Disabled;
                        Ok(json_ok(json!({})))
                    }
                }
            }

            "ScheduleKeyDeletion" => {
                let key_id = match str_param(ctx, "KeyId") {
                    Some(k) => k,
                    None => return Ok(json_error("ValidationException", "KeyId is required", 400)),
                };
                let pending_window = ctx
                    .request_body
                    .get("PendingWindowInDays")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(30);
                let deletion_date = Utc::now() + chrono::Duration::days(pending_window);
                let mut store = self.store.get_or_create(account_id, region);
                match store.resolve_key_mut(&key_id) {
                    None => Ok(json_error(
                        "NotFoundException",
                        &format!("Invalid keyId {key_id}"),
                        404,
                    )),
                    Some(k) => {
                        k.key_state = KeyState::PendingDeletion;
                        Ok(json_ok(json!({
                            "KeyId": k.key_id,
                            "DeletionDate": deletion_date.timestamp(),
                        })))
                    }
                }
            }

            "CreateAlias" => {
                let alias_name = match str_param(ctx, "AliasName") {
                    Some(a) => a,
                    None => {
                        return Ok(json_error(
                            "ValidationException",
                            "AliasName is required",
                            400,
                        ));
                    }
                };
                let target_key_id = match str_param(ctx, "TargetKeyId") {
                    Some(k) => k,
                    None => {
                        return Ok(json_error(
                            "ValidationException",
                            "TargetKeyId is required",
                            400,
                        ));
                    }
                };
                let mut store = self.store.get_or_create(account_id, region);
                // Resolve target
                let resolved_key_id = match store.resolve_key(&target_key_id) {
                    None => {
                        return Ok(json_error(
                            "NotFoundException",
                            &format!("Invalid keyId {target_key_id}"),
                            404,
                        ));
                    }
                    Some(k) => k.key_id.clone(),
                };
                store
                    .alias_to_key
                    .insert(alias_name.clone(), resolved_key_id.clone());
                if let Some(k) = store.keys.get_mut(&resolved_key_id)
                    && !k.aliases.contains(&alias_name)
                {
                    k.aliases.push(alias_name);
                }
                Ok(json_ok(json!({})))
            }

            "ListAliases" => {
                let store = self.store.get_or_create(account_id, region);
                let aliases: Vec<Value> = store
                    .alias_to_key
                    .iter()
                    .map(|(alias, key_id)| {
                        let arn = store
                            .keys
                            .get(key_id)
                            .map(|_k| {
                                format!(
                                    "arn:aws:kms:{region}:{account_id}:alias/{}",
                                    alias.trim_start_matches("alias/")
                                )
                            })
                            .unwrap_or_default();
                        json!({
                            "AliasName": alias,
                            "AliasArn": arn,
                            "TargetKeyId": key_id,
                        })
                    })
                    .collect();
                Ok(json_ok(json!({ "Aliases": aliases, "Truncated": false })))
            }

            "Encrypt" => {
                let key_id = match str_param(ctx, "KeyId") {
                    Some(k) => k,
                    None => return Ok(json_error("ValidationException", "KeyId is required", 400)),
                };
                let plaintext_b64 = match str_param(ctx, "Plaintext") {
                    Some(p) => p,
                    None => {
                        return Ok(json_error(
                            "ValidationException",
                            "Plaintext is required",
                            400,
                        ));
                    }
                };
                let store = self.store.get_or_create(account_id, region);
                let key = match store.resolve_key(&key_id) {
                    None => {
                        return Ok(json_error(
                            "NotFoundException",
                            &format!("Invalid keyId {key_id}"),
                            404,
                        ));
                    }
                    Some(k) => k.clone(),
                };
                if key.key_state != KeyState::Enabled {
                    return Ok(json_error("DisabledException", "Key is disabled", 400));
                }
                // Simple envelope: base64(key_id + ":" + plaintext_b64)
                let envelope = format!("{}:{}", key.key_id, plaintext_b64);
                let ciphertext = B64.encode(envelope.as_bytes());
                Ok(json_ok(json!({
                    "CiphertextBlob": ciphertext,
                    "KeyId": key.arn,
                    "EncryptionAlgorithm": "SYMMETRIC_DEFAULT",
                })))
            }

            "Decrypt" => {
                let ciphertext_b64 = match str_param(ctx, "CiphertextBlob") {
                    Some(c) => c,
                    None => {
                        return Ok(json_error(
                            "ValidationException",
                            "CiphertextBlob is required",
                            400,
                        ));
                    }
                };
                let decoded = match B64.decode(ciphertext_b64.as_bytes()) {
                    Ok(d) => d,
                    Err(_) => {
                        return Ok(json_error(
                            "InvalidCiphertextException",
                            "Invalid ciphertext",
                            400,
                        ));
                    }
                };
                let envelope = String::from_utf8_lossy(&decoded);
                let mut parts = envelope.splitn(2, ':');
                let key_id = parts.next().unwrap_or("").to_string();
                let plaintext_b64 = parts.next().unwrap_or("").to_string();
                let store = self.store.get_or_create(account_id, region);
                let key = match store.resolve_key(&key_id) {
                    None => {
                        return Ok(json_error(
                            "NotFoundException",
                            &format!("Invalid keyId {key_id}"),
                            404,
                        ));
                    }
                    Some(k) => k.clone(),
                };
                if key.key_state != KeyState::Enabled {
                    return Ok(json_error("DisabledException", "Key is disabled", 400));
                }
                Ok(json_ok(json!({
                    "Plaintext": plaintext_b64,
                    "KeyId": key.arn,
                    "EncryptionAlgorithm": "SYMMETRIC_DEFAULT",
                })))
            }

            "GenerateDataKey" | "GenerateDataKeyWithoutPlaintext" => {
                let key_id = match str_param(ctx, "KeyId") {
                    Some(k) => k,
                    None => return Ok(json_error("ValidationException", "KeyId is required", 400)),
                };
                let store = self.store.get_or_create(account_id, region);
                let key = match store.resolve_key(&key_id) {
                    None => {
                        return Ok(json_error(
                            "NotFoundException",
                            &format!("Invalid keyId {key_id}"),
                            404,
                        ));
                    }
                    Some(k) => k.clone(),
                };
                drop(store);
                if key.key_state != KeyState::Enabled {
                    return Ok(json_error("DisabledException", "Key is disabled", 400));
                }
                let plaintext_bytes = rand_hex(32);
                let plaintext_b64 = B64.encode(plaintext_bytes.as_bytes());
                let envelope = format!("{}:{}", key.key_id, plaintext_b64);
                let ciphertext = B64.encode(envelope.as_bytes());
                if op == "GenerateDataKeyWithoutPlaintext" {
                    Ok(json_ok(json!({
                        "CiphertextBlob": ciphertext,
                        "KeyId": key.arn,
                    })))
                } else {
                    Ok(json_ok(json!({
                        "CiphertextBlob": ciphertext,
                        "Plaintext": plaintext_b64,
                        "KeyId": key.arn,
                    })))
                }
            }

            "GenerateRandom" => {
                let num_bytes = ctx
                    .request_body
                    .get("NumberOfBytes")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(32) as usize;
                let random_hex = rand_hex(num_bytes);
                let b64 = B64.encode(random_hex.as_bytes());
                Ok(json_ok(json!({ "Plaintext": b64 })))
            }

            "Sign" => {
                // Stub: return a fake signature
                let key_id = str_param(ctx, "KeyId").unwrap_or_default();
                let store = self.store.get_or_create(account_id, region);
                let key_arn = store
                    .resolve_key(&key_id)
                    .map(|k| k.arn.clone())
                    .unwrap_or_else(|| format!("arn:aws:kms:{region}:{account_id}:key/{key_id}"));
                let sig = B64.encode(rand_hex(64).as_bytes());
                Ok(json_ok(json!({
                    "Signature": sig,
                    "SigningAlgorithm": "RSASSA_PKCS1_V1_5_SHA_256",
                    "KeyId": key_arn,
                })))
            }

            "Verify" => {
                // Stub: always return valid
                let key_id = str_param(ctx, "KeyId").unwrap_or_default();
                let store = self.store.get_or_create(account_id, region);
                let key_arn = store
                    .resolve_key(&key_id)
                    .map(|k| k.arn.clone())
                    .unwrap_or_else(|| format!("arn:aws:kms:{region}:{account_id}:key/{key_id}"));
                Ok(json_ok(json!({
                    "KeyId": key_arn,
                    "SignatureValid": true,
                    "SigningAlgorithm": "RSASSA_PKCS1_V1_5_SHA_256",
                })))
            }

            _ => Ok(json_error(
                "NotImplementedException",
                &format!("Operation not implemented: {op}"),
                501,
            )),
        }
    }
}
