use std::borrow::Cow;
use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use chrono::Utc;
use openstack_service_framework::traits::{
    DispatchError, DispatchResponse, RequestContext, ResponseBody, ServiceProvider,
};
use openstack_service_framework::xml::xml_escape;
use openstack_state::AccountRegionBundle;
use uuid::Uuid;

use crate::store::{
    Cluster, ClusterEndpoint, ClusterParameterGroup, ClusterSnapshot, ClusterSubnetGroup,
    RedshiftStore,
};

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
                xml_escape(&e.address),
                e.port
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
        xml_escape(&c.cluster_identifier),
        xml_escape(&c.node_type),
        xml_escape(&c.master_username),
        xml_escape(&c.db_name),
        xml_escape(&c.cluster_status)
    )
}

fn snapshot_xml(s: &ClusterSnapshot) -> String {
    format!(
        "<SnapshotIdentifier>{}</SnapshotIdentifier>\
<ClusterIdentifier>{}</ClusterIdentifier>\
<Status>{}</Status>\
<DBName>{}</DBName>\
<MasterUsername>{}</MasterUsername>\
<NodeType>{}</NodeType>\
<SnapshotCreateTime>{}</SnapshotCreateTime>",
        xml_escape(&s.snapshot_identifier),
        xml_escape(&s.cluster_identifier),
        xml_escape(&s.status),
        xml_escape(&s.db_name),
        xml_escape(&s.master_username),
        xml_escape(&s.node_type),
        s.created.format("%Y-%m-%dT%H:%M:%SZ")
    )
}

fn subnet_group_xml(sg: &ClusterSubnetGroup) -> String {
    let subnets: String = sg
        .subnet_ids
        .iter()
        .map(|id| {
            format!(
                "<member><SubnetIdentifier>{}</SubnetIdentifier></member>",
                xml_escape(id)
            )
        })
        .collect();
    format!(
        "<ClusterSubnetGroupName>{}</ClusterSubnetGroupName>\
<Description>{}</Description>\
<VpcId>{}</VpcId>\
<SubnetGroupStatus>{}</SubnetGroupStatus>\
<Subnets>{subnets}</Subnets>",
        xml_escape(&sg.cluster_subnet_group_name),
        xml_escape(&sg.description),
        xml_escape(&sg.vpc_id),
        xml_escape(&sg.status),
    )
}

