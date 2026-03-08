use std::collections::HashMap;

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// VPC
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Vpc {
    pub vpc_id: String,
    pub cidr_block: String,
    pub state: String,
    pub is_default: bool,
    pub tags: HashMap<String, String>,
}

// ---------------------------------------------------------------------------
// Subnet
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Subnet {
    pub subnet_id: String,
    pub vpc_id: String,
    pub cidr_block: String,
    pub availability_zone: String,
    pub state: String,
    pub tags: HashMap<String, String>,
}

// ---------------------------------------------------------------------------
// SecurityGroup
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IpPermission {
    pub ip_protocol: String,
    pub from_port: i32,
    pub to_port: i32,
    pub ip_ranges: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityGroup {
    pub group_id: String,
    pub group_name: String,
    pub description: String,
    pub vpc_id: String,
    pub ingress_rules: Vec<IpPermission>,
    pub tags: HashMap<String, String>,
}

// ---------------------------------------------------------------------------
// Instance
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Instance {
    pub instance_id: String,
    pub image_id: String,
    pub instance_type: String,
    pub state: String, // "running" | "stopped" | "terminated"
    pub subnet_id: String,
    pub vpc_id: String,
    pub private_ip: String,
    pub tags: HashMap<String, String>,
}

// ---------------------------------------------------------------------------
// Store
// ---------------------------------------------------------------------------

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct Ec2Store {
    pub vpcs: HashMap<String, Vpc>,
    pub subnets: HashMap<String, Subnet>,
    pub security_groups: HashMap<String, SecurityGroup>,
    pub instances: HashMap<String, Instance>,
}
