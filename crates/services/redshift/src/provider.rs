use std::borrow::Cow;
use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use chrono::Utc;
use openstack_service_framework::traits::{
    DispatchError, DispatchResponse, RequestContext, ResponseBody, ServiceProvider,
};
use openstack_state::AccountRegionBundle;
use uuid::Uuid;

use crate::store::{Cluster, ClusterEndpoint, RedshiftStore};

pub struct RedshiftProvider {
    store: Arc<AccountRegionBundle<RedshiftStore>>,
}

impl RedshiftProvider {
    pub fn new() -> Self {
        Self {
            store: Arc::new(AccountRegionBundle::new()),
        }
    }
}

impl Default for RedshiftProvider {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Helpers — Redshift uses query protocol (XML responses)
// ---------------------------------------------------------------------------

const REDSHIFT_NS: &str = "http://redshift.amazonaws.com/doc/2012-12-01/";

fn xml_resp(action: &str, rid: &str, inner: &str) -> DispatchResponse {
    let xml = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
<{action}Response xmlns=\"{REDSHIFT_NS}\">\
<{action}Result>{inner}</{action}Result>\
<ResponseMetadata><RequestId>{rid}</RequestId></ResponseMetadata>\
</{action}Response>"
    );
    DispatchResponse {
        status_code: 200,
        body: ResponseBody::Buffered(Bytes::from(xml.into_bytes())),
        content_type: Cow::Borrowed("text/xml"),
        headers: Vec::new(),
    }
}

fn xml_error(code: &str, message: &str, status: u16) -> DispatchResponse {
    let xml = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
<ErrorResponse xmlns=\"{REDSHIFT_NS}\">\
<Error><Code>{code}</Code><Message>{message}</Message></Error>\
</ErrorResponse>"
    );
    DispatchResponse {
        status_code: status,
        body: ResponseBody::Buffered(Bytes::from(xml.into_bytes())),
        content_type: Cow::Borrowed("text/xml"),
        headers: Vec::new(),
    }
}

fn req_id() -> String {
    Uuid::new_v4().to_string()
}

fn str_param<'a>(ctx: &'a RequestContext, key: &str) -> Option<&'a str> {
    ctx.query_params
        .get(key)
        .map(|s| s.as_str())
        .or_else(|| ctx.request_body.get(key).and_then(|v| v.as_str()))
}

fn cluster_xml(c: &Cluster) -> String {
    let endpoint_xml = c
        .endpoint
        .as_ref()
        .map(|e| {
            format!(
                "<Endpoint><Address>{}</Address><Port>{}</Port></Endpoint>",
                e.address, e.port
            )
        })
        .unwrap_or_default();
    format!(
        "<ClusterIdentifier>{}</ClusterIdentifier>\
<NodeType>{}</NodeType>\
<MasterUsername>{}</MasterUsername>\
<DBName>{}</DBName>\
<ClusterStatus>{}</ClusterStatus>\
{endpoint_xml}",
        c.cluster_identifier, c.node_type, c.master_username, c.db_name, c.cluster_status
    )
}

// ---------------------------------------------------------------------------
// ServiceProvider
// ---------------------------------------------------------------------------

#[async_trait]
impl ServiceProvider for RedshiftProvider {
    fn service_name(&self) -> &str {
        "redshift"
    }

