use std::borrow::Cow;
use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use chrono::Utc;
use openstack_service_framework::traits::{
    DispatchError, DispatchResponse, RequestContext, ResponseBody, ServiceProvider,
};
use openstack_state::AccountRegionBundle;
use uuid::Uuid;

use crate::store::{
    Address, Ec2Store, IgwAttachment, Instance, InternetGateway, IpPermission, KeyPair,
    SecurityGroup, Subnet, Volume, VolumeAttachment, Vpc,
};

pub struct Ec2Provider {
    store: Arc<AccountRegionBundle<Ec2Store>>,
}

impl Ec2Provider {
    pub fn new() -> Self {
        Self {
            store: Arc::new(AccountRegionBundle::new()),
        }
    }
}

impl Default for Ec2Provider {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Helpers — EC2 uses query protocol (XML responses, Action= param)
// ---------------------------------------------------------------------------

fn xml_ok(action: &str, request_id: &str, inner: &str) -> DispatchResponse {
    let xml = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
<{action}Response xmlns=\"http://ec2.amazonaws.com/doc/2016-11-15/\">\
{inner}\
<requestId>{request_id}</requestId>\
</{action}Response>"
    );
    DispatchResponse {
        status_code: 200,
        body: ResponseBody::Buffered(Bytes::from(xml.into_bytes())),
        content_type: Cow::Borrowed("text/xml"),
        headers: Vec::new(),
    }
}

fn xml_error(code: &str, message: &str, status: u16) -> DispatchResponse {
    let xml = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
<Response><Errors><Error>\
<Code>{code}</Code><Message>{message}</Message>\
</Error></Errors><RequestID>{}</RequestID></Response>",
        req_id()
    );
    DispatchResponse {
        status_code: status,
        body: ResponseBody::Buffered(Bytes::from(xml.into_bytes())),
        content_type: Cow::Borrowed("text/xml"),
        headers: Vec::new(),
    }
}

fn req_id() -> String {
    Uuid::new_v4().to_string()
}

fn short_id() -> String {
    Uuid::new_v4().to_string().replace('-', "")[..12].to_string()
}

fn str_param<'a>(ctx: &'a RequestContext, key: &str) -> Option<&'a str> {
    ctx.query_params
        .get(key)
        .map(|s| s.as_str())
        .or_else(|| ctx.request_body.get(key).and_then(|v| v.as_str()))
}

fn tags_xml(tags: &std::collections::HashMap<String, String>) -> String {
    if tags.is_empty() {
        return "<tagSet/>".to_string();
    }
    let items: String = tags
        .iter()
        .map(|(k, v)| format!("<item><key>{k}</key><value>{v}</value></item>"))
        .collect();
    format!("<tagSet>{items}</tagSet>")
}

fn instance_state_code(state: &str) -> u16 {
    match state {
        "running" => 16,
        "stopped" => 80,
        "terminated" => 48,
        "stopping" => 64,
        "pending" => 0,
        _ => 16,
    }
}

// ---------------------------------------------------------------------------
// ServiceProvider
// ---------------------------------------------------------------------------

#[async_trait]
impl ServiceProvider for Ec2Provider {
    fn service_name(&self) -> &str {
        "ec2"
    }

