use std::collections::HashMap;

use openstack_ec2::Ec2Provider;
use openstack_service_framework::traits::{RequestContext, ServiceProvider};

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

fn body_str(resp: &openstack_service_framework::traits::DispatchResponse) -> String {
    String::from_utf8_lossy(resp.body.as_bytes()).to_string()
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
async fn test_terminate_missing_instance_returns_not_found() {
    let p = Ec2Provider::new();
    let mut params = HashMap::new();
    params.insert(
        "InstanceId.1".to_string(),
        "i-1234567890abcdef0".to_string(),
    );

    let resp = p
        .dispatch(&make_ctx("TerminateInstances", params))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 400);
    let body = body_str(&resp);
    assert!(body.contains("InvalidInstanceID.NotFound"));
    assert!(body.contains("The instance ID 'i-1234567890abcdef0' does not exist"));
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

// ---------------------------------------------------------------------------
// DeleteSubnet / DeleteSecurityGroup
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_delete_subnet() {
    let p = Ec2Provider::new();
    let mut vpc_p = HashMap::new();
    vpc_p.insert("CidrBlock".to_string(), "10.10.0.0/16".to_string());
    let vpc_resp = p.dispatch(&make_ctx("CreateVpc", vpc_p)).await.unwrap();
    let vpc_body = body_str(&vpc_resp);
    let vpc_id = extract_tag(&vpc_body, "vpcId");

    let mut sn_p = HashMap::new();
    sn_p.insert("VpcId".to_string(), vpc_id);
    sn_p.insert("CidrBlock".to_string(), "10.10.1.0/24".to_string());
    let sn_resp = p.dispatch(&make_ctx("CreateSubnet", sn_p)).await.unwrap();
    let subnet_id = extract_tag(&body_str(&sn_resp), "subnetId");

    let mut del = HashMap::new();
    del.insert("SubnetId".to_string(), subnet_id.clone());
    let resp = p.dispatch(&make_ctx("DeleteSubnet", del)).await.unwrap();
    assert_eq!(resp.status_code, 200);
    assert!(body_str(&resp).contains("<return>true</return>"));

    let list = p
        .dispatch(&make_ctx("DescribeSubnets", HashMap::new()))
        .await
        .unwrap();
    assert!(!body_str(&list).contains(&subnet_id));
}

#[tokio::test]
async fn test_delete_subnet_not_found() {
    let p = Ec2Provider::new();
    let mut del = HashMap::new();
    del.insert("SubnetId".to_string(), "subnet-notexist".to_string());
    let resp = p.dispatch(&make_ctx("DeleteSubnet", del)).await.unwrap();
    assert_eq!(resp.status_code, 400);
    assert!(body_str(&resp).contains("InvalidSubnetID.NotFound"));
}

#[tokio::test]
async fn test_delete_security_group() {
    let p = Ec2Provider::new();
    let mut sg_p = HashMap::new();
    sg_p.insert("GroupName".to_string(), "del-sg".to_string());
    sg_p.insert("Description".to_string(), "to delete".to_string());
    let sg_resp = p
        .dispatch(&make_ctx("CreateSecurityGroup", sg_p))
        .await
        .unwrap();
    let group_id = extract_tag(&body_str(&sg_resp), "groupId");

    let mut del = HashMap::new();
    del.insert("GroupId".to_string(), group_id.clone());
    let resp = p
        .dispatch(&make_ctx("DeleteSecurityGroup", del))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);

    let list = p
        .dispatch(&make_ctx("DescribeSecurityGroups", HashMap::new()))
        .await
        .unwrap();
    assert!(!body_str(&list).contains(&group_id));
}

// ---------------------------------------------------------------------------
// Start / Stop instances
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_stop_and_start_instances() {
    let p = Ec2Provider::new();
    let mut run_p = HashMap::new();
    run_p.insert("ImageId".to_string(), "ami-ss".to_string());
    run_p.insert("InstanceType".to_string(), "t2.micro".to_string());
    run_p.insert("MaxCount".to_string(), "1".to_string());
    let run_resp = p.dispatch(&make_ctx("RunInstances", run_p)).await.unwrap();
    let instance_id = extract_tag(&body_str(&run_resp), "instanceId");

    let mut stop_p = HashMap::new();
    stop_p.insert("InstanceId.1".to_string(), instance_id.clone());
    let stop_resp = p
        .dispatch(&make_ctx("StopInstances", stop_p))
        .await
        .unwrap();
    assert_eq!(stop_resp.status_code, 200);
    assert!(body_str(&stop_resp).contains("stopped"));

    let mut start_p = HashMap::new();
    start_p.insert("InstanceId.1".to_string(), instance_id.clone());
    let start_resp = p
        .dispatch(&make_ctx("StartInstances", start_p))
        .await
        .unwrap();
    assert_eq!(start_resp.status_code, 200);
    assert!(body_str(&start_resp).contains("running"));
}

