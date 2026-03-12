use std::collections::HashMap;
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

use crate::store::{CfnStack, CloudFormationStore, StackResource, StackStatus};

pub struct CloudFormationProvider {
    store: Arc<AccountRegionBundle<CloudFormationStore>>,
}

impl CloudFormationProvider {
    pub fn new() -> Self {
        Self {
            store: Arc::new(AccountRegionBundle::new()),
        }
    }
}

impl Default for CloudFormationProvider {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn xml_ok(body: String) -> DispatchResponse {
    DispatchResponse {
        status_code: 200,
        body: ResponseBody::Buffered(Bytes::from(body.into_bytes())),
        content_type: "text/xml".to_string(),
        headers: Vec::new(),
    }
}

fn xml_error(code: &str, message: &str, status: u16) -> DispatchResponse {
    DispatchResponse {
        status_code: status,
        body: ResponseBody::Buffered(Bytes::from(format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<ErrorResponse>
  <Error>
    <Type>Sender</Type>
    <Code>{code}</Code>
    <Message>{message}</Message>
  </Error>
</ErrorResponse>"#
        ))),
        content_type: "text/xml".to_string(),
        headers: Vec::new(),
    }
}

fn str_param<'a>(ctx: &'a RequestContext, key: &str) -> Option<&'a str> {
    ctx.query_params
        .get(key)
        .map(|s| s.as_str())
        .or_else(|| ctx.request_body.get(key).and_then(|v| v.as_str()))
}

/// Resolve CloudFormation intrinsic functions within a template value.
fn resolve_value(
    v: &Value,
    resources: &HashMap<String, StackResource>,
    params: &HashMap<String, String>,
    region: &str,
    account_id: &str,
) -> Value {
    match v {
        Value::Object(obj) => {
            // Ref
            if let Some(r) = obj.get("Ref").and_then(|v| v.as_str()) {
                if let Some(p) = params.get(r) {
                    return Value::String(p.clone());
                }
                if let Some(res) = resources.get(r) {
                    return Value::String(res.physical_id.clone());
                }
                // Pseudo-params
                return Value::String(match r {
                    "AWS::Region" => region.to_string(),
                    "AWS::AccountId" => account_id.to_string(),
                    "AWS::NoValue" => "".to_string(),
                    other => other.to_string(),
                });
            }
            // Fn::GetAtt
            if let Some(arr) = obj.get("Fn::GetAtt").and_then(|v| v.as_array())
                && arr.len() == 2
            {
                let logical = arr[0].as_str().unwrap_or("");
                let attr = arr[1].as_str().unwrap_or("");
                if let Some(res) = resources.get(logical) {
                    // Return ARN-like for Arn attributes, physical_id for others
                    return Value::String(if attr == "Arn" {
                        format!(
                            "arn:aws:{}:{}:{}:{}",
                            res.resource_type.to_lowercase(),
                            region,
                            account_id,
                            res.physical_id
                        )
                    } else {
                        res.physical_id.clone()
                    });
                }
            }
            // Fn::Join
            if let Some(arr) = obj.get("Fn::Join").and_then(|v| v.as_array())
                && arr.len() == 2
            {
                let sep = arr[0].as_str().unwrap_or("");
                if let Some(items) = arr[1].as_array() {
                    let parts: Vec<String> = items
                        .iter()
                        .map(|i| {
                            resolve_value(i, resources, params, region, account_id)
                                .as_str()
                                .unwrap_or("")
                                .to_string()
                        })
                        .collect();
                    return Value::String(parts.join(sep));
                }
            }
            // Fn::Sub
            if let Some(s) = obj.get("Fn::Sub").and_then(|v| v.as_str()) {
                let mut result = s.to_string();
                for (k, v) in params {
                    result = result.replace(&format!("${{{k}}}"), v);
                }
                for (k, v) in resources {
                    result = result.replace(&format!("${{{k}}}"), &v.physical_id);
                }
                result = result.replace("${AWS::Region}", region);
                result = result.replace("${AWS::AccountId}", account_id);
                return Value::String(result);
            }
            // Fn::Select
            if let Some(arr) = obj.get("Fn::Select").and_then(|v| v.as_array())
                && arr.len() == 2
            {
                let idx = arr[0].as_u64().unwrap_or(0) as usize;
                if let Some(items) = arr[1].as_array()
                    && let Some(item) = items.get(idx)
                {
                    return resolve_value(item, resources, params, region, account_id);
                }
            }
            // Fn::If - always use first branch (always true)
            if let Some(arr) = obj.get("Fn::If").and_then(|v| v.as_array())
                && arr.len() == 3
            {
                return resolve_value(&arr[1], resources, params, region, account_id);
            }
            // Fn::Split
            if let Some(arr) = obj.get("Fn::Split").and_then(|v| v.as_array())
                && arr.len() == 2
            {
                let sep = arr[0].as_str().unwrap_or(",");
                let s = arr[1].as_str().unwrap_or("");
                return Value::Array(s.split(sep).map(|p| Value::String(p.to_string())).collect());
            }
            // Recurse
            Value::Object(
                obj.iter()
                    .map(|(k, v)| {
                        (
                            k.clone(),
                            resolve_value(v, resources, params, region, account_id),
                        )
                    })
                    .collect(),
            )
        }
        Value::Array(arr) => Value::Array(
            arr.iter()
                .map(|v| resolve_value(v, resources, params, region, account_id))
                .collect(),
        ),
        other => other.clone(),
    }
}