    async fn dispatch(&self, ctx: &RequestContext) -> Result<DispatchResponse, DispatchError> {
        let region = &ctx.region;
        let account_id = &ctx.account_id;
        let rid = req_id();

        match ctx.operation.as_str() {
            // ----------------------------------------------------------------
            // CreateVpc
            // ----------------------------------------------------------------
            "CreateVpc" => {
                let cidr = str_param(ctx, "CidrBlock")
                    .unwrap_or("10.0.0.0/16")
                    .to_string();
                let vpc_id = format!("vpc-{}", short_id());
                let vpc = Vpc {
                    vpc_id: vpc_id.clone(),
                    cidr_block: cidr.clone(),
                    state: "available".to_string(),
                    is_default: false,
                    tags: Default::default(),
                };
                let mut store = self.store.get_or_create(account_id, region);
                store.vpcs.insert(vpc_id.clone(), vpc);
                let inner = format!(
                    "<vpc>\
<vpcId>{vpc_id}</vpcId>\
<cidrBlock>{cidr}</cidrBlock>\
<state>available</state>\
<isDefault>false</isDefault>\
<tagSet/>\
</vpc>"
                );
                Ok(xml_ok("CreateVpc", &rid, &inner))
            }

            // ----------------------------------------------------------------
            // DescribeVpcs
            // ----------------------------------------------------------------
            "DescribeVpcs" => {
                let Some(store) = self.store.get(account_id, region) else {
                    return Ok(xml_ok("DescribeVpcs", &rid, "<vpcSet></vpcSet>"));
                };
                let items: String = store
                    .vpcs
                    .values()
                    .map(|v| {
                        format!(
                            "<item>\
<vpcId>{}</vpcId>\
<cidrBlock>{}</cidrBlock>\
<state>{}</state>\
<isDefault>{}</isDefault>\
{}\
</item>",
                            v.vpc_id,
                            v.cidr_block,
                            v.state,
                            v.is_default,
                            tags_xml(&v.tags)
                        )
                    })
                    .collect();
                let inner = format!("<vpcSet>{items}</vpcSet>");
                Ok(xml_ok("DescribeVpcs", &rid, &inner))
            }

            // ----------------------------------------------------------------
            // DeleteVpc
            // ----------------------------------------------------------------
            "DeleteVpc" => {
                let vpc_id = match str_param(ctx, "VpcId") {
                    Some(id) => id.to_string(),
                    None => return Ok(xml_error("MissingParameter", "VpcId required", 400)),
                };
                let mut store = self.store.get_or_create(account_id, region);
                store.vpcs.remove(&vpc_id);
                Ok(xml_ok("DeleteVpc", &rid, "<return>true</return>"))
            }

            // ----------------------------------------------------------------
            // CreateSubnet
            // ----------------------------------------------------------------
            "CreateSubnet" => {
                let vpc_id = match str_param(ctx, "VpcId") {
                    Some(id) => id.to_string(),
                    None => return Ok(xml_error("MissingParameter", "VpcId required", 400)),
                };
                let cidr = str_param(ctx, "CidrBlock")
                    .unwrap_or("10.0.1.0/24")
                    .to_string();
                let az = format!("{region}a");
                let az = str_param(ctx, "AvailabilityZone")
                    .unwrap_or(&az)
                    .to_string();
                let subnet_id = format!("subnet-{}", short_id());
                let subnet = Subnet {
                    subnet_id: subnet_id.clone(),
                    vpc_id: vpc_id.clone(),
                    cidr_block: cidr.clone(),
                    availability_zone: az.clone(),
                    state: "available".to_string(),
                    tags: Default::default(),
                };
                let mut store = self.store.get_or_create(account_id, region);
                store.subnets.insert(subnet_id.clone(), subnet);
                let inner = format!(
                    "<subnet>\
<subnetId>{subnet_id}</subnetId>\
<vpcId>{vpc_id}</vpcId>\
<cidrBlock>{cidr}</cidrBlock>\
<availabilityZone>{az}</availabilityZone>\
<state>available</state>\
<tagSet/>\
</subnet>"
                );
                Ok(xml_ok("CreateSubnet", &rid, &inner))
            }

            // ----------------------------------------------------------------
            // DescribeSubnets
            // ----------------------------------------------------------------
            "DescribeSubnets" => {
                let Some(store) = self.store.get(account_id, region) else {
                    return Ok(xml_ok("DescribeSubnets", &rid, "<subnetSet></subnetSet>"));
                };
                let items: String = store
                    .subnets
                    .values()
                    .map(|s| {
                        format!(
                            "<item>\
<subnetId>{}</subnetId>\
<vpcId>{}</vpcId>\
<cidrBlock>{}</cidrBlock>\
<availabilityZone>{}</availabilityZone>\
<state>{}</state>\
{}\
</item>",
                            s.subnet_id,
                            s.vpc_id,
                            s.cidr_block,
                            s.availability_zone,
                            s.state,
                            tags_xml(&s.tags)
                        )
                    })
                    .collect();
                let inner = format!("<subnetSet>{items}</subnetSet>");
                Ok(xml_ok("DescribeSubnets", &rid, &inner))
            }

            // ----------------------------------------------------------------
            // DeleteSubnet
            // ----------------------------------------------------------------
            "DeleteSubnet" => {
                let subnet_id = match str_param(ctx, "SubnetId") {
                    Some(id) => id.to_string(),
                    None => return Ok(xml_error("MissingParameter", "SubnetId required", 400)),
                };
                let mut store = self.store.get_or_create(account_id, region);
                if store.subnets.remove(&subnet_id).is_none() {
                    return Ok(xml_error(
                        "InvalidSubnetID.NotFound",
                        &format!("Subnet {subnet_id} not found"),
                        400,
                    ));
                }
                Ok(xml_ok("DeleteSubnet", &rid, "<return>true</return>"))
            }

            // ----------------------------------------------------------------
            // CreateSecurityGroup
            // ----------------------------------------------------------------
            "CreateSecurityGroup" => {
                let group_name = match str_param(ctx, "GroupName") {
                    Some(n) => n.to_string(),
                    None => return Ok(xml_error("MissingParameter", "GroupName required", 400)),
                };
                let description = str_param(ctx, "Description").unwrap_or("").to_string();
                let vpc_id = str_param(ctx, "VpcId").unwrap_or("").to_string();
                let group_id = format!("sg-{}", short_id());
                let sg = SecurityGroup {
                    group_id: group_id.clone(),
                    group_name,
                    description,
                    vpc_id,
                    ingress_rules: Vec::new(),
                    egress_rules: Vec::new(),
                    tags: Default::default(),
                };
                let mut store = self.store.get_or_create(account_id, region);
                store.security_groups.insert(group_id.clone(), sg);
                let inner = format!("<groupId>{group_id}</groupId><return>true</return>");
                Ok(xml_ok("CreateSecurityGroup", &rid, &inner))
            }

            // ----------------------------------------------------------------
            // DescribeSecurityGroups
            // ----------------------------------------------------------------
            "DescribeSecurityGroups" => {
                let Some(store) = self.store.get(account_id, region) else {
                    return Ok(xml_ok(
                        "DescribeSecurityGroups",
                        &rid,
                        "<securityGroupInfo></securityGroupInfo>",
                    ));
                };
                let items: String = store
                    .security_groups
                    .values()
                    .map(|sg| {
                        let ingress: String = sg
                            .ingress_rules
                            .iter()
                            .map(|r| {
                                let ranges: String = r
                                    .ip_ranges
                                    .iter()
                                    .map(|ip| format!("<item><cidrIp>{ip}</cidrIp></item>"))
                                    .collect();
                                format!(
                                    "<item>\
<ipProtocol>{}</ipProtocol>\
<fromPort>{}</fromPort>\
<toPort>{}</toPort>\
<ipRanges>{ranges}</ipRanges>\
</item>",
                                    r.ip_protocol, r.from_port, r.to_port
                                )
                            })
                            .collect();
                        format!(
                            "<item>\
<groupId>{}</groupId>\
<groupName>{}</groupName>\
<groupDescription>{}</groupDescription>\
<vpcId>{}</vpcId>\
<ipPermissions>{ingress}</ipPermissions>\
{}\
</item>",
                            sg.group_id,
                            sg.group_name,
                            sg.description,
                            sg.vpc_id,
                            tags_xml(&sg.tags)
                        )
                    })
                    .collect();
                let inner = format!("<securityGroupInfo>{items}</securityGroupInfo>");
                Ok(xml_ok("DescribeSecurityGroups", &rid, &inner))
            }

            // ----------------------------------------------------------------
            // DeleteSecurityGroup
            // ----------------------------------------------------------------
            "DeleteSecurityGroup" => {
                let group_id = match str_param(ctx, "GroupId") {
                    Some(id) => id.to_string(),
                    None => return Ok(xml_error("MissingParameter", "GroupId required", 400)),
                };
                let mut store = self.store.get_or_create(account_id, region);
                if store.security_groups.remove(&group_id).is_none() {
                    return Ok(xml_error(
                        "InvalidGroup.NotFound",
                        &format!("Security group {group_id} not found"),
                        400,
                    ));
                }
                Ok(xml_ok("DeleteSecurityGroup", &rid, "<return>true</return>"))
            }

            // ----------------------------------------------------------------
            // AuthorizeSecurityGroupIngress
            // ----------------------------------------------------------------
            "AuthorizeSecurityGroupIngress" => {
                let group_id = match str_param(ctx, "GroupId") {
                    Some(id) => id.to_string(),
                    None => return Ok(xml_error("MissingParameter", "GroupId required", 400)),
                };
                let ip_protocol = str_param(ctx, "IpPermissions.1.IpProtocol")
                    .or_else(|| str_param(ctx, "IpProtocol"))
                    .unwrap_or("tcp")
                    .to_string();
                let from_port = str_param(ctx, "IpPermissions.1.FromPort")
                    .or_else(|| str_param(ctx, "FromPort"))
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(0i32);
                let to_port = str_param(ctx, "IpPermissions.1.ToPort")
                    .or_else(|| str_param(ctx, "ToPort"))
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(65535i32);
                let cidr = str_param(ctx, "IpPermissions.1.IpRanges.1.CidrIp")
                    .or_else(|| str_param(ctx, "CidrIp"))
                    .unwrap_or("0.0.0.0/0")
                    .to_string();
                let rule = IpPermission {
                    ip_protocol,
                    from_port,
                    to_port,
                    ip_ranges: vec![cidr],
                };
                let mut store = self.store.get_or_create(account_id, region);
                if let Some(sg) = store.security_groups.get_mut(&group_id) {
                    sg.ingress_rules.push(rule);
                    Ok(xml_ok(
                        "AuthorizeSecurityGroupIngress",
                        &rid,
                        "<return>true</return>",
                    ))
                } else {
                    Ok(xml_error(
                        "InvalidGroup.NotFound",
                        "Security group not found",
                        400,
                    ))
                }
            }

            // ----------------------------------------------------------------
            // AuthorizeSecurityGroupEgress
            // ----------------------------------------------------------------
            "AuthorizeSecurityGroupEgress" => {
                let group_id = match str_param(ctx, "GroupId") {
                    Some(id) => id.to_string(),
                    None => return Ok(xml_error("MissingParameter", "GroupId required", 400)),
                };
                let ip_protocol = str_param(ctx, "IpPermissions.1.IpProtocol")
                    .or_else(|| str_param(ctx, "IpProtocol"))
                    .unwrap_or("-1")
                    .to_string();
                let from_port = str_param(ctx, "IpPermissions.1.FromPort")
                    .or_else(|| str_param(ctx, "FromPort"))
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(0i32);
                let to_port = str_param(ctx, "IpPermissions.1.ToPort")
                    .or_else(|| str_param(ctx, "ToPort"))
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(65535i32);
                let cidr = str_param(ctx, "IpPermissions.1.IpRanges.1.CidrIp")
                    .or_else(|| str_param(ctx, "CidrIp"))
                    .unwrap_or("0.0.0.0/0")
                    .to_string();
                let rule = IpPermission {
                    ip_protocol,
                    from_port,
                    to_port,
                    ip_ranges: vec![cidr],
                };
                let mut store = self.store.get_or_create(account_id, region);
                if let Some(sg) = store.security_groups.get_mut(&group_id) {
                    sg.egress_rules.push(rule);
                    Ok(xml_ok(
                        "AuthorizeSecurityGroupEgress",
                        &rid,
                        "<return>true</return>",
                    ))
                } else {
                    Ok(xml_error(
                        "InvalidGroup.NotFound",
                        "Security group not found",
                        400,
                    ))
                }
            }

            // ----------------------------------------------------------------
            // RunInstances
            // ----------------------------------------------------------------
            "RunInstances" => {
                let image_id = str_param(ctx, "ImageId")
                    .unwrap_or("ami-00000000")
                    .to_string();
                let instance_type = str_param(ctx, "InstanceType")
                    .unwrap_or("t2.micro")
                    .to_string();
                let subnet_id = str_param(ctx, "SubnetId").unwrap_or("").to_string();
                let key_name = str_param(ctx, "KeyName").map(|s| s.to_string());
                let max_count: u32 = str_param(ctx, "MaxCount")
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(1);

                let vpc_id = {
                    let store = self.store.get(account_id, region);
                    store
                        .as_ref()
                        .and_then(|s| s.subnets.get(&subnet_id))
                        .map(|s| s.vpc_id.clone())
                        .unwrap_or_default()
                };

                let mut instance_items = String::new();
                let mut store = self.store.get_or_create(account_id, region);
                for _ in 0..max_count {
                    let instance_id = format!("i-{}", short_id());
                    let private_ip = format!("10.0.0.{}", (store.instances.len() + 1) % 254 + 1);
                    let key_xml = key_name
                        .as_deref()
                        .map(|k| format!("<keyName>{k}</keyName>"))
                        .unwrap_or_default();
                    let inst = Instance {
                        instance_id: instance_id.clone(),
                        image_id: image_id.clone(),
                        instance_type: instance_type.clone(),
                        state: "running".to_string(),
                        subnet_id: subnet_id.clone(),
                        vpc_id: vpc_id.clone(),
                        private_ip: private_ip.clone(),
                        key_name: key_name.clone(),
                        tags: Default::default(),
                    };
                    store.instances.insert(instance_id.clone(), inst);
                    instance_items.push_str(&format!(
                        "<item>\
<instanceId>{instance_id}</instanceId>\
<imageId>{image_id}</imageId>\
<instanceType>{instance_type}</instanceType>\
{key_xml}\
<instanceState><code>16</code><name>running</name></instanceState>\
<privateIpAddress>{private_ip}</privateIpAddress>\
<tagSet/>\
</item>"
                    ));
                }
                let inner = format!("<instancesSet>{instance_items}</instancesSet>");
                Ok(xml_ok("RunInstances", &rid, &inner))
            }

            // ----------------------------------------------------------------
            // DescribeInstances
            // ----------------------------------------------------------------
            "DescribeInstances" => {
                let Some(store) = self.store.get(account_id, region) else {
                    return Ok(xml_ok(
                        "DescribeInstances",
                        &rid,
                        "<reservationSet></reservationSet>",
                    ));
                };
                let instance_items: String = store
                    .instances
                    .values()
                    .filter(|i| i.state != "terminated")
                    .map(|i| {
                        let state_code = instance_state_code(&i.state);
                        let key_xml = i
                            .key_name
                            .as_deref()
                            .map(|k| format!("<keyName>{k}</keyName>"))
                            .unwrap_or_default();
                        format!(
                            "<item>\
<instanceId>{}</instanceId>\
<imageId>{}</imageId>\
<instanceType>{}</instanceType>\
{key_xml}\
<instanceState><code>{state_code}</code><name>{}</name></instanceState>\
<privateIpAddress>{}</privateIpAddress>\
{}\
</item>",
                            i.instance_id,
                            i.image_id,
                            i.instance_type,
                            i.state,
                            i.private_ip,
                            tags_xml(&i.tags)
                        )
                    })
                    .collect();
                let inner = format!(
                    "<reservationSet><item><instancesSet>{instance_items}</instancesSet></item></reservationSet>"
                );
                Ok(xml_ok("DescribeInstances", &rid, &inner))
            }

            // ----------------------------------------------------------------
            // StopInstances
            // ----------------------------------------------------------------
            "StopInstances" => {
                let ids = collect_indexed_params(ctx, "InstanceId");
                let mut items_xml = String::new();
                let mut store = self.store.get_or_create(account_id, region);
                for id in &ids {
                    if let Some(inst) = store.instances.get_mut(id) {
                        let prev = inst.state.clone();
                        inst.state = "stopped".to_string();
                        items_xml.push_str(&format!(
                            "<item><instanceId>{}</instanceId>\
<currentState><code>80</code><name>stopped</name></currentState>\
<previousState><code>{}</code><name>{prev}</name></previousState>\
</item>",
                            inst.instance_id,
                            instance_state_code(&prev)
                        ));
                    }
                }
                let inner = format!("<instancesSet>{items_xml}</instancesSet>");
                Ok(xml_ok("StopInstances", &rid, &inner))
            }

            // ----------------------------------------------------------------
            // StartInstances
            // ----------------------------------------------------------------
            "StartInstances" => {
                let ids = collect_indexed_params(ctx, "InstanceId");
                let mut items_xml = String::new();
                let mut store = self.store.get_or_create(account_id, region);
                for id in &ids {
                    if let Some(inst) = store.instances.get_mut(id) {
                        let prev = inst.state.clone();
                        inst.state = "running".to_string();
                        items_xml.push_str(&format!(
                            "<item><instanceId>{}</instanceId>\
<currentState><code>16</code><name>running</name></currentState>\
<previousState><code>{}</code><name>{prev}</name></previousState>\
</item>",
                            inst.instance_id,
                            instance_state_code(&prev)
                        ));
                    }
                }
                let inner = format!("<instancesSet>{items_xml}</instancesSet>");
                Ok(xml_ok("StartInstances", &rid, &inner))
            }

            // ----------------------------------------------------------------
            // TerminateInstances
            // ----------------------------------------------------------------
            "TerminateInstances" => {
                let ids = collect_indexed_params(ctx, "InstanceId");
                let mut items_xml = String::new();
                let Some(existing_store) = self.store.get(account_id, region) else {
                    if let Some(id) = ids.first() {
                        return Ok(xml_error(
                            "InvalidInstanceID.NotFound",
                            &format!("The instance ID '{id}' does not exist"),
                            400,
                        ));
                    }
                    return Ok(xml_ok(
                        "TerminateInstances",
                        &rid,
                        "<instancesSet></instancesSet>",
                    ));
                };
                for id in &ids {
                    if !existing_store.instances.contains_key(id) {
                        return Ok(xml_error(
                            "InvalidInstanceID.NotFound",
                            &format!("The instance ID '{id}' does not exist"),
                            400,
                        ));
                    }
                }
                drop(existing_store);
                let mut store = self.store.get_or_create(account_id, region);
                for id in &ids {
                    if let Some(inst) = store.instances.get_mut(id) {
                        inst.state = "terminated".to_string();
                        items_xml.push_str(&format!(
                            "<item><instanceId>{}</instanceId>\
<currentState><code>48</code><name>terminated</name></currentState>\
<previousState><code>16</code><name>running</name></previousState>\
</item>",
                            inst.instance_id
                        ));
                    }
                }
                let inner = format!("<instancesSet>{items_xml}</instancesSet>");
                Ok(xml_ok("TerminateInstances", &rid, &inner))
            }

            // ----------------------------------------------------------------
            // CreateKeyPair
            // ----------------------------------------------------------------
            "CreateKeyPair" => {
                let key_name = match str_param(ctx, "KeyName") {
                    Some(n) => n.to_string(),
                    None => return Ok(xml_error("MissingParameter", "KeyName required", 400)),
                };
                let mut store = self.store.get_or_create(account_id, region);
                if store.key_pairs.values().any(|kp| kp.key_name == key_name) {
                    return Ok(xml_error(
                        "InvalidKeyPair.Duplicate",
                        &format!("Key pair '{key_name}' already exists"),
                        400,
                    ));
                }
                let key_pair_id = format!("key-{}", short_id());
                let fingerprint = format!(
                    "{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
                    rand_byte(),
                    rand_byte(),
                    rand_byte(),
                    rand_byte(),
                    rand_byte(),
                    rand_byte()
                );
                let key_material = format!(
                    "-----BEGIN RSA PRIVATE KEY-----\nMIIFake{key_name}\n-----END RSA PRIVATE KEY-----"
                );
                let kp = KeyPair {
                    key_pair_id: key_pair_id.clone(),
                    key_name: key_name.clone(),
                    key_fingerprint: fingerprint.clone(),
                    key_material: Some(key_material.clone()),
                    tags: Default::default(),
                    created: Utc::now(),
                };
                store.key_pairs.insert(key_pair_id.clone(), kp);
                let inner = format!(
                    "<keyName>{key_name}</keyName>\
<keyFingerprint>{fingerprint}</keyFingerprint>\
<keyMaterial>{key_material}</keyMaterial>\
<keyPairId>{key_pair_id}</keyPairId>"
                );
                Ok(xml_ok("CreateKeyPair", &rid, &inner))
            }

            // ----------------------------------------------------------------
            // DeleteKeyPair
            // ----------------------------------------------------------------
            "DeleteKeyPair" => {
                let key_name = str_param(ctx, "KeyName").map(|s| s.to_string());
                let key_pair_id = str_param(ctx, "KeyPairId").map(|s| s.to_string());
                let mut store = self.store.get_or_create(account_id, region);
                if let Some(id) = key_pair_id {
                    store.key_pairs.remove(&id);
                } else if let Some(name) = key_name {
                    let found_id = store
                        .key_pairs
                        .values()
                        .find(|kp| kp.key_name == name)
                        .map(|kp| kp.key_pair_id.clone());
                    if let Some(id) = found_id {
                        store.key_pairs.remove(&id);
                    }
                }
                Ok(xml_ok("DeleteKeyPair", &rid, "<return>true</return>"))
            }

            // ----------------------------------------------------------------
            // DescribeKeyPairs
            // ----------------------------------------------------------------
            "DescribeKeyPairs" => {
                let Some(store) = self.store.get(account_id, region) else {
                    return Ok(xml_ok("DescribeKeyPairs", &rid, "<keySet></keySet>"));
                };
                let items: String = store
                    .key_pairs
                    .values()
                    .map(|kp| {
                        format!(
                            "<item>\
<keyPairId>{}</keyPairId>\
<keyName>{}</keyName>\
<keyFingerprint>{}</keyFingerprint>\
{}\
</item>",
                            kp.key_pair_id,
                            kp.key_name,
                            kp.key_fingerprint,
                            tags_xml(&kp.tags)
                        )
                    })
                    .collect();
                let inner = format!("<keySet>{items}</keySet>");
                Ok(xml_ok("DescribeKeyPairs", &rid, &inner))
            }

            // ----------------------------------------------------------------
            // AllocateAddress
            // ----------------------------------------------------------------
            "AllocateAddress" => {
                let domain = str_param(ctx, "Domain").unwrap_or("vpc").to_string();
                let allocation_id = format!("eipalloc-{}", short_id());
                let public_ip = format!("54.{}.{}.{}", rand_byte(), rand_byte(), rand_byte());
                let addr = Address {
                    allocation_id: allocation_id.clone(),
                    public_ip: public_ip.clone(),
                    domain: domain.clone(),
                    instance_id: None,
                    association_id: None,
                    network_interface_id: None,
                    private_ip: None,
                    tags: Default::default(),
                };
                let mut store = self.store.get_or_create(account_id, region);
                store.addresses.insert(allocation_id.clone(), addr);
                let inner = format!(
                    "<publicIp>{public_ip}</publicIp>\
<allocationId>{allocation_id}</allocationId>\
<domain>{domain}</domain>"
                );
                Ok(xml_ok("AllocateAddress", &rid, &inner))
            }

            // ----------------------------------------------------------------
            // ReleaseAddress
            // ----------------------------------------------------------------
            "ReleaseAddress" => {
                let allocation_id = match str_param(ctx, "AllocationId") {
                    Some(id) => id.to_string(),
                    None => return Ok(xml_error("MissingParameter", "AllocationId required", 400)),
                };
                let mut store = self.store.get_or_create(account_id, region);
                if store.addresses.remove(&allocation_id).is_none() {
                    return Ok(xml_error(
                        "InvalidAllocationID.NotFound",
                        &format!("Allocation {allocation_id} not found"),
                        400,
                    ));
                }
                Ok(xml_ok("ReleaseAddress", &rid, "<return>true</return>"))
            }

            // ----------------------------------------------------------------
            // DescribeAddresses
            // ----------------------------------------------------------------
            "DescribeAddresses" => {
                let Some(store) = self.store.get(account_id, region) else {
                    return Ok(xml_ok(
                        "DescribeAddresses",
                        &rid,
                        "<addressesSet></addressesSet>",
                    ));
                };
                let items: String = store
                    .addresses
                    .values()
                    .map(|a| {
                        let instance_xml = a
                            .instance_id
                            .as_deref()
                            .map(|i| format!("<instanceId>{i}</instanceId>"))
                            .unwrap_or_default();
                        let assoc_xml = a
                            .association_id
                            .as_deref()
                            .map(|i| format!("<associationId>{i}</associationId>"))
                            .unwrap_or_default();
                        format!(
                            "<item>\
<publicIp>{}</publicIp>\
<allocationId>{}</allocationId>\
<domain>{}</domain>\
{instance_xml}{assoc_xml}\
{}\
</item>",
                            a.public_ip,
                            a.allocation_id,
                            a.domain,
                            tags_xml(&a.tags)
                        )
                    })
                    .collect();
                let inner = format!("<addressesSet>{items}</addressesSet>");
                Ok(xml_ok("DescribeAddresses", &rid, &inner))
            }

            // ----------------------------------------------------------------
            // AssociateAddress
            // ----------------------------------------------------------------
            "AssociateAddress" => {
                let allocation_id = match str_param(ctx, "AllocationId") {
                    Some(id) => id.to_string(),
                    None => return Ok(xml_error("MissingParameter", "AllocationId required", 400)),
                };
                let instance_id = str_param(ctx, "InstanceId").map(|s| s.to_string());
                let association_id = format!("eipassoc-{}", short_id());
                let mut store = self.store.get_or_create(account_id, region);
                match store.addresses.get_mut(&allocation_id) {
                    Some(addr) => {
                        addr.instance_id = instance_id;
                        addr.association_id = Some(association_id.clone());
                        let inner = format!("<associationId>{association_id}</associationId>");
                        Ok(xml_ok("AssociateAddress", &rid, &inner))
                    }
                    None => Ok(xml_error(
                        "InvalidAllocationID.NotFound",
                        &format!("Allocation {allocation_id} not found"),
                        400,
                    )),
                }
            }

            // ----------------------------------------------------------------
            // DisassociateAddress
            // ----------------------------------------------------------------
            "DisassociateAddress" => {
                let association_id = match str_param(ctx, "AssociationId") {
                    Some(id) => id.to_string(),
                    None => {
                        return Ok(xml_error("MissingParameter", "AssociationId required", 400));
                    }
                };
                let mut store = self.store.get_or_create(account_id, region);
                let addr = store
                    .addresses
                    .values_mut()
                    .find(|a| a.association_id.as_deref() == Some(&association_id));
                if let Some(a) = addr {
                    a.instance_id = None;
                    a.association_id = None;
                }
                Ok(xml_ok("DisassociateAddress", &rid, "<return>true</return>"))
            }

            // ----------------------------------------------------------------
            // CreateInternetGateway
            // ----------------------------------------------------------------
            "CreateInternetGateway" => {
                let igw_id = format!("igw-{}", short_id());
                let igw = InternetGateway {
                    internet_gateway_id: igw_id.clone(),
                    state: "detached".to_string(),
                    attachments: Vec::new(),
                    tags: Default::default(),
                };
                let mut store = self.store.get_or_create(account_id, region);
                store.internet_gateways.insert(igw_id.clone(), igw);
                let inner = format!(
                    "<internetGateway>\
<internetGatewayId>{igw_id}</internetGatewayId>\
<attachmentSet/>\
<tagSet/>\
</internetGateway>"
                );
                Ok(xml_ok("CreateInternetGateway", &rid, &inner))
            }

            // ----------------------------------------------------------------
            // DeleteInternetGateway
            // ----------------------------------------------------------------
            "DeleteInternetGateway" => {
                let igw_id = match str_param(ctx, "InternetGatewayId") {
                    Some(id) => id.to_string(),
                    None => {
                        return Ok(xml_error(
                            "MissingParameter",
                            "InternetGatewayId required",
                            400,
                        ));
                    }
                };
                let mut store = self.store.get_or_create(account_id, region);
                if store.internet_gateways.remove(&igw_id).is_none() {
                    return Ok(xml_error(
                        "InvalidInternetGatewayID.NotFound",
                        &format!("Internet gateway {igw_id} not found"),
                        400,
                    ));
                }
                Ok(xml_ok(
                    "DeleteInternetGateway",
                    &rid,
                    "<return>true</return>",
                ))
            }

            // ----------------------------------------------------------------
            // DescribeInternetGateways
            // ----------------------------------------------------------------
            "DescribeInternetGateways" => {
                let Some(store) = self.store.get(account_id, region) else {
                    return Ok(xml_ok(
                        "DescribeInternetGateways",
                        &rid,
                        "<internetGatewaySet></internetGatewaySet>",
                    ));
                };
                let items: String = store
                    .internet_gateways
                    .values()
                    .map(|igw| {
                        let attachments: String = igw
                            .attachments
                            .iter()
                            .map(|a| {
                                format!(
                                    "<item><vpcId>{}</vpcId><state>{}</state></item>",
                                    a.vpc_id, a.state
                                )
                            })
                            .collect();
                        format!(
                            "<item>\
<internetGatewayId>{}</internetGatewayId>\
<attachmentSet>{attachments}</attachmentSet>\
{}\
</item>",
                            igw.internet_gateway_id,
                            tags_xml(&igw.tags)
                        )
                    })
                    .collect();
                let inner = format!("<internetGatewaySet>{items}</internetGatewaySet>");
                Ok(xml_ok("DescribeInternetGateways", &rid, &inner))
            }

            // ----------------------------------------------------------------
            // AttachInternetGateway
            // ----------------------------------------------------------------
            "AttachInternetGateway" => {
                let igw_id = match str_param(ctx, "InternetGatewayId") {
                    Some(id) => id.to_string(),
                    None => {
                        return Ok(xml_error(
                            "MissingParameter",
                            "InternetGatewayId required",
                            400,
                        ));
                    }
                };
                let vpc_id = match str_param(ctx, "VpcId") {
                    Some(id) => id.to_string(),
                    None => return Ok(xml_error("MissingParameter", "VpcId required", 400)),
                };
                let mut store = self.store.get_or_create(account_id, region);
                match store.internet_gateways.get_mut(&igw_id) {
                    Some(igw) => {
                        igw.state = "available".to_string();
                        igw.attachments.push(IgwAttachment {
                            vpc_id,
                            state: "available".to_string(),
                        });
                        Ok(xml_ok(
                            "AttachInternetGateway",
                            &rid,
                            "<return>true</return>",
                        ))
                    }
                    None => Ok(xml_error(
                        "InvalidInternetGatewayID.NotFound",
                        &format!("Internet gateway {igw_id} not found"),
                        400,
                    )),
                }
            }

            // ----------------------------------------------------------------
            // DetachInternetGateway
            // ----------------------------------------------------------------
            "DetachInternetGateway" => {
                let igw_id = match str_param(ctx, "InternetGatewayId") {
                    Some(id) => id.to_string(),
                    None => {
                        return Ok(xml_error(
                            "MissingParameter",
                            "InternetGatewayId required",
                            400,
                        ));
                    }
                };
                let vpc_id = str_param(ctx, "VpcId").unwrap_or("").to_string();
                let mut store = self.store.get_or_create(account_id, region);
                if let Some(igw) = store.internet_gateways.get_mut(&igw_id) {
                    igw.attachments.retain(|a| a.vpc_id != vpc_id);
                    if igw.attachments.is_empty() {
                        igw.state = "detached".to_string();
                    }
                }
                Ok(xml_ok(
                    "DetachInternetGateway",
                    &rid,
                    "<return>true</return>",
                ))
            }

            // ----------------------------------------------------------------
            // CreateVolume
            // ----------------------------------------------------------------
            "CreateVolume" => {
                let az = str_param(ctx, "AvailabilityZone")
                    .unwrap_or(&format!("{region}a"))
                    .to_string();
                let size: u32 = str_param(ctx, "Size")
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(8);
                let volume_type = str_param(ctx, "VolumeType").unwrap_or("gp2").to_string();
                let encrypted = str_param(ctx, "Encrypted")
                    .map(|s| s.eq_ignore_ascii_case("true"))
                    .unwrap_or(false);
                let volume_id = format!("vol-{}", short_id());
                let vol = Volume {
                    volume_id: volume_id.clone(),
                    size,
                    availability_zone: az.clone(),
                    state: "available".to_string(),
                    volume_type: volume_type.clone(),
                    encrypted,
                    attachments: Vec::new(),
                    created: Utc::now(),
                    tags: Default::default(),
                };
                let mut store = self.store.get_or_create(account_id, region);
                store.volumes.insert(volume_id.clone(), vol);
                let inner = format!(
                    "<volumeId>{volume_id}</volumeId>\
<size>{size}</size>\
<availabilityZone>{az}</availabilityZone>\
<state>available</state>\
<volumeType>{volume_type}</volumeType>\
<encrypted>{encrypted}</encrypted>\
<tagSet/>"
                );
                Ok(xml_ok("CreateVolume", &rid, &inner))
            }

            // ----------------------------------------------------------------
            // DeleteVolume
            // ----------------------------------------------------------------
            "DeleteVolume" => {
                let volume_id = match str_param(ctx, "VolumeId") {
                    Some(id) => id.to_string(),
                    None => return Ok(xml_error("MissingParameter", "VolumeId required", 400)),
                };
                let mut store = self.store.get_or_create(account_id, region);
                if store.volumes.remove(&volume_id).is_none() {
                    return Ok(xml_error(
                        "InvalidVolume.NotFound",
                        &format!("Volume {volume_id} not found"),
                        400,
                    ));
                }
                Ok(xml_ok("DeleteVolume", &rid, "<return>true</return>"))
            }

            // ----------------------------------------------------------------
            // DescribeVolumes
            // ----------------------------------------------------------------
            "DescribeVolumes" => {
                let Some(store) = self.store.get(account_id, region) else {
                    return Ok(xml_ok("DescribeVolumes", &rid, "<volumeSet></volumeSet>"));
                };
                let items: String = store
                    .volumes
                    .values()
                    .map(|v| {
                        let attachments: String = v
                            .attachments
                            .iter()
                            .map(|a| {
                                format!(
                                    "<item>\
<instanceId>{}</instanceId>\
<device>{}</device>\
<status>{}</status>\
</item>",
                                    a.instance_id, a.device, a.state
                                )
                            })
                            .collect();
                        format!(
                            "<item>\
<volumeId>{}</volumeId>\
<size>{}</size>\
<availabilityZone>{}</availabilityZone>\
<status>{}</status>\
<volumeType>{}</volumeType>\
<encrypted>{}</encrypted>\
<attachmentSet>{attachments}</attachmentSet>\
{}\
</item>",
                            v.volume_id,
                            v.size,
                            v.availability_zone,
                            v.state,
                            v.volume_type,
                            v.encrypted,
                            tags_xml(&v.tags)
                        )
                    })
                    .collect();
                let inner = format!("<volumeSet>{items}</volumeSet>");
                Ok(xml_ok("DescribeVolumes", &rid, &inner))
            }

            // ----------------------------------------------------------------
            // AttachVolume
            // ----------------------------------------------------------------
            "AttachVolume" => {
                let volume_id = match str_param(ctx, "VolumeId") {
                    Some(id) => id.to_string(),
                    None => return Ok(xml_error("MissingParameter", "VolumeId required", 400)),
                };
                let instance_id = match str_param(ctx, "InstanceId") {
                    Some(id) => id.to_string(),
                    None => return Ok(xml_error("MissingParameter", "InstanceId required", 400)),
                };
                let device = str_param(ctx, "Device").unwrap_or("/dev/xvda").to_string();
                let mut store = self.store.get_or_create(account_id, region);
                match store.volumes.get_mut(&volume_id) {
                    Some(vol) => {
                        vol.state = "in-use".to_string();
                        vol.attachments.push(VolumeAttachment {
                            instance_id: instance_id.clone(),
                            device: device.clone(),
                            state: "attached".to_string(),
                        });
                        let inner = format!(
                            "<volumeId>{volume_id}</volumeId>\
<instanceId>{instance_id}</instanceId>\
<device>{device}</device>\
<status>attached</status>"
                        );
                        Ok(xml_ok("AttachVolume", &rid, &inner))
                    }
                    None => Ok(xml_error(
                        "InvalidVolume.NotFound",
                        &format!("Volume {volume_id} not found"),
                        400,
                    )),
                }
            }

            // ----------------------------------------------------------------
            // DetachVolume
            // ----------------------------------------------------------------
            "DetachVolume" => {
                let volume_id = match str_param(ctx, "VolumeId") {
                    Some(id) => id.to_string(),
                    None => return Ok(xml_error("MissingParameter", "VolumeId required", 400)),
                };
                let mut store = self.store.get_or_create(account_id, region);
                match store.volumes.get_mut(&volume_id) {
                    Some(vol) => {
                        vol.state = "available".to_string();
                        vol.attachments.clear();
                        let inner =
                            format!("<volumeId>{volume_id}</volumeId><status>detached</status>");
                        Ok(xml_ok("DetachVolume", &rid, &inner))
                    }
                    None => Ok(xml_error(
                        "InvalidVolume.NotFound",
                        &format!("Volume {volume_id} not found"),
                        400,
                    )),
                }
            }

            // ----------------------------------------------------------------
            // CreateTags
            // ----------------------------------------------------------------
            "CreateTags" => {
                // ResourceId.1, ResourceId.2, ... and Tag.1.Key / Tag.1.Value
                let resource_ids = collect_indexed_params(ctx, "ResourceId");
                let mut tags = Vec::new();
                let mut idx = 1usize;
                loop {
                    let key_param = format!("Tag.{idx}.Key");
                    let val_param = format!("Tag.{idx}.Value");
                    match (str_param(ctx, &key_param), str_param(ctx, &val_param)) {
                        (Some(k), Some(v)) => {
                            tags.push((k.to_string(), v.to_string()));
                            idx += 1;
                        }
                        _ => break,
                    }
                }
                let mut store = self.store.get_or_create(account_id, region);
                for id in &resource_ids {
                    apply_tags_to_resource(&mut store, id, &tags);
                }
                Ok(xml_ok("CreateTags", &rid, "<return>true</return>"))
            }

            // ----------------------------------------------------------------
            // DeleteTags
            // ----------------------------------------------------------------
            "DeleteTags" => {
                let resource_ids = collect_indexed_params(ctx, "ResourceId");
                let mut keys = Vec::new();
                let mut idx = 1usize;
                loop {
                    let key_param = format!("Tag.{idx}.Key");
                    if let Some(k) = str_param(ctx, &key_param) {
                        keys.push(k.to_string());
                        idx += 1;
                    } else {
                        break;
                    }
                }
                let mut store = self.store.get_or_create(account_id, region);
                for id in &resource_ids {
                    remove_tags_from_resource(&mut store, id, &keys);
                }
                Ok(xml_ok("DeleteTags", &rid, "<return>true</return>"))
            }

            // ----------------------------------------------------------------
            // DescribeTags
            // ----------------------------------------------------------------
            "DescribeTags" => {
                let Some(store) = self.store.get(account_id, region) else {
                    return Ok(xml_ok("DescribeTags", &rid, "<tagSet></tagSet>"));
                };
                let mut items = String::new();
                for v in store.vpcs.values() {
                    for (k, val) in &v.tags {
                        items.push_str(&format!("<item><resourceId>{}</resourceId><resourceType>vpc</resourceType><key>{k}</key><value>{val}</value></item>", v.vpc_id));
                    }
                }
                for s in store.subnets.values() {
                    for (k, val) in &s.tags {
                        items.push_str(&format!("<item><resourceId>{}</resourceId><resourceType>subnet</resourceType><key>{k}</key><value>{val}</value></item>", s.subnet_id));
                    }
                }
                for sg in store.security_groups.values() {
                    for (k, val) in &sg.tags {
                        items.push_str(&format!("<item><resourceId>{}</resourceId><resourceType>security-group</resourceType><key>{k}</key><value>{val}</value></item>", sg.group_id));
                    }
                }
                for inst in store.instances.values() {
                    for (k, val) in &inst.tags {
                        items.push_str(&format!("<item><resourceId>{}</resourceId><resourceType>instance</resourceType><key>{k}</key><value>{val}</value></item>", inst.instance_id));
                    }
                }
                for vol in store.volumes.values() {
                    for (k, val) in &vol.tags {
                        items.push_str(&format!("<item><resourceId>{}</resourceId><resourceType>volume</resourceType><key>{k}</key><value>{val}</value></item>", vol.volume_id));
                    }
                }
                let inner = format!("<tagSet>{items}</tagSet>");
                Ok(xml_ok("DescribeTags", &rid, &inner))
            }

            // ----------------------------------------------------------------
            // DescribeAvailabilityZones
            // ----------------------------------------------------------------
            "DescribeAvailabilityZones" => {
                let zones: String = ["a", "b", "c"]
                    .iter()
                    .map(|z| {
                        format!(
                            "<item>\
<zoneName>{region}{z}</zoneName>\
<zoneState>available</zoneState>\
<regionName>{region}</regionName>\
</item>"
                        )
                    })
                    .collect();
                let inner = format!("<availabilityZoneInfo>{zones}</availabilityZoneInfo>");
                Ok(xml_ok("DescribeAvailabilityZones", &rid, &inner))
            }

            // ----------------------------------------------------------------
            // DescribeRegions
            // ----------------------------------------------------------------
            "DescribeRegions" => {
                let regions = [
                    "us-east-1",
                    "us-east-2",
                    "us-west-1",
                    "us-west-2",
                    "eu-west-1",
                    "eu-west-2",
                    "eu-central-1",
                    "ap-southeast-1",
                    "ap-southeast-2",
                    "ap-northeast-1",
                ];
                let items: String = regions
                    .iter()
                    .map(|r| {
                        format!(
                            "<item>\
<regionName>{r}</regionName>\
<regionEndpoint>ec2.{r}.amazonaws.com</regionEndpoint>\
</item>"
                        )
                    })
                    .collect();
                let inner = format!("<regionInfo>{items}</regionInfo>");
                Ok(xml_ok("DescribeRegions", &rid, &inner))
            }

            _ => Err(DispatchError::NotImplemented(ctx.operation.clone())),
        }
    }

