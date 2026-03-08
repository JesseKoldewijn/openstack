use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use chrono::Utc;
use openstack_service_framework::traits::{
    DispatchError, DispatchResponse, RequestContext, ServiceProvider,
};
use openstack_state::AccountRegionBundle;
use uuid::Uuid;

use crate::store::{IamGroup, IamPolicy, IamRole, IamStore, IamUser};

pub struct IamProvider {
    store: Arc<AccountRegionBundle<IamStore>>,
}

impl IamProvider {
    pub fn new() -> Self {
        Self {
            store: Arc::new(AccountRegionBundle::new()),
        }
    }
}

impl Default for IamProvider {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// XML helpers — IAM uses query protocol (XML responses)
// ---------------------------------------------------------------------------

fn xml_resp(action: &str, request_id: &str, inner: &str) -> DispatchResponse {
    let xml = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
<{action}Response xmlns=\"https://iam.amazonaws.com/doc/2010-05-08/\">\
<{action}Result>{inner}</{action}Result>\
<ResponseMetadata><RequestId>{request_id}</RequestId></ResponseMetadata>\
</{action}Response>"
    );
    DispatchResponse {
        status_code: 200,
        body: Bytes::from(xml.into_bytes()),
        content_type: "text/xml".to_string(),
        headers: Vec::new(),
    }
}

fn xml_no_result(action: &str, request_id: &str) -> DispatchResponse {
    let xml = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
<{action}Response xmlns=\"https://iam.amazonaws.com/doc/2010-05-08/\">\
<ResponseMetadata><RequestId>{request_id}</RequestId></ResponseMetadata>\
</{action}Response>"
    );
    DispatchResponse {
        status_code: 200,
        body: Bytes::from(xml.into_bytes()),
        content_type: "text/xml".to_string(),
        headers: Vec::new(),
    }
}

