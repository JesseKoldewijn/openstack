use std::collections::HashMap;

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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_create_cluster() {
    let p = RedshiftProvider::new();
    let mut params = HashMap::new();
    params.insert("ClusterIdentifier".to_string(), "my-cluster".to_string());
    params.insert("NodeType".to_string(), "dc2.large".to_string());
    params.insert("MasterUsername".to_string(), "admin".to_string());
    params.insert("MasterUserPassword".to_string(), "Password123!".to_string());
    params.insert("DBName".to_string(), "mydb".to_string());
    let resp = p
        .dispatch(&make_ctx("CreateCluster", params))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
    assert_eq!(resp.content_type, "text/xml");
    let body = body_str(&resp);
    assert!(body.contains("CreateClusterResponse"));
    assert!(body.contains("<ClusterIdentifier>my-cluster</ClusterIdentifier>"));
    assert!(body.contains("dc2.large"));
    assert!(body.contains("available"));
    assert!(body.contains("<Endpoint>"));
}

#[tokio::test]
async fn test_create_cluster_duplicate_fails() {
    let p = RedshiftProvider::new();
    let mut params = HashMap::new();
    params.insert("ClusterIdentifier".to_string(), "dup-cluster".to_string());
    p.dispatch(&make_ctx("CreateCluster", params.clone()))
        .await
        .unwrap();
    let resp = p
        .dispatch(&make_ctx("CreateCluster", params))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 400);
    let body = body_str(&resp);
    assert!(body.contains("ClusterAlreadyExists"));
}

#[tokio::test]
async fn test_describe_clusters() {
    let p = RedshiftProvider::new();
    // Create two clusters
    let mut p1 = HashMap::new();
    p1.insert("ClusterIdentifier".to_string(), "cluster-alpha".to_string());
    p.dispatch(&make_ctx("CreateCluster", p1)).await.unwrap();

    let mut p2 = HashMap::new();
    p2.insert("ClusterIdentifier".to_string(), "cluster-beta".to_string());
    p.dispatch(&make_ctx("CreateCluster", p2)).await.unwrap();

    let resp = p
        .dispatch(&make_ctx("DescribeClusters", HashMap::new()))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
    let body = body_str(&resp);
    assert!(body.contains("DescribeClustersResponse"));
    assert!(body.contains("<Clusters>"));
    assert!(body.contains("cluster-alpha"));
    assert!(body.contains("cluster-beta"));
}

#[tokio::test]
async fn test_describe_clusters_without_store_returns_empty_clusters() {
    let p = RedshiftProvider::new();

    let resp = p
        .dispatch(&make_ctx("DescribeClusters", HashMap::new()))
        .await
        .unwrap();

    assert_eq!(resp.status_code, 200);
    let body = body_str(&resp);
    assert!(body.contains("DescribeClustersResponse"));
    assert!(body.contains("<Clusters></Clusters>"));
}

#[tokio::test]
async fn test_delete_cluster() {
    let p = RedshiftProvider::new();
    let mut params = HashMap::new();
    params.insert("ClusterIdentifier".to_string(), "del-cluster".to_string());
    p.dispatch(&make_ctx("CreateCluster", params))
        .await
        .unwrap();

    let mut del_params = HashMap::new();
    del_params.insert("ClusterIdentifier".to_string(), "del-cluster".to_string());
    let resp = p
        .dispatch(&make_ctx("DeleteCluster", del_params))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
    let body = body_str(&resp);
    assert!(body.contains("DeleteClusterResponse"));
    assert!(body.contains("del-cluster"));

    // After delete, describe should return empty
    let desc_resp = p
        .dispatch(&make_ctx("DescribeClusters", HashMap::new()))
        .await
        .unwrap();
    let desc_body = body_str(&desc_resp);
    // Clusters element should be empty
    assert!(!desc_body.contains("del-cluster"));
}

#[tokio::test]
async fn test_delete_cluster_not_found() {
    let p = RedshiftProvider::new();
    let mut params = HashMap::new();
    params.insert("ClusterIdentifier".to_string(), "ghost-cluster".to_string());
    let resp = p
        .dispatch(&make_ctx("DeleteCluster", params))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 400);
    let body = body_str(&resp);
    assert!(body.contains("ClusterNotFound"));
}