    async fn storage_snapshot(&self) -> Option<serde_json::Value> {
        use serde_json::json;
        let mut instances = Vec::new();
        let mut vpcs = Vec::new();
        for entry in self.store.iter() {
            let store = entry.value();
            for inst in store.instances.values() {
                instances.push(json!({
                    "id": inst.instance_id, "kind": "instance",
                    "attributes": [
                        {"key": "type", "value": inst.instance_type.clone()},
                        {"key": "state", "value": inst.state.clone()},
                        {"key": "vpc_id", "value": inst.vpc_id.clone()},
                    ]
                }));
            }
            for vpc in store.vpcs.values() {
                vpcs.push(json!({
                    "id": vpc.vpc_id, "kind": "vpc",
                    "attributes": [{"key": "cidr", "value": vpc.cidr_block.clone()}]
                }));
            }
        }
        Some(json!({ "kind": "ec2", "instances": instances, "vpcs": vpcs }))
    }
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

fn collect_indexed_params(ctx: &RequestContext, prefix: &str) -> Vec<String> {
    let mut result = Vec::new();
    let mut idx = 1usize;
    loop {
        let key = format!("{prefix}.{idx}");
        if let Some(val) = ctx.query_params.get(&key).cloned().or_else(|| {
            ctx.request_body
                .get(&key)
                .and_then(|v| v.as_str())
                .map(String::from)
        }) {
            result.push(val);
            idx += 1;
        } else {
            break;
        }
    }
    result
}

fn rand_byte() -> u8 {
    (Uuid::new_v4().as_bytes()[0]) ^ (Uuid::new_v4().as_bytes()[1])
}

fn apply_tags_to_resource(store: &mut Ec2Store, id: &str, tags: &[(String, String)]) {
    macro_rules! try_apply {
        ($map:expr) => {
            if let Some(r) = $map.get_mut(id) {
                for (k, v) in tags {
                    r.tags.insert(k.clone(), v.clone());
                }
                return;
            }
        };
    }
    try_apply!(store.vpcs);
    try_apply!(store.subnets);
    try_apply!(store.security_groups);
    try_apply!(store.instances);
    try_apply!(store.key_pairs);
    try_apply!(store.addresses);
    try_apply!(store.internet_gateways);
    try_apply!(store.volumes);
}

fn remove_tags_from_resource(store: &mut Ec2Store, id: &str, keys: &[String]) {
    macro_rules! try_remove {
        ($map:expr) => {
            if let Some(r) = $map.get_mut(id) {
                for k in keys {
                    r.tags.remove(k);
                }
                return;
            }
        };
    }
    try_remove!(store.vpcs);
    try_remove!(store.subnets);
    try_remove!(store.security_groups);
    try_remove!(store.instances);
    try_remove!(store.key_pairs);
    try_remove!(store.addresses);
    try_remove!(store.internet_gateways);
    try_remove!(store.volumes);
}
