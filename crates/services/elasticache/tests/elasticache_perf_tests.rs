/// Performance tests for ElastiCache provider.
use std::collections::HashMap;
use std::time::Instant;

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

#[tokio::test]
async fn perf_create_cache_cluster_throughput() {
    let svc = ElastiCacheProvider::new();
    let n = 100usize;
    let start = Instant::now();
    for i in 0..n {
        let mut params = HashMap::new();
        params.insert("CacheClusterId".to_string(), format!("perf-cluster-{i:03}"));
        params.insert("Engine".to_string(), "redis".to_string());
        let resp = svc
            .dispatch(&make_ctx("CreateCacheCluster", params))
            .await
            .unwrap();
        assert_eq!(resp.status_code, 200, "{}", body_str(&resp));
    }
    let elapsed = start.elapsed();
    assert!(
        elapsed.as_millis() < 2000,
        "CreateCacheCluster x{n} took {}ms — expected <2000ms",
        elapsed.as_millis()
    );
}

#[tokio::test]
async fn perf_describe_cache_clusters_many() {
    let svc = ElastiCacheProvider::new();
    let n = 100usize;
    for i in 0..n {
        let mut params = HashMap::new();
        params.insert("CacheClusterId".to_string(), format!("list-cluster-{i:03}"));
        params.insert("Engine".to_string(), "redis".to_string());
        svc.dispatch(&make_ctx("CreateCacheCluster", params))
            .await
            .unwrap();
    }
    let start = Instant::now();
    let resp = svc
        .dispatch(&make_ctx("DescribeCacheClusters", HashMap::new()))
        .await
        .unwrap();
    let elapsed = start.elapsed();
    assert_eq!(resp.status_code, 200);
    assert!(body_str(&resp).contains("list-cluster-000"));
    assert!(
        elapsed.as_millis() < 500,
        "DescribeCacheClusters({n}) took {}ms — expected <500ms",
        elapsed.as_millis()
    );
}
