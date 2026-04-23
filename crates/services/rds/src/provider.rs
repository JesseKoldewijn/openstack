use std::borrow::Cow;
use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use chrono::Utc;
use openstack_service_framework::traits::{
    DispatchError, DispatchResponse, RequestContext, ResponseBody, ServiceProvider,
};
use openstack_service_framework::xml::xml_escape;
use openstack_state::AccountRegionBundle;
use uuid::Uuid;

use crate::store::{DbEndpoint, DbInstance, DbParameterGroup, DbSnapshot, DbSubnetGroup, RdsStore};

pub struct RdsProvider {
    store: Arc<AccountRegionBundle<RdsStore>>,
}

impl RdsProvider {
    pub fn new() -> Self {
        Self {
            store: Arc::new(AccountRegionBundle::new()),
        }
    }
}

impl Default for RdsProvider {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Helpers — RDS uses query protocol (XML responses)
// ---------------------------------------------------------------------------

const RDS_NS: &str = "http://rds.amazonaws.com/doc/2014-10-31/";

fn xml_resp(action: &str, rid: &str, inner: &str) -> DispatchResponse {
    let xml = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
<{action}Response xmlns=\"{RDS_NS}\">\
<{action}Result>{inner}</{action}Result>\
<ResponseMetadata><RequestId>{rid}</RequestId></ResponseMetadata>\
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
<ErrorResponse xmlns=\"{RDS_NS}\">\
<Error><Code>{code}</Code><Message>{message}</Message></Error>\
</ErrorResponse>"
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

fn str_param<'a>(ctx: &'a RequestContext, key: &str) -> Option<&'a str> {
    ctx.query_params
        .get(key)
        .map(|s| s.as_str())
        .or_else(|| ctx.request_body.get(key).and_then(|v| v.as_str()))
}

fn instance_xml(db: &DbInstance) -> String {
    let endpoint_xml = db
        .endpoint
        .as_ref()
        .map(|e| {
            format!(
                "<Endpoint><Address>{}</Address><Port>{}</Port></Endpoint>",
                xml_escape(&e.address),
                e.port
            )
        })
        .unwrap_or_default();
    let db_name_xml = db
        .db_name
        .as_deref()
        .map(|n| format!("<DBName>{}</DBName>", xml_escape(n)))
        .unwrap_or_default();
    format!(
        "<DBInstanceIdentifier>{}</DBInstanceIdentifier>\
<DBInstanceClass>{}</DBInstanceClass>\
<Engine>{}</Engine>\
<EngineVersion>{}</EngineVersion>\
<DBInstanceStatus>{}</DBInstanceStatus>\
<MasterUsername>{}</MasterUsername>\
{db_name_xml}\
{endpoint_xml}\
<AllocatedStorage>{}</AllocatedStorage>\
<MultiAZ>{}</MultiAZ>",
        xml_escape(&db.db_instance_identifier),
        xml_escape(&db.db_instance_class),
        xml_escape(&db.engine),
        xml_escape(&db.engine_version),
        xml_escape(&db.db_instance_status),
        xml_escape(&db.master_username),
        db.allocated_storage,
        db.multi_az,
    )
}

fn snapshot_xml(s: &DbSnapshot) -> String {
    format!(
        "<DBSnapshotIdentifier>{}</DBSnapshotIdentifier>\
<DBInstanceIdentifier>{}</DBInstanceIdentifier>\
<SnapshotType>{}</SnapshotType>\
<Status>{}</Status>\
<Engine>{}</Engine>\
<EngineVersion>{}</EngineVersion>\
<AllocatedStorage>{}</AllocatedStorage>\
<MasterUsername>{}</MasterUsername>\
<SnapshotCreateTime>{}</SnapshotCreateTime>",
        xml_escape(&s.db_snapshot_identifier),
        xml_escape(&s.db_instance_identifier),
        xml_escape(&s.snapshot_type),
        xml_escape(&s.status),
        xml_escape(&s.engine),
        xml_escape(&s.engine_version),
        s.allocated_storage,
        xml_escape(&s.master_username),
        s.created.format("%Y-%m-%dT%H:%M:%SZ"),
    )
}

