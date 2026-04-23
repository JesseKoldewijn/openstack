use std::collections::HashMap;

use chrono::{DateTime, Utc};
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
    pub egress_rules: Vec<IpPermission>,
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
    pub key_name: Option<String>,
    pub tags: HashMap<String, String>,
}

// ---------------------------------------------------------------------------
// Key Pair
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyPair {
    pub key_pair_id: String,
    pub key_name: String,
    pub key_fingerprint: String,
    pub key_material: Option<String>, // only on create
    pub tags: HashMap<String, String>,
    pub created: DateTime<Utc>,
}

// ---------------------------------------------------------------------------
// Elastic IP (Address)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Address {
    pub allocation_id: String,
    pub public_ip: String,
    pub domain: String, // "vpc" | "standard"
    pub instance_id: Option<String>,
    pub association_id: Option<String>,
    pub network_interface_id: Option<String>,
    pub private_ip: Option<String>,
    pub tags: HashMap<String, String>,
}

// ---------------------------------------------------------------------------
// Internet Gateway
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InternetGateway {
    pub internet_gateway_id: String,
    pub state: String, // "available" | "detached"
    pub attachments: Vec<IgwAttachment>,
    pub tags: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IgwAttachment {
    pub vpc_id: String,
    pub state: String, // "available"
}

// ---------------------------------------------------------------------------
// EBS Volume
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Volume {
    pub volume_id: String,
    pub size: u32, // GiB
    pub availability_zone: String,
    pub state: String, // "available" | "in-use"
    pub volume_type: String,
    pub encrypted: bool,
    pub attachments: Vec<VolumeAttachment>,
    pub created: DateTime<Utc>,
    pub tags: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VolumeAttachment {
    pub instance_id: String,
    pub device: String,
    pub state: String, // "attached" | "detaching"
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
    pub key_pairs: HashMap<String, KeyPair>,
    pub addresses: HashMap<String, Address>,
    pub internet_gateways: HashMap<String, InternetGateway>,
    pub volumes: HashMap<String, Volume>,
}