fn iam_error(code: &str, message: &str, status: u16) -> DispatchResponse {
    let xml = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
<ErrorResponse xmlns=\"https://iam.amazonaws.com/doc/2010-05-08/\">\
<Error><Type>Sender</Type><Code>{code}</Code><Message>{message}</Message></Error>\
</ErrorResponse>"
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

// ---------------------------------------------------------------------------
// User XML serializer
// ---------------------------------------------------------------------------

fn user_xml(u: &IamUser) -> String {
    format!(
        "<User>\
<UserId>{}</UserId>\
<UserName>{}</UserName>\
<Arn>{}</Arn>\
<Path>{}</Path>\
<CreateDate>{}</CreateDate>\
</User>",
        u.user_id,
        u.user_name,
        u.arn,
        u.path,
        u.created.format("%Y-%m-%dT%H:%M:%SZ"),
    )
}

fn role_xml(r: &IamRole) -> String {
    format!(
        "<Role>\
<RoleId>{}</RoleId>\
<RoleName>{}</RoleName>\
<Arn>{}</Arn>\
<Path>{}</Path>\
<CreateDate>{}</CreateDate>\
<AssumeRolePolicyDocument>{}</AssumeRolePolicyDocument>\
<Description>{}</Description>\
</Role>",
        r.role_id,
        r.role_name,
        r.arn,
        r.path,
        r.created.format("%Y-%m-%dT%H:%M:%SZ"),
        xml_escape(&r.assume_role_policy_document),
        xml_escape(&r.description),
    )
}

fn policy_xml(p: &IamPolicy) -> String {
    format!(
        "<Policy>\
<PolicyId>{}</PolicyId>\
<PolicyName>{}</PolicyName>\
<Arn>{}</Arn>\
<Path>{}</Path>\
<CreateDate>{}</CreateDate>\
</Policy>",
        p.policy_id,
        p.policy_name,
        p.arn,
        p.path,
        p.created.format("%Y-%m-%dT%H:%M:%SZ"),
    )
}

fn group_xml(g: &IamGroup) -> String {
    format!(
        "<Group>\
<GroupId>{}</GroupId>\
<GroupName>{}</GroupName>\
<Arn>{}</Arn>\
<Path>{}</Path>\
<CreateDate>{}</CreateDate>\
</Group>",
        g.group_id,
        g.group_name,
        g.arn,
        g.path,
        g.created.format("%Y-%m-%dT%H:%M:%SZ"),
    )
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn param(ctx: &RequestContext, key: &str) -> Option<String> {
    ctx.query_params.get(key).cloned().or_else(|| {
        if let Some(obj) = ctx.request_body.as_object() {
            obj.get(key).and_then(|v| v.as_str()).map(String::from)
        } else {
            None
        }
    })
}

// ---------------------------------------------------------------------------
// ServiceProvider
// ---------------------------------------------------------------------------

#[async_trait]
impl ServiceProvider for IamProvider {
    fn service_name(&self) -> &str {
        "iam"
    }

    async fn dispatch(&self, ctx: &RequestContext) -> Result<DispatchResponse, DispatchError> {
        let op = ctx.operation.as_str();
        let rid = req_id();
        // IAM is global (no region) — use account_id only; map to us-east-1
        let account_id = &ctx.account_id;
        let region = "us-east-1";

        match op {
            // ---------------------------------------------------------------
            // User operations
            // ---------------------------------------------------------------
            "CreateUser" => {
                let name = match param(ctx, "UserName") {
                    Some(n) => n,
                    None => return Ok(iam_error("ValidationError", "UserName is required", 400)),
                };
                let path = param(ctx, "Path").unwrap_or_else(|| "/".to_string());
                let mut store = self.store.get_or_create(account_id, region);
                if store.users.contains_key(&name) {
                    return Ok(iam_error(
                        "EntityAlreadyExists",
                        &format!("User {name} already exists"),
                        409,
                    ));
                }
                let user = IamUser {
                    user_id: format!(
                        "AIDA{}",
                        &Uuid::new_v4().to_string().replace('-', "")[..16].to_uppercase()
                    ),
                    arn: format!("arn:aws:iam::{account_id}:user{path}{name}"),
                    user_name: name.clone(),
                    path,
                    created: Utc::now(),
                    tags: HashMap::new(),
                    attached_policies: Vec::new(),
                    groups: Vec::new(),
                };
                let xml = user_xml(&user);
                store.users.insert(name, user);
                Ok(xml_resp(
                    "CreateUser",
                    &rid,
                    &format!(
                        "<User>{}</User>",
                        xml.trim_start_matches("<User>").trim_end_matches("</User>")
                    ),
                ))
            }

            "GetUser" => {
                let name = param(ctx, "UserName");
                let store = self.store.get_or_create(account_id, region);
                let user = match &name {
                    Some(n) => store.users.get(n.as_str()),
                    None => {
                        // "get current user" — return a synthetic caller identity
                        let xml = format!(
                            "<User><UserId>AIDADEFAULT</UserId><UserName>default</UserName><Arn>arn:aws:iam::{account_id}:user/default</Arn><Path>/</Path><CreateDate>2020-01-01T00:00:00Z</CreateDate></User>"
                        );
                        return Ok(xml_resp("GetUser", &rid, &xml));
                    }
                };
                match user {
                    None => Ok(iam_error(
                        "NoSuchEntity",
                        &format!("User not found: {}", name.as_deref().unwrap_or("")),
                        404,
                    )),
                    Some(u) => Ok(xml_resp("GetUser", &rid, &user_xml(u))),
                }
            }

            "DeleteUser" => {
                let name = match param(ctx, "UserName") {
                    Some(n) => n,
                    None => return Ok(iam_error("ValidationError", "UserName is required", 400)),
                };
                let mut store = self.store.get_or_create(account_id, region);
                if store.users.remove(&name).is_none() {
                    return Ok(iam_error(
                        "NoSuchEntity",
                        &format!("User {name} not found"),
                        404,
                    ));
                }
                Ok(xml_no_result("DeleteUser", &rid))
            }

            "ListUsers" => {
                let store = self.store.get_or_create(account_id, region);
                let mut users: Vec<String> = store.users.values().map(user_xml).collect();
                users.sort();
                let inner = format!(
                    "<Users>{}</Users><IsTruncated>false</IsTruncated>",
                    users.join("")
                );
                Ok(xml_resp("ListUsers", &rid, &inner))
            }

            // ---------------------------------------------------------------
            // Role operations
            // ---------------------------------------------------------------
            "CreateRole" => {
                let name = match param(ctx, "RoleName") {
                    Some(n) => n,
                    None => return Ok(iam_error("ValidationError", "RoleName is required", 400)),
                };
                let path = param(ctx, "Path").unwrap_or_else(|| "/".to_string());
                let policy_doc = param(ctx, "AssumeRolePolicyDocument").unwrap_or_default();
                let description = param(ctx, "Description").unwrap_or_default();
                let mut store = self.store.get_or_create(account_id, region);
                if store.roles.contains_key(&name) {
                    return Ok(iam_error(
                        "EntityAlreadyExists",
                        &format!("Role {name} already exists"),
                        409,
                    ));
                }
                let role = IamRole {
                    role_id: format!(
                        "AROA{}",
                        &Uuid::new_v4().to_string().replace('-', "")[..16].to_uppercase()
                    ),
                    arn: format!("arn:aws:iam::{account_id}:role{path}{name}"),
                    role_name: name.clone(),
                    path,
                    assume_role_policy_document: policy_doc,
                    description,
                    created: Utc::now(),
                    tags: HashMap::new(),
                    attached_policies: Vec::new(),
                    inline_policies: HashMap::new(),
                };
                let xml = role_xml(&role);
                store.roles.insert(name, role);
                Ok(xml_resp("CreateRole", &rid, &xml))
            }

            "GetRole" => {
                let name = match param(ctx, "RoleName") {
                    Some(n) => n,
                    None => return Ok(iam_error("ValidationError", "RoleName is required", 400)),
                };
                let store = self.store.get_or_create(account_id, region);
                match store.roles.get(&name) {
                    None => Ok(iam_error(
                        "NoSuchEntity",
                        &format!("Role {name} not found"),
                        404,
                    )),
                    Some(r) => Ok(xml_resp("GetRole", &rid, &role_xml(r))),
                }
            }

            "DeleteRole" => {
                let name = match param(ctx, "RoleName") {
                    Some(n) => n,
                    None => return Ok(iam_error("ValidationError", "RoleName is required", 400)),
                };
                let mut store = self.store.get_or_create(account_id, region);
                if store.roles.remove(&name).is_none() {
                    return Ok(iam_error(
                        "NoSuchEntity",
                        &format!("Role {name} not found"),
                        404,
                    ));
                }
                Ok(xml_no_result("DeleteRole", &rid))
            }

            "ListRoles" => {
                let store = self.store.get_or_create(account_id, region);
                let roles: Vec<String> = store.roles.values().map(role_xml).collect();
                let inner = format!(
                    "<Roles>{}</Roles><IsTruncated>false</IsTruncated>",
                    roles.join("")
                );
                Ok(xml_resp("ListRoles", &rid, &inner))
            }

            // ---------------------------------------------------------------
            // Policy operations
            // ---------------------------------------------------------------
            "CreatePolicy" => {
                let name = match param(ctx, "PolicyName") {
                    Some(n) => n,
                    None => return Ok(iam_error("ValidationError", "PolicyName is required", 400)),
                };
                let path = param(ctx, "Path").unwrap_or_else(|| "/".to_string());
                let document = param(ctx, "PolicyDocument").unwrap_or_default();
                let description = param(ctx, "Description").unwrap_or_default();
                let arn = format!("arn:aws:iam::{account_id}:policy{path}{name}");
                let mut store = self.store.get_or_create(account_id, region);
                if store.policies.contains_key(&arn) {
                    return Ok(iam_error(
                        "EntityAlreadyExists",
                        &format!("Policy {name} already exists"),
                        409,
                    ));
                }
                let policy = IamPolicy {
                    policy_id: format!(
                        "ANPA{}",
                        &Uuid::new_v4().to_string().replace('-', "")[..16].to_uppercase()
                    ),
                    policy_name: name,
                    arn: arn.clone(),
                    path,
                    document,
                    description,
                    created: Utc::now(),
                };
                let xml = policy_xml(&policy);
                store.policies.insert(arn, policy);
                Ok(xml_resp("CreatePolicy", &rid, &xml))
            }

            "GetPolicy" => {
                let arn = match param(ctx, "PolicyArn") {
                    Some(n) => n,
                    None => return Ok(iam_error("ValidationError", "PolicyArn is required", 400)),
                };
                let store = self.store.get_or_create(account_id, region);
                match store.policies.get(&arn) {
                    None => Ok(iam_error(
                        "NoSuchEntity",
                        &format!("Policy {arn} not found"),
                        404,
                    )),
                    Some(p) => Ok(xml_resp("GetPolicy", &rid, &policy_xml(p))),
                }
            }

            "ListPolicies" => {
                let store = self.store.get_or_create(account_id, region);
                let policies: Vec<String> = store.policies.values().map(policy_xml).collect();
                let inner = format!(
                    "<Policies>{}</Policies><IsTruncated>false</IsTruncated>",
                    policies.join("")
                );
                Ok(xml_resp("ListPolicies", &rid, &inner))
            }

            "AttachUserPolicy" => {
                let user_name = match param(ctx, "UserName") {
                    Some(n) => n,
                    None => return Ok(iam_error("ValidationError", "UserName is required", 400)),
                };
                let policy_arn = match param(ctx, "PolicyArn") {
                    Some(a) => a,
                    None => return Ok(iam_error("ValidationError", "PolicyArn is required", 400)),
                };
                let mut store = self.store.get_or_create(account_id, region);
                match store.users.get_mut(&user_name) {
                    None => Ok(iam_error(
                        "NoSuchEntity",
                        &format!("User {user_name} not found"),
                        404,
                    )),
                    Some(u) => {
                        if !u.attached_policies.contains(&policy_arn) {
                            u.attached_policies.push(policy_arn);
                        }
                        Ok(xml_no_result("AttachUserPolicy", &rid))
                    }
                }
            }

            "AttachRolePolicy" => {
                let role_name = match param(ctx, "RoleName") {
                    Some(n) => n,
                    None => return Ok(iam_error("ValidationError", "RoleName is required", 400)),
                };
                let policy_arn = match param(ctx, "PolicyArn") {
                    Some(a) => a,
                    None => return Ok(iam_error("ValidationError", "PolicyArn is required", 400)),
                };
                let mut store = self.store.get_or_create(account_id, region);
                match store.roles.get_mut(&role_name) {
                    None => Ok(iam_error(
                        "NoSuchEntity",
                        &format!("Role {role_name} not found"),
                        404,
                    )),
                    Some(r) => {
                        if !r.attached_policies.contains(&policy_arn) {
                            r.attached_policies.push(policy_arn);
                        }
                        Ok(xml_no_result("AttachRolePolicy", &rid))
                    }
                }
            }

            "PutRolePolicy" => {
                let role_name = match param(ctx, "RoleName") {
                    Some(n) => n,
                    None => return Ok(iam_error("ValidationError", "RoleName is required", 400)),
                };
                let policy_name = match param(ctx, "PolicyName") {
                    Some(n) => n,
                    None => return Ok(iam_error("ValidationError", "PolicyName is required", 400)),
                };
                let policy_doc = param(ctx, "PolicyDocument").unwrap_or_default();
                let mut store = self.store.get_or_create(account_id, region);
                match store.roles.get_mut(&role_name) {
                    None => Ok(iam_error(
                        "NoSuchEntity",
                        &format!("Role {role_name} not found"),
                        404,
                    )),
                    Some(r) => {
                        r.inline_policies.insert(policy_name, policy_doc);
                        Ok(xml_no_result("PutRolePolicy", &rid))
                    }
                }
            }

            // ---------------------------------------------------------------
            // Group operations
            // ---------------------------------------------------------------
            "CreateGroup" => {
                let name = match param(ctx, "GroupName") {
                    Some(n) => n,
                    None => return Ok(iam_error("ValidationError", "GroupName is required", 400)),
                };
                let path = param(ctx, "Path").unwrap_or_else(|| "/".to_string());
                let mut store = self.store.get_or_create(account_id, region);
                if store.groups.contains_key(&name) {
                    return Ok(iam_error(
                        "EntityAlreadyExists",
                        &format!("Group {name} already exists"),
                        409,
                    ));
                }
                let group = IamGroup {
                    group_id: format!(
                        "AGPA{}",
                        &Uuid::new_v4().to_string().replace('-', "")[..16].to_uppercase()
                    ),
                    arn: format!("arn:aws:iam::{account_id}:group{path}{name}"),
                    group_name: name.clone(),
                    path,
                    created: Utc::now(),
                    members: Vec::new(),
                    attached_policies: Vec::new(),
                };
                let xml = group_xml(&group);
                store.groups.insert(name, group);
                Ok(xml_resp("CreateGroup", &rid, &xml))
            }

            "AddUserToGroup" => {
                let group_name = match param(ctx, "GroupName") {
                    Some(n) => n,
                    None => return Ok(iam_error("ValidationError", "GroupName is required", 400)),
                };
                let user_name = match param(ctx, "UserName") {
                    Some(n) => n,
                    None => return Ok(iam_error("ValidationError", "UserName is required", 400)),
                };
                let mut store = self.store.get_or_create(account_id, region);
                match store.groups.get_mut(&group_name) {
                    None => Ok(iam_error(
                        "NoSuchEntity",
                        &format!("Group {group_name} not found"),
                        404,
                    )),
                    Some(g) => {
                        if !g.members.contains(&user_name) {
                            g.members.push(user_name.clone());
                        }
                        // Also update user's group list
                        if let Some(u) = store.users.get_mut(&user_name)
                            && !u.groups.contains(&group_name)
                        {
                            u.groups.push(group_name);
                        }
                        Ok(xml_no_result("AddUserToGroup", &rid))
                    }
                }
            }

            "ListGroups" => {
                let store = self.store.get_or_create(account_id, region);
                let groups: Vec<String> = store.groups.values().map(group_xml).collect();
                let inner = format!(
                    "<Groups>{}</Groups><IsTruncated>false</IsTruncated>",
                    groups.join("")
                );
                Ok(xml_resp("ListGroups", &rid, &inner))
            }

            // ---------------------------------------------------------------
            // AssumeRole (also available via STS, but IAM can handle it too)
            // ---------------------------------------------------------------
            "AssumeRole" => {
                let role_arn = param(ctx, "RoleArn").unwrap_or_default();
                let session_name =
                    param(ctx, "RoleSessionName").unwrap_or_else(|| "session".to_string());
                let expiry = (Utc::now() + chrono::Duration::hours(1)).format("%Y-%m-%dT%H:%M:%SZ");
                let creds_xml = format!(
                    "<Credentials>\
<AccessKeyId>ASIA{}</AccessKeyId>\
<SecretAccessKey>{}</SecretAccessKey>\
<SessionToken>FQoGZXIvYXdzENr//</SessionToken>\
<Expiration>{expiry}</Expiration>\
</Credentials>\
<AssumedRoleUser>\
<AssumedRoleId>AROA{}:{session_name}</AssumedRoleId>\
<Arn>{role_arn}/{session_name}</Arn>\
</AssumedRoleUser>",
                    &Uuid::new_v4().to_string().replace('-', "")[..16].to_uppercase(),
                    &Uuid::new_v4().to_string().replace('-', ""),
                    &Uuid::new_v4().to_string().replace('-', "")[..16].to_uppercase(),
                );
                Ok(xml_resp("AssumeRole", &rid, &creds_xml))
            }

            _ => Ok(iam_error(
                "NotImplemented",
                &format!("Operation not implemented: {op}"),
                501,
            )),
        }
    }
}
