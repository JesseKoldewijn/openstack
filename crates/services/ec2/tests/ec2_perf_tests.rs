/// Performance tests for EC2 provider.
///
/// These cover the in-memory lifecycle and inventory paths we emulate: VPCs,
/// instances, and security-group mutations.
use std::collections::HashMap;
use std::time::Instant;

use openstack_ec2::Ec2Provider;
use openstack_service_framework::traits::{DispatchResponse, RequestContext, ServiceProvider};

fn make_ctx(operation: &str, params: HashMap<String, String>) -> RequestContext {
    RequestContext {
        service: "ec2".to_string(),
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

async fn create_vpc(p: &Ec2Provider, cidr: &str) -> String {
    let mut params = HashMap::new();
    params.insert("CidrBlock".to_string(), cidr.to_string());
    let resp = p.dispatch(&make_ctx("CreateVpc", params)).await.unwrap();
    assert_eq!(resp.status_code, 200);
    let body = body_str(&resp);
    let start = body.find("<vpcId>").unwrap() + 7;
    let end = body.find("</vpcId>").unwrap();
    body[start..end].to_string()
}

#[tokio::test]
async fn perf_create_vpc_throughput() {
    let p = Ec2Provider::new();
    let n = 100usize;

    let start = Instant::now();
    for i in 0..n {
        let _ = create_vpc(&p, &format!("10.{}.0.0/16", i % 250 + 1)).await;
    }
    let elapsed = start.elapsed();

    assert!(
        elapsed.as_millis() < 2000,
        "CreateVpc x{n} took {}ms — expected <2000ms",
        elapsed.as_millis()
    );
}

#[tokio::test]
async fn perf_describe_instances_many() {
    let p = Ec2Provider::new();
    let n = 100usize;
    for i in 0..n {
        let mut run_params = HashMap::new();
        run_params.insert("ImageId".to_string(), format!("ami-{i:08}"));
        run_params.insert("InstanceType".to_string(), "t2.micro".to_string());
        run_params.insert("MaxCount".to_string(), "1".to_string());
        run_params.insert("MinCount".to_string(), "1".to_string());
        let resp = p
            .dispatch(&make_ctx("RunInstances", run_params))
            .await
            .unwrap();
        assert_eq!(resp.status_code, 200);
    }

    let start = Instant::now();
    let resp = p
        .dispatch(&make_ctx("DescribeInstances", HashMap::new()))
        .await
        .unwrap();
    let elapsed = start.elapsed();
    assert_eq!(resp.status_code, 200);
    let body = body_str(&resp);
    assert!(body.contains("ami-00000000"));

    assert!(
        elapsed.as_millis() < 500,
        "DescribeInstances({n}) took {}ms — expected <500ms",
        elapsed.as_millis()
    );
}

#[tokio::test]
async fn perf_security_group_ingress_round_trip() {
    let p = Ec2Provider::new();
    let mut sg_params = HashMap::new();
    sg_params.insert("GroupName".to_string(), "perf-sg".to_string());
    sg_params.insert("Description".to_string(), "perf security group".to_string());
    let sg_resp = p
        .dispatch(&make_ctx("CreateSecurityGroup", sg_params))
        .await
        .unwrap();
    assert_eq!(sg_resp.status_code, 200);
    let sg_body = body_str(&sg_resp);
    let gstart = sg_body.find("<groupId>").unwrap() + 9;
    let gend = sg_body.find("</groupId>").unwrap();
    let group_id = sg_body[gstart..gend].to_string();

    let start = Instant::now();
    for i in 0..50usize {
        let mut params = HashMap::new();
        params.insert("GroupId".to_string(), group_id.clone());
        params.insert(
            format!("IpPermissions.1.IpRanges.{i}.CidrIp"),
            format!("10.0.{}.0/24", i % 250),
        );
        params.insert("IpPermissions.1.IpProtocol".to_string(), "tcp".to_string());
        params.insert("IpPermissions.1.FromPort".to_string(), "80".to_string());
        params.insert("IpPermissions.1.ToPort".to_string(), "80".to_string());
        let resp = p
            .dispatch(&make_ctx("AuthorizeSecurityGroupIngress", params))
            .await
            .unwrap();
        assert_eq!(resp.status_code, 200);
    }
    let elapsed = start.elapsed();

    assert!(
        elapsed.as_millis() < 1000,
        "AuthorizeSecurityGroupIngress x50 took {}ms — expected <1000ms",
        elapsed.as_millis()
    );
}
