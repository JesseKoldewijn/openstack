use std::collections::HashMap;

use bytes::Bytes;
use openstack_ec2::Ec2Provider;
use openstack_service_framework::traits::{RequestContext, ServiceProvider};

fn make_ctx(operation: &str, params: HashMap<String, String>) -> RequestContext {
    RequestContext {
        service: "ec2".to_string(),
        operation: operation.to_string(),
        region: "us-east-1".to_string(),
        account_id: "000000000000".to_string(),
        request_body: serde_json::json!({}),
        raw_body: Bytes::new(),
        headers: HashMap::new(),
        path: "/".to_string(),
        method: "POST".to_string(),
        query_params: params,
    }
}

fn body_str(resp: &openstack_service_framework::traits::DispatchResponse) -> String {
    String::from_utf8_lossy(&resp.body).to_string()
}

// ---------------------------------------------------------------------------
// VPC Tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_create_vpc() {
    let p = Ec2Provider::new();
    let mut params = HashMap::new();
    params.insert("CidrBlock".to_string(), "10.1.0.0/16".to_string());
    let resp = p.dispatch(&make_ctx("CreateVpc", params)).await.unwrap();
    assert_eq!(resp.status_code, 200);
    assert_eq!(resp.content_type, "text/xml");
    let body = body_str(&resp);
    assert!(body.contains("<vpcId>"));
    assert!(body.contains("10.1.0.0/16"));
    assert!(body.contains("available"));
}

#[tokio::test]
async fn test_describe_vpcs() {
    let p = Ec2Provider::new();
    // Create a VPC first
    let mut params = HashMap::new();
    params.insert("CidrBlock".to_string(), "10.2.0.0/16".to_string());
    p.dispatch(&make_ctx("CreateVpc", params)).await.unwrap();

    let resp = p
        .dispatch(&make_ctx("DescribeVpcs", HashMap::new()))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
    let body = body_str(&resp);
    assert!(body.contains("<vpcSet>"));
    assert!(body.contains("10.2.0.0/16"));
}

#[tokio::test]
async fn test_delete_vpc() {
    let p = Ec2Provider::new();
    // Create a VPC
    let mut params = HashMap::new();
    params.insert("CidrBlock".to_string(), "10.3.0.0/16".to_string());
    let create_resp = p.dispatch(&make_ctx("CreateVpc", params)).await.unwrap();
    let body = body_str(&create_resp);
    // Extract vpc_id from XML
    let start = body.find("<vpcId>").unwrap() + 7;
    let end = body.find("</vpcId>").unwrap();
    let vpc_id = &body[start..end];

    // Delete it
    let mut del_params = HashMap::new();
    del_params.insert("VpcId".to_string(), vpc_id.to_string());
    let del_resp = p
        .dispatch(&make_ctx("DeleteVpc", del_params))
        .await
        .unwrap();
    assert_eq!(del_resp.status_code, 200);
    let del_body = body_str(&del_resp);
    assert!(del_body.contains("<return>true</return>"));
}

// ---------------------------------------------------------------------------
// Subnet Tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_create_subnet() {
    let p = Ec2Provider::new();
    // Create a VPC first
    let mut vpc_params = HashMap::new();
    vpc_params.insert("CidrBlock".to_string(), "10.4.0.0/16".to_string());
    let vpc_resp = p
        .dispatch(&make_ctx("CreateVpc", vpc_params))
        .await
        .unwrap();
    let vpc_body = body_str(&vpc_resp);
    let vstart = vpc_body.find("<vpcId>").unwrap() + 7;
    let vend = vpc_body.find("</vpcId>").unwrap();
    let vpc_id = vpc_body[vstart..vend].to_string();

    let mut params = HashMap::new();
    params.insert("VpcId".to_string(), vpc_id.clone());
    params.insert("CidrBlock".to_string(), "10.4.1.0/24".to_string());
    let resp = p.dispatch(&make_ctx("CreateSubnet", params)).await.unwrap();
    assert_eq!(resp.status_code, 200);
    let body = body_str(&resp);
    assert!(body.contains("<subnetId>"));
    assert!(body.contains(&vpc_id));
    assert!(body.contains("10.4.1.0/24"));
}

#[tokio::test]
async fn test_describe_subnets() {
    let p = Ec2Provider::new();
    // Create a VPC and subnet
    let mut vpc_params = HashMap::new();
    vpc_params.insert("CidrBlock".to_string(), "10.5.0.0/16".to_string());
    let vpc_resp = p
        .dispatch(&make_ctx("CreateVpc", vpc_params))
        .await
        .unwrap();
    let vpc_body = body_str(&vpc_resp);
    let vstart = vpc_body.find("<vpcId>").unwrap() + 7;
    let vend = vpc_body.find("</vpcId>").unwrap();
    let vpc_id = vpc_body[vstart..vend].to_string();

    let mut subnet_params = HashMap::new();
    subnet_params.insert("VpcId".to_string(), vpc_id);
    subnet_params.insert("CidrBlock".to_string(), "10.5.1.0/24".to_string());
    p.dispatch(&make_ctx("CreateSubnet", subnet_params))
        .await
        .unwrap();

    let resp = p
        .dispatch(&make_ctx("DescribeSubnets", HashMap::new()))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
    let body = body_str(&resp);
    assert!(body.contains("<subnetSet>"));
    assert!(body.contains("10.5.1.0/24"));
}