/// Synthesise a physical resource ID for a given logical ID and resource type.
fn make_physical_id(logical_id: &str, resource_type: &str) -> String {
    let suffix = &Uuid::new_v4().to_string()[..8];
    match resource_type {
        "AWS::S3::Bucket" => format!("{}-{}", logical_id.to_lowercase(), suffix),
        "AWS::SQS::Queue" => format!("{}-{}", logical_id, suffix),
        "AWS::SNS::Topic" => format!("{}-{}", logical_id, suffix),
        "AWS::DynamoDB::Table" => logical_id.to_string(),
        "AWS::Lambda::Function" => logical_id.to_string(),
        "AWS::IAM::Role" => logical_id.to_string(),
        "AWS::IAM::Policy" => format!("{}-{}", logical_id, suffix),
        _ => format!("{}-{}", logical_id, suffix),
    }
}

fn stack_to_xml(stack: &CfnStack) -> String {
    let params_xml: String = stack
        .parameters
        .iter()
        .map(|(k, v)| format!("<member><ParameterKey>{k}</ParameterKey><ParameterValue>{v}</ParameterValue></member>"))
        .collect();
    let outputs_xml: String = stack
        .outputs
        .iter()
        .map(|(k, v)| {
            format!("<member><OutputKey>{k}</OutputKey><OutputValue>{v}</OutputValue></member>")
        })
        .collect();
    format!(
        r#"<member>
  <StackId>{stack_id}</StackId>
  <StackName>{stack_name}</StackName>
  <Description>{description}</Description>
  <StackStatus>{status}</StackStatus>
  <StackStatusReason>{reason}</StackStatusReason>
  <CreationTime>{created}</CreationTime>
  <LastUpdatedTime>{updated}</LastUpdatedTime>
  <Parameters>{params}</Parameters>
  <Outputs>{outputs}</Outputs>
</member>"#,
        stack_id = stack.stack_id,
        stack_name = stack.stack_name,
        description = stack.description,
        status = stack.status.as_str(),
        reason = stack.status_reason,
        created = stack.created.to_rfc3339(),
        updated = stack.updated.to_rfc3339(),
        params = params_xml,
        outputs = outputs_xml,
    )
}

// ---------------------------------------------------------------------------
// ServiceProvider
// ---------------------------------------------------------------------------

#[async_trait]
impl ServiceProvider for CloudFormationProvider {
    fn service_name(&self) -> &str {
        "cloudformation"
    }

