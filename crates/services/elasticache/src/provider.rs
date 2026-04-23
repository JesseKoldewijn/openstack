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
    CacheCluster, CacheClusterEndpoint, CacheSubnetGroup, ElastiCacheStore, NodeGroup,
    ReplicationGroup,
};

pub struct ElastiCacheProvider {
    store: Arc<AccountRegionBundle<ElastiCacheStore>>,
}

impl ElastiCacheProvider {
    pub fn new() -> Self {
        Self {
            store: Arc::new(AccountRegionBundle::new()),
        }
    }
}

impl Default for ElastiCacheProvider {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Helpers — ElastiCache uses query protocol (XML)
// ---------------------------------------------------------------------------

const EC_NS: &str = "http://elasticache.amazonaws.com/doc/2015-02-02/";

fn xml_resp(action: &str, rid: &str, inner: &str) -> DispatchResponse {
    let xml = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
<{action}Response xmlns=\"{EC_NS}\">\
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
<ErrorResponse xmlns=\"{EC_NS}\">\
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

fn cluster_endpoint_xml(id: &str, region: &str, engine: &str) -> String {
    let port: u16 = if engine == "memcached" { 11211 } else { 6379 };
    format!(
        "<ConfigurationEndpoint>\
<Address>{id}.{region}.cache.amazonaws.com</Address>\
<Port>{port}</Port>\
</ConfigurationEndpoint>"
    )
}

fn cluster_xml(c: &CacheCluster, region: &str) -> String {
    let endpoint = cluster_endpoint_xml(&c.cache_cluster_id, region, &c.engine);
    let subnet_xml = c
        .cache_subnet_group_name
        .as_deref()
        .map(|n| {
            format!(
                "<CacheSubnetGroupName>{}</CacheSubnetGroupName>",
                xml_escape(n)
            )
        })
        .unwrap_or_default();
    let rg_xml = c
        .replication_group_id
        .as_deref()
        .map(|id| {
            format!(
                "<ReplicationGroupId>{}</ReplicationGroupId>",
                xml_escape(id)
            )
        })
        .unwrap_or_default();
    format!(
        "<CacheClusterId>{}</CacheClusterId>\
<CacheNodeType>{}</CacheNodeType>\
<Engine>{}</Engine>\
<EngineVersion>{}</EngineVersion>\
<CacheClusterStatus>{}</CacheClusterStatus>\
<NumCacheNodes>{}</NumCacheNodes>\
{subnet_xml}\
{rg_xml}\
{endpoint}",
        xml_escape(&c.cache_cluster_id),
        xml_escape(&c.cache_node_type),
        xml_escape(&c.engine),
        xml_escape(&c.engine_version),
        xml_escape(&c.cache_cluster_status),
        c.num_cache_nodes,
    )
}

fn rg_xml(rg: &ReplicationGroup, region: &str) -> String {
    let members: String = rg
        .member_clusters
        .iter()
        .map(|id| format!("<member>{}</member>", xml_escape(id)))
        .collect();
    let node_groups: String = rg
        .node_groups
        .iter()
        .map(|ng| {
            let primary = ng
                .primary_endpoint
                .as_ref()
                .map(|e| {
                    format!(
                        "<PrimaryEndpoint><Address>{}</Address><Port>{}</Port></PrimaryEndpoint>",
                        xml_escape(&e.address),
                        e.port
                    )
                })
                .unwrap_or_default();
            format!(
                "<NodeGroup>\
<NodeGroupId>{}</NodeGroupId>\
<Status>{}</Status>\
{primary}\
</NodeGroup>",
                xml_escape(&ng.node_group_id),
                xml_escape(&ng.status)
            )
        })
        .collect();
    let _ = region; // used implicitly via cluster members
    format!(
        "<ReplicationGroupId>{}</ReplicationGroupId>\
<Description>{}</Description>\
<Status>{}</Status>\
<AutomaticFailover>{}</AutomaticFailover>\
<MultiAZ>{}</MultiAZ>\
<NumCacheClusters>{}</NumCacheClusters>\
<MemberClusters>{members}</MemberClusters>\
<NodeGroups>{node_groups}</NodeGroups>",
        xml_escape(&rg.replication_group_id),
        xml_escape(&rg.description),
        xml_escape(&rg.status),
        xml_escape(&rg.automatic_failover),
        xml_escape(&rg.multi_az),
        rg.num_cache_clusters,
    )
}

fn subnet_group_xml(sg: &CacheSubnetGroup) -> String {
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
        "<CacheSubnetGroupName>{}</CacheSubnetGroupName>\
<CacheSubnetGroupDescription>{}</CacheSubnetGroupDescription>\
<VpcId>{}</VpcId>\
<Subnets>{subnets}</Subnets>",
        xml_escape(&sg.cache_subnet_group_name),
        xml_escape(&sg.cache_subnet_group_description),
        xml_escape(&sg.vpc_id),
    )
}

fn engine_default_version(engine: &str) -> &'static str {
    match engine {
        "memcached" => "1.6.12",
        _ => "7.0.7",
    }
}