// ---------------------------------------------------------------------------
// Security Group Tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_create_security_group() {
    let p = Ec2Provider::new();
    let mut params = HashMap::new();
    params.insert("GroupName".to_string(), "my-sg".to_string());
    params.insert("Description".to_string(), "Test SG".to_string());
    let resp = p
        .dispatch(&make_ctx("CreateSecurityGroup", params))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
    let body = body_str(&resp);
    assert!(body.contains("<groupId>"));
    assert!(body.contains("sg-"));
}

#[tokio::test]
async fn test_describe_security_groups() {
    let p = Ec2Provider::new();
    let mut params = HashMap::new();
    params.insert("GroupName".to_string(), "desc-sg".to_string());
    params.insert(
        "Description".to_string(),
        "SG for describe test".to_string(),
    );
    p.dispatch(&make_ctx("CreateSecurityGroup", params))
        .await
        .unwrap();

    let resp = p
        .dispatch(&make_ctx("DescribeSecurityGroups", HashMap::new()))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
    let body = body_str(&resp);
    assert!(body.contains("<securityGroupInfo>"));
    assert!(body.contains("desc-sg"));
}

#[tokio::test]
async fn test_authorize_security_group_ingress() {
    let p = Ec2Provider::new();
    // Create a SG
    let mut sg_params = HashMap::new();
    sg_params.insert("GroupName".to_string(), "ingress-sg".to_string());
    sg_params.insert("Description".to_string(), "Ingress test".to_string());
    let sg_resp = p
        .dispatch(&make_ctx("CreateSecurityGroup", sg_params))
        .await
        .unwrap();
    let sg_body = body_str(&sg_resp);
    let gstart = sg_body.find("<groupId>").unwrap() + 9;
    let gend = sg_body.find("</groupId>").unwrap();
    let group_id = sg_body[gstart..gend].to_string();

    // Authorize ingress
    let mut params = HashMap::new();
    params.insert("GroupId".to_string(), group_id);
    params.insert("IpPermissions.1.IpProtocol".to_string(), "tcp".to_string());
    params.insert("IpPermissions.1.FromPort".to_string(), "80".to_string());
    params.insert("IpPermissions.1.ToPort".to_string(), "80".to_string());
    params.insert(
        "IpPermissions.1.IpRanges.1.CidrIp".to_string(),
        "0.0.0.0/0".to_string(),
    );
    let resp = p
        .dispatch(&make_ctx("AuthorizeSecurityGroupIngress", params))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
    let body = body_str(&resp);
    assert!(body.contains("<return>true</return>"));
}

// ---------------------------------------------------------------------------
// Instance Tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_run_instances() {
    let p = Ec2Provider::new();
    let mut params = HashMap::new();
    params.insert("ImageId".to_string(), "ami-12345678".to_string());
    params.insert("InstanceType".to_string(), "t2.micro".to_string());
    params.insert("MaxCount".to_string(), "1".to_string());
    params.insert("MinCount".to_string(), "1".to_string());
    let resp = p.dispatch(&make_ctx("RunInstances", params)).await.unwrap();
    assert_eq!(resp.status_code, 200);
    let body = body_str(&resp);
    assert!(body.contains("<instanceId>"));
    assert!(body.contains("ami-12345678"));
    assert!(body.contains("t2.micro"));
    assert!(body.contains("running"));
}

#[tokio::test]
async fn test_describe_instances() {
    let p = Ec2Provider::new();
    // Run an instance
    let mut run_params = HashMap::new();
    run_params.insert("ImageId".to_string(), "ami-00000001".to_string());
    run_params.insert("InstanceType".to_string(), "t3.small".to_string());
    run_params.insert("MaxCount".to_string(), "1".to_string());
    run_params.insert("MinCount".to_string(), "1".to_string());
    p.dispatch(&make_ctx("RunInstances", run_params))
        .await
        .unwrap();

    let resp = p
        .dispatch(&make_ctx("DescribeInstances", HashMap::new()))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
    let body = body_str(&resp);
    assert!(body.contains("<reservationSet>"));
    assert!(body.contains("<instanceId>"));
    assert!(body.contains("ami-00000001"));
}

#[tokio::test]
async fn test_terminate_instances() {
    let p = Ec2Provider::new();
    // Run an instance
    let mut run_params = HashMap::new();
    run_params.insert("ImageId".to_string(), "ami-terminate".to_string());
    run_params.insert("InstanceType".to_string(), "t2.micro".to_string());
    run_params.insert("MaxCount".to_string(), "1".to_string());
    run_params.insert("MinCount".to_string(), "1".to_string());
    let run_resp = p
        .dispatch(&make_ctx("RunInstances", run_params))
        .await
        .unwrap();
    let run_body = body_str(&run_resp);
    let istart = run_body.find("<instanceId>").unwrap() + 12;
    let iend = run_body.find("</instanceId>").unwrap();
    let instance_id = run_body[istart..iend].to_string();

    // Terminate it
    let mut term_params = HashMap::new();
    term_params.insert("InstanceId.1".to_string(), instance_id.clone());
    let term_resp = p
        .dispatch(&make_ctx("TerminateInstances", term_params))
        .await
        .unwrap();
    assert_eq!(term_resp.status_code, 200);
    let body = body_str(&term_resp);
    assert!(body.contains("terminated"));
    assert!(body.contains(&instance_id));
}
