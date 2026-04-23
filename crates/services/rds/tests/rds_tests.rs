use std::collections::HashMap;

use openstack_rds::RdsProvider;
use openstack_service_framework::traits::{DispatchResponse, RequestContext, ServiceProvider};

fn make_ctx(operation: &str, params: HashMap<String, String>) -> RequestContext {
    RequestContext {
        service: "rds".to_string(),
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

fn base_instance_params(id: &str) -> HashMap<String, String> {
    let mut p = HashMap::new();
    p.insert("DBInstanceIdentifier".to_string(), id.to_string());
    p.insert("DBInstanceClass".to_string(), "db.t3.micro".to_string());
    p.insert("Engine".to_string(), "mysql".to_string());
    p.insert("MasterUsername".to_string(), "admin".to_string());
    p.insert("MasterUserPassword".to_string(), "Password1!".to_string());
    p
}

// ---------------------------------------------------------------------------
// DB Instances
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_create_db_instance() {
    let p = RdsProvider::new();
    let resp = p
        .dispatch(&make_ctx("CreateDBInstance", base_instance_params("mydb")))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
    assert_eq!(resp.content_type, "text/xml");
    let body = body_str(&resp);
    assert!(body.contains("CreateDBInstanceResponse"));
    assert!(body.contains("<DBInstanceIdentifier>mydb</DBInstanceIdentifier>"));
    assert!(body.contains("<DBInstanceStatus>available</DBInstanceStatus>"));
    assert!(body.contains("<Engine>mysql</Engine>"));
    assert!(body.contains("<Endpoint>"));
    assert!(body.contains("3306")); // mysql default port
}

#[tokio::test]
async fn test_create_db_instance_duplicate_fails() {
    let p = RdsProvider::new();
    p.dispatch(&make_ctx(
        "CreateDBInstance",
        base_instance_params("dup-db"),
    ))
    .await
    .unwrap();
    let resp = p
        .dispatch(&make_ctx(
            "CreateDBInstance",
            base_instance_params("dup-db"),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 400);
    assert!(body_str(&resp).contains("DBInstanceAlreadyExists"));
}

#[tokio::test]
async fn test_describe_db_instances() {
    let p = RdsProvider::new();
    for id in ["db-1", "db-2"] {
        p.dispatch(&make_ctx("CreateDBInstance", base_instance_params(id)))
            .await
            .unwrap();
    }
    let resp = p
        .dispatch(&make_ctx("DescribeDBInstances", HashMap::new()))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
    let body = body_str(&resp);
    assert!(body.contains("db-1"));
    assert!(body.contains("db-2"));
}

#[tokio::test]
async fn test_describe_db_instances_filter_by_id() {
    let p = RdsProvider::new();
    for id in ["db-a", "db-b"] {
        p.dispatch(&make_ctx("CreateDBInstance", base_instance_params(id)))
            .await
            .unwrap();
    }
    let mut params = HashMap::new();
    params.insert("DBInstanceIdentifier".to_string(), "db-a".to_string());
    let resp = p
        .dispatch(&make_ctx("DescribeDBInstances", params))
        .await
        .unwrap();
    let body = body_str(&resp);
    assert!(body.contains("db-a"));
    assert!(!body.contains("db-b"));
}

#[tokio::test]
async fn test_delete_db_instance() {
    let p = RdsProvider::new();
    p.dispatch(&make_ctx(
        "CreateDBInstance",
        base_instance_params("del-db"),
    ))
    .await
    .unwrap();

    let mut params = HashMap::new();
    params.insert("DBInstanceIdentifier".to_string(), "del-db".to_string());
    let resp = p
        .dispatch(&make_ctx("DeleteDBInstance", params))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
    assert!(body_str(&resp).contains("del-db"));

    let resp = p
        .dispatch(&make_ctx("DescribeDBInstances", HashMap::new()))
        .await
        .unwrap();
    assert!(!body_str(&resp).contains("del-db"));
}

#[tokio::test]
async fn test_delete_db_instance_not_found() {
    let p = RdsProvider::new();
    let mut params = HashMap::new();
    params.insert("DBInstanceIdentifier".to_string(), "ghost-db".to_string());
    let resp = p
        .dispatch(&make_ctx("DeleteDBInstance", params))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 400);
    assert!(body_str(&resp).contains("DBInstanceNotFound"));
}

#[tokio::test]
async fn test_modify_db_instance() {
    let p = RdsProvider::new();
    p.dispatch(&make_ctx(
        "CreateDBInstance",
        base_instance_params("mod-db"),
    ))
    .await
    .unwrap();

    let mut params = HashMap::new();
    params.insert("DBInstanceIdentifier".to_string(), "mod-db".to_string());
    params.insert("DBInstanceClass".to_string(), "db.m5.large".to_string());
    params.insert("AllocatedStorage".to_string(), "100".to_string());
    params.insert("MultiAZ".to_string(), "true".to_string());
    let resp = p
        .dispatch(&make_ctx("ModifyDBInstance", params))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
    let body = body_str(&resp);
    assert!(body.contains("db.m5.large"));
    assert!(body.contains("<AllocatedStorage>100</AllocatedStorage>"));
    assert!(body.contains("<MultiAZ>true</MultiAZ>"));
}

#[tokio::test]
async fn test_reboot_db_instance() {
    let p = RdsProvider::new();
    p.dispatch(&make_ctx(
        "CreateDBInstance",
        base_instance_params("reboot-db"),
    ))
    .await
    .unwrap();

    let mut params = HashMap::new();
    params.insert("DBInstanceIdentifier".to_string(), "reboot-db".to_string());
    let resp = p
        .dispatch(&make_ctx("RebootDBInstance", params))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
    let body = body_str(&resp);
    assert!(body.contains("RebootDBInstanceResponse"));
    assert!(body.contains("<DBInstanceStatus>available</DBInstanceStatus>"));
}

#[tokio::test]
async fn test_engine_port_postgres() {
    let p = RdsProvider::new();
    let mut params = base_instance_params("pg-db");
    params.insert("Engine".to_string(), "postgres".to_string());
    let resp = p
        .dispatch(&make_ctx("CreateDBInstance", params))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
    assert!(body_str(&resp).contains("5432")); // postgres default port
}

// ---------------------------------------------------------------------------
// Snapshots
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_create_and_describe_snapshot() {
    let p = RdsProvider::new();
    p.dispatch(&make_ctx(
        "CreateDBInstance",
        base_instance_params("snap-db"),
    ))
    .await
    .unwrap();

    let mut params = HashMap::new();
    params.insert("DBInstanceIdentifier".to_string(), "snap-db".to_string());
    params.insert("DBSnapshotIdentifier".to_string(), "snap-001".to_string());
    let resp = p
        .dispatch(&make_ctx("CreateDBSnapshot", params))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
    let body = body_str(&resp);
    assert!(body.contains("snap-001"));
    assert!(body.contains("<Status>available</Status>"));

    let desc_resp = p
        .dispatch(&make_ctx("DescribeDBSnapshots", HashMap::new()))
        .await
        .unwrap();
    assert!(body_str(&desc_resp).contains("snap-001"));
}

#[tokio::test]
async fn test_delete_snapshot() {
    let p = RdsProvider::new();
    let mut params = HashMap::new();
    params.insert("DBInstanceIdentifier".to_string(), "db-x".to_string());
    params.insert("DBSnapshotIdentifier".to_string(), "del-snap".to_string());
    p.dispatch(&make_ctx("CreateDBSnapshot", params))
        .await
        .unwrap();

    let mut del = HashMap::new();
    del.insert("DBSnapshotIdentifier".to_string(), "del-snap".to_string());
    let resp = p
        .dispatch(&make_ctx("DeleteDBSnapshot", del))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
    assert!(
        !body_str(
            &p.dispatch(&make_ctx("DescribeDBSnapshots", HashMap::new()))
                .await
                .unwrap()
        )
        .contains("del-snap")
    );
}

#[tokio::test]
async fn test_restore_db_instance_from_snapshot() {
    let p = RdsProvider::new();
    p.dispatch(&make_ctx(
        "CreateDBInstance",
        base_instance_params("restore-src"),
    ))
    .await
    .unwrap();

    let mut snap_params = HashMap::new();
    snap_params.insert(
        "DBInstanceIdentifier".to_string(),
        "restore-src".to_string(),
    );
    snap_params.insert(
        "DBSnapshotIdentifier".to_string(),
        "restore-snap".to_string(),
    );
    p.dispatch(&make_ctx("CreateDBSnapshot", snap_params))
        .await
        .unwrap();

    let mut restore_params = HashMap::new();
    restore_params.insert(
        "DBInstanceIdentifier".to_string(),
        "restore-new".to_string(),
    );
    restore_params.insert(
        "DBSnapshotIdentifier".to_string(),
        "restore-snap".to_string(),
    );
    let resp = p
        .dispatch(&make_ctx("RestoreDBInstanceFromDBSnapshot", restore_params))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
    let body = body_str(&resp);
    assert!(body.contains("restore-new"));
    assert!(body.contains("<DBInstanceStatus>available</DBInstanceStatus>"));
}

// ---------------------------------------------------------------------------
// Subnet groups
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_create_and_describe_subnet_group() {
    let p = RdsProvider::new();
    let mut params = HashMap::new();
    params.insert("DBSubnetGroupName".to_string(), "my-rds-sg".to_string());
    params.insert(
        "DBSubnetGroupDescription".to_string(),
        "test subnet group".to_string(),
    );
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
        .dispatch(&make_ctx("CreateDBSubnetGroup", params))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
    assert!(body_str(&resp).contains("<DBSubnetGroupName>my-rds-sg</DBSubnetGroupName>"));

    let desc_resp = p
        .dispatch(&make_ctx("DescribeDBSubnetGroups", HashMap::new()))
        .await
        .unwrap();
    let body = body_str(&desc_resp);
    assert!(body.contains("my-rds-sg"));
    assert!(body.contains("subnet-aaa"));
    assert!(body.contains("subnet-bbb"));
}

#[tokio::test]
async fn test_delete_subnet_group() {
    let p = RdsProvider::new();
    let mut params = HashMap::new();
    params.insert("DBSubnetGroupName".to_string(), "sg-del".to_string());
    p.dispatch(&make_ctx("CreateDBSubnetGroup", params))
        .await
        .unwrap();

    let mut del = HashMap::new();
    del.insert("DBSubnetGroupName".to_string(), "sg-del".to_string());
    let resp = p
        .dispatch(&make_ctx("DeleteDBSubnetGroup", del))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
    assert!(
        !body_str(
            &p.dispatch(&make_ctx("DescribeDBSubnetGroups", HashMap::new()))
                .await
                .unwrap()
        )
        .contains("sg-del")
    );
}

// ---------------------------------------------------------------------------
// Parameter groups
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_create_and_describe_parameter_group() {
    let p = RdsProvider::new();
    let mut params = HashMap::new();
    params.insert("DBParameterGroupName".to_string(), "my-pg".to_string());
    params.insert("DBParameterGroupFamily".to_string(), "mysql8.0".to_string());
    params.insert("Description".to_string(), "test pg".to_string());
    let resp = p
        .dispatch(&make_ctx("CreateDBParameterGroup", params))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
    assert!(body_str(&resp).contains("<DBParameterGroupName>my-pg</DBParameterGroupName>"));

    let desc_resp = p
        .dispatch(&make_ctx("DescribeDBParameterGroups", HashMap::new()))
        .await
        .unwrap();
    let body = body_str(&desc_resp);
    assert!(body.contains("my-pg"));
    assert!(body.contains("mysql8.0"));
}

#[tokio::test]
async fn test_delete_parameter_group() {
    let p = RdsProvider::new();
    let mut params = HashMap::new();
    params.insert("DBParameterGroupName".to_string(), "pg-del".to_string());
    p.dispatch(&make_ctx("CreateDBParameterGroup", params))
        .await
        .unwrap();

    let mut del = HashMap::new();
    del.insert("DBParameterGroupName".to_string(), "pg-del".to_string());
    let resp = p
        .dispatch(&make_ctx("DeleteDBParameterGroup", del))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
    assert!(
        !body_str(
            &p.dispatch(&make_ctx("DescribeDBParameterGroups", HashMap::new()))
                .await
                .unwrap()
        )
        .contains("pg-del")
    );
}
