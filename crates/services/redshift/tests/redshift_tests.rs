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

    let desc_resp = p
        .dispatch(&make_ctx("DescribeClusters", HashMap::new()))
        .await
        .unwrap();
    let desc_body = body_str(&desc_resp);
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
async fn test_modify_cluster_escapes_xml_fields() {
    let p = RedshiftProvider::new();
    let mut params = HashMap::new();
    params.insert(
        "ClusterIdentifier".to_string(),
        "mod-xml-cluster".to_string(),
    );
    params.insert("NodeType".to_string(), "dc2.large".to_string());
    params.insert("DBName".to_string(), "dev".to_string());
    p.dispatch(&make_ctx("CreateCluster", params))
        .await
        .unwrap();

    let mut modify = HashMap::new();
    modify.insert(
        "ClusterIdentifier".to_string(),
        "mod-xml-cluster".to_string(),
    );
    modify.insert("NodeType".to_string(), "ra3&xl<plus>".to_string());
    modify.insert("DBName".to_string(), "analytics<&>".to_string());

    let resp = p
        .dispatch(&make_ctx("ModifyCluster", modify))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
    let body = body_str(&resp);
    assert!(body.contains("ra3&amp;xl&lt;plus&gt;"), "{body}");
    assert!(body.contains("analytics&lt;&amp;&gt;"), "{body}");
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

// ---------------------------------------------------------------------------
// Snapshots
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_create_and_describe_snapshot() {
    let p = RedshiftProvider::new();
    let mut cluster_params = HashMap::new();
    cluster_params.insert("ClusterIdentifier".to_string(), "snap-cluster".to_string());
    cluster_params.insert("NodeType".to_string(), "dc2.large".to_string());
    p.dispatch(&make_ctx("CreateCluster", cluster_params))
        .await
        .unwrap();

    let mut params = HashMap::new();
    params.insert(
        "SnapshotIdentifier".to_string(),
        "snap-001".to_string(),
    );
    params.insert("ClusterIdentifier".to_string(), "snap-cluster".to_string());
    let resp = p
        .dispatch(&make_ctx("CreateClusterSnapshot", params))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
    let body = body_str(&resp);
    assert!(body.contains("CreateClusterSnapshotResponse"));
    assert!(body.contains("<SnapshotIdentifier>snap-001</SnapshotIdentifier>"));
    assert!(body.contains("<Status>available</Status>"));

    let resp = p
        .dispatch(&make_ctx("DescribeClusterSnapshots", HashMap::new()))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
    let body = body_str(&resp);
    assert!(body.contains("snap-001"));
    assert!(body.contains("snap-cluster"));
}

#[tokio::test]
async fn test_create_snapshot_duplicate_fails() {
    let p = RedshiftProvider::new();
    let mut params = HashMap::new();
    params.insert("SnapshotIdentifier".to_string(), "dup-snap".to_string());
    params.insert("ClusterIdentifier".to_string(), "c".to_string());
    p.dispatch(&make_ctx("CreateClusterSnapshot", params.clone()))
        .await
        .unwrap();
    let resp = p
        .dispatch(&make_ctx("CreateClusterSnapshot", params))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 400);
    let body = body_str(&resp);
    assert!(body.contains("ClusterSnapshotAlreadyExists"));
}

#[tokio::test]
async fn test_delete_snapshot() {
    let p = RedshiftProvider::new();
    let mut params = HashMap::new();
    params.insert("SnapshotIdentifier".to_string(), "del-snap".to_string());
    params.insert("ClusterIdentifier".to_string(), "c".to_string());
    p.dispatch(&make_ctx("CreateClusterSnapshot", params))
        .await
        .unwrap();

    let mut del = HashMap::new();
    del.insert("SnapshotIdentifier".to_string(), "del-snap".to_string());
    let resp = p
        .dispatch(&make_ctx("DeleteClusterSnapshot", del))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);

    let resp = p
        .dispatch(&make_ctx("DescribeClusterSnapshots", HashMap::new()))
        .await
        .unwrap();
    assert!(!body_str(&resp).contains("del-snap"));
}

#[tokio::test]
async fn test_delete_snapshot_not_found() {
    let p = RedshiftProvider::new();
    let mut del = HashMap::new();
    del.insert("SnapshotIdentifier".to_string(), "ghost-snap".to_string());
    let resp = p
        .dispatch(&make_ctx("DeleteClusterSnapshot", del))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 400);
    assert!(body_str(&resp).contains("ClusterSnapshotNotFound"));
}

#[tokio::test]
async fn test_describe_snapshots_filtered_by_cluster() {
    let p = RedshiftProvider::new();
    for (snap, cluster) in [("s1", "c1"), ("s2", "c2"), ("s3", "c1")] {
        let mut params = HashMap::new();
        params.insert("SnapshotIdentifier".to_string(), snap.to_string());
        params.insert("ClusterIdentifier".to_string(), cluster.to_string());
        p.dispatch(&make_ctx("CreateClusterSnapshot", params))
            .await
            .unwrap();
    }

    let mut filter = HashMap::new();
    filter.insert("ClusterIdentifier".to_string(), "c1".to_string());
    let resp = p
        .dispatch(&make_ctx("DescribeClusterSnapshots", filter))
        .await
        .unwrap();
    let body = body_str(&resp);
    assert!(body.contains("s1"));
    assert!(body.contains("s3"));
    assert!(!body.contains("s2"));
}

