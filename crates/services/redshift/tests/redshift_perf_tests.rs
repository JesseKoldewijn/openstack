/// Performance tests for Redshift provider.
///
/// These cover cluster creation, describe scans, and lightweight cluster
/// lifecycle transitions.
use std::collections::HashMap;
use std::time::Instant;

use openstack_redshift::RedshiftProvider;
use openstack_service_framework::traits::{DispatchResponse, RequestContext, ServiceProvider};

fn make_ctx(operation: &str, params: HashMap<String, String>) -> RequestContext {
    RequestContext {
        service: "redshift".to_string(),
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

async fn create_cluster(p: &RedshiftProvider, name: &str) {
    let mut params = HashMap::new();
    params.insert("ClusterIdentifier".to_string(), name.to_string());
    params.insert("NodeType".to_string(), "dc2.large".to_string());
    params.insert("MasterUsername".to_string(), "admin".to_string());
    params.insert("MasterUserPassword".to_string(), "Password123!".to_string());
    params.insert("DBName".to_string(), "benchdb".to_string());
    let resp = p
        .dispatch(&make_ctx("CreateCluster", params))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
}

async fn delete_cluster(p: &RedshiftProvider, name: &str) {
    let mut params = HashMap::new();
    params.insert("ClusterIdentifier".to_string(), name.to_string());
    let resp = p
        .dispatch(&make_ctx("DeleteCluster", params))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
}

#[tokio::test]
async fn perf_create_cluster_throughput() {
    let p = RedshiftProvider::new();
    let n = 100usize;

    let start = Instant::now();
    for i in 0..n {
        create_cluster(&p, &format!("perf-cluster-{i:03}")).await;
    }
    let elapsed = start.elapsed();

    assert!(
        elapsed.as_millis() < 2000,
        "CreateCluster x{n} took {}ms — expected <2000ms",
        elapsed.as_millis()
    );
}

#[tokio::test]
async fn perf_describe_clusters_many() {
    let p = RedshiftProvider::new();
    let n = 100usize;
    for i in 0..n {
        create_cluster(&p, &format!("perf-desc-{i:03}")).await;
    }

    let start = Instant::now();
    let resp = p
        .dispatch(&make_ctx("DescribeClusters", HashMap::new()))
        .await
        .unwrap();
    let elapsed = start.elapsed();
    assert_eq!(resp.status_code, 200);
    let xml = body_str(&resp);
    assert!(xml.contains("perf-desc-000"));
    assert!(xml.contains("perf-desc-099"));

    assert!(
        elapsed.as_millis() < 500,
        "DescribeClusters({n}) took {}ms — expected <500ms",
        elapsed.as_millis()
    );
}

#[tokio::test]
async fn perf_modify_and_reboot_round_trip() {
    let p = RedshiftProvider::new();
    create_cluster(&p, "perf-mod-reboot").await;

    let mut modify = HashMap::new();
    modify.insert(
        "ClusterIdentifier".to_string(),
        "perf-mod-reboot".to_string(),
    );
    modify.insert("NodeType".to_string(), "ra3.xlplus".to_string());
    modify.insert("DBName".to_string(), "analytics".to_string());
    modify.insert("Port".to_string(), "15439".to_string());

    let start = Instant::now();
    let modify_resp = p
        .dispatch(&make_ctx("ModifyCluster", modify))
        .await
        .unwrap();
    assert_eq!(modify_resp.status_code, 200);

    let mut reboot = HashMap::new();
    reboot.insert(
        "ClusterIdentifier".to_string(),
        "perf-mod-reboot".to_string(),
    );
    let reboot_resp = p
        .dispatch(&make_ctx("RebootCluster", reboot))
        .await
        .unwrap();
    assert_eq!(reboot_resp.status_code, 200);

    let elapsed = start.elapsed();
    assert!(
        elapsed.as_millis() < 500,
        "ModifyCluster + RebootCluster took {}ms — expected <500ms",
        elapsed.as_millis()
    );
}

#[tokio::test]
async fn perf_create_delete_cluster_round_trip() {
    let p = RedshiftProvider::new();
    let n = 100usize;

    let start = Instant::now();
    for i in 0..n {
        let name = format!("perf-delete-cluster-{i:03}");
        create_cluster(&p, &name).await;
        delete_cluster(&p, &name).await;
    }
    let elapsed = start.elapsed();

    assert!(
        elapsed.as_millis() < 2500,
        "CreateCluster/DeleteCluster x{n} took {}ms — expected <2500ms",
        elapsed.as_millis()
    );
}
