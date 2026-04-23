use std::borrow::Cow;
use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use chrono::Utc;
use openstack_service_framework::traits::{
    DispatchError, DispatchResponse, RequestContext, ResponseBody, ServiceProvider,
};
use openstack_state::AccountRegionBundle;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::store::{Command, CommandInvocation, Document, Parameter, ParameterType, SsmStore};

pub struct SsmProvider {
    store: Arc<AccountRegionBundle<SsmStore>>,
}

impl SsmProvider {
    pub fn new() -> Self {
        Self {
            store: Arc::new(AccountRegionBundle::new()),
        }
    }
}

impl Default for SsmProvider {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// JSON response helpers
// ---------------------------------------------------------------------------

fn json_ok(body: Value) -> DispatchResponse {
    DispatchResponse {
        status_code: 200,
        body: ResponseBody::Buffered(Bytes::from(serde_json::to_vec(&body).unwrap())),
        content_type: Cow::Borrowed("application/x-amz-json-1.1"),
        headers: Vec::new(),
    }
}

fn json_error(code: &str, message: &str, status: u16) -> DispatchResponse {
    DispatchResponse {
        status_code: status,
        body: ResponseBody::Buffered(Bytes::from(
            serde_json::to_vec(&json!({
                "__type": code,
                "message": message,
            }))
            .unwrap(),
        )),
        content_type: Cow::Borrowed("application/json"),
        headers: Vec::new(),
    }
}

fn str_param(ctx: &RequestContext, key: &str) -> Option<String> {
    ctx.request_body
        .get(key)
        .and_then(|v| v.as_str())
        .map(String::from)
}

fn param_to_json(p: &Parameter) -> Value {
    json!({
        "Name": p.name,
        "Type": p.type_.as_str(),
        "Value": p.value,
        "Version": p.version,
        "LastModifiedDate": p.last_modified.timestamp(),
        "ARN": p.arn,
        "DataType": "text",
    })
}

fn req_id() -> String {
    Uuid::new_v4().to_string()
}

// ---------------------------------------------------------------------------
// ServiceProvider
// ---------------------------------------------------------------------------

#[async_trait]
impl ServiceProvider for SsmProvider {
    fn service_name(&self) -> &str {
        "ssm"
    }

