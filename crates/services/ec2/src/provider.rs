use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use openstack_service_framework::traits::{
    DispatchError, DispatchResponse, RequestContext, ServiceProvider,
};
use openstack_state::AccountRegionBundle;
use uuid::Uuid;

use crate::store::{Ec2Store, Instance, IpPermission, SecurityGroup, Subnet, Vpc};

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
        body: Bytes::from(xml.into_bytes()),
        content_type: "text/xml".to_string(),
        headers: Vec::new(),
    }
}

fn xml_error(code: &str, message: &str, status: u16) -> DispatchResponse {
    let xml = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
<Response><Errors><Error>\
<Code>{code}</Code><Message>{message}</Message>\
</Error></Errors></Response>"
    );
    DispatchResponse {
        status_code: status,
        body: Bytes::from(xml.into_bytes()),
        content_type: "text/xml".to_string(),
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
</vpc>"
                );
                Ok(xml_ok("CreateVpc", &rid, &inner))
            }

            // ----------------------------------------------------------------
            // DescribeVpcs
            // ----------------------------------------------------------------
            "DescribeVpcs" => {
                let store = self.store.get_or_create(account_id, region);
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
</item>",
                            v.vpc_id, v.cidr_block, v.state, v.is_default
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
</subnet>"
                );
                Ok(xml_ok("CreateSubnet", &rid, &inner))
            }

            // ----------------------------------------------------------------
            // DescribeSubnets
            // ----------------------------------------------------------------
            "DescribeSubnets" => {
                let store = self.store.get_or_create(account_id, region);
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
</item>",
                            s.subnet_id, s.vpc_id, s.cidr_block, s.availability_zone, s.state
                        )
                    })
                    .collect();
                let inner = format!("<subnetSet>{items}</subnetSet>");
                Ok(xml_ok("DescribeSubnets", &rid, &inner))
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
                let store = self.store.get_or_create(account_id, region);
                let items: String = store
                    .security_groups
                    .values()
                    .map(|sg| {
                        format!(
                            "<item>\
<groupId>{}</groupId>\
<groupName>{}</groupName>\
<groupDescription>{}</groupDescription>\
<vpcId>{}</vpcId>\
</item>",
                            sg.group_id, sg.group_name, sg.description, sg.vpc_id
                        )
                    })
                    .collect();
                let inner = format!("<securityGroupInfo>{items}</securityGroupInfo>");
                Ok(xml_ok("DescribeSecurityGroups", &rid, &inner))
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
                let max_count: u32 = str_param(ctx, "MaxCount")
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(1);

                // Fetch vpc_id from subnet if possible
                let vpc_id = {
                    let store = self.store.get_or_create(account_id, region);
                    store
                        .subnets
                        .get(&subnet_id)
                        .map(|s| s.vpc_id.clone())
                        .unwrap_or_default()
                };

                let mut instance_items = String::new();
                let mut store = self.store.get_or_create(account_id, region);
                for _ in 0..max_count {
                    let instance_id = format!("i-{}", short_id());
                    let private_ip = format!("10.0.0.{}", (store.instances.len() + 1) % 254 + 1);
                    let inst = Instance {
                        instance_id: instance_id.clone(),
                        image_id: image_id.clone(),
                        instance_type: instance_type.clone(),
                        state: "running".to_string(),
                        subnet_id: subnet_id.clone(),
                        vpc_id: vpc_id.clone(),
                        private_ip: private_ip.clone(),
                        tags: Default::default(),
                    };
                    store.instances.insert(instance_id.clone(), inst);
                    instance_items.push_str(&format!(
                        "<item>\
<instanceId>{instance_id}</instanceId>\
<imageId>{image_id}</imageId>\
<instanceType>{instance_type}</instanceType>\
<instanceState><code>16</code><name>running</name></instanceState>\
<privateIpAddress>{private_ip}</privateIpAddress>\
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
                let store = self.store.get_or_create(account_id, region);
                let instance_items: String = store
                    .instances
                    .values()
                    .filter(|i| i.state != "terminated")
                    .map(|i| {
                        format!(
                            "<item>\
<instanceId>{}</instanceId>\
<imageId>{}</imageId>\
<instanceType>{}</instanceType>\
<instanceState><code>16</code><name>{}</name></instanceState>\
<privateIpAddress>{}</privateIpAddress>\
</item>",
                            i.instance_id, i.image_id, i.instance_type, i.state, i.private_ip
                        )
                    })
                    .collect();
                let inner = format!(
                    "<reservationSet><item><instancesSet>{instance_items}</instancesSet></item></reservationSet>"
                );
                Ok(xml_ok("DescribeInstances", &rid, &inner))
            }

            // ----------------------------------------------------------------
            // TerminateInstances
            // ----------------------------------------------------------------
            "TerminateInstances" => {
                let mut items_xml = String::new();
                // InstanceId.1, InstanceId.2, ...
                let ids: Vec<String> = {
                    let mut idx = 1;
                    let mut result = Vec::new();
                    loop {
                        let key = format!("InstanceId.{idx}");
                        if let Some(id) = ctx.query_params.get(&key) {
                            result.push(id.clone());
                        } else {
                            break;
                        }
                        idx += 1;
                    }
                    result
                };
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

            _ => Err(DispatchError::NotImplemented(ctx.operation.clone())),
        }
    }
}
