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

use crate::store::{ClusterConfig, Domain, OpenSearchStore, ServiceSoftwareOptions};

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

fn domain_exists_by_arn(store: &OpenSearchStore, arn: &str) -> bool {
    store.domains.values().any(|domain| domain.arn == arn)
}

fn domain_endpoint(name: &str, region: &str) -> String {
    format!("search-{name}-fake.{region}.es.amazonaws.com")
}

fn domain_status_json(d: &Domain) -> Value {
    json!({
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
        },
        "ServiceSoftwareOptions": {
            "CurrentVersion": d.service_software_options.current_version,
            "NewVersion": d.service_software_options.new_version,
            "UpdateAvailable": d.service_software_options.update_available,
            "Cancellable": d.service_software_options.cancellable,
            "UpdateStatus": d.service_software_options.update_status,
            "Description": d.service_software_options.description,
        }
    })
}

// Static list of compatible OpenSearch/Elasticsearch upgrade paths
static COMPATIBLE_VERSIONS: &[(&str, &[&str])] = &[
    (
        "Elasticsearch_7.10",
        &[
            "OpenSearch_1.0",
            "OpenSearch_1.1",
            "OpenSearch_1.2",
            "OpenSearch_1.3",
        ],
    ),
    ("OpenSearch_1.3", &["OpenSearch_2.3", "OpenSearch_2.5"]),
    ("OpenSearch_2.3", &["OpenSearch_2.5", "OpenSearch_2.7"]),
    ("OpenSearch_2.5", &["OpenSearch_2.7", "OpenSearch_2.11"]),
];