    async fn dispatch(&self, ctx: &RequestContext) -> Result<DispatchResponse, DispatchError> {
        let region = &ctx.region;
        let account_id = &ctx.account_id;
        let rid = req_id();

        match ctx.operation.as_str() {
            // ----------------------------------------------------------------
            // CreateCluster
            // ----------------------------------------------------------------
            "CreateCluster" => {
                let cluster_id = match str_param(ctx, "ClusterIdentifier") {
                    Some(id) => id.to_string(),
                    None => {
                        return Ok(xml_error(
                            "MissingParameter",
                            "ClusterIdentifier required",
                            400,
                        ));
                    }
                };
                let node_type = str_param(ctx, "NodeType")
                    .unwrap_or("dc2.large")
                    .to_string();
                let master_username = str_param(ctx, "MasterUsername")
                    .unwrap_or("admin")
                    .to_string();
                let master_password = str_param(ctx, "MasterUserPassword")
                    .unwrap_or("")
                    .to_string();
                let db_name = str_param(ctx, "DBName").unwrap_or("dev").to_string();
                let port: u16 = str_param(ctx, "Port")
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(5439);

                let _ = master_password; // not stored for security
                let endpoint = ClusterEndpoint {
                    address: format!("{cluster_id}.fake.{region}.redshift.amazonaws.com"),
                    port,
                };
                let cluster = Cluster {
                    cluster_identifier: cluster_id.clone(),
                    node_type,
                    master_username,
                    db_name,
                    port,
                    cluster_status: "available".to_string(),
                    endpoint: Some(endpoint),
                    created: Utc::now(),
                };

                let mut store = self.store.get_or_create(account_id, region);
                if store.clusters.contains_key(&cluster_id) {
                    return Ok(xml_error(
                        "ClusterAlreadyExists",
                        &format!("Cluster {cluster_id} already exists"),
                        400,
                    ));
                }
                store.clusters.insert(cluster_id.clone(), cluster.clone());
                let inner = format!("<Cluster>{}</Cluster>", cluster_xml(&cluster));
                Ok(xml_resp("CreateCluster", &rid, &inner))
            }

            // ----------------------------------------------------------------
            // DeleteCluster
            // ----------------------------------------------------------------
            "DeleteCluster" => {
                let cluster_id = match str_param(ctx, "ClusterIdentifier") {
                    Some(id) => id.to_string(),
                    None => {
                        return Ok(xml_error(
                            "MissingParameter",
                            "ClusterIdentifier required",
                            400,
                        ));
                    }
                };
                let mut store = self.store.get_or_create(account_id, region);
                match store.clusters.remove(&cluster_id) {
                    Some(c) => {
                        let inner = format!("<Cluster>{}</Cluster>", cluster_xml(&c));
                        Ok(xml_resp("DeleteCluster", &rid, &inner))
                    }
                    None => Ok(xml_error(
                        "ClusterNotFound",
                        &format!("Cluster {cluster_id} not found."),
                        400,
                    )),
                }
            }

            // ----------------------------------------------------------------
            // DescribeClusters
            // ----------------------------------------------------------------
            "DescribeClusters" => {
                let Some(store) = self.store.get(account_id, region) else {
                    let inner = "<Clusters></Clusters>";
                    return Ok(xml_resp("DescribeClusters", &rid, inner));
                };
                let clusters_xml: String = store
                    .clusters
                    .values()
                    .map(|c| format!("<member>{}</member>", cluster_xml(c)))
                    .collect();
                let inner = format!("<Clusters>{clusters_xml}</Clusters>");
                Ok(xml_resp("DescribeClusters", &rid, &inner))
            }

            // ----------------------------------------------------------------
            // ModifyCluster
            // ----------------------------------------------------------------
            "ModifyCluster" => {
                let cluster_id = match str_param(ctx, "ClusterIdentifier") {
                    Some(id) => id.to_string(),
                    None => {
                        return Ok(xml_error(
                            "MissingParameter",
                            "ClusterIdentifier required",
                            400,
                        ));
                    }
                };
                let mut store = self.store.get_or_create(account_id, region);
                match store.clusters.get_mut(&cluster_id) {
                    Some(cluster) => {
                        if let Some(node_type) = str_param(ctx, "NodeType") {
                            cluster.node_type = node_type.to_string();
                        }
                        if let Some(db_name) = str_param(ctx, "DBName") {
                            cluster.db_name = db_name.to_string();
                        }
                        if let Some(port) = str_param(ctx, "Port").and_then(|s| s.parse().ok()) {
                            cluster.port = port;
                            if let Some(endpoint) = cluster.endpoint.as_mut() {
                                endpoint.port = port;
                            }
                        }
                        let inner = format!("<Cluster>{}</Cluster>", cluster_xml(cluster));
                        Ok(xml_resp("ModifyCluster", &rid, &inner))
                    }
                    None => Ok(xml_error(
                        "ClusterNotFound",
                        &format!("Cluster {cluster_id} not found."),
                        400,
                    )),
                }
            }

            _ => Err(DispatchError::NotImplemented(ctx.operation.clone())),
        }
    }

    async fn storage_snapshot(&self) -> Option<serde_json::Value> {
        use serde_json::json;
        let mut clusters = Vec::new();
        for entry in self.store.iter() {
            for cluster in entry.value().clusters.values() {
                clusters.push(json!({
                    "id": cluster.cluster_identifier, "kind": "cluster",
                    "attributes": [
                        {"key": "status", "value": cluster.cluster_status.clone()},
                        {"key": "node_type", "value": cluster.node_type.clone()},
                        {"key": "db_name", "value": cluster.db_name.clone()},
                    ]
                }));
            }
        }
        Some(json!({ "kind": "redshift", "clusters": clusters }))
    }
}