// ---------------------------------------------------------------------------
// Key Pairs
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_create_and_describe_key_pair() {
    let p = Ec2Provider::new();
    let mut params = HashMap::new();
    params.insert("KeyName".to_string(), "my-key".to_string());
    let resp = p
        .dispatch(&make_ctx("CreateKeyPair", params))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
    let body = body_str(&resp);
    assert!(body.contains("<keyName>my-key</keyName>"));
    assert!(body.contains("<keyPairId>key-"));
    assert!(body.contains("BEGIN RSA PRIVATE KEY"));

    let desc = p
        .dispatch(&make_ctx("DescribeKeyPairs", HashMap::new()))
        .await
        .unwrap();
    assert_eq!(desc.status_code, 200);
    assert!(body_str(&desc).contains("my-key"));
}

#[tokio::test]
async fn test_create_key_pair_duplicate_fails() {
    let p = Ec2Provider::new();
    let mut params = HashMap::new();
    params.insert("KeyName".to_string(), "dup-key".to_string());
    p.dispatch(&make_ctx("CreateKeyPair", params.clone()))
        .await
        .unwrap();
    let resp = p
        .dispatch(&make_ctx("CreateKeyPair", params))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 400);
    assert!(body_str(&resp).contains("InvalidKeyPair.Duplicate"));
}

#[tokio::test]
async fn test_delete_key_pair() {
    let p = Ec2Provider::new();
    let mut params = HashMap::new();
    params.insert("KeyName".to_string(), "del-key".to_string());
    p.dispatch(&make_ctx("CreateKeyPair", params))
        .await
        .unwrap();

    let mut del = HashMap::new();
    del.insert("KeyName".to_string(), "del-key".to_string());
    let resp = p.dispatch(&make_ctx("DeleteKeyPair", del)).await.unwrap();
    assert_eq!(resp.status_code, 200);

    let desc = p
        .dispatch(&make_ctx("DescribeKeyPairs", HashMap::new()))
        .await
        .unwrap();
    assert!(!body_str(&desc).contains("del-key"));
}

// ---------------------------------------------------------------------------
// Elastic IPs
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_allocate_and_describe_address() {
    let p = Ec2Provider::new();
    let mut params = HashMap::new();
    params.insert("Domain".to_string(), "vpc".to_string());
    let resp = p
        .dispatch(&make_ctx("AllocateAddress", params))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
    let body = body_str(&resp);
    assert!(body.contains("<allocationId>eipalloc-"));
    assert!(body.contains("<domain>vpc</domain>"));

    let desc = p
        .dispatch(&make_ctx("DescribeAddresses", HashMap::new()))
        .await
        .unwrap();
    assert_eq!(desc.status_code, 200);
    assert!(body_str(&desc).contains("eipalloc-"));
}

#[tokio::test]
async fn test_associate_and_disassociate_address() {
    let p = Ec2Provider::new();
    let alloc_resp = p
        .dispatch(&make_ctx("AllocateAddress", HashMap::new()))
        .await
        .unwrap();
    let allocation_id = extract_tag(&body_str(&alloc_resp), "allocationId");

    let mut run_p = HashMap::new();
    run_p.insert("ImageId".to_string(), "ami-eip".to_string());
    run_p.insert("MaxCount".to_string(), "1".to_string());
    let run_resp = p.dispatch(&make_ctx("RunInstances", run_p)).await.unwrap();
    let instance_id = extract_tag(&body_str(&run_resp), "instanceId");

    let mut assoc_p = HashMap::new();
    assoc_p.insert("AllocationId".to_string(), allocation_id.clone());
    assoc_p.insert("InstanceId".to_string(), instance_id.clone());
    let assoc_resp = p
        .dispatch(&make_ctx("AssociateAddress", assoc_p))
        .await
        .unwrap();
    assert_eq!(assoc_resp.status_code, 200);
    let association_id = extract_tag(&body_str(&assoc_resp), "associationId");

    let mut disassoc_p = HashMap::new();
    disassoc_p.insert("AssociationId".to_string(), association_id);
    let dis_resp = p
        .dispatch(&make_ctx("DisassociateAddress", disassoc_p))
        .await
        .unwrap();
    assert_eq!(dis_resp.status_code, 200);
}