    async fn dispatch(&self, ctx: &RequestContext) -> Result<DispatchResponse, DispatchError> {
        let region = &ctx.region;
        let account_id = &ctx.account_id;

        match ctx.operation.as_str() {
            // ----------------------------------------------------------------
            // CreateStack
            // ----------------------------------------------------------------
            "CreateStack" => {
                let stack_name = match str_param(ctx, "StackName") {
                    Some(n) => n.to_string(),
                    None => return Ok(xml_error("ValidationError", "StackName is required", 400)),
                };
                let template_body = str_param(ctx, "TemplateBody").unwrap_or("{}");
                let template: Value = serde_json::from_str(template_body).unwrap_or(json!({}));

                // Parse parameters
                let mut parameters: HashMap<String, String> = HashMap::new();
                // Parameters come as Parameters.member.N.ParameterKey / ParameterValue
                let mut i = 1;
                loop {
                    let key_k = format!("Parameters.member.{i}.ParameterKey");
                    let val_k = format!("Parameters.member.{i}.ParameterValue");
                    let k = ctx
                        .query_params
                        .get(&key_k)
                        .or_else(|| ctx.query_params.get(&key_k));
                    let v = ctx.query_params.get(&val_k);
                    if k.is_none() && v.is_none() {
                        break;
                    }
                    if let (Some(k), Some(v)) = (k, v) {
                        parameters.insert(k.clone(), v.clone());
                    }
                    i += 1;
                }

                let stack_id = format!(
                    "arn:aws:cloudformation:{region}:{account_id}:stack/{stack_name}/{}",
                    Uuid::new_v4()
                );

                // Process template resources
                let mut resources: HashMap<String, StackResource> = HashMap::new();
                let mut outputs: HashMap<String, String> = HashMap::new();

                if let Some(cfn_resources) = template.get("Resources").and_then(|r| r.as_object()) {
                    for (logical_id, resource_def) in cfn_resources {
                        let resource_type = resource_def
                            .get("Type")
                            .and_then(|v| v.as_str())
                            .unwrap_or("Unknown");
                        let physical_id = make_physical_id(logical_id, resource_type);
                        resources.insert(
                            logical_id.clone(),
                            StackResource {
                                logical_id: logical_id.clone(),
                                physical_id,
                                resource_type: resource_type.to_string(),
                                status: "CREATE_COMPLETE".to_string(),
                            },
                        );
                    }
                }

                // Resolve outputs
                if let Some(cfn_outputs) = template.get("Outputs").and_then(|o| o.as_object()) {
                    for (output_key, output_def) in cfn_outputs {
                        if let Some(value_def) = output_def.get("Value") {
                            let resolved = resolve_value(
                                value_def,
                                &resources,
                                &parameters,
                                region,
                                account_id,
                            );
                            outputs.insert(
                                output_key.clone(),
                                resolved.as_str().unwrap_or("").to_string(),
                            );
                        }
                    }
                }

                let now = Utc::now();
                let stack = CfnStack {
                    stack_id: stack_id.clone(),
                    stack_name: stack_name.clone(),
                    description: template
                        .get("Description")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    status: StackStatus::CreateComplete,
                    status_reason: String::new(),
                    template,
                    parameters,
                    outputs,
                    resources,
                    created: now,
                    updated: now,
                };

                {
                    let mut store = self.store.get_or_create(account_id, region);
                    if store.stacks.contains_key(&stack_name) {
                        return Ok(xml_error(
                            "AlreadyExistsException",
                            &format!("Stack [{stack_name}] already exists"),
                            400,
                        ));
                    }
                    store.stacks.insert(stack_name.clone(), stack);
                }

                Ok(xml_ok(format!(
                    r#"<?xml version="1.0" encoding="UTF-8"?>
<CreateStackResponse xmlns="https://cloudformation.amazonaws.com/doc/2010-05-15/">
  <CreateStackResult>
    <StackId>{stack_id}</StackId>
  </CreateStackResult>
</CreateStackResponse>"#
                )))
            }

            // ----------------------------------------------------------------
            // UpdateStack
            // ----------------------------------------------------------------
            "UpdateStack" => {
                let stack_name = match str_param(ctx, "StackName") {
                    Some(n) => n.to_string(),
                    None => return Ok(xml_error("ValidationError", "StackName is required", 400)),
                };
                let template_body = str_param(ctx, "TemplateBody").unwrap_or("{}");
                let template: Value = serde_json::from_str(template_body).unwrap_or(json!({}));

                let mut store = self.store.get_or_create(account_id, region);
                match store.stacks.get_mut(&stack_name) {
                    Some(stack) => {
                        stack.template = template;
                        stack.status = StackStatus::UpdateComplete;
                        stack.updated = Utc::now();
                        let stack_id = stack.stack_id.clone();
                        Ok(xml_ok(format!(
                            r#"<?xml version="1.0" encoding="UTF-8"?>
<UpdateStackResponse xmlns="https://cloudformation.amazonaws.com/doc/2010-05-15/">
  <UpdateStackResult>
    <StackId>{stack_id}</StackId>
  </UpdateStackResult>
</UpdateStackResponse>"#
                        )))
                    }
                    None => Ok(xml_error(
                        "ValidationError",
                        &format!("Stack [{stack_name}] does not exist"),
                        400,
                    )),
                }
            }

            // ----------------------------------------------------------------
            // DeleteStack
            // ----------------------------------------------------------------
            "DeleteStack" => {
                let stack_name = match str_param(ctx, "StackName") {
                    Some(n) => n.to_string(),
                    None => return Ok(xml_error("ValidationError", "StackName is required", 400)),
                };

                let mut store = self.store.get_or_create(account_id, region);
                store.stacks.remove(&stack_name);

                Ok(xml_ok(
                    r#"<?xml version="1.0" encoding="UTF-8"?>
<DeleteStackResponse xmlns="https://cloudformation.amazonaws.com/doc/2010-05-15/">
  <ResponseMetadata><RequestId>x</RequestId></ResponseMetadata>
</DeleteStackResponse>"#
                        .to_string(),
                ))
            }

            // ----------------------------------------------------------------
            // DescribeStacks
            // ----------------------------------------------------------------
            "DescribeStacks" => {
                let stack_name = str_param(ctx, "StackName");
                let store = self.store.get_or_create(account_id, region);

                let stacks_xml: String = store
                    .stacks
                    .values()
                    .filter(|s| stack_name.map(|n| n == s.stack_name).unwrap_or(true))
                    .map(stack_to_xml)
                    .collect();

                Ok(xml_ok(format!(
                    r#"<?xml version="1.0" encoding="UTF-8"?>
<DescribeStacksResponse xmlns="https://cloudformation.amazonaws.com/doc/2010-05-15/">
  <DescribeStacksResult>
    <Stacks>{stacks_xml}</Stacks>
  </DescribeStacksResult>
</DescribeStacksResponse>"#
                )))
            }

            // ----------------------------------------------------------------
            // ListStacks
            // ----------------------------------------------------------------
            "ListStacks" => {
                let store = self.store.get_or_create(account_id, region);
                let summaries: String = store
                    .stacks
                    .values()
                    .map(|s| {
                        format!(
                            r#"<member>
  <StackId>{}</StackId>
  <StackName>{}</StackName>
  <StackStatus>{}</StackStatus>
  <CreationTime>{}</CreationTime>
</member>"#,
                            s.stack_id,
                            s.stack_name,
                            s.status.as_str(),
                            s.created.to_rfc3339()
                        )
                    })
                    .collect();

                Ok(xml_ok(format!(
                    r#"<?xml version="1.0" encoding="UTF-8"?>
<ListStacksResponse xmlns="https://cloudformation.amazonaws.com/doc/2010-05-15/">
  <ListStacksResult>
    <StackSummaries>{summaries}</StackSummaries>
  </ListStacksResult>
</ListStacksResponse>"#
                )))
            }

            // ----------------------------------------------------------------
            // DescribeStackResources
            // ----------------------------------------------------------------
            "DescribeStackResources" => {
                let stack_name = match str_param(ctx, "StackName") {
                    Some(n) => n.to_string(),
                    None => return Ok(xml_error("ValidationError", "StackName is required", 400)),
                };
                let store = self.store.get_or_create(account_id, region);
                let resources_xml = match store.stacks.get(&stack_name) {
                    Some(stack) => stack
                        .resources
                        .values()
                        .map(|r| {
                            format!(
                                r#"<member>
  <StackName>{}</StackName>
  <LogicalResourceId>{}</LogicalResourceId>
  <PhysicalResourceId>{}</PhysicalResourceId>
  <ResourceType>{}</ResourceType>
  <ResourceStatus>{}</ResourceStatus>
</member>"#,
                                stack_name, r.logical_id, r.physical_id, r.resource_type, r.status
                            )
                        })
                        .collect::<String>(),
                    None => {
                        return Ok(xml_error(
                            "ValidationError",
                            &format!("Stack [{stack_name}] does not exist"),
                            400,
                        ));
                    }
                };

                Ok(xml_ok(format!(
                    r#"<?xml version="1.0" encoding="UTF-8"?>
<DescribeStackResourcesResponse xmlns="https://cloudformation.amazonaws.com/doc/2010-05-15/">
  <DescribeStackResourcesResult>
    <StackResources>{resources_xml}</StackResources>
  </DescribeStackResourcesResult>
</DescribeStackResourcesResponse>"#
                )))
            }

            // ----------------------------------------------------------------
            // GetTemplate
            // ----------------------------------------------------------------
            "GetTemplate" => {
                let stack_name = match str_param(ctx, "StackName") {
                    Some(n) => n.to_string(),
                    None => return Ok(xml_error("ValidationError", "StackName is required", 400)),
                };
                let store = self.store.get_or_create(account_id, region);
                match store.stacks.get(&stack_name) {
                    Some(stack) => {
                        let template_str =
                            serde_json::to_string(&stack.template).unwrap_or_default();
                        Ok(xml_ok(format!(
                            r#"<?xml version="1.0" encoding="UTF-8"?>
<GetTemplateResponse xmlns="https://cloudformation.amazonaws.com/doc/2010-05-15/">
  <GetTemplateResult>
    <TemplateBody>{template_str}</TemplateBody>
  </GetTemplateResult>
</GetTemplateResponse>"#
                        )))
                    }
                    None => Ok(xml_error(
                        "ValidationError",
                        &format!("Stack [{stack_name}] does not exist"),
                        400,
                    )),
                }
            }

            // ----------------------------------------------------------------
            // ValidateTemplate
            // ----------------------------------------------------------------
            "ValidateTemplate" => Ok(xml_ok(
                r#"<?xml version="1.0" encoding="UTF-8"?>
<ValidateTemplateResponse xmlns="https://cloudformation.amazonaws.com/doc/2010-05-15/">
  <ValidateTemplateResult>
    <Parameters/>
    <Description/>
    <Capabilities/>
  </ValidateTemplateResult>
</ValidateTemplateResponse>"#
                    .to_string(),
            )),

            _ => Err(DispatchError::NotImplemented(ctx.operation.clone())),
        }
    }
}
