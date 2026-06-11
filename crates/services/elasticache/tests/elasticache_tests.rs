use std::collections::HashMap;

use openstack_elasticache::ElastiCacheProvider;
use openstack_service_framework::traits::{DispatchResponse, RequestContext, ServiceProvider};

fn make_ctx(operation: &str, params: HashMap<String, String>) -> RequestContext {
    RequestContext {
        service: "elasticache".to_string(),
        operation: operation.to_string(),
        region: "us-east-1".to_string(),
        account_id: "000000000000".to_string(),
        request_body: serde_json::json!({}),
        raw_body: None,
        headers: Default::default(),
        path: "/".to_string(),
        method: "POST".to_string(),
        query_params: params,
        request_id: String::new(),
        spooled_body: None,
        body_reader: None,
    }
}

fn body_str(resp: &DispatchResponse) -> String {
    String::from_utf8_lossy(resp.body.as_bytes()).to_string()
}

fn p(k: &str, v: &str) -> (String, String) {
    (k.to_string(), v.to_string())
}

// ---------------------------------------------------------------------------
// Cache Clusters
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_create_cache_cluster_redis() {
    let svc = ElastiCacheProvider::new();
    let resp = svc
        .dispatch(&make_ctx(
            "CreateCacheCluster",
            [
                p("CacheClusterId", "my-redis"),
                p("Engine", "redis"),
                p("CacheNodeType", "cache.t3.micro"),
                p("NumCacheNodes", "1"),
            ]
            .into(),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
    assert_eq!(resp.content_type, "text/xml");
    let body = body_str(&resp);
    assert!(body.contains("CreateCacheClusterResponse"));
    assert!(body.contains("<CacheClusterId>my-redis</CacheClusterId>"));
    assert!(body.contains("<Engine>redis</Engine>"));
    assert!(body.contains("<CacheClusterStatus>available</CacheClusterStatus>"));
    assert!(body.contains("<Port>6379</Port>"));
}

#[tokio::test]
async fn test_create_cache_cluster_memcached() {
    let svc = ElastiCacheProvider::new();
    let resp = svc
        .dispatch(&make_ctx(
            "CreateCacheCluster",
            [
                p("CacheClusterId", "my-memcached"),
                p("Engine", "memcached"),
                p("NumCacheNodes", "3"),
            ]
            .into(),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
    let body = body_str(&resp);
    assert!(body.contains("<Engine>memcached</Engine>"));
    assert!(body.contains("<NumCacheNodes>3</NumCacheNodes>"));
    assert!(body.contains("<Port>11211</Port>"));
}

#[tokio::test]
async fn test_create_cache_cluster_duplicate_fails() {
    let svc = ElastiCacheProvider::new();
    let params: HashMap<String, String> =
        [p("CacheClusterId", "dup-cluster"), p("Engine", "redis")].into();
    svc.dispatch(&make_ctx("CreateCacheCluster", params.clone()))
        .await
        .unwrap();
    let resp = svc
        .dispatch(&make_ctx("CreateCacheCluster", params))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 400);
    assert!(body_str(&resp).contains("CacheClusterAlreadyExists"));
}

#[tokio::test]
async fn test_describe_cache_clusters() {
    let svc = ElastiCacheProvider::new();
    for id in ["c1", "c2"] {
        svc.dispatch(&make_ctx(
            "CreateCacheCluster",
            [p("CacheClusterId", id), p("Engine", "redis")].into(),
        ))
        .await
        .unwrap();
    }
    let resp = svc
        .dispatch(&make_ctx("DescribeCacheClusters", HashMap::new()))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
    let body = body_str(&resp);
    assert!(body.contains("c1"));
    assert!(body.contains("c2"));
}

#[tokio::test]
async fn test_describe_cache_clusters_filter() {
    let svc = ElastiCacheProvider::new();
    for id in ["filter-a", "filter-b"] {
        svc.dispatch(&make_ctx(
            "CreateCacheCluster",
            [p("CacheClusterId", id), p("Engine", "redis")].into(),
        ))
        .await
        .unwrap();
    }
    let resp = svc
        .dispatch(&make_ctx(
            "DescribeCacheClusters",
            [p("CacheClusterId", "filter-a")].into(),
        ))
        .await
        .unwrap();
    let body = body_str(&resp);
    assert!(body.contains("filter-a"));
    assert!(!body.contains("filter-b"));
}

#[tokio::test]
async fn test_delete_cache_cluster() {
    let svc = ElastiCacheProvider::new();
    svc.dispatch(&make_ctx(
        "CreateCacheCluster",
        [p("CacheClusterId", "del-cluster"), p("Engine", "redis")].into(),
    ))
    .await
    .unwrap();

    let resp = svc
        .dispatch(&make_ctx(
            "DeleteCacheCluster",
            [p("CacheClusterId", "del-cluster")].into(),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
    assert!(body_str(&resp).contains("DeleteCacheClusterResponse"));

    let desc = svc
        .dispatch(&make_ctx("DescribeCacheClusters", HashMap::new()))
        .await
        .unwrap();
    assert!(!body_str(&desc).contains("del-cluster"));
}

#[tokio::test]
async fn test_delete_cache_cluster_not_found() {
    let svc = ElastiCacheProvider::new();
    let resp = svc
        .dispatch(&make_ctx(
            "DeleteCacheCluster",
            [p("CacheClusterId", "ghost")].into(),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 400);
    assert!(body_str(&resp).contains("CacheClusterNotFound"));
}

#[tokio::test]
async fn test_modify_cache_cluster() {
    let svc = ElastiCacheProvider::new();
    svc.dispatch(&make_ctx(
        "CreateCacheCluster",
        [p("CacheClusterId", "mod-cluster"), p("Engine", "redis")].into(),
    ))
    .await
    .unwrap();

    let resp = svc
        .dispatch(&make_ctx(
            "ModifyCacheCluster",
            [
                p("CacheClusterId", "mod-cluster"),
                p("CacheNodeType", "cache.r6g.large"),
            ]
            .into(),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
    assert!(body_str(&resp).contains("cache.r6g.large"));
}

#[tokio::test]
async fn test_reboot_cache_cluster() {
    let svc = ElastiCacheProvider::new();
    svc.dispatch(&make_ctx(
        "CreateCacheCluster",
        [p("CacheClusterId", "reboot-cluster"), p("Engine", "redis")].into(),
    ))
    .await
    .unwrap();

    let resp = svc
        .dispatch(&make_ctx(
            "RebootCacheCluster",
            [p("CacheClusterId", "reboot-cluster")].into(),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
    assert!(body_str(&resp).contains("available"));
}

// ---------------------------------------------------------------------------
// Replication Groups
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_create_and_describe_replication_group() {
    let svc = ElastiCacheProvider::new();
    let resp = svc
        .dispatch(&make_ctx(
            "CreateReplicationGroup",
            [
                p("ReplicationGroupId", "my-rg"),
                p("ReplicationGroupDescription", "Test RG"),
                p("NumCacheClusters", "2"),
                p("AutomaticFailoverEnabled", "true"),
                p("CacheNodeType", "cache.t3.micro"),
            ]
            .into(),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
    let body = body_str(&resp);
    assert!(body.contains("CreateReplicationGroupResponse"));
    assert!(body.contains("<ReplicationGroupId>my-rg</ReplicationGroupId>"));
    assert!(body.contains("<AutomaticFailover>enabled</AutomaticFailover>"));
    assert!(body.contains("<NumCacheClusters>2</NumCacheClusters>"));

    let desc = svc
        .dispatch(&make_ctx("DescribeReplicationGroups", HashMap::new()))
        .await
        .unwrap();
    assert_eq!(desc.status_code, 200);
    assert!(body_str(&desc).contains("my-rg"));
}

#[tokio::test]
async fn test_create_replication_group_creates_member_clusters() {
    let svc = ElastiCacheProvider::new();
    svc.dispatch(&make_ctx(
        "CreateReplicationGroup",
        [
            p("ReplicationGroupId", "rg-with-members"),
            p("NumCacheClusters", "3"),
        ]
        .into(),
    ))
    .await
    .unwrap();

    let desc = svc
        .dispatch(&make_ctx("DescribeCacheClusters", HashMap::new()))
        .await
        .unwrap();
    let body = body_str(&desc);
    assert!(body.contains("rg-with-members-0001"));
    assert!(body.contains("rg-with-members-0002"));
    assert!(body.contains("rg-with-members-0003"));
}

#[tokio::test]
async fn test_delete_replication_group_removes_members() {
    let svc = ElastiCacheProvider::new();
    svc.dispatch(&make_ctx(
        "CreateReplicationGroup",
        [
            p("ReplicationGroupId", "del-rg"),
            p("NumCacheClusters", "2"),
        ]
        .into(),
    ))
    .await
    .unwrap();

    let del_resp = svc
        .dispatch(&make_ctx(
            "DeleteReplicationGroup",
            [p("ReplicationGroupId", "del-rg")].into(),
        ))
        .await
        .unwrap();
    assert_eq!(del_resp.status_code, 200);

    let desc = svc
        .dispatch(&make_ctx("DescribeCacheClusters", HashMap::new()))
        .await
        .unwrap();
    assert!(!body_str(&desc).contains("del-rg"));
}

#[tokio::test]
async fn test_delete_replication_group_not_found() {
    let svc = ElastiCacheProvider::new();
    let resp = svc
        .dispatch(&make_ctx(
            "DeleteReplicationGroup",
            [p("ReplicationGroupId", "ghost-rg")].into(),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 400);
    assert!(body_str(&resp).contains("ReplicationGroupNotFoundFault"));
}

#[tokio::test]
async fn test_modify_replication_group() {
    let svc = ElastiCacheProvider::new();
    svc.dispatch(&make_ctx(
        "CreateReplicationGroup",
        [
            p("ReplicationGroupId", "mod-rg"),
            p("ReplicationGroupDescription", "original"),
        ]
        .into(),
    ))
    .await
    .unwrap();

    let resp = svc
        .dispatch(&make_ctx(
            "ModifyReplicationGroup",
            [
                p("ReplicationGroupId", "mod-rg"),
                p("ReplicationGroupDescription", "updated"),
                p("AutomaticFailoverEnabled", "true"),
            ]
            .into(),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
    let body = body_str(&resp);
    assert!(body.contains("<Description>updated</Description>"));
    assert!(body.contains("<AutomaticFailover>enabled</AutomaticFailover>"));
}

// ---------------------------------------------------------------------------
// Subnet Groups
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_create_and_describe_subnet_group() {
    let svc = ElastiCacheProvider::new();
    let resp = svc
        .dispatch(&make_ctx(
            "CreateCacheSubnetGroup",
            [
                p("CacheSubnetGroupName", "my-sg"),
                p("CacheSubnetGroupDescription", "test"),
                p("VpcId", "vpc-12345"),
                p("SubnetIds.SubnetIdentifier.1", "subnet-aaa"),
                p("SubnetIds.SubnetIdentifier.2", "subnet-bbb"),
            ]
            .into(),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
    let body = body_str(&resp);
    assert!(body.contains("<CacheSubnetGroupName>my-sg</CacheSubnetGroupName>"));

    let desc = svc
        .dispatch(&make_ctx("DescribeCacheSubnetGroups", HashMap::new()))
        .await
        .unwrap();
    let desc_body = body_str(&desc);
    assert!(desc_body.contains("my-sg"));
    assert!(desc_body.contains("subnet-aaa"));
    assert!(desc_body.contains("subnet-bbb"));
}

#[tokio::test]
async fn test_delete_cache_subnet_group() {
    let svc = ElastiCacheProvider::new();
    svc.dispatch(&make_ctx(
        "CreateCacheSubnetGroup",
        [p("CacheSubnetGroupName", "del-sg")].into(),
    ))
    .await
    .unwrap();

    let resp = svc
        .dispatch(&make_ctx(
            "DeleteCacheSubnetGroup",
            [p("CacheSubnetGroupName", "del-sg")].into(),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);

    let desc = svc
        .dispatch(&make_ctx("DescribeCacheSubnetGroups", HashMap::new()))
        .await
        .unwrap();
    assert!(!body_str(&desc).contains("del-sg"));
}