    async fn dispatch(&self, ctx: &RequestContext) -> Result<DispatchResponse, DispatchError> {
        let op = ctx.operation.as_str();
        let account_id = &ctx.account_id;
        let region = &ctx.region;

        match op {
            // ----------------------------------------------------------------
            // PutParameter
            // ----------------------------------------------------------------
            "PutParameter" => {
                let name = match str_param(ctx, "Name") {
                    Some(n) => n,
                    None => return Ok(json_error("ValidationException", "Name is required", 400)),
                };
                let value = match str_param(ctx, "Value") {
                    Some(v) => v,
                    None => return Ok(json_error("ValidationException", "Value is required", 400)),
                };
                let type_str = str_param(ctx, "Type").unwrap_or_else(|| "String".to_string());
                let type_ = type_str
                    .parse::<ParameterType>()
                    .unwrap_or(ParameterType::String);
                let description = str_param(ctx, "Description").unwrap_or_default();
                let overwrite = ctx
                    .request_body
                    .get("Overwrite")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);

                let mut store = self.store.get_or_create(account_id, region);
                if store.parameters.contains_key(&name) && !overwrite {
                    return Ok(json_error(
                        "ParameterAlreadyExists",
                        &format!("Parameter {name} already exists"),
                        400,
                    ));
                }
                let version = store
                    .parameters
                    .get(&name)
                    .map(|p| p.version + 1)
                    .unwrap_or(1);
                let arn = format!("arn:aws:ssm:{region}:{account_id}:parameter{name}");
                let param = Parameter {
                    name: name.clone(),
                    type_,
                    value,
                    description,
                    version,
                    last_modified: Utc::now(),
                    arn,
                    overwrite,
                };
                // Keep version history
                store
                    .parameter_history
                    .entry(name.clone())
                    .or_default()
                    .push(param.clone());
                store.parameters.insert(name, param);
                Ok(json_ok(json!({ "Version": version, "Tier": "Standard" })))
            }

            // ----------------------------------------------------------------
            // GetParameter
            // ----------------------------------------------------------------
            "GetParameter" => {
                let name = match str_param(ctx, "Name") {
                    Some(n) => n,
                    None => return Ok(json_error("ValidationException", "Name is required", 400)),
                };
                let Some(store) = self.store.get(account_id, region) else {
                    return Ok(json_error(
                        "ParameterNotFound",
                        &format!("Parameter {name} not found."),
                        400,
                    ));
                };
                match store.parameters.get(&name) {
                    None => Ok(json_error(
                        "ParameterNotFound",
                        &format!("Parameter {name} not found."),
                        400,
                    )),
                    Some(p) => Ok(json_ok(json!({ "Parameter": param_to_json(p) }))),
                }
            }

            // ----------------------------------------------------------------
            // GetParameters
            // ----------------------------------------------------------------
            "GetParameters" => {
                let names = ctx
                    .request_body
                    .get("Names")
                    .and_then(|v| v.as_array())
                    .cloned()
                    .unwrap_or_default();
                let Some(store) = self.store.get(account_id, region) else {
                    let invalid_parameters: Vec<Value> = names
                        .iter()
                        .filter_map(|n| n.as_str().map(|s| json!(s)))
                        .collect();
                    return Ok(json_ok(json!({
                        "Parameters": [],
                        "InvalidParameters": invalid_parameters,
                    })));
                };
                let mut parameters = Vec::new();
                let mut invalid_parameters = Vec::new();
                for n in &names {
                    if let Some(name) = n.as_str() {
                        match store.parameters.get(name) {
                            Some(p) => parameters.push(param_to_json(p)),
                            None => invalid_parameters.push(json!(name)),
                        }
                    }
                }
                Ok(json_ok(json!({
                    "Parameters": parameters,
                    "InvalidParameters": invalid_parameters,
                })))
            }

            // ----------------------------------------------------------------
            // GetParametersByPath
            // ----------------------------------------------------------------
            "GetParametersByPath" => {
                let path = match str_param(ctx, "Path") {
                    Some(p) => p,
                    None => return Ok(json_error("ValidationException", "Path is required", 400)),
                };
                let recursive = ctx
                    .request_body
                    .get("Recursive")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let Some(store) = self.store.get(account_id, region) else {
                    return Ok(json_ok(json!({ "Parameters": [] })));
                };
                let params: Vec<Value> = store
                    .parameters
                    .values()
                    .filter(|p| {
                        if recursive {
                            p.name.starts_with(&path)
                        } else {
                            p.name.starts_with(&path)
                                && !p.name[path.len()..].trim_start_matches('/').contains('/')
                        }
                    })
                    .map(param_to_json)
                    .collect();
                Ok(json_ok(json!({ "Parameters": params })))
            }

            // ----------------------------------------------------------------
            // DeleteParameter
            // ----------------------------------------------------------------
            "DeleteParameter" => {
                let name = match str_param(ctx, "Name") {
                    Some(n) => n,
                    None => return Ok(json_error("ValidationException", "Name is required", 400)),
                };
                let mut store = self.store.get_or_create(account_id, region);
                if store.parameters.remove(&name).is_none() {
                    return Ok(json_error(
                        "ParameterNotFound",
                        &format!("Parameter {name} not found"),
                        400,
                    ));
                }
                Ok(json_ok(json!({})))
            }

            // ----------------------------------------------------------------
            // DeleteParameters
            // ----------------------------------------------------------------
            "DeleteParameters" => {
                let names = ctx
                    .request_body
                    .get("Names")
                    .and_then(|v| v.as_array())
                    .cloned()
                    .unwrap_or_default();
                let mut store = self.store.get_or_create(account_id, region);
                let mut deleted = Vec::new();
                let mut invalid = Vec::new();
                for n in &names {
                    if let Some(name) = n.as_str() {
                        if store.parameters.remove(name).is_some() {
                            deleted.push(json!(name));
                        } else {
                            invalid.push(json!(name));
                        }
                    }
                }
                Ok(json_ok(json!({
                    "DeletedParameters": deleted,
                    "InvalidParameters": invalid,
                })))
            }

            // ----------------------------------------------------------------
            // DescribeParameters
            // ----------------------------------------------------------------
            "DescribeParameters" => {
                let Some(store) = self.store.get(account_id, region) else {
                    return Ok(json_ok(json!({ "Parameters": [] })));
                };
                let params: Vec<Value> = store
                    .parameters
                    .values()
                    .map(|p| {
                        json!({
                            "Name": p.name,
                            "Type": p.type_.as_str(),
                            "Description": p.description,
                            "Version": p.version,
                            "LastModifiedDate": p.last_modified.timestamp(),
                            "ARN": p.arn,
                        })
                    })
                    .collect();
                Ok(json_ok(json!({ "Parameters": params })))
            }

            // ----------------------------------------------------------------
            // GetParameterHistory
            // ----------------------------------------------------------------
            "GetParameterHistory" => {
                let name = match str_param(ctx, "Name") {
                    Some(n) => n,
                    None => return Ok(json_error("ValidationException", "Name is required", 400)),
                };
                let Some(store) = self.store.get(account_id, region) else {
                    return Ok(json_error(
                        "ParameterNotFound",
                        &format!("Parameter {name} not found"),
                        400,
                    ));
                };
                if !store.parameters.contains_key(&name) {
                    return Ok(json_error(
                        "ParameterNotFound",
                        &format!("Parameter {name} not found"),
                        400,
                    ));
                }
                let history: Vec<Value> = store
                    .parameter_history
                    .get(&name)
                    .map(|versions| versions.iter().map(param_to_json).collect())
                    .unwrap_or_default();
                Ok(json_ok(json!({ "Parameters": history })))
            }

            // ----------------------------------------------------------------
            // CreateDocument
            // ----------------------------------------------------------------
            "CreateDocument" => {
                let name = match str_param(ctx, "Name") {
                    Some(n) => n,
                    None => return Ok(json_error("ValidationException", "Name is required", 400)),
                };
                let content = match str_param(ctx, "Content") {
                    Some(c) => c,
                    None => {
                        return Ok(json_error(
                            "ValidationException",
                            "Content is required",
                            400,
                        ));
                    }
                };
                let document_type =
                    str_param(ctx, "DocumentType").unwrap_or_else(|| "Command".to_string());
                let document_format =
                    str_param(ctx, "DocumentFormat").unwrap_or_else(|| "JSON".to_string());

                let mut store = self.store.get_or_create(account_id, region);
                if store.documents.contains_key(&name) {
                    return Ok(json_error(
                        "DocumentAlreadyExists",
                        &format!("Document {name} already exists"),
                        400,
                    ));
                }
                let doc = Document {
                    name: name.clone(),
                    document_type: document_type.clone(),
                    document_format: document_format.clone(),
                    schema_version: "2.2".to_string(),
                    status: "Active".to_string(),
                    content,
                    owner: account_id.clone(),
                    created: Utc::now(),
                    tags: Default::default(),
                };
                store.documents.insert(name.clone(), doc);
                Ok(json_ok(json!({
                    "DocumentDescription": {
                        "Name": name,
                        "DocumentType": document_type,
                        "DocumentFormat": document_format,
                        "Status": "Active",
                        "SchemaVersion": "2.2",
                        "Owner": account_id,
                    }
                })))
            }

            // ----------------------------------------------------------------
            // DeleteDocument
            // ----------------------------------------------------------------
            "DeleteDocument" => {
                let name = match str_param(ctx, "Name") {
                    Some(n) => n,
                    None => return Ok(json_error("ValidationException", "Name is required", 400)),
                };
                let mut store = self.store.get_or_create(account_id, region);
                if store.documents.remove(&name).is_none() {
                    return Ok(json_error(
                        "InvalidDocument",
                        &format!("Document {name} does not exist"),
                        400,
                    ));
                }
                Ok(json_ok(json!({})))
            }

            // ----------------------------------------------------------------
            // DescribeDocument
            // ----------------------------------------------------------------
            "DescribeDocument" => {
                let name = match str_param(ctx, "Name") {
                    Some(n) => n,
                    None => return Ok(json_error("ValidationException", "Name is required", 400)),
                };
                let Some(store) = self.store.get(account_id, region) else {
                    return Ok(json_error(
                        "InvalidDocument",
                        &format!("Document {name} does not exist"),
                        400,
                    ));
                };
                match store.documents.get(&name) {
                    Some(doc) => Ok(json_ok(json!({
                        "Document": {
                            "Name": doc.name,
                            "DocumentType": doc.document_type,
                            "DocumentFormat": doc.document_format,
                            "SchemaVersion": doc.schema_version,
                            "Status": doc.status,
                            "Owner": doc.owner,
                        }
                    }))),
                    None => Ok(json_error(
                        "InvalidDocument",
                        &format!("Document {name} does not exist"),
                        400,
                    )),
                }
            }

            // ----------------------------------------------------------------
            // GetDocument
            // ----------------------------------------------------------------
            "GetDocument" => {
                let name = match str_param(ctx, "Name") {
                    Some(n) => n,
                    None => return Ok(json_error("ValidationException", "Name is required", 400)),
                };
                let Some(store) = self.store.get(account_id, region) else {
                    return Ok(json_error(
                        "InvalidDocument",
                        &format!("Document {name} does not exist"),
                        400,
                    ));
                };
                match store.documents.get(&name) {
                    Some(doc) => Ok(json_ok(json!({
                        "Name": doc.name,
                        "DocumentType": doc.document_type,
                        "DocumentFormat": doc.document_format,
                        "Content": doc.content,
                        "Status": doc.status,
                    }))),
                    None => Ok(json_error(
                        "InvalidDocument",
                        &format!("Document {name} does not exist"),
                        400,
                    )),
                }
            }

            // ----------------------------------------------------------------
            // ListDocuments
            // ----------------------------------------------------------------
            "ListDocuments" => {
                let Some(store) = self.store.get(account_id, region) else {
                    return Ok(json_ok(json!({ "DocumentIdentifiers": [] })));
                };
                let docs: Vec<Value> = store
                    .documents
                    .values()
                    .map(|d| {
                        json!({
                            "Name": d.name,
                            "DocumentType": d.document_type,
                            "DocumentFormat": d.document_format,
                            "SchemaVersion": d.schema_version,
                            "Owner": d.owner,
                        })
                    })
                    .collect();
                Ok(json_ok(json!({ "DocumentIdentifiers": docs })))
            }

            // ----------------------------------------------------------------
            // SendCommand
            // ----------------------------------------------------------------
            "SendCommand" => {
                let document_name = match str_param(ctx, "DocumentName") {
                    Some(n) => n,
                    None => {
                        return Ok(json_error(
                            "ValidationException",
                            "DocumentName is required",
                            400,
                        ));
                    }
                };
                let instance_ids: Vec<String> = ctx
                    .request_body
                    .get("InstanceIds")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str().map(String::from))
                            .collect()
                    })
                    .unwrap_or_default();

