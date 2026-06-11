/// Performance tests for RDS provider.
use std::collections::HashMap;
use std::time::Instant;

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

#[tokio::test]
async fn perf_create_db_instance_throughput() {
    let p = RdsProvider::new();
    let n = 100usize;
    let start = Instant::now();
    for i in 0..n {
        let mut params = HashMap::new();
        params.insert(
            "DBInstanceIdentifier".to_string(),
            format!("perf-db-{i:03}"),
        );
        params.insert("DBInstanceClass".to_string(), "db.t3.micro".to_string());
        params.insert("Engine".to_string(), "mysql".to_string());
        params.insert("MasterUsername".to_string(), "admin".to_string());
        let resp = p
            .dispatch(&make_ctx("CreateDBInstance", params))
            .await
            .unwrap();
        assert_eq!(resp.status_code, 200, "{}", body_str(&resp));
    }
    let elapsed = start.elapsed();
    assert!(
        elapsed.as_millis() < 2000,
        "CreateDBInstance x{n} took {}ms — expected <2000ms",
        elapsed.as_millis()
    );
}

#[tokio::test]
async fn perf_describe_db_instances_many() {
    let p = RdsProvider::new();
    let n = 100usize;
    for i in 0..n {
        let mut params = HashMap::new();
        params.insert(
            "DBInstanceIdentifier".to_string(),
            format!("list-db-{i:03}"),
        );
        params.insert("DBInstanceClass".to_string(), "db.t3.micro".to_string());
        params.insert("Engine".to_string(), "mysql".to_string());
        params.insert("MasterUsername".to_string(), "admin".to_string());
        p.dispatch(&make_ctx("CreateDBInstance", params))
            .await
            .unwrap();
    }
    let start = Instant::now();
    let resp = p
        .dispatch(&make_ctx("DescribeDBInstances", HashMap::new()))
        .await
        .unwrap();
    let elapsed = start.elapsed();
    assert_eq!(resp.status_code, 200);
    assert!(body_str(&resp).contains("list-db-000"));
    assert!(body_str(&resp).contains("list-db-099"));
    assert!(
        elapsed.as_millis() < 500,
        "DescribeDBInstances({n}) took {}ms — expected <500ms",
        elapsed.as_millis()
    );
}

#[tokio::test]
async fn perf_create_snapshot_throughput() {
    let p = RdsProvider::new();
    let mut db_params = HashMap::new();
    db_params.insert(
        "DBInstanceIdentifier".to_string(),
        "snap-src-db".to_string(),
    );
    db_params.insert("DBInstanceClass".to_string(), "db.t3.micro".to_string());
    db_params.insert("Engine".to_string(), "mysql".to_string());
    db_params.insert("MasterUsername".to_string(), "admin".to_string());
    p.dispatch(&make_ctx("CreateDBInstance", db_params))
        .await
        .unwrap();

    let n = 100usize;
    let start = Instant::now();
    for i in 0..n {
        let mut params = HashMap::new();
        params.insert(
            "DBInstanceIdentifier".to_string(),
            "snap-src-db".to_string(),
        );
        params.insert(
            "DBSnapshotIdentifier".to_string(),
            format!("perf-snap-{i:03}"),
        );
        let resp = p
            .dispatch(&make_ctx("CreateDBSnapshot", params))
            .await
            .unwrap();
        assert_eq!(resp.status_code, 200, "{}", body_str(&resp));
    }
    let elapsed = start.elapsed();
    assert!(
        elapsed.as_millis() < 2000,
        "CreateDBSnapshot x{n} took {}ms — expected <2000ms",
        elapsed.as_millis()
    );
}