fn subnet_group_xml(sg: &DbSubnetGroup) -> String {
    let subnets: String = sg
        .subnet_ids
        .iter()
        .map(|id| {
            format!(
                "<member><SubnetIdentifier>{}</SubnetIdentifier><SubnetStatus>Active</SubnetStatus></member>",
                xml_escape(id)
            )
        })
        .collect();
    format!(
        "<DBSubnetGroupName>{}</DBSubnetGroupName>\
<DBSubnetGroupDescription>{}</DBSubnetGroupDescription>\
<VpcId>{}</VpcId>\
<SubnetGroupStatus>{}</SubnetGroupStatus>\
<Subnets>{subnets}</Subnets>",
        xml_escape(&sg.db_subnet_group_name),
        xml_escape(&sg.db_subnet_group_description),
        xml_escape(&sg.vpc_id),
        xml_escape(&sg.status),
    )
}

fn param_group_xml(pg: &DbParameterGroup) -> String {
    format!(
        "<DBParameterGroupName>{}</DBParameterGroupName>\
<DBParameterGroupFamily>{}</DBParameterGroupFamily>\
<Description>{}</Description>",
        xml_escape(&pg.db_parameter_group_name),
        xml_escape(&pg.db_parameter_group_family),
        xml_escape(&pg.description),
    )
}

fn engine_default_port(engine: &str) -> u16 {
    match engine {
        "mysql" | "mariadb" | "aurora" | "aurora-mysql" => 3306,
        "postgres" | "aurora-postgresql" => 5432,
        "oracle-ee" | "oracle-se2" | "oracle-se1" | "oracle-se" => 1521,
        "sqlserver-ee" | "sqlserver-se" | "sqlserver-ex" | "sqlserver-web" => 1433,
        _ => 3306,
    }
}

// ---------------------------------------------------------------------------
// ServiceProvider
// ---------------------------------------------------------------------------

#[async_trait]
impl ServiceProvider for RdsProvider {
    fn service_name(&self) -> &str {
        "rds"
    }