// ---------------------------------------------------------------------------
// Subnet groups
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_create_and_describe_subnet_group() {
    let p = RedshiftProvider::new();
    let mut params = HashMap::new();
    params.insert(
        "ClusterSubnetGroupName".to_string(),
        "my-sg".to_string(),
    );
    params.insert("Description".to_string(), "test subnet group".to_string());
    params.insert("VpcId".to_string(), "vpc-12345".to_string());
    params.insert(
        "SubnetIds.SubnetIdentifier.1".to_string(),
        "subnet-aaa".to_string(),
    );
    params.insert(
        "SubnetIds.SubnetIdentifier.2".to_string(),
        "subnet-bbb".to_string(),
    );
    let resp = p
        .dispatch(&make_ctx("CreateClusterSubnetGroup", params))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
    let body = body_str(&resp);
    assert!(body.contains("CreateClusterSubnetGroupResponse"));
    assert!(body.contains("<ClusterSubnetGroupName>my-sg</ClusterSubnetGroupName>"));

    let resp = p
        .dispatch(&make_ctx("DescribeClusterSubnetGroups", HashMap::new()))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
    let body = body_str(&resp);
    assert!(body.contains("my-sg"));
    assert!(body.contains("subnet-aaa"));
    assert!(body.contains("subnet-bbb"));
}

#[tokio::test]
async fn test_delete_subnet_group() {
    let p = RedshiftProvider::new();
    let mut params = HashMap::new();
    params.insert("ClusterSubnetGroupName".to_string(), "sg-del".to_string());
    p.dispatch(&make_ctx("CreateClusterSubnetGroup", params))
        .await
        .unwrap();

    let mut del = HashMap::new();
    del.insert("ClusterSubnetGroupName".to_string(), "sg-del".to_string());
    let resp = p
        .dispatch(&make_ctx("DeleteClusterSubnetGroup", del))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);

    let resp = p
        .dispatch(&make_ctx("DescribeClusterSubnetGroups", HashMap::new()))
        .await
        .unwrap();
    assert!(!body_str(&resp).contains("sg-del"));
}

#[tokio::test]
async fn test_delete_subnet_group_not_found() {
    let p = RedshiftProvider::new();
    let mut del = HashMap::new();
    del.insert("ClusterSubnetGroupName".to_string(), "ghost-sg".to_string());
    let resp = p
        .dispatch(&make_ctx("DeleteClusterSubnetGroup", del))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 400);
    assert!(body_str(&resp).contains("ClusterSubnetGroupNotFoundFault"));
}

// ---------------------------------------------------------------------------
// Parameter groups
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_create_and_describe_parameter_group() {
    let p = RedshiftProvider::new();
    let mut params = HashMap::new();
    params.insert(
        "ParameterGroupName".to_string(),
        "my-pg".to_string(),
    );
    params.insert(
        "ParameterGroupFamily".to_string(),
        "redshift-1.0".to_string(),
    );
    params.insert("Description".to_string(), "test pg".to_string());
    let resp = p
        .dispatch(&make_ctx("CreateClusterParameterGroup", params))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
    let body = body_str(&resp);
    assert!(body.contains("CreateClusterParameterGroupResponse"));
    assert!(body.contains("<ParameterGroupName>my-pg</ParameterGroupName>"));

    let resp = p
        .dispatch(&make_ctx("DescribeClusterParameterGroups", HashMap::new()))
        .await
        .unwrap();
    let body = body_str(&resp);
    assert!(body.contains("my-pg"));
    assert!(body.contains("redshift-1.0"));
}

#[tokio::test]
async fn test_delete_parameter_group() {
    let p = RedshiftProvider::new();
    let mut params = HashMap::new();
    params.insert("ParameterGroupName".to_string(), "pg-del".to_string());
    p.dispatch(&make_ctx("CreateClusterParameterGroup", params))
        .await
        .unwrap();

    let mut del = HashMap::new();
    del.insert("ParameterGroupName".to_string(), "pg-del".to_string());
    let resp = p
        .dispatch(&make_ctx("DeleteClusterParameterGroup", del))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);

    let resp = p
        .dispatch(&make_ctx("DescribeClusterParameterGroups", HashMap::new()))
        .await
        .unwrap();
    assert!(!body_str(&resp).contains("pg-del"));
}

#[tokio::test]
async fn test_delete_parameter_group_not_found() {
    let p = RedshiftProvider::new();
    let mut del = HashMap::new();
    del.insert("ParameterGroupName".to_string(), "ghost-pg".to_string());
    let resp = p
        .dispatch(&make_ctx("DeleteClusterParameterGroup", del))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 400);
    assert!(body_str(&resp).contains("ClusterParameterGroupNotFound"));
}

// ---------------------------------------------------------------------------
// Logging
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_enable_and_disable_logging() {
    let p = RedshiftProvider::new();
    let mut params = HashMap::new();
    params.insert("ClusterIdentifier".to_string(), "log-cluster".to_string());
    p.dispatch(&make_ctx("CreateCluster", params))
        .await
        .unwrap();

    let mut enable = HashMap::new();
    enable.insert("ClusterIdentifier".to_string(), "log-cluster".to_string());
    let resp = p
        .dispatch(&make_ctx("EnableLogging", enable.clone()))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
    let body = body_str(&resp);
    assert!(body.contains("EnableLoggingResponse"));
    assert!(body.contains("<LoggingEnabled>true</LoggingEnabled>"));

    let resp = p
        .dispatch(&make_ctx("DisableLogging", enable))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
    let body = body_str(&resp);
    assert!(body.contains("DisableLoggingResponse"));
    assert!(body.contains("<LoggingEnabled>false</LoggingEnabled>"));
}