#[tokio::test]
async fn test_release_address() {
    let p = Ec2Provider::new();
    let alloc_resp = p
        .dispatch(&make_ctx("AllocateAddress", HashMap::new()))
        .await
        .unwrap();
    let allocation_id = extract_tag(&body_str(&alloc_resp), "allocationId");

    let mut rel = HashMap::new();
    rel.insert("AllocationId".to_string(), allocation_id.clone());
    let resp = p.dispatch(&make_ctx("ReleaseAddress", rel)).await.unwrap();
    assert_eq!(resp.status_code, 200);

    let desc = p
        .dispatch(&make_ctx("DescribeAddresses", HashMap::new()))
        .await
        .unwrap();
    assert!(!body_str(&desc).contains(&allocation_id));
}

// ---------------------------------------------------------------------------
// Internet Gateways
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_create_and_describe_internet_gateway() {
    let p = Ec2Provider::new();
    let resp = p
        .dispatch(&make_ctx("CreateInternetGateway", HashMap::new()))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
    let body = body_str(&resp);
    assert!(body.contains("<internetGatewayId>igw-"));

    let desc = p
        .dispatch(&make_ctx("DescribeInternetGateways", HashMap::new()))
        .await
        .unwrap();
    assert_eq!(desc.status_code, 200);
    assert!(body_str(&desc).contains("igw-"));
}

#[tokio::test]
async fn test_attach_and_detach_internet_gateway() {
    let p = Ec2Provider::new();
    let igw_resp = p
        .dispatch(&make_ctx("CreateInternetGateway", HashMap::new()))
        .await
        .unwrap();
    let igw_id = extract_tag(&body_str(&igw_resp), "internetGatewayId");

    let mut vpc_p = HashMap::new();
    vpc_p.insert("CidrBlock".to_string(), "10.20.0.0/16".to_string());
    let vpc_resp = p.dispatch(&make_ctx("CreateVpc", vpc_p)).await.unwrap();
    let vpc_id = extract_tag(&body_str(&vpc_resp), "vpcId");

    let mut attach_p = HashMap::new();
    attach_p.insert("InternetGatewayId".to_string(), igw_id.clone());
    attach_p.insert("VpcId".to_string(), vpc_id.clone());
    let attach_resp = p
        .dispatch(&make_ctx("AttachInternetGateway", attach_p))
        .await
        .unwrap();
    assert_eq!(attach_resp.status_code, 200);

    let desc = p
        .dispatch(&make_ctx("DescribeInternetGateways", HashMap::new()))
        .await
        .unwrap();
    assert!(body_str(&desc).contains(&vpc_id));

    let mut detach_p = HashMap::new();
    detach_p.insert("InternetGatewayId".to_string(), igw_id.clone());
    detach_p.insert("VpcId".to_string(), vpc_id.clone());
    let detach_resp = p
        .dispatch(&make_ctx("DetachInternetGateway", detach_p))
        .await
        .unwrap();
    assert_eq!(detach_resp.status_code, 200);
}

#[tokio::test]
async fn test_delete_internet_gateway() {
    let p = Ec2Provider::new();
    let igw_resp = p
        .dispatch(&make_ctx("CreateInternetGateway", HashMap::new()))
        .await
        .unwrap();
    let igw_id = extract_tag(&body_str(&igw_resp), "internetGatewayId");

    let mut del = HashMap::new();
    del.insert("InternetGatewayId".to_string(), igw_id.clone());
    let resp = p
        .dispatch(&make_ctx("DeleteInternetGateway", del))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);

    let desc = p
        .dispatch(&make_ctx("DescribeInternetGateways", HashMap::new()))
        .await
        .unwrap();
    assert!(!body_str(&desc).contains(&igw_id));
}

// ---------------------------------------------------------------------------
// EBS Volumes
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_create_and_describe_volume() {
    let p = Ec2Provider::new();
    let mut params = HashMap::new();
    params.insert("AvailabilityZone".to_string(), "us-east-1a".to_string());
    params.insert("Size".to_string(), "20".to_string());
    params.insert("VolumeType".to_string(), "gp3".to_string());
    let resp = p.dispatch(&make_ctx("CreateVolume", params)).await.unwrap();
    assert_eq!(resp.status_code, 200);
    let body = body_str(&resp);
    assert!(body.contains("<volumeId>vol-"));
    assert!(body.contains("<size>20</size>"));
    assert!(body.contains("gp3"));

    let desc = p
        .dispatch(&make_ctx("DescribeVolumes", HashMap::new()))
        .await
        .unwrap();
    assert!(body_str(&desc).contains("vol-"));
}