fn param_group_xml(pg: &ClusterParameterGroup) -> String {
    format!(
        "<ParameterGroupName>{}</ParameterGroupName>\
<ParameterGroupFamily>{}</ParameterGroupFamily>\
<Description>{}</Description>",
        xml_escape(&pg.parameter_group_name),
        xml_escape(&pg.parameter_group_family),
        xml_escape(&pg.description),
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
                    logging_enabled: false,
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
            // RebootCluster
            // ----------------------------------------------------------------
            "RebootCluster" => {
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
                        cluster.cluster_status = "available".to_string();
                        let inner = format!("<Cluster>{}</Cluster>", cluster_xml(cluster));
                        Ok(xml_resp("RebootCluster", &rid, &inner))
                    }
                    None => Ok(xml_error(
                        "ClusterNotFound",
                        &format!("Cluster {cluster_id} not found."),
                        400,
                    )),
                }
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
                        if let Some(port_str) = str_param(ctx, "Port") {
                            let Ok(port) = port_str.parse::<u16>() else {
                                return Ok(xml_error(
                                    "InvalidParameterValue",
                                    "Port must be a valid 16-bit integer",
                                    400,
                                ));
                            };
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

            // ----------------------------------------------------------------
            // CreateClusterSnapshot
            // ----------------------------------------------------------------
            "CreateClusterSnapshot" => {
                let snapshot_id = match str_param(ctx, "SnapshotIdentifier") {
                    Some(id) => id.to_string(),
                    None => {
                        return Ok(xml_error(
                            "MissingParameter",
                            "SnapshotIdentifier required",
                            400,
                        ));
                    }
                };
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
                if store.snapshots.contains_key(&snapshot_id) {
                    return Ok(xml_error(
                        "ClusterSnapshotAlreadyExists",
                        &format!("Snapshot {snapshot_id} already exists"),
                        400,
                    ));
                }
                let (node_type, db_name, master_username) = store
                    .clusters
                    .get(&cluster_id)
                    .map(|c| {
                        (
                            c.node_type.clone(),
                            c.db_name.clone(),
                            c.master_username.clone(),
                        )
                    })
                    .unwrap_or_else(|| {
                        (
                            "dc2.large".to_string(),
                            "dev".to_string(),
                            "admin".to_string(),
                        )
                    });
                let snapshot = ClusterSnapshot {
                    snapshot_identifier: snapshot_id.clone(),
                    cluster_identifier: cluster_id,
                    status: "available".to_string(),
                    created: Utc::now(),
                    node_type,
                    db_name,
                    master_username,
                };
                store
                    .snapshots
                    .insert(snapshot_id.clone(), snapshot.clone());
                let inner = format!("<Snapshot>{}</Snapshot>", snapshot_xml(&snapshot));
                Ok(xml_resp("CreateClusterSnapshot", &rid, &inner))
            }

            // ----------------------------------------------------------------
            // DeleteClusterSnapshot
            // ----------------------------------------------------------------
            "DeleteClusterSnapshot" => {
                let snapshot_id = match str_param(ctx, "SnapshotIdentifier") {
                    Some(id) => id.to_string(),
                    None => {
                        return Ok(xml_error(
                            "MissingParameter",
                            "SnapshotIdentifier required",
                            400,
                        ));
                    }
                };
                let mut store = self.store.get_or_create(account_id, region);
                match store.snapshots.remove(&snapshot_id) {
                    Some(s) => {
                        let inner = format!("<Snapshot>{}</Snapshot>", snapshot_xml(&s));
                        Ok(xml_resp("DeleteClusterSnapshot", &rid, &inner))
                    }
                    None => Ok(xml_error(
                        "ClusterSnapshotNotFound",
                        &format!("Snapshot {snapshot_id} not found"),
                        400,
                    )),
                }
            }

            // ----------------------------------------------------------------
            // DescribeClusterSnapshots
            // ----------------------------------------------------------------
            "DescribeClusterSnapshots" => {
                let Some(store) = self.store.get(account_id, region) else {
                    return Ok(xml_resp(
                        "DescribeClusterSnapshots",
                        &rid,
                        "<Snapshots></Snapshots>",
                    ));
                };
                let filter_cluster = str_param(ctx, "ClusterIdentifier");
                let snapshots_xml: String = store
                    .snapshots
                    .values()
                    .filter(|s| {
                        filter_cluster
                            .map(|id| s.cluster_identifier == id)
                            .unwrap_or(true)
                    })
                    .map(|s| format!("<member>{}</member>", snapshot_xml(s)))
                    .collect();
                let inner = format!("<Snapshots>{snapshots_xml}</Snapshots>");
                Ok(xml_resp("DescribeClusterSnapshots", &rid, &inner))
            }

            // ----------------------------------------------------------------
            // CreateClusterSubnetGroup
            // ----------------------------------------------------------------
            "CreateClusterSubnetGroup" => {
                let name = match str_param(ctx, "ClusterSubnetGroupName") {
                    Some(n) => n.to_string(),
                    None => {
                        return Ok(xml_error(
                            "MissingParameter",
                            "ClusterSubnetGroupName required",
                            400,
                        ));
                    }
                };
                let description = str_param(ctx, "Description").unwrap_or("").to_string();
                let vpc_id = str_param(ctx, "VpcId").unwrap_or("").to_string();

                // Collect subnet IDs: SubnetIds.SubnetIdentifier.1, .2, ...
                let mut subnet_ids = Vec::new();
                let mut idx = 1usize;
                loop {
                    let key = format!("SubnetIds.SubnetIdentifier.{idx}");
                    if let Some(sid) = str_param(ctx, &key) {
                        subnet_ids.push(sid.to_string());
                        idx += 1;
                    } else {
                        break;
                    }
                }

                let mut store = self.store.get_or_create(account_id, region);
                if store.subnet_groups.contains_key(&name) {
                    return Ok(xml_error(
                        "ClusterSubnetGroupAlreadyExists",
                        &format!("Subnet group {name} already exists"),
                        400,
                    ));
                }
                let sg = ClusterSubnetGroup {
                    cluster_subnet_group_name: name.clone(),
                    description,
                    vpc_id,
                    subnet_ids,
                    status: "Complete".to_string(),
                };
                store.subnet_groups.insert(name, sg.clone());
                let inner = format!(
                    "<ClusterSubnetGroup>{}</ClusterSubnetGroup>",
                    subnet_group_xml(&sg)
                );
                Ok(xml_resp("CreateClusterSubnetGroup", &rid, &inner))
            }

            // ----------------------------------------------------------------
            // DeleteClusterSubnetGroup
            // ----------------------------------------------------------------
            "DeleteClusterSubnetGroup" => {
                let name = match str_param(ctx, "ClusterSubnetGroupName") {
                    Some(n) => n.to_string(),
                    None => {
                        return Ok(xml_error(
                            "MissingParameter",
                            "ClusterSubnetGroupName required",
                            400,
                        ));
                    }
                };
                let mut store = self.store.get_or_create(account_id, region);
                if store.subnet_groups.remove(&name).is_none() {
                    return Ok(xml_error(
                        "ClusterSubnetGroupNotFoundFault",
                        &format!("Subnet group {name} not found"),
                        400,
                    ));
                }
                Ok(xml_resp("DeleteClusterSubnetGroup", &rid, ""))
            }

            // ----------------------------------------------------------------
            // DescribeClusterSubnetGroups
            // ----------------------------------------------------------------
            "DescribeClusterSubnetGroups" => {
                let Some(store) = self.store.get(account_id, region) else {
                    return Ok(xml_resp(
                        "DescribeClusterSubnetGroups",
                        &rid,
                        "<ClusterSubnetGroups></ClusterSubnetGroups>",
                    ));
                };
                let items_xml: String = store
                    .subnet_groups
                    .values()
                    .map(|sg| format!("<member>{}</member>", subnet_group_xml(sg)))
                    .collect();
                let inner = format!("<ClusterSubnetGroups>{items_xml}</ClusterSubnetGroups>");
                Ok(xml_resp("DescribeClusterSubnetGroups", &rid, &inner))
            }

            // ----------------------------------------------------------------
            // CreateClusterParameterGroup
            // ----------------------------------------------------------------
            "CreateClusterParameterGroup" => {
                let name = match str_param(ctx, "ParameterGroupName") {
                    Some(n) => n.to_string(),
                    None => {
                        return Ok(xml_error(
                            "MissingParameter",
                            "ParameterGroupName required",
                            400,
                        ));
                    }
                };
                let family = str_param(ctx, "ParameterGroupFamily")
                    .unwrap_or("redshift-1.0")
                    .to_string();
                let description = str_param(ctx, "Description").unwrap_or("").to_string();

                let mut store = self.store.get_or_create(account_id, region);
                if store.parameter_groups.contains_key(&name) {
                    return Ok(xml_error(
                        "ClusterParameterGroupAlreadyExists",
                        &format!("Parameter group {name} already exists"),
                        400,
                    ));
                }
                let pg = ClusterParameterGroup {
                    parameter_group_name: name.clone(),
                    parameter_group_family: family,
                    description,
                };
                store.parameter_groups.insert(name, pg.clone());
                let inner = format!(
                    "<ClusterParameterGroup>{}</ClusterParameterGroup>",
                    param_group_xml(&pg)
                );
                Ok(xml_resp("CreateClusterParameterGroup", &rid, &inner))
            }

            // ----------------------------------------------------------------
            // DeleteClusterParameterGroup
            // ----------------------------------------------------------------
            "DeleteClusterParameterGroup" => {
                let name = match str_param(ctx, "ParameterGroupName") {
                    Some(n) => n.to_string(),
                    None => {
                        return Ok(xml_error(
                            "MissingParameter",
                            "ParameterGroupName required",
                            400,
                        ));
                    }
                };
                let mut store = self.store.get_or_create(account_id, region);
                if store.parameter_groups.remove(&name).is_none() {
                    return Ok(xml_error(
                        "ClusterParameterGroupNotFound",
                        &format!("Parameter group {name} not found"),
                        400,
                    ));
                }
                Ok(xml_resp("DeleteClusterParameterGroup", &rid, ""))
            }

            // ----------------------------------------------------------------
            // DescribeClusterParameterGroups
            // ----------------------------------------------------------------
            "DescribeClusterParameterGroups" => {
                let Some(store) = self.store.get(account_id, region) else {
                    return Ok(xml_resp(
                        "DescribeClusterParameterGroups",
                        &rid,
                        "<ParameterGroups></ParameterGroups>",
                    ));
                };
                let items_xml: String = store
                    .parameter_groups
                    .values()
                    .map(|pg| format!("<member>{}</member>", param_group_xml(pg)))
                    .collect();
                let inner = format!("<ParameterGroups>{items_xml}</ParameterGroups>");
                Ok(xml_resp("DescribeClusterParameterGroups", &rid, &inner))
            }

            // ----------------------------------------------------------------
            // EnableLogging
            // ----------------------------------------------------------------
            "EnableLogging" => {
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
                        cluster.logging_enabled = true;
                        let inner = "<LoggingEnabled>true</LoggingEnabled>";
                        Ok(xml_resp("EnableLogging", &rid, inner))
                    }
                    None => Ok(xml_error(
                        "ClusterNotFound",
                        &format!("Cluster {cluster_id} not found."),
                        400,
                    )),
                }
            }

            // ----------------------------------------------------------------
            // DisableLogging
            // ----------------------------------------------------------------
            "DisableLogging" => {
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
                        cluster.logging_enabled = false;
                        let inner = "<LoggingEnabled>false</LoggingEnabled>";
                        Ok(xml_resp("DisableLogging", &rid, inner))
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