#[tokio::test]
async fn test_modify_cluster() {
    let p = RedshiftProvider::new();
    let mut params = HashMap::new();
    params.insert("ClusterIdentifier".to_string(), "mod-cluster".to_string());
    params.insert("NodeType".to_string(), "dc2.large".to_string());
    params.insert("DBName".to_string(), "dev".to_string());
    p.dispatch(&make_ctx("CreateCluster", params))
        .await
        .unwrap();

    let mut modify = HashMap::new();
    modify.insert("ClusterIdentifier".to_string(), "mod-cluster".to_string());
    modify.insert("NodeType".to_string(), "ra3.xlplus".to_string());
    modify.insert("DBName".to_string(), "analytics".to_string());
    modify.insert("Port".to_string(), "15439".to_string());

    let resp = p
        .dispatch(&make_ctx("ModifyCluster", modify))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
    let body = body_str(&resp);
    assert!(body.contains("ModifyClusterResponse"));
    assert!(body.contains("ra3.xlplus"));
    assert!(body.contains("analytics"));
    assert!(body.contains("15439"));
}

#[tokio::test]
async fn test_reboot_cluster() {
    let p = RedshiftProvider::new();
    let mut params = HashMap::new();
    params.insert(
        "ClusterIdentifier".to_string(),
        "reboot-cluster".to_string(),
    );
    p.dispatch(&make_ctx("CreateCluster", params))
        .await
        .unwrap();

    let mut reboot = HashMap::new();
    reboot.insert(
        "ClusterIdentifier".to_string(),
        "reboot-cluster".to_string(),
    );
    let resp = p
        .dispatch(&make_ctx("RebootCluster", reboot))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
    let body = body_str(&resp);
    assert!(body.contains("RebootClusterResponse"));
    assert!(body.contains("reboot-cluster"));
    assert!(body.contains("available"));
}

#[tokio::test]
async fn test_modify_cluster_not_found() {
    let p = RedshiftProvider::new();
    let mut modify = HashMap::new();
    modify.insert(
        "ClusterIdentifier".to_string(),
        "missing-cluster".to_string(),
    );
    let resp = p
        .dispatch(&make_ctx("ModifyCluster", modify))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 400);
    let body = body_str(&resp);
    assert!(body.contains("ClusterNotFound"));
    assert!(body.contains("missing-cluster"));
}

#[tokio::test]
async fn test_modify_cluster_rejects_invalid_port() {
    let p = RedshiftProvider::new();
    let mut params = HashMap::new();
    params.insert("ClusterIdentifier".to_string(), "port-cluster".to_string());
    p.dispatch(&make_ctx("CreateCluster", params))
        .await
        .unwrap();

    let mut modify = HashMap::new();
    modify.insert("ClusterIdentifier".to_string(), "port-cluster".to_string());
    modify.insert("Port".to_string(), "not-a-port".to_string());
    let resp = p
        .dispatch(&make_ctx("ModifyCluster", modify))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 400);
    let body = body_str(&resp);
    assert!(body.contains("InvalidParameterValue"));
    assert!(body.contains("Port must be a valid 16-bit integer"));
}

#[tokio::test]
async fn test_reboot_cluster_not_found() {
    let p = RedshiftProvider::new();
    let mut reboot = HashMap::new();
    reboot.insert(
        "ClusterIdentifier".to_string(),
        "missing-reboot-cluster".to_string(),
    );
    let resp = p
        .dispatch(&make_ctx("RebootCluster", reboot))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 400);
    let body = body_str(&resp);
    assert!(body.contains("ClusterNotFound"));
    assert!(body.contains("missing-reboot-cluster"));
}

#[tokio::test]
async fn test_create_cluster_missing_identifier() {
    let p = RedshiftProvider::new();
    let resp = p
        .dispatch(&make_ctx("CreateCluster", HashMap::new()))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 400);
    let body = body_str(&resp);
    assert!(body.contains("MissingParameter"));
}