#[tokio::test]
async fn test_attach_and_detach_volume() {
    let p = Ec2Provider::new();
    let mut run_p = HashMap::new();
    run_p.insert("ImageId".to_string(), "ami-vol".to_string());
    run_p.insert("MaxCount".to_string(), "1".to_string());
    let run_resp = p.dispatch(&make_ctx("RunInstances", run_p)).await.unwrap();
    let instance_id = extract_tag(&body_str(&run_resp), "instanceId");

    let vol_resp = p
        .dispatch(&make_ctx("CreateVolume", HashMap::new()))
        .await
        .unwrap();
    let volume_id = extract_tag(&body_str(&vol_resp), "volumeId");

    let mut attach_p = HashMap::new();
    attach_p.insert("VolumeId".to_string(), volume_id.clone());
    attach_p.insert("InstanceId".to_string(), instance_id.clone());
    attach_p.insert("Device".to_string(), "/dev/xvdf".to_string());
    let attach_resp = p
        .dispatch(&make_ctx("AttachVolume", attach_p))
        .await
        .unwrap();
    assert_eq!(attach_resp.status_code, 200);
    assert!(body_str(&attach_resp).contains("attached"));

    let mut detach_p = HashMap::new();
    detach_p.insert("VolumeId".to_string(), volume_id.clone());
    let detach_resp = p
        .dispatch(&make_ctx("DetachVolume", detach_p))
        .await
        .unwrap();
    assert_eq!(detach_resp.status_code, 200);
    assert!(body_str(&detach_resp).contains("detached"));
}

#[tokio::test]
async fn test_delete_volume() {
    let p = Ec2Provider::new();
    let vol_resp = p
        .dispatch(&make_ctx("CreateVolume", HashMap::new()))
        .await
        .unwrap();
    let volume_id = extract_tag(&body_str(&vol_resp), "volumeId");

    let mut del = HashMap::new();
    del.insert("VolumeId".to_string(), volume_id.clone());
    let resp = p.dispatch(&make_ctx("DeleteVolume", del)).await.unwrap();
    assert_eq!(resp.status_code, 200);

    let desc = p
        .dispatch(&make_ctx("DescribeVolumes", HashMap::new()))
        .await
        .unwrap();
    assert!(!body_str(&desc).contains(&volume_id));
}

// ---------------------------------------------------------------------------
// Tags
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_create_and_describe_tags() {
    let p = Ec2Provider::new();
    let mut vpc_p = HashMap::new();
    vpc_p.insert("CidrBlock".to_string(), "10.30.0.0/16".to_string());
    let vpc_resp = p.dispatch(&make_ctx("CreateVpc", vpc_p)).await.unwrap();
    let vpc_id = extract_tag(&body_str(&vpc_resp), "vpcId");

    let mut tag_p = HashMap::new();
    tag_p.insert("ResourceId.1".to_string(), vpc_id.clone());
    tag_p.insert("Tag.1.Key".to_string(), "Name".to_string());
    tag_p.insert("Tag.1.Value".to_string(), "my-vpc".to_string());
    tag_p.insert("Tag.2.Key".to_string(), "Env".to_string());
    tag_p.insert("Tag.2.Value".to_string(), "test".to_string());
    let resp = p.dispatch(&make_ctx("CreateTags", tag_p)).await.unwrap();
    assert_eq!(resp.status_code, 200);

    let desc = p
        .dispatch(&make_ctx("DescribeTags", HashMap::new()))
        .await
        .unwrap();
    let body = body_str(&desc);
    assert!(body.contains("<key>Name</key>"));
    assert!(body.contains("<value>my-vpc</value>"));
    assert!(body.contains("<key>Env</key>"));
}

// ---------------------------------------------------------------------------
// DescribeAvailabilityZones / DescribeRegions
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_describe_availability_zones() {
    let p = Ec2Provider::new();
    let resp = p
        .dispatch(&make_ctx("DescribeAvailabilityZones", HashMap::new()))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
    let body = body_str(&resp);
    assert!(body.contains("DescribeAvailabilityZonesResponse"));
    assert!(body.contains("us-east-1a"));
    assert!(body.contains("us-east-1b"));
    assert!(body.contains("available"));
}

#[tokio::test]
async fn test_describe_regions() {
    let p = Ec2Provider::new();
    let resp = p
        .dispatch(&make_ctx("DescribeRegions", HashMap::new()))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
    let body = body_str(&resp);
    assert!(body.contains("DescribeRegionsResponse"));
    assert!(body.contains("us-east-1"));
    assert!(body.contains("eu-west-1"));
}

// ---------------------------------------------------------------------------
// Test helper
// ---------------------------------------------------------------------------

fn extract_tag(xml: &str, tag: &str) -> String {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let start = xml.find(&open).unwrap_or(0) + open.len();
    let end = xml.find(&close).unwrap_or(xml.len());
    xml[start..end].to_string()
}
