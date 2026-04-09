use std::borrow::Cow;
use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use chrono::Utc;
use openstack_service_framework::traits::{
    DispatchError, DispatchResponse, RequestContext, ResponseBody, ServiceProvider,
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
        body: ResponseBody::Buffered(Bytes::from(serde_json::to_vec(&body).unwrap())),
        content_type: Cow::Borrowed("application/json"),
        headers: Vec::new(),
    }
}

fn json_ok_text_plain(body: Value) -> DispatchResponse {
    DispatchResponse {
        status_code: 200,
        body: ResponseBody::Buffered(Bytes::from(serde_json::to_vec(&body).unwrap())),
        // LocalStack returns text/plain for ListDomainNames while the payload is JSON.
        content_type: Cow::Borrowed("text/plain; charset=utf-8"),
        headers: Vec::new(),
    }
}

fn json_error(code: &str, message: &str, status: u16) -> DispatchResponse {
    DispatchResponse {
        status_code: status,
        body: ResponseBody::Buffered(Bytes::from(
            serde_json::to_vec(&json!({
                "message": message,
                "code": code,
            }))
            .unwrap(),
        )),
        content_type: Cow::Borrowed("application/json"),
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
                        &format!("Domain not found: {domain_name}"),
                        404,
                    )),
                }
            }

            // ----------------------------------------------------------------
            // DescribeDomain  GET /2021-01-01/opensearch/domain/{DomainName}
            // ----------------------------------------------------------------
            "DescribeDomain" => {
                let domain_name = ctx.path.split('/').next_back().unwrap_or("").to_string();
                let Some(store) = self.store.get(account_id, region) else {
                    return Ok(json_error(
                        "ResourceNotFoundException",
                        &format!("Domain not found: {domain_name}"),
                        404,
                    ));
                };
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
                        &format!("Domain not found: {domain_name}"),
                        404,
                    )),
                }
            }

            // ----------------------------------------------------------------
            // ListDomainNames  GET /2021-01-01/domain
            // ----------------------------------------------------------------
            "ListDomainNames" => {
                let Some(store) = self.store.get(account_id, region) else {
                    return Ok(json_ok_text_plain(json!({ "DomainNames": [] })));
                };
                let mut domain_names = store
                    .domains
                    .values()
                    .map(|domain| {
                        json!({
                            "DomainName": domain.domain_name,
                            "EngineType": "OpenSearch",
                        })
                    })
                    .collect::<Vec<_>>();
                domain_names.sort_by(|left, right| {
                    left["DomainName"]
                        .as_str()
                        .unwrap_or_default()
                        .cmp(right["DomainName"].as_str().unwrap_or_default())
                });

                Ok(json_ok_text_plain(json!({
                    "DomainNames": domain_names,
                })))
            }

            // ----------------------------------------------------------------
            // DescribeDomainConfig  GET /2021-01-01/opensearch/domain/{DomainName}/config
            // ----------------------------------------------------------------
            "DescribeDomainConfig" => {
                let domain_name = ctx
                    .path
                    .trim_end_matches("/config")
                    .split('/')
                    .next_back()
                    .unwrap_or("")
                    .to_string();
                let Some(store) = self.store.get(account_id, region) else {
                    return Ok(json_error(
                        "ResourceNotFoundException",
                        &format!("Domain not found: {domain_name}"),
                        404,
                    ));
                };
                match store.domains.get(&domain_name) {
                    Some(domain) => Ok(json_ok(json!({
                        "DomainConfig": {
                            "EngineVersion": { "Options": domain.engine_version },
                            "ClusterConfig": {
                                "Options": {
                                    "InstanceType": domain.cluster_config.instance_type,
                                    "InstanceCount": domain.cluster_config.instance_count,
                                }
                            }
                        }
                    }))),
                    None => Ok(json_error(
                        "ResourceNotFoundException",
                        &format!("Domain not found: {domain_name}"),
                        404,
                    )),
                }
            }

            // ----------------------------------------------------------------
            // UpdateDomainConfig  POST /2021-01-01/opensearch/domain/{DomainName}/config
            // ----------------------------------------------------------------
            "UpdateDomainConfig" => {
                let domain_name = ctx
                    .path
                    .trim_end_matches("/config")
                    .split('/')
                    .next_back()
                    .unwrap_or("")
                    .to_string();
                let mut store = self.store.get_or_create(account_id, region);
                match store.domains.get_mut(&domain_name) {
                    Some(domain) => {
                        if let Some(engine_version) = str_param(ctx, "EngineVersion") {
                            domain.engine_version = engine_version;
                        }
                        if let Some(cluster_config) = ctx.request_body.get("ClusterConfig") {
                            if let Some(instance_type) =
                                cluster_config.get("InstanceType").and_then(|v| v.as_str())
                            {
                                domain.cluster_config.instance_type = instance_type.to_string();
                            }
                            if let Some(instance_count_value) = cluster_config.get("InstanceCount")
                            {
                                let Some(instance_count) = instance_count_value.as_u64() else {
                                    return Ok(json_error(
                                        "ValidationException",
                                        "ClusterConfig.InstanceCount must be a non-negative integer",
                                        400,
                                    ));
                                };
                                let Ok(instance_count) = u32::try_from(instance_count) else {
                                    return Ok(json_error(
                                        "ValidationException",
                                        "ClusterConfig.InstanceCount is too large",
                                        400,
                                    ));
                                };
                                domain.cluster_config.instance_count = instance_count;
                            }
                        }
                        Ok(json_ok(json!({
                            "DomainConfig": {
                                "EngineVersion": { "Options": domain.engine_version },
                                "ClusterConfig": {
                                    "Options": {
                                        "InstanceType": domain.cluster_config.instance_type,
                                        "InstanceCount": domain.cluster_config.instance_count,
                                    }
                                }
                            }
                        })))
                    }
                    None => Ok(json_error(
                        "ResourceNotFoundException",
                        &format!("Domain not found: {domain_name}"),
                        404,
                    )),
                }
            }

            _ => Err(DispatchError::NotImplemented(ctx.operation.clone())),
        }
    }

    async fn storage_snapshot(&self) -> Option<serde_json::Value> {
        use serde_json::json;
        let mut domains = Vec::new();
        for entry in self.store.iter() {
            for domain in entry.value().domains.values() {
                domains.push(json!({
                    "id": domain.arn, "kind": "domain",
                    "attributes": [
                        {"key": "name", "value": domain.domain_name.clone()},
                        {"key": "status", "value": domain.status.clone()},
                        {"key": "engine", "value": domain.engine_version.clone()},
                    ]
                }));
            }
        }
        Some(json!({ "kind": "opensearch", "domains": domains }))
    }
}