                let command_id = req_id();
                let now = Utc::now();

                let invocations: Vec<CommandInvocation> = instance_ids
                    .iter()
                    .map(|iid| CommandInvocation {
                        command_id: command_id.clone(),
                        instance_id: iid.clone(),
                        document_name: document_name.clone(),
                        status: "Success".to_string(),
                        status_details: "Success".to_string(),
                        output: String::new(),
                        response_code: 0,
                    })
                    .collect();

                let command = Command {
                    command_id: command_id.clone(),
                    document_name: document_name.clone(),
                    status: "Success".to_string(),
                    requested_date: now,
                    instance_ids: instance_ids.clone(),
                    invocations,
                };

                let mut store = self.store.get_or_create(account_id, region);
                store.commands.insert(command_id.clone(), command);

                Ok(json_ok(json!({
                    "Command": {
                        "CommandId": command_id,
                        "DocumentName": document_name,
                        "Status": "Success",
                        "RequestedDateTime": now.to_rfc3339(),
                        "InstanceIds": instance_ids,
                    }
                })))
            }

            // ----------------------------------------------------------------
            // ListCommands
            // ----------------------------------------------------------------
            "ListCommands" => {
                let command_id_filter = str_param(ctx, "CommandId");
                let instance_id_filter = str_param(ctx, "InstanceId");

                let Some(store) = self.store.get(account_id, region) else {
                    return Ok(json_ok(json!({ "Commands": [] })));
                };

                let commands: Vec<Value> = store
                    .commands
                    .values()
                    .filter(|c| {
                        command_id_filter
                            .as_deref()
                            .map(|id| c.command_id == id)
                            .unwrap_or(true)
                    })
                    .filter(|c| {
                        instance_id_filter
                            .as_deref()
                            .map(|id| c.instance_ids.contains(&id.to_string()))
                            .unwrap_or(true)
                    })
                    .map(|c| {
                        json!({
                            "CommandId": c.command_id,
                            "DocumentName": c.document_name,
                            "Status": c.status,
                            "RequestedDateTime": c.requested_date.to_rfc3339(),
                            "InstanceIds": c.instance_ids,
                        })
                    })
                    .collect();

                Ok(json_ok(json!({ "Commands": commands })))
            }

            // ----------------------------------------------------------------
            // GetCommandInvocation
            // ----------------------------------------------------------------
            "GetCommandInvocation" => {
                let command_id = match str_param(ctx, "CommandId") {
                    Some(id) => id,
                    None => {
                        return Ok(json_error(
                            "ValidationException",
                            "CommandId is required",
                            400,
                        ));
                    }
                };
                let instance_id = match str_param(ctx, "InstanceId") {
                    Some(id) => id,
                    None => {
                        return Ok(json_error(
                            "ValidationException",
                            "InstanceId is required",
                            400,
                        ));
                    }
                };

                let Some(store) = self.store.get(account_id, region) else {
                    return Ok(json_error(
                        "InvocationDoesNotExist",
                        "Command invocation not found",
                        400,
                    ));
                };

                let invocation = store.commands.get(&command_id).and_then(|cmd| {
                    cmd.invocations
                        .iter()
                        .find(|inv| inv.instance_id == instance_id)
                });

                match invocation {
                    Some(inv) => Ok(json_ok(json!({
                        "CommandId": inv.command_id,
                        "InstanceId": inv.instance_id,
                        "DocumentName": inv.document_name,
                        "Status": inv.status,
                        "StatusDetails": inv.status_details,
                        "StandardOutputContent": inv.output,
                        "ResponseCode": inv.response_code,
                    }))),
                    None => Ok(json_error(
                        "InvocationDoesNotExist",
                        "Command invocation not found",
                        400,
                    )),
                }
            }

            // ----------------------------------------------------------------
            // AddTagsToResource / ListTagsForResource / RemoveTagsFromResource
            // ----------------------------------------------------------------
            "AddTagsToResource" => {
                let resource_type = match str_param(ctx, "ResourceType") {
                    Some(t) => t,
                    None => {
                        return Ok(json_error(
                            "ValidationException",
                            "ResourceType is required",
                            400,
                        ));
                    }
                };
                let resource_id = match str_param(ctx, "ResourceId") {
                    Some(id) => id,
                    None => {
                        return Ok(json_error(
                            "ValidationException",
                            "ResourceId is required",
                            400,
                        ));
                    }
                };
                let tag_list = ctx
                    .request_body
                    .get("Tags")
                    .and_then(|v| v.as_array())
                    .cloned()
                    .unwrap_or_default();

                let mut store = self.store.get_or_create(account_id, region);
                if resource_type == "Document"
                    && let Some(doc) = store.documents.get_mut(&resource_id)
                {
                    for tag in &tag_list {
                        if let (Some(k), Some(v)) = (
                            tag.get("Key").and_then(|v| v.as_str()),
                            tag.get("Value").and_then(|v| v.as_str()),
                        ) {
                            doc.tags.insert(k.to_string(), v.to_string());
                        }
                    }
                }
                Ok(json_ok(json!({})))
            }

            "ListTagsForResource" => {
                let resource_type = str_param(ctx, "ResourceType").unwrap_or_default();
                let resource_id = str_param(ctx, "ResourceId").unwrap_or_default();

                let tags: Vec<Value> = self
                    .store
                    .get(account_id, region)
                    .and_then(|store| {
                        if resource_type == "Document" {
                            store.documents.get(&resource_id).map(|doc| {
                                doc.tags
                                    .iter()
                                    .map(|(k, v)| json!({"Key": k, "Value": v}))
                                    .collect()
                            })
                        } else {
                            None
                        }
                    })
                    .unwrap_or_default();

                Ok(json_ok(json!({ "TagList": tags })))
            }

            "RemoveTagsFromResource" => {
                let resource_type = str_param(ctx, "ResourceType").unwrap_or_default();
                let resource_id = str_param(ctx, "ResourceId").unwrap_or_default();
                let tag_keys: Vec<String> = ctx
                    .request_body
                    .get("TagKeys")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str().map(String::from))
                            .collect()
                    })
                    .unwrap_or_default();

                let mut store = self.store.get_or_create(account_id, region);
                if resource_type == "Document"
                    && let Some(doc) = store.documents.get_mut(&resource_id)
                {
                    for k in &tag_keys {
                        doc.tags.remove(k);
                    }
                }
                Ok(json_ok(json!({})))
            }

            _ => Ok(json_error(
                "NotImplementedException",
                &format!("Operation not implemented: {op}"),
                501,
            )),
        }
    }

    async fn storage_snapshot(&self) -> Option<serde_json::Value> {
        use serde_json::json;
        let mut parameters = Vec::new();
        for entry in self.store.iter() {
            let store = entry.value();
            for param in store.parameters.values() {
                parameters.push(json!({
                    "id": param.arn.clone(),
                    "kind": "parameter",
                    "attributes": [
                        {"key": "name", "value": param.name.clone()},
                        {"key": "type", "value": format!("{:?}", param.type_)},
                        {"key": "version", "value": param.version.to_string()},
                    ]
                }));
            }
        }
        Some(json!({ "kind": "ssm", "parameters": parameters }))
    }
}