    async fn dispatch(&self, ctx: &RequestContext) -> Result<DispatchResponse, DispatchError> {
        let region = &ctx.region;
        let account_id = &ctx.account_id;
        let rid = req_id();

        match ctx.operation.as_str() {
            // ----------------------------------------------------------------
            // CreateDBInstance
            // ----------------------------------------------------------------
            "CreateDBInstance" => {
                let db_id = match str_param(ctx, "DBInstanceIdentifier") {
                    Some(id) => id.to_string(),
                    None => {
                        return Ok(xml_error(
                            "MissingParameter",
                            "DBInstanceIdentifier required",
                            400,
                        ));
                    }
                };
                let db_class = str_param(ctx, "DBInstanceClass")
                    .unwrap_or("db.t3.micro")
                    .to_string();
                let engine = str_param(ctx, "Engine").unwrap_or("mysql").to_string();
                let engine_version = str_param(ctx, "EngineVersion").unwrap_or("8.0").to_string();
                let master_username = str_param(ctx, "MasterUsername")
                    .unwrap_or("admin")
                    .to_string();
                let master_password = str_param(ctx, "MasterUserPassword").unwrap_or("");
                let _ = master_password; // not stored
                let db_name = str_param(ctx, "DBName").map(|s| s.to_string());
                let allocated_storage: u32 = str_param(ctx, "AllocatedStorage")
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(20);
                let multi_az = str_param(ctx, "MultiAZ")
                    .map(|s| s.eq_ignore_ascii_case("true"))
                    .unwrap_or(false);
                let port = str_param(ctx, "Port")
                    .and_then(|s| s.parse().ok())
                    .unwrap_or_else(|| engine_default_port(&engine));
                let db_subnet_group = str_param(ctx, "DBSubnetGroupName").map(|s| s.to_string());
                let db_param_group = str_param(ctx, "DBParameterGroupName").map(|s| s.to_string());

                let mut store = self.store.get_or_create(account_id, region);
                if store.instances.contains_key(&db_id) {
                    return Ok(xml_error(
                        "DBInstanceAlreadyExists",
                        &format!("DB instance {db_id} already exists"),
                        400,
                    ));
                }
                let endpoint = DbEndpoint {
                    address: format!("{db_id}.fake.{region}.rds.amazonaws.com"),
                    port,
                };
                let instance = DbInstance {
                    db_instance_identifier: db_id.clone(),
                    db_instance_class: db_class,
                    engine: engine.clone(),
                    engine_version,
                    db_instance_status: "available".to_string(),
                    master_username,
                    db_name,
                    endpoint: Some(endpoint),
                    allocated_storage,
                    multi_az,
                    db_subnet_group_name: db_subnet_group,
                    db_parameter_group_name: db_param_group,
                    created: Utc::now(),
                };
                store.instances.insert(db_id, instance.clone());
                let inner = format!("<DBInstance>{}</DBInstance>", instance_xml(&instance));
                Ok(xml_resp("CreateDBInstance", &rid, &inner))
            }

            // ----------------------------------------------------------------
            // DeleteDBInstance
            // ----------------------------------------------------------------
            "DeleteDBInstance" => {
                let db_id = match str_param(ctx, "DBInstanceIdentifier") {
                    Some(id) => id.to_string(),
                    None => {
                        return Ok(xml_error(
                            "MissingParameter",
                            "DBInstanceIdentifier required",
                            400,
                        ));
                    }
                };
                let mut store = self.store.get_or_create(account_id, region);
                match store.instances.remove(&db_id) {
                    Some(db) => {
                        let inner = format!("<DBInstance>{}</DBInstance>", instance_xml(&db));
                        Ok(xml_resp("DeleteDBInstance", &rid, &inner))
                    }
                    None => Ok(xml_error(
                        "DBInstanceNotFound",
                        &format!("DB instance {db_id} not found"),
                        400,
                    )),
                }
            }

            // ----------------------------------------------------------------
            // DescribeDBInstances
            // ----------------------------------------------------------------
            "DescribeDBInstances" => {
                let Some(store) = self.store.get(account_id, region) else {
                    return Ok(xml_resp(
                        "DescribeDBInstances",
                        &rid,
                        "<DBInstances></DBInstances>",
                    ));
                };
                let filter_id = str_param(ctx, "DBInstanceIdentifier");
                let instances_xml: String = store
                    .instances
                    .values()
                    .filter(|db| {
                        filter_id
                            .map(|id| db.db_instance_identifier == id)
                            .unwrap_or(true)
                    })
                    .map(|db| format!("<member>{}</member>", instance_xml(db)))
                    .collect();
                let inner = format!("<DBInstances>{instances_xml}</DBInstances>");
                Ok(xml_resp("DescribeDBInstances", &rid, &inner))
            }

            // ----------------------------------------------------------------
            // ModifyDBInstance
            // ----------------------------------------------------------------
            "ModifyDBInstance" => {
                let db_id = match str_param(ctx, "DBInstanceIdentifier") {
                    Some(id) => id.to_string(),
                    None => {
                        return Ok(xml_error(
                            "MissingParameter",
                            "DBInstanceIdentifier required",
                            400,
                        ));
                    }
                };
                let mut store = self.store.get_or_create(account_id, region);
                match store.instances.get_mut(&db_id) {
                    Some(db) => {
                        if let Some(class) = str_param(ctx, "DBInstanceClass") {
                            db.db_instance_class = class.to_string();
                        }
                        if let Some(Ok(storage)) =
                            str_param(ctx, "AllocatedStorage").map(|s| s.parse::<u32>())
                        {
                            db.allocated_storage = storage;
                        }
                        if let Some(multi_az_str) = str_param(ctx, "MultiAZ") {
                            db.multi_az = multi_az_str.eq_ignore_ascii_case("true");
                        }
                        let inner = format!("<DBInstance>{}</DBInstance>", instance_xml(db));
                        Ok(xml_resp("ModifyDBInstance", &rid, &inner))
                    }
                    None => Ok(xml_error(
                        "DBInstanceNotFound",
                        &format!("DB instance {db_id} not found"),
                        400,
                    )),
                }
            }

            // ----------------------------------------------------------------
            // RebootDBInstance
            // ----------------------------------------------------------------
            "RebootDBInstance" => {
                let db_id = match str_param(ctx, "DBInstanceIdentifier") {
                    Some(id) => id.to_string(),
                    None => {
                        return Ok(xml_error(
                            "MissingParameter",
                            "DBInstanceIdentifier required",
                            400,
                        ));
                    }
                };
                let mut store = self.store.get_or_create(account_id, region);
                match store.instances.get_mut(&db_id) {
                    Some(db) => {
                        db.db_instance_status = "available".to_string();
                        let inner = format!("<DBInstance>{}</DBInstance>", instance_xml(db));
                        Ok(xml_resp("RebootDBInstance", &rid, &inner))
                    }
                    None => Ok(xml_error(
                        "DBInstanceNotFound",
                        &format!("DB instance {db_id} not found"),
                        400,
                    )),
                }
            }

            // ----------------------------------------------------------------
            // CreateDBSnapshot
            // ----------------------------------------------------------------
            "CreateDBSnapshot" => {
                let snapshot_id = match str_param(ctx, "DBSnapshotIdentifier") {
                    Some(id) => id.to_string(),
                    None => {
                        return Ok(xml_error(
                            "MissingParameter",
                            "DBSnapshotIdentifier required",
                            400,
                        ));
                    }
                };
                let db_id = match str_param(ctx, "DBInstanceIdentifier") {
                    Some(id) => id.to_string(),
                    None => {
                        return Ok(xml_error(
                            "MissingParameter",
                            "DBInstanceIdentifier required",
                            400,
                        ));
                    }
                };
                let mut store = self.store.get_or_create(account_id, region);
                if store.snapshots.contains_key(&snapshot_id) {
                    return Ok(xml_error(
                        "DBSnapshotAlreadyExists",
                        &format!("Snapshot {snapshot_id} already exists"),
                        400,
                    ));
                }
                let (engine, engine_version, allocated_storage, master_username) = store
                    .instances
                    .get(&db_id)
                    .map(|db| {
                        (
                            db.engine.clone(),
                            db.engine_version.clone(),
                            db.allocated_storage,
                            db.master_username.clone(),
                        )
                    })
                    .unwrap_or_else(|| {
                        (
                            "mysql".to_string(),
                            "8.0".to_string(),
                            20,
                            "admin".to_string(),
                        )
                    });
                let snap = DbSnapshot {
                    db_snapshot_identifier: snapshot_id.clone(),
                    db_instance_identifier: db_id,
                    snapshot_type: "manual".to_string(),
                    status: "available".to_string(),
                    engine,
                    engine_version,
                    allocated_storage,
                    master_username,
                    created: Utc::now(),
                };
                store.snapshots.insert(snapshot_id, snap.clone());
                let inner = format!("<DBSnapshot>{}</DBSnapshot>", snapshot_xml(&snap));
                Ok(xml_resp("CreateDBSnapshot", &rid, &inner))
            }

            // ----------------------------------------------------------------
            // DeleteDBSnapshot
            // ----------------------------------------------------------------
            "DeleteDBSnapshot" => {
                let snapshot_id = match str_param(ctx, "DBSnapshotIdentifier") {
                    Some(id) => id.to_string(),
                    None => {
                        return Ok(xml_error(
                            "MissingParameter",
                            "DBSnapshotIdentifier required",
                            400,
                        ));
                    }
                };
                let mut store = self.store.get_or_create(account_id, region);
                match store.snapshots.remove(&snapshot_id) {
                    Some(s) => {
                        let inner = format!("<DBSnapshot>{}</DBSnapshot>", snapshot_xml(&s));
                        Ok(xml_resp("DeleteDBSnapshot", &rid, &inner))
                    }
                    None => Ok(xml_error(
                        "DBSnapshotNotFound",
                        &format!("Snapshot {snapshot_id} not found"),
                        400,
                    )),
                }
            }

            // ----------------------------------------------------------------
            // DescribeDBSnapshots
            // ----------------------------------------------------------------
            "DescribeDBSnapshots" => {
                let Some(store) = self.store.get(account_id, region) else {
                    return Ok(xml_resp(
                        "DescribeDBSnapshots",
                        &rid,
                        "<DBSnapshots></DBSnapshots>",
                    ));
                };
                let filter_db = str_param(ctx, "DBInstanceIdentifier");
                let snaps_xml: String = store
                    .snapshots
                    .values()
                    .filter(|s| {
                        filter_db
                            .map(|id| s.db_instance_identifier == id)
                            .unwrap_or(true)
                    })
                    .map(|s| format!("<member>{}</member>", snapshot_xml(s)))
                    .collect();
                let inner = format!("<DBSnapshots>{snaps_xml}</DBSnapshots>");
                Ok(xml_resp("DescribeDBSnapshots", &rid, &inner))
            }

            // ----------------------------------------------------------------
            // RestoreDBInstanceFromDBSnapshot
            // ----------------------------------------------------------------
            "RestoreDBInstanceFromDBSnapshot" => {
                let new_db_id = match str_param(ctx, "DBInstanceIdentifier") {
                    Some(id) => id.to_string(),
                    None => {
                        return Ok(xml_error(
                            "MissingParameter",
                            "DBInstanceIdentifier required",
                            400,
                        ));
                    }
                };
                let snapshot_id = match str_param(ctx, "DBSnapshotIdentifier") {
                    Some(id) => id.to_string(),
                    None => {
                        return Ok(xml_error(
                            "MissingParameter",
                            "DBSnapshotIdentifier required",
                            400,
                        ));
                    }
                };
                let mut store = self.store.get_or_create(account_id, region);
                if store.instances.contains_key(&new_db_id) {
                    return Ok(xml_error(
                        "DBInstanceAlreadyExists",
                        &format!("DB instance {new_db_id} already exists"),
                        400,
                    ));
                }
                let snap = match store.snapshots.get(&snapshot_id) {
                    Some(s) => s.clone(),
                    None => {
                        return Ok(xml_error(
                            "DBSnapshotNotFound",
                            &format!("Snapshot {snapshot_id} not found"),
                            400,
                        ));
                    }
                };
                let db_class = str_param(ctx, "DBInstanceClass")
                    .unwrap_or("db.t3.micro")
                    .to_string();
                let port = engine_default_port(&snap.engine);
                let endpoint = DbEndpoint {
                    address: format!("{new_db_id}.fake.{region}.rds.amazonaws.com"),
                    port,
                };
                let instance = DbInstance {
                    db_instance_identifier: new_db_id.clone(),
                    db_instance_class: db_class,
                    engine: snap.engine.clone(),
                    engine_version: snap.engine_version.clone(),
                    db_instance_status: "available".to_string(),
                    master_username: snap.master_username.clone(),
                    db_name: None,
                    endpoint: Some(endpoint),
                    allocated_storage: snap.allocated_storage,
                    multi_az: false,
                    db_subnet_group_name: None,
                    db_parameter_group_name: None,
                    created: Utc::now(),
                };
                store.instances.insert(new_db_id, instance.clone());
                let inner = format!("<DBInstance>{}</DBInstance>", instance_xml(&instance));
                Ok(xml_resp("RestoreDBInstanceFromDBSnapshot", &rid, &inner))
            }

            // ----------------------------------------------------------------
            // CreateDBSubnetGroup
            // ----------------------------------------------------------------
            "CreateDBSubnetGroup" => {
                let name = match str_param(ctx, "DBSubnetGroupName") {
                    Some(n) => n.to_string(),
                    None => {
                        return Ok(xml_error(
                            "MissingParameter",
                            "DBSubnetGroupName required",
                            400,
                        ));
                    }
                };
                let description = str_param(ctx, "DBSubnetGroupDescription")
                    .unwrap_or("")
                    .to_string();
                let vpc_id = str_param(ctx, "VpcId").unwrap_or("").to_string();

                let mut subnet_ids = Vec::new();
                let mut idx = 1usize;
                loop {
                    let key = format!("SubnetIds.SubnetIdentifier.{idx}");
                    if let Some(sid) = str_param(ctx, &key) {
                        subnet_ids.push(sid.to_string());
                        idx += 1;
                    } else {
                        break;
                    }
                }

                let mut store = self.store.get_or_create(account_id, region);
                if store.subnet_groups.contains_key(&name) {
                    return Ok(xml_error(
                        "DBSubnetGroupAlreadyExists",
                        &format!("Subnet group {name} already exists"),
                        400,
                    ));
                }
                let sg = DbSubnetGroup {
                    db_subnet_group_name: name.clone(),
                    db_subnet_group_description: description,
                    vpc_id,
                    subnet_ids,
                    status: "Complete".to_string(),
                };
                store.subnet_groups.insert(name, sg.clone());
                let inner = format!("<DBSubnetGroup>{}</DBSubnetGroup>", subnet_group_xml(&sg));
                Ok(xml_resp("CreateDBSubnetGroup", &rid, &inner))
            }

            // ----------------------------------------------------------------
            // DeleteDBSubnetGroup
            // ----------------------------------------------------------------
            "DeleteDBSubnetGroup" => {
                let name = match str_param(ctx, "DBSubnetGroupName") {
                    Some(n) => n.to_string(),
                    None => {
                        return Ok(xml_error(
                            "MissingParameter",
                            "DBSubnetGroupName required",
                            400,
                        ));
                    }
                };
                let mut store = self.store.get_or_create(account_id, region);
                if store.subnet_groups.remove(&name).is_none() {
                    return Ok(xml_error(
                        "DBSubnetGroupNotFoundFault",
                        &format!("Subnet group {name} not found"),
                        400,
                    ));
                }
                Ok(xml_resp("DeleteDBSubnetGroup", &rid, ""))
            }

            // ----------------------------------------------------------------
            // DescribeDBSubnetGroups
            // ----------------------------------------------------------------
            "DescribeDBSubnetGroups" => {
                let Some(store) = self.store.get(account_id, region) else {
                    return Ok(xml_resp(
                        "DescribeDBSubnetGroups",
                        &rid,
                        "<DBSubnetGroups></DBSubnetGroups>",
                    ));
                };
                let items_xml: String = store
                    .subnet_groups
                    .values()
                    .map(|sg| format!("<member>{}</member>", subnet_group_xml(sg)))
                    .collect();
                let inner = format!("<DBSubnetGroups>{items_xml}</DBSubnetGroups>");
                Ok(xml_resp("DescribeDBSubnetGroups", &rid, &inner))
            }

            // ----------------------------------------------------------------
            // CreateDBParameterGroup
            // ----------------------------------------------------------------
            "CreateDBParameterGroup" => {
                let name = match str_param(ctx, "DBParameterGroupName") {
                    Some(n) => n.to_string(),
                    None => {
                        return Ok(xml_error(
                            "MissingParameter",
                            "DBParameterGroupName required",
                            400,
                        ));
                    }
                };
                let family = str_param(ctx, "DBParameterGroupFamily")
                    .unwrap_or("mysql8.0")
                    .to_string();
                let description = str_param(ctx, "Description").unwrap_or("").to_string();

                let mut store = self.store.get_or_create(account_id, region);
                if store.parameter_groups.contains_key(&name) {
                    return Ok(xml_error(
                        "DBParameterGroupAlreadyExists",
                        &format!("Parameter group {name} already exists"),
                        400,
                    ));
                }
                let pg = DbParameterGroup {
                    db_parameter_group_name: name.clone(),
                    db_parameter_group_family: family,
                    description,
                };
                store.parameter_groups.insert(name, pg.clone());
                let inner = format!(
                    "<DBParameterGroup>{}</DBParameterGroup>",
                    param_group_xml(&pg)
                );
                Ok(xml_resp("CreateDBParameterGroup", &rid, &inner))
            }

            // ----------------------------------------------------------------
            // DeleteDBParameterGroup
            // ----------------------------------------------------------------
            "DeleteDBParameterGroup" => {
                let name = match str_param(ctx, "DBParameterGroupName") {
                    Some(n) => n.to_string(),
                    None => {
                        return Ok(xml_error(
                            "MissingParameter",
                            "DBParameterGroupName required",
                            400,
                        ));
                    }
                };
                let mut store = self.store.get_or_create(account_id, region);
                if store.parameter_groups.remove(&name).is_none() {
                    return Ok(xml_error(
                        "DBParameterGroupNotFound",
                        &format!("Parameter group {name} not found"),
                        400,
                    ));
                }
                Ok(xml_resp("DeleteDBParameterGroup", &rid, ""))
            }

            // ----------------------------------------------------------------
            // DescribeDBParameterGroups
            // ----------------------------------------------------------------
            "DescribeDBParameterGroups" => {
                let Some(store) = self.store.get(account_id, region) else {
                    return Ok(xml_resp(
                        "DescribeDBParameterGroups",
                        &rid,
                        "<DBParameterGroups></DBParameterGroups>",
                    ));
                };
                let items_xml: String = store
                    .parameter_groups
                    .values()
                    .map(|pg| format!("<member>{}</member>", param_group_xml(pg)))
                    .collect();
                let inner = format!("<DBParameterGroups>{items_xml}</DBParameterGroups>");
                Ok(xml_resp("DescribeDBParameterGroups", &rid, &inner))
            }

            _ => Err(DispatchError::NotImplemented(ctx.operation.clone())),
        }
    }

    async fn storage_snapshot(&self) -> Option<serde_json::Value> {
        use serde_json::json;
        let mut instances = Vec::new();
        for entry in self.store.iter() {
            for db in entry.value().instances.values() {
                instances.push(json!({
                    "id": db.db_instance_identifier, "kind": "db_instance",
                    "attributes": [
                        {"key": "status", "value": db.db_instance_status.clone()},
                        {"key": "engine", "value": db.engine.clone()},
                        {"key": "class", "value": db.db_instance_class.clone()},
                    ]
                }));
            }
        }
        Some(json!({ "kind": "rds", "instances": instances }))
    }
}
