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

use crate::store::{AcmStore, Certificate, CertificateStatus};

pub struct AcmProvider {
    store: Arc<AccountRegionBundle<AcmStore>>,
}

impl AcmProvider {
    pub fn new() -> Self {
        Self {
            store: Arc::new(AccountRegionBundle::new()),
        }
    }
}

impl Default for AcmProvider {
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

fn cert_detail(c: &Certificate) -> Value {
    json!({
        "CertificateArn": c.arn,
        "DomainName": c.domain_name,
        "SubjectAlternativeNames": c.subject_alternative_names,
        "Status": c.status.as_str(),
        "CreatedAt": c.created.timestamp(),
        "IssuedAt": c.created.timestamp(),
        "Type": "AMAZON_ISSUED",
        "KeyAlgorithm": "RSA_2048",
        "SignatureAlgorithm": "SHA256WITHRSA",
        "InUseBy": [],
        "RenewalEligibility": "INELIGIBLE",
        "Options": { "CertificateTransparencyLoggingPreference": "ENABLED" },
    })
}

// ---------------------------------------------------------------------------
// ServiceProvider
// ---------------------------------------------------------------------------

#[async_trait]
impl ServiceProvider for AcmProvider {
    fn service_name(&self) -> &str {
        "acm"
    }

    async fn dispatch(&self, ctx: &RequestContext) -> Result<DispatchResponse, DispatchError> {
        let op = ctx.operation.as_str();
        let account_id = &ctx.account_id;
        let region = &ctx.region;

        match op {
            "RequestCertificate" => {
                let domain_name = match str_param(ctx, "DomainName") {
                    Some(d) => d,
                    None => {
                        return Ok(json_error(
                            "ValidationException",
                            "DomainName is required",
                            400,
                        ));
                    }
                };
                let sans: Vec<String> = ctx
                    .request_body
                    .get("SubjectAlternativeNames")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str().map(String::from))
                            .collect()
                    })
                    .unwrap_or_default();

                let cert_id = Uuid::new_v4().to_string();
                let arn = format!("arn:aws:acm:{region}:{account_id}:certificate/{cert_id}");
                let cert = Certificate {
                    arn: arn.clone(),
                    domain_name,
                    subject_alternative_names: sans,
                    status: CertificateStatus::Issued, // auto-issue
                    created: Utc::now(),
                    tags: Default::default(),
                };
                let mut store = self.store.get_or_create(account_id, region);
                store.certificates.insert(arn.clone(), cert);
                Ok(json_ok(json!({ "CertificateArn": arn })))
            }

            "DescribeCertificate" => {
                let arn = match str_param(ctx, "CertificateArn") {
                    Some(a) => a,
                    None => {
                        return Ok(json_error(
                            "ValidationException",
                            "CertificateArn is required",
                            400,
                        ));
                    }
                };
                let store = self.store.get_or_create(account_id, region);
                match store.certificates.get(&arn) {
                    None => Ok(json_error(
                        "ResourceNotFoundException",
                        &format!("Certificate {arn} not found"),
                        400,
                    )),
                    Some(c) => Ok(json_ok(json!({ "Certificate": cert_detail(c) }))),
                }
            }

            "ListCertificates" => {
                let store = self.store.get_or_create(account_id, region);
                let certs: Vec<Value> = store
                    .certificates
                    .values()
                    .map(|c| {
                        json!({
                            "CertificateArn": c.arn,
                            "DomainName": c.domain_name,
                            "Status": c.status.as_str(),
                        })
                    })
                    .collect();
                Ok(json_ok(json!({ "CertificateSummaryList": certs })))
            }

            "DeleteCertificate" => {
                let arn = match str_param(ctx, "CertificateArn") {
                    Some(a) => a,
                    None => {
                        return Ok(json_error(
                            "ValidationException",
                            "CertificateArn is required",
                            400,
                        ));
                    }
                };
                let mut store = self.store.get_or_create(account_id, region);
                if store.certificates.remove(&arn).is_none() {
                    return Ok(json_error(
                        "ResourceNotFoundException",
                        &format!("Certificate {arn} not found"),
                        400,
                    ));
                }
                Ok(json_ok(json!({})))
            }

            "AddTagsToCertificate" => {
                let arn = match str_param(ctx, "CertificateArn") {
                    Some(a) => a,
                    None => {
                        return Ok(json_error(
                            "ValidationException",
                            "CertificateArn is required",
                            400,
                        ));
                    }
                };
                let tags = ctx
                    .request_body
                    .get("Tags")
                    .and_then(|v| v.as_array())
                    .cloned()
                    .unwrap_or_default();
                let mut store = self.store.get_or_create(account_id, region);
                match store.certificates.get_mut(&arn) {
                    None => Ok(json_error(
                        "ResourceNotFoundException",
                        &format!("Certificate {arn} not found"),
                        400,
                    )),
                    Some(c) => {
                        for tag in &tags {
                            if let (Some(k), Some(v)) = (
                                tag.get("Key").and_then(|v| v.as_str()),
                                tag.get("Value").and_then(|v| v.as_str()),
                            ) {
                                c.tags.insert(k.to_string(), v.to_string());
                            }
                        }
                        Ok(json_ok(json!({})))
                    }
                }
            }

            "ListTagsForCertificate" => {
                let arn = match str_param(ctx, "CertificateArn") {
                    Some(a) => a,
                    None => {
                        return Ok(json_error(
                            "ValidationException",
                            "CertificateArn is required",
                            400,
                        ));
                    }
                };
                let store = self.store.get_or_create(account_id, region);
                match store.certificates.get(&arn) {
                    None => Ok(json_error(
                        "ResourceNotFoundException",
                        &format!("Certificate {arn} not found"),
                        400,
                    )),
                    Some(c) => {
                        let tags: Vec<Value> = c
                            .tags
                            .iter()
                            .map(|(k, v)| json!({ "Key": k, "Value": v }))
                            .collect();
                        Ok(json_ok(json!({ "Tags": tags })))
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
