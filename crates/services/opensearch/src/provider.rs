use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use chrono::Utc;
use openstack_service_framework::traits::{
    DispatchError, DispatchResponse, RequestContext, ServiceProvider,
};
use openstack_state::AccountRegionBundle;
use serde_json::{Value, json};

use crate::store::{ClusterConfig, Domain, OpenSearchStore};

pub struct OpenSearchProvider {
    store: Arc<AccountRegionBundle<OpenSearchStore>>,
}

impl OpenSearchProvider {
    pub fn new() -> Self {
        Self {
            store: Arc::new(AccountRegionBundle::new()),
        }
    }
}

impl Default for OpenSearchProvider {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Helpers — OpenSearch uses JSON protocol with REST paths (no X-Amz-Target)
// Operations are derived from HTTP method + path by the gateway layer.
// ---------------------------------------------------------------------------

fn json_ok(body: Value) -> DispatchResponse {
    DispatchResponse {
        status_code: 200,
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

fn domain_arn(account_id: &str, region: &str, name: &str) -> String {
    format!("arn:aws:es:{region}:{account_id}:domain/{name}")
}

fn domain_endpoint(name: &str, region: &str) -> String {
    format!("search-{name}-fake.{region}.es.amazonaws.com")
}

// ---------------------------------------------------------------------------
// ServiceProvider
// ---------------------------------------------------------------------------

#[async_trait]
impl ServiceProvider for OpenSearchProvider {
    fn service_name(&self) -> &str {
        "opensearch"
    }

    async fn dispatch(&self, ctx: &RequestContext) -> Result<DispatchResponse, DispatchError> {
        let region = &ctx.region;
        let account_id = &ctx.account_id;

        match ctx.operation.as_str() {
            // ----------------------------------------------------------------
            // CreateDomain  POST /2021-01-01/opensearch/domain
            // ----------------------------------------------------------------
            "CreateDomain" => {
                let domain_name = match str_param(ctx, "DomainName") {
                    Some(n) => n,
                    None => {
                        return Ok(json_error(
                            "ValidationException",
                            "DomainName required",
                            400,
                        ));
                    }
                };
                let engine_version =
                    str_param(ctx, "EngineVersion").unwrap_or_else(|| "OpenSearch_2.5".to_string());
                let instance_type = ctx
                    .request_body
                    .get("ClusterConfig")
                    .and_then(|c| c.get("InstanceType"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("t3.small.search")
                    .to_string();
                let instance_count = ctx
                    .request_body
                    .get("ClusterConfig")
                    .and_then(|c| c.get("InstanceCount"))
                    .and_then(|v| v.as_u64())
                    .unwrap_or(1) as u32;

                let arn = domain_arn(account_id, region, &domain_name);
                let endpoint = domain_endpoint(&domain_name, region);
                let now = Utc::now();
                let domain = Domain {
                    domain_name: domain_name.clone(),
                    arn: arn.clone(),
                    engine_version: engine_version.clone(),
                    cluster_config: ClusterConfig {
                        instance_type: instance_type.clone(),
                        instance_count,
                    },
                    endpoint: Some(endpoint.clone()),
                    status: "ACTIVE".to_string(),
                    created: now,
                };

                let mut store = self.store.get_or_create(account_id, region);
                if store.domains.contains_key(&domain_name) {
                    return Ok(json_error(
                        "ResourceAlreadyExistsException",
                        &format!("Domain {domain_name} already exists"),
                        409,
                    ));
                }
                store.domains.insert(domain_name.clone(), domain);
                Ok(json_ok(json!({
                    "DomainStatus": {
                        "DomainName": domain_name,
                        "ARN": arn,
                        "EngineVersion": engine_version,
                        "Endpoint": endpoint,
                        "Processing": false,
                        "Created": true,
                        "Deleted": false,
                    }
                })))
            }

            // ----------------------------------------------------------------
            // DeleteDomain  DELETE /2021-01-01/opensearch/domain/{DomainName}
            // ----------------------------------------------------------------
            "DeleteDomain" => {
                // domain name is last path segment
                let domain_name = ctx.path.split('/').next_back().unwrap_or("").to_string();
                let mut store = self.store.get_or_create(account_id, region);
                match store.domains.remove(&domain_name) {
                    Some(d) => Ok(json_ok(json!({
                        "DomainStatus": {
                            "DomainName": d.domain_name,
                            "ARN": d.arn,
                            "Deleted": true,
                        }
                    }))),
                    None => Ok(json_error(
                        "ResourceNotFoundException",
                        &format!("Domain {domain_name} not found"),
                        409,
                    )),
                }
            }

            // ----------------------------------------------------------------
            // DescribeDomain  GET /2021-01-01/opensearch/domain/{DomainName}
            // ----------------------------------------------------------------
            "DescribeDomain" => {
                let domain_name = ctx.path.split('/').next_back().unwrap_or("").to_string();
                let store = self.store.get_or_create(account_id, region);
                match store.domains.get(&domain_name) {
                    Some(d) => Ok(json_ok(json!({
                        "DomainStatus": {
                            "DomainName": d.domain_name,
                            "ARN": d.arn,
                            "EngineVersion": d.engine_version,
                            "Endpoint": d.endpoint,
                            "Processing": false,
                            "Created": true,
                            "Deleted": false,
                            "ClusterConfig": {
                                "InstanceType": d.cluster_config.instance_type,
                                "InstanceCount": d.cluster_config.instance_count,
                            }
                        }
                    }))),
                    None => Ok(json_error(
                        "ResourceNotFoundException",
                        &format!("Domain {domain_name} not found"),
                        409,
                    )),
                }
            }

            // ----------------------------------------------------------------
            // ListDomainNames  GET /2021-01-01/opensearch/domain
            // ----------------------------------------------------------------
            "ListDomainNames" => {
                let store = self.store.get_or_create(account_id, region);
                let domains: Vec<Value> = store
                    .domains
                    .values()
                    .map(|d| json!({ "DomainName": d.domain_name }))
                    .collect();
                Ok(json_ok(json!({ "DomainNames": domains })))
            }

            _ => Err(DispatchError::NotImplemented(ctx.operation.clone())),
        }
    }
}