// ---------------------------------------------------------------------------
// ServiceProvider
// ---------------------------------------------------------------------------

#[async_trait]
impl ServiceProvider for ElastiCacheProvider {
    fn service_name(&self) -> &str {
        "elasticache"
    }

    async fn dispatch(&self, ctx: &RequestContext) -> Result<DispatchResponse, DispatchError> {
        let region = &ctx.region;
        let account_id = &ctx.account_id;
        let rid = req_id();

        match ctx.operation.as_str() {
            // ----------------------------------------------------------------
            // CreateCacheCluster
            // ----------------------------------------------------------------
            "CreateCacheCluster" => {
                let cluster_id = match str_param(ctx, "CacheClusterId") {
                    Some(id) => id.to_string(),
                    None => {
                        return Ok(xml_error(
                            "MissingParameter",
                            "CacheClusterId required",
                            400,
                        ));
                    }
                };
                let engine = str_param(ctx, "Engine").unwrap_or("redis").to_string();
                let engine_version = str_param(ctx, "EngineVersion")
                    .unwrap_or_else(|| engine_default_version(&engine))
                    .to_string();
                let node_type = str_param(ctx, "CacheNodeType")
                    .unwrap_or("cache.t3.micro")
                    .to_string();
                let num_nodes: u32 = str_param(ctx, "NumCacheNodes")
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(1);
                let subnet_group = str_param(ctx, "CacheSubnetGroupName").map(|s| s.to_string());
                let rg_id = str_param(ctx, "ReplicationGroupId").map(|s| s.to_string());

                let mut store = self.store.get_or_create(account_id, region);
                if store.clusters.contains_key(&cluster_id) {
                    return Ok(xml_error(
                        "CacheClusterAlreadyExists",
                        &format!("Cache cluster {cluster_id} already exists"),
                        400,
                    ));
                }
                let cluster = CacheCluster {
                    cache_cluster_id: cluster_id.clone(),
                    cache_node_type: node_type,
                    engine: engine.clone(),
                    engine_version,
                    cache_cluster_status: "available".to_string(),
                    num_cache_nodes: num_nodes,
                    cache_subnet_group_name: subnet_group,
                    configuration_endpoint: Some(CacheClusterEndpoint {
                        address: format!("{cluster_id}.{region}.cache.amazonaws.com"),
                        port: if engine == "memcached" { 11211 } else { 6379 },
                    }),
                    replication_group_id: rg_id,
                    created: Utc::now(),
                };
                store.clusters.insert(cluster_id, cluster.clone());
                let inner = format!(
                    "<CacheCluster>{}</CacheCluster>",
                    cluster_xml(&cluster, region)
                );
                Ok(xml_resp("CreateCacheCluster", &rid, &inner))
            }

            // ----------------------------------------------------------------
            // DeleteCacheCluster
            // ----------------------------------------------------------------
            "DeleteCacheCluster" => {
                let cluster_id = match str_param(ctx, "CacheClusterId") {
                    Some(id) => id.to_string(),
                    None => {
                        return Ok(xml_error(
                            "MissingParameter",
                            "CacheClusterId required",
                            400,
                        ));
                    }
                };
                let mut store = self.store.get_or_create(account_id, region);
                match store.clusters.remove(&cluster_id) {
                    Some(c) => {
                        let inner =
                            format!("<CacheCluster>{}</CacheCluster>", cluster_xml(&c, region));
                        Ok(xml_resp("DeleteCacheCluster", &rid, &inner))
                    }
                    None => Ok(xml_error(
                        "CacheClusterNotFound",
                        &format!("Cache cluster {cluster_id} not found"),
                        400,
                    )),
                }
            }

            // ----------------------------------------------------------------
            // DescribeCacheClusters
            // ----------------------------------------------------------------
            "DescribeCacheClusters" => {
                let Some(store) = self.store.get(account_id, region) else {
                    return Ok(xml_resp(
                        "DescribeCacheClusters",
                        &rid,
                        "<CacheClusters></CacheClusters>",
                    ));
                };
                let filter_id = str_param(ctx, "CacheClusterId");
                let items: String = store
                    .clusters
                    .values()
                    .filter(|c| filter_id.map(|id| c.cache_cluster_id == id).unwrap_or(true))
                    .map(|c| format!("<member>{}</member>", cluster_xml(c, region)))
                    .collect();
                let inner = format!("<CacheClusters>{items}</CacheClusters>");
                Ok(xml_resp("DescribeCacheClusters", &rid, &inner))
            }

            // ----------------------------------------------------------------
            // ModifyCacheCluster
            // ----------------------------------------------------------------
            "ModifyCacheCluster" => {
                let cluster_id = match str_param(ctx, "CacheClusterId") {
                    Some(id) => id.to_string(),
                    None => {
                        return Ok(xml_error(
                            "MissingParameter",
                            "CacheClusterId required",
                            400,
                        ));
                    }
                };
                let mut store = self.store.get_or_create(account_id, region);
                match store.clusters.get_mut(&cluster_id) {
                    Some(cluster) => {
                        if let Some(node_type) = str_param(ctx, "CacheNodeType") {
                            cluster.cache_node_type = node_type.to_string();
                        }
                        if let Some(engine_version) = str_param(ctx, "EngineVersion") {
                            cluster.engine_version = engine_version.to_string();
                        }
                        let inner = format!(
                            "<CacheCluster>{}</CacheCluster>",
                            cluster_xml(cluster, region)
                        );
                        Ok(xml_resp("ModifyCacheCluster", &rid, &inner))
                    }
                    None => Ok(xml_error(
                        "CacheClusterNotFound",
                        &format!("Cache cluster {cluster_id} not found"),
                        400,
                    )),
                }
            }

            // ----------------------------------------------------------------
            // RebootCacheCluster
            // ----------------------------------------------------------------
            "RebootCacheCluster" => {
                let cluster_id = match str_param(ctx, "CacheClusterId") {
                    Some(id) => id.to_string(),
                    None => {
                        return Ok(xml_error(
                            "MissingParameter",
                            "CacheClusterId required",
                            400,
                        ));
                    }
                };
                let mut store = self.store.get_or_create(account_id, region);
                match store.clusters.get_mut(&cluster_id) {
                    Some(cluster) => {
                        cluster.cache_cluster_status = "available".to_string();
                        let inner = format!(
                            "<CacheCluster>{}</CacheCluster>",
                            cluster_xml(cluster, region)
                        );
                        Ok(xml_resp("RebootCacheCluster", &rid, &inner))
                    }
                    None => Ok(xml_error(
                        "CacheClusterNotFound",
                        &format!("Cache cluster {cluster_id} not found"),
                        400,
                    )),
                }
            }

            // ----------------------------------------------------------------
            // CreateReplicationGroup
            // ----------------------------------------------------------------
            "CreateReplicationGroup" => {
                let rg_id = match str_param(ctx, "ReplicationGroupId") {
                    Some(id) => id.to_string(),
                    None => {
                        return Ok(xml_error(
                            "MissingParameter",
                            "ReplicationGroupId required",
                            400,
                        ));
                    }
                };
                let description = str_param(ctx, "ReplicationGroupDescription")
                    .unwrap_or("")
                    .to_string();
                let num_clusters: u32 = str_param(ctx, "NumCacheClusters")
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(1);
                let auto_failover = str_param(ctx, "AutomaticFailoverEnabled")
                    .map(|s| {
                        if s.eq_ignore_ascii_case("true") {
                            "enabled"
                        } else {
                            "disabled"
                        }
                    })
                    .unwrap_or("disabled")
                    .to_string();
                let multi_az = str_param(ctx, "MultiAZEnabled")
                    .map(|s| {
                        if s.eq_ignore_ascii_case("true") {
                            "enabled"
                        } else {
                            "disabled"
                        }
                    })
                    .unwrap_or("disabled")
                    .to_string();
                let node_type = str_param(ctx, "CacheNodeType")
                    .unwrap_or("cache.t3.micro")
                    .to_string();
                let engine_version = str_param(ctx, "EngineVersion")
                    .unwrap_or("7.0.7")
                    .to_string();
                let snapshot_retention: u32 = str_param(ctx, "SnapshotRetentionLimit")
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(0);

                let mut store = self.store.get_or_create(account_id, region);
                if store.replication_groups.contains_key(&rg_id) {
                    return Ok(xml_error(
                        "ReplicationGroupAlreadyExists",
                        &format!("Replication group {rg_id} already exists"),
                        400,
                    ));
                }

                // Create member clusters
                let mut member_clusters = Vec::new();
                for i in 0..num_clusters {
                    let cluster_id = format!("{rg_id}-{:04}", i + 1);
                    let cluster = CacheCluster {
                        cache_cluster_id: cluster_id.clone(),
                        cache_node_type: node_type.clone(),
                        engine: "redis".to_string(),
                        engine_version: engine_version.clone(),
                        cache_cluster_status: "available".to_string(),
                        num_cache_nodes: 1,
                        cache_subnet_group_name: None,
                        configuration_endpoint: None,
                        replication_group_id: Some(rg_id.clone()),
                        created: Utc::now(),
                    };
                    store.clusters.insert(cluster_id.clone(), cluster);
                    member_clusters.push(cluster_id);
                }

                let primary_endpoint = CacheClusterEndpoint {
                    address: format!("{rg_id}.{region}.cache.amazonaws.com"),
                    port: 6379,
                };
                let node_group = NodeGroup {
                    node_group_id: "0001".to_string(),
                    status: "available".to_string(),
                    primary_endpoint: Some(primary_endpoint),
                    reader_endpoint: None,
                };

                let rg = ReplicationGroup {
                    replication_group_id: rg_id.clone(),
                    description,
                    status: "available".to_string(),
                    automatic_failover: auto_failover,
                    multi_az,
                    num_cache_clusters: num_clusters,
                    member_clusters: member_clusters.clone(),
                    node_groups: vec![node_group],
                    snapshot_retention_limit: snapshot_retention,
                    created: Utc::now(),
                };
                store.replication_groups.insert(rg_id, rg.clone());
                let inner = format!(
                    "<ReplicationGroup>{}</ReplicationGroup>",
                    rg_xml(&rg, region)
                );
                Ok(xml_resp("CreateReplicationGroup", &rid, &inner))
            }

            // ----------------------------------------------------------------
            // DeleteReplicationGroup
            // ----------------------------------------------------------------
            "DeleteReplicationGroup" => {
                let rg_id = match str_param(ctx, "ReplicationGroupId") {
                    Some(id) => id.to_string(),
                    None => {
                        return Ok(xml_error(
                            "MissingParameter",
                            "ReplicationGroupId required",
                            400,
                        ));
                    }
                };
                let mut store = self.store.get_or_create(account_id, region);
                match store.replication_groups.remove(&rg_id) {
                    Some(rg) => {
                        // Clean up member clusters
                        for member_id in &rg.member_clusters {
                            store.clusters.remove(member_id);
                        }
                        let inner = format!(
                            "<ReplicationGroup>{}</ReplicationGroup>",
                            rg_xml(&rg, region)
                        );
                        Ok(xml_resp("DeleteReplicationGroup", &rid, &inner))
                    }
                    None => Ok(xml_error(
                        "ReplicationGroupNotFoundFault",
                        &format!("Replication group {rg_id} not found"),
                        400,
                    )),
                }
            }

            // ----------------------------------------------------------------
            // DescribeReplicationGroups
            // ----------------------------------------------------------------
            "DescribeReplicationGroups" => {
                let Some(store) = self.store.get(account_id, region) else {
                    return Ok(xml_resp(
                        "DescribeReplicationGroups",
                        &rid,
                        "<ReplicationGroups></ReplicationGroups>",
                    ));
                };
                let filter_id = str_param(ctx, "ReplicationGroupId");
                let items: String = store
                    .replication_groups
                    .values()
                    .filter(|rg| {
                        filter_id
                            .map(|id| rg.replication_group_id == id)
                            .unwrap_or(true)
                    })
                    .map(|rg| format!("<member>{}</member>", rg_xml(rg, region)))
                    .collect();
                let inner = format!("<ReplicationGroups>{items}</ReplicationGroups>");
                Ok(xml_resp("DescribeReplicationGroups", &rid, &inner))
            }

            // ----------------------------------------------------------------
            // ModifyReplicationGroup
            // ----------------------------------------------------------------
            "ModifyReplicationGroup" => {
                let rg_id = match str_param(ctx, "ReplicationGroupId") {
                    Some(id) => id.to_string(),
                    None => {
                        return Ok(xml_error(
                            "MissingParameter",
                            "ReplicationGroupId required",
                            400,
                        ));
                    }
                };
                let mut store = self.store.get_or_create(account_id, region);
                match store.replication_groups.get_mut(&rg_id) {
                    Some(rg) => {
                        if let Some(desc) = str_param(ctx, "ReplicationGroupDescription") {
                            rg.description = desc.to_string();
                        }
                        if let Some(af) = str_param(ctx, "AutomaticFailoverEnabled") {
                            rg.automatic_failover = if af.eq_ignore_ascii_case("true") {
                                "enabled".to_string()
                            } else {
                                "disabled".to_string()
                            };
                        }
                        let inner = format!(
                            "<ReplicationGroup>{}</ReplicationGroup>",
                            rg_xml(rg, region)
                        );
                        Ok(xml_resp("ModifyReplicationGroup", &rid, &inner))
                    }
                    None => Ok(xml_error(
                        "ReplicationGroupNotFoundFault",
                        &format!("Replication group {rg_id} not found"),
                        400,
                    )),
                }
            }

            // ----------------------------------------------------------------
            // CreateCacheSubnetGroup
            // ----------------------------------------------------------------
            "CreateCacheSubnetGroup" => {
                let name = match str_param(ctx, "CacheSubnetGroupName") {
                    Some(n) => n.to_string(),
                    None => {
                        return Ok(xml_error(
                            "MissingParameter",
                            "CacheSubnetGroupName required",
                            400,
                        ));
                    }
                };
                let description = str_param(ctx, "CacheSubnetGroupDescription")
                    .unwrap_or("")
                    .to_string();
                let vpc_id = str_param(ctx, "VpcId").unwrap_or("").to_string();

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
                        "CacheSubnetGroupAlreadyExists",
                        &format!("Cache subnet group {name} already exists"),
                        400,
                    ));
                }
                let sg = CacheSubnetGroup {
                    cache_subnet_group_name: name.clone(),
                    cache_subnet_group_description: description,
                    vpc_id,
                    subnet_ids,
                };
                store.subnet_groups.insert(name, sg.clone());
                let inner = format!(
                    "<CacheSubnetGroup>{}</CacheSubnetGroup>",
                    subnet_group_xml(&sg)
                );
                Ok(xml_resp("CreateCacheSubnetGroup", &rid, &inner))
            }

            // ----------------------------------------------------------------
            // DeleteCacheSubnetGroup
            // ----------------------------------------------------------------
            "DeleteCacheSubnetGroup" => {
                let name = match str_param(ctx, "CacheSubnetGroupName") {
                    Some(n) => n.to_string(),
                    None => {
                        return Ok(xml_error(
                            "MissingParameter",
                            "CacheSubnetGroupName required",
                            400,
                        ));
                    }
                };
                let mut store = self.store.get_or_create(account_id, region);
                if store.subnet_groups.remove(&name).is_none() {
                    return Ok(xml_error(
                        "CacheSubnetGroupNotFoundFault",
                        &format!("Cache subnet group {name} not found"),
                        400,
                    ));
                }
                Ok(xml_resp("DeleteCacheSubnetGroup", &rid, ""))
            }

            // ----------------------------------------------------------------
            // DescribeCacheSubnetGroups
            // ----------------------------------------------------------------
            "DescribeCacheSubnetGroups" => {
                let Some(store) = self.store.get(account_id, region) else {
                    return Ok(xml_resp(
                        "DescribeCacheSubnetGroups",
                        &rid,
                        "<CacheSubnetGroups></CacheSubnetGroups>",
                    ));
                };
                let items: String = store
                    .subnet_groups
                    .values()
                    .map(|sg| format!("<member>{}</member>", subnet_group_xml(sg)))
                    .collect();
                let inner = format!("<CacheSubnetGroups>{items}</CacheSubnetGroups>");
                Ok(xml_resp("DescribeCacheSubnetGroups", &rid, &inner))
            }

            _ => Err(DispatchError::NotImplemented(ctx.operation.clone())),
        }
    }

    async fn storage_snapshot(&self) -> Option<serde_json::Value> {
        use serde_json::json;
        let mut clusters = Vec::new();
        for entry in self.store.iter() {
            for c in entry.value().clusters.values() {
                clusters.push(json!({
                    "id": c.cache_cluster_id, "kind": "cache_cluster",
                    "attributes": [
                        {"key": "engine", "value": c.engine.clone()},
                        {"key": "status", "value": c.cache_cluster_status.clone()},
                        {"key": "node_type", "value": c.cache_node_type.clone()},
                    ]
                }));
            }
        }
        Some(json!({ "kind": "elasticache", "clusters": clusters }))
    }
}