static SUPPORTED_VERSIONS: &[&str] = &[
    "OpenSearch_2.11",
    "OpenSearch_2.7",
    "OpenSearch_2.5",
    "OpenSearch_2.3",
    "OpenSearch_1.3",
    "OpenSearch_1.2",
    "OpenSearch_1.1",
    "OpenSearch_1.0",
    "Elasticsearch_7.10",
    "Elasticsearch_7.9",
    "Elasticsearch_7.8",
    "Elasticsearch_7.7",
    "Elasticsearch_6.8",
];

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
                    service_software_options: ServiceSoftwareOptions {
                        current_version: engine_version.clone(),
                        ..ServiceSoftwareOptions::default()
                    },
                };

                let mut store = self.store.get_or_create(account_id, region);
                if store.domains.contains_key(&domain_name) {
                    return Ok(json_error(
                        "ResourceAlreadyExistsException",
                        &format!("Domain {domain_name} already exists"),
                        409,
                    ));
                }
                store.domains.insert(domain_name.clone(), domain.clone());
                Ok(json_ok(json!({
                    "DomainStatus": domain_status_json(&domain)
                })))
            }

            // ----------------------------------------------------------------
            // DeleteDomain  DELETE /2021-01-01/opensearch/domain/{DomainName}
            // ----------------------------------------------------------------
            "DeleteDomain" => {
                let domain_name = ctx.path.split('/').next_back().unwrap_or("").to_string();
                let mut store = self.store.get_or_create(account_id, region);
                match store.domains.remove(&domain_name) {
                    Some(d) => {
                        store.tags.remove(&d.arn);
                        Ok(json_ok(json!({
                            "DomainStatus": {
                                "DomainName": d.domain_name,
                                "ARN": d.arn,
                                "Deleted": true,
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
                        "DomainStatus": domain_status_json(d)
                    }))),
                    None => Ok(json_error(
                        "ResourceNotFoundException",
                        &format!("Domain not found: {domain_name}"),
                        404,
                    )),
                }
            }

            // ----------------------------------------------------------------
            // DescribeDomains  POST /2021-01-01/opensearch/domain-info
            // ----------------------------------------------------------------
            "DescribeDomains" => {
                let domain_names: Vec<String> = ctx
                    .request_body
                    .get("DomainNames")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str().map(String::from))
                            .collect()
                    })
                    .unwrap_or_default();

                let Some(store) = self.store.get(account_id, region) else {
                    return Ok(json_ok(json!({ "DomainStatusList": [] })));
                };

                let statuses: Vec<Value> = domain_names
                    .iter()
                    .filter_map(|name| store.domains.get(name))
                    .map(domain_status_json)
                    .collect();

                Ok(json_ok(json!({ "DomainStatusList": statuses })))
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

            // ----------------------------------------------------------------
            // AddTags  POST /2021-01-01/tags
            // ----------------------------------------------------------------
            "AddTags" => {
                let arn = match ctx.request_body.get("ARN").and_then(|v| v.as_str()) {
                    Some(a) => a.to_string(),
                    None => {
                        return Ok(json_error("ValidationException", "ARN required", 400));
                    }
                };
                let tag_list = ctx
                    .request_body
                    .get("TagList")
                    .and_then(|v| v.as_array())
                    .cloned()
                    .unwrap_or_default();

                let Some(mut store) = self.store.get_mut(account_id, region) else {
                    return Ok(json_error(
                        "ResourceNotFoundException",
                        &format!("Domain not found for ARN: {arn}"),
                        404,
                    ));
                };
                if !domain_exists_by_arn(&store, &arn) {
                    return Ok(json_error(
                        "ResourceNotFoundException",
                        &format!("Domain not found for ARN: {arn}"),
                        404,
                    ));
                }
                let tag_map = store.tags.entry(arn).or_default();
                for tag in &tag_list {
                    if let (Some(key), Some(value)) = (
                        tag.get("Key").and_then(|v| v.as_str()),
                        tag.get("Value").and_then(|v| v.as_str()),
                    ) {
                        tag_map.insert(key.to_string(), value.to_string());
                    }
                }
                Ok(json_ok(json!({})))
            }

            // ----------------------------------------------------------------
            // RemoveTags  POST /2021-01-01/tags-removal
            // ----------------------------------------------------------------
            "RemoveTags" => {
                let arn = match ctx.request_body.get("ARN").and_then(|v| v.as_str()) {
                    Some(a) => a.to_string(),
                    None => {
                        return Ok(json_error("ValidationException", "ARN required", 400));
                    }
                };
                let tag_keys: Vec<String> = ctx
                    .request_body
                    .get("TagKeys")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str().map(String::from))
                            .collect()
                    })
                    .unwrap_or_default();

                let Some(mut store) = self.store.get_mut(account_id, region) else {
                    return Ok(json_error(
                        "ResourceNotFoundException",
                        &format!("Domain not found for ARN: {arn}"),
                        404,
                    ));
                };
                if !domain_exists_by_arn(&store, &arn) {
                    return Ok(json_error(
                        "ResourceNotFoundException",
                        &format!("Domain not found for ARN: {arn}"),
                        404,
                    ));
                }
                if let Some(tag_map) = store.tags.get_mut(&arn) {
                    for key in &tag_keys {
                        tag_map.remove(key);
                    }
                }
                Ok(json_ok(json!({})))
            }

            // ----------------------------------------------------------------
            // ListTags  GET /2021-01-01/tags?arn={ARN}
            // ----------------------------------------------------------------
            "ListTags" => {
                let arn = ctx
                    .query_params
                    .get("arn")
                    .cloned()
                    .or_else(|| {
                        ctx.request_body
                            .get("ARN")
                            .and_then(|v| v.as_str())
                            .map(String::from)
                    })
                    .unwrap_or_default();

                let Some(store) = self.store.get(account_id, region) else {
                    return Ok(json_error(
                        "ResourceNotFoundException",
                        &format!("Domain not found for ARN: {arn}"),
                        404,
                    ));
                };
                if !domain_exists_by_arn(&store, &arn) {
                    return Ok(json_error(
                        "ResourceNotFoundException",
                        &format!("Domain not found for ARN: {arn}"),
                        404,
                    ));
                }
                let tags = store.tags.get(&arn).cloned().unwrap_or_default();

                let tag_list: Vec<Value> = tags
                    .iter()
                    .map(|(k, v)| json!({ "Key": k, "Value": v }))
                    .collect();

                Ok(json_ok(json!({ "TagList": tag_list })))
            }

            // ----------------------------------------------------------------
            // GetCompatibleVersions  GET /2021-01-01/opensearch/compatibleVersions
            // ----------------------------------------------------------------
            "GetCompatibleVersions" => {
                let compatible_versions: Vec<Value> = COMPATIBLE_VERSIONS
                    .iter()
                    .map(|(source, targets)| {
                        json!({
                            "SourceVersion": source,
                            "TargetVersions": targets,
                        })
                    })
                    .collect();
                Ok(json_ok(json!({
                    "CompatibleVersions": compatible_versions
                })))
            }

            // ----------------------------------------------------------------
            // ListVersions  GET /2021-01-01/opensearch/versions
            // ----------------------------------------------------------------
            "ListVersions" => Ok(json_ok(json!({
                "Versions": SUPPORTED_VERSIONS,
                "NextToken": null,
            }))),

            // ----------------------------------------------------------------
            // StartServiceSoftwareUpdate  POST /2021-01-01/opensearch/serviceSoftwareUpdate/start
            // ----------------------------------------------------------------
            "StartServiceSoftwareUpdate" => {
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
                let mut store = self.store.get_or_create(account_id, region);
                match store.domains.get_mut(&domain_name) {
                    Some(domain) => {
                        domain.service_software_options.update_status = "IN_PROGRESS".to_string();
                        domain.service_software_options.update_available = false;
                        domain.service_software_options.cancellable = true;
                        domain.service_software_options.description =
                            "A service software update is in progress.".to_string();
                        let options = domain.service_software_options.clone();
                        Ok(json_ok(json!({
                            "ServiceSoftwareOptions": {
                                "CurrentVersion": options.current_version,
                                "NewVersion": options.new_version,
                                "UpdateAvailable": options.update_available,
                                "Cancellable": options.cancellable,
                                "UpdateStatus": options.update_status,
                                "Description": options.description,
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

            // ----------------------------------------------------------------
            // CancelServiceSoftwareUpdate  POST /2021-01-01/opensearch/serviceSoftwareUpdate/cancel
            // ----------------------------------------------------------------
            "CancelServiceSoftwareUpdate" => {
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
                let mut store = self.store.get_or_create(account_id, region);
                match store.domains.get_mut(&domain_name) {
                    Some(domain) => {
                        domain.service_software_options.update_status = "NOT_ELIGIBLE".to_string();
                        domain.service_software_options.cancellable = false;
                        domain.service_software_options.description =
                            "Service software update was cancelled.".to_string();
                        let options = domain.service_software_options.clone();
                        Ok(json_ok(json!({
                            "ServiceSoftwareOptions": {
                                "CurrentVersion": options.current_version,
                                "NewVersion": options.new_version,
                                "UpdateAvailable": options.update_available,
                                "Cancellable": options.cancellable,
                                "UpdateStatus": options.update_status,
                                "Description": options.description,
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
