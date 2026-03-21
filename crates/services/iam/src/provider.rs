use std::borrow::Cow;
use std::collections::HashMap;
use std::fmt::Write as FmtWrite;
use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use chrono::Utc;
use openstack_service_framework::traits::{
    DispatchError, DispatchResponse, RequestContext, ResponseBody, ServiceProvider,
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
        body: ResponseBody::Buffered(Bytes::from(xml.into_bytes())),
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
        body: ResponseBody::Buffered(Bytes::from(xml.into_bytes())),
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
        body: ResponseBody::Buffered(Bytes::from(xml.into_bytes())),
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
    let mut buf = String::with_capacity(256);
    write_user_xml(&mut buf, u);
    buf
}

fn write_user_xml(buf: &mut String, u: &IamUser) {
    write!(
        buf,
        "<User>\
<Path>{}</Path>\
<UserName>{}</UserName>\
<UserId>{}</UserId>\
<Arn>{}</Arn>\
<CreateDate>{}</CreateDate>\
</User>",
        u.path,
        u.user_name,
        u.user_id,
        u.arn,
        u.created.format("%Y-%m-%dT%H:%M:%S%.6fZ"),
    )
    .expect("write to String is infallible");
}

fn role_xml(r: &IamRole) -> String {
    let mut buf = String::with_capacity(512);
    write_role_xml(&mut buf, r);
    buf
}

fn write_role_xml(buf: &mut String, r: &IamRole) {
    write!(
        buf,
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
    .expect("write to String is infallible");
}

fn policy_xml(p: &IamPolicy) -> String {
    let mut buf = String::with_capacity(256);
    write_policy_xml(&mut buf, p);
    buf
}

fn write_policy_xml(buf: &mut String, p: &IamPolicy) {
    write!(
        buf,
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
    .expect("write to String is infallible");
}

fn group_xml(g: &IamGroup) -> String {
    let mut buf = String::with_capacity(256);
    write_group_xml(&mut buf, g);
    buf
}

fn write_group_xml(buf: &mut String, g: &IamGroup) {
    write!(
        buf,
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
    .expect("write to String is infallible");
}

/// Fast-path XML escaping: returns the original string as `Cow::Borrowed` when
/// no escaping is needed (the common case), avoiding any heap allocation.
fn xml_escape(s: &str) -> Cow<'_, str> {
    if !s.bytes().any(|b| matches!(b, b'&' | b'<' | b'>' | b'"')) {
        return Cow::Borrowed(s);
    }
    let mut out = String::with_capacity(s.len() + 16);
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(c),
        }
    }
    Cow::Owned(out)
}

/// Encode the first `n` UUID bytes as uppercase hex (2*n chars), one allocation.
fn uuid_hex_upper(n: usize) -> String {
    let bytes = Uuid::new_v4().into_bytes();
    let mut s = String::with_capacity(2 * n);
    for b in &bytes[..n] {
        write!(s, "{b:02X}").expect("write to String is infallible");
    }
    s
}

/// Encode the first `n` UUID bytes as lowercase hex (2*n chars), one allocation.
fn uuid_hex_lower(n: usize) -> String {
    let bytes = Uuid::new_v4().into_bytes();
    let mut s = String::with_capacity(2 * n);
    for b in &bytes[..n] {
        write!(s, "{b:02x}").expect("write to String is infallible");
    }
    s
}

/// Look up a request parameter from query_params first, then from the JSON request body.
/// Returns a borrowed `&str` — callers that need ownership call `.to_owned()`.
fn param<'a>(ctx: &'a RequestContext, key: &str) -> Option<&'a str> {
    ctx.query_params
        .get(key)
        .map(String::as_str)
        .or_else(|| ctx.request_body.as_object()?.get(key)?.as_str())
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
                let path = param(ctx, "Path").unwrap_or("/");
                let mut store = self.store.get_or_create(account_id, region);
                if store.users.contains_key(name) {
                    return Ok(iam_error(
                        "EntityAlreadyExists",
                        &format!("User {name} already exists"),
                        409,
                    ));
                }
                let user = IamUser {
                    // 20 lowercase hex chars from 10 UUID bytes — one allocation.
                    user_id: uuid_hex_lower(10),
                    arn: format!("arn:aws:iam::{account_id}:user{path}{name}"),
                    user_name: name.to_owned(),
                    path: path.to_owned(),
                    created: Utc::now(),
                    tags: HashMap::new(),
                    attached_policies: Vec::new(),
                    groups: Vec::new(),
                };
                // I4: user_xml already wraps in <User>...</User>; use it directly.
                let xml = user_xml(&user);
                store.users.insert(name.to_owned(), user);
                Ok(xml_resp("CreateUser", &rid, &xml))
            }

            "GetUser" => {
                let name = param(ctx, "UserName");
                // "get current user" — return a synthetic caller identity without touching the store
                let Some(n) = name else {
                    let xml = format!(
                        "<User><UserId>AIDADEFAULT</UserId><UserName>default</UserName><Arn>arn:aws:iam::{account_id}:user/default</Arn><Path>/</Path><CreateDate>2020-01-01T00:00:00Z</CreateDate></User>"
                    );
                    return Ok(xml_resp("GetUser", &rid, &xml));
                };
                let store = self.store.get(account_id, region);
                let user = store.as_ref().and_then(|s| s.users.get(n));
                match user {
                    None => Ok(iam_error(
                        "NoSuchEntity",
                        &format!("User not found: {n}"),
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
                if store.users.remove(name).is_none() {
                    return Ok(iam_error(
                        "NoSuchEntity",
                        &format!("User {name} not found"),
                        404,
                    ));
                }
                Ok(xml_no_result("DeleteUser", &rid))
            }

            "ListUsers" => {
                let Some(store) = self.store.get(account_id, region) else {
                    let inner = "<Users /><IsTruncated>false</IsTruncated>";
                    return Ok(xml_resp("ListUsers", &rid, inner));
                };
                let mut buf = String::with_capacity(512);
                let mut items: Vec<&IamUser> = store.users.values().collect();
                items.sort_by_key(|u| u.user_name.as_str());
                buf.push_str("<Users>");
                for u in &items {
                    write_user_xml(&mut buf, u);
                }
                buf.push_str("</Users><IsTruncated>false</IsTruncated>");
                Ok(xml_resp("ListUsers", &rid, &buf))
            }

            // ---------------------------------------------------------------
            // Role operations
            // ---------------------------------------------------------------
            "CreateRole" => {
                let name = match param(ctx, "RoleName") {
                    Some(n) => n,
                    None => return Ok(iam_error("ValidationError", "RoleName is required", 400)),
                };
                let path = param(ctx, "Path").unwrap_or("/");
                let policy_doc = param(ctx, "AssumeRolePolicyDocument")
                    .unwrap_or("")
                    .to_owned();
                let description = param(ctx, "Description").unwrap_or("").to_owned();
                let mut store = self.store.get_or_create(account_id, region);
                if store.roles.contains_key(name) {
                    return Ok(iam_error(
                        "EntityAlreadyExists",
                        &format!("Role {name} already exists"),
                        409,
                    ));
                }
                let role = IamRole {
                    // "AROA" + 16 uppercase hex chars from 8 UUID bytes — one allocation.
                    role_id: format!("AROA{}", uuid_hex_upper(8)),
                    arn: format!("arn:aws:iam::{account_id}:role{path}{name}"),
                    role_name: name.to_owned(),
                    path: path.to_owned(),
                    assume_role_policy_document: policy_doc,
                    description,
                    created: Utc::now(),
                    tags: HashMap::new(),
                    attached_policies: Vec::new(),
                    inline_policies: HashMap::new(),
                };
                let xml = role_xml(&role);
                store.roles.insert(name.to_owned(), role);
                Ok(xml_resp("CreateRole", &rid, &xml))
            }

            "GetRole" => {
                let name = match param(ctx, "RoleName") {
                    Some(n) => n,
                    None => return Ok(iam_error("ValidationError", "RoleName is required", 400)),
                };
                let Some(store) = self.store.get(account_id, region) else {
                    return Ok(iam_error(
                        "NoSuchEntity",
                        &format!("Role {name} not found"),
                        404,
                    ));
                };
                match store.roles.get(name) {
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
                if store.roles.remove(name).is_none() {
                    return Ok(iam_error(
                        "NoSuchEntity",
                        &format!("Role {name} not found"),
                        404,
                    ));
                }
                Ok(xml_no_result("DeleteRole", &rid))
            }

            "ListRoles" => {
                let Some(store) = self.store.get(account_id, region) else {
                    let inner = "<Roles /><IsTruncated>false</IsTruncated>";
                    return Ok(xml_resp("ListRoles", &rid, inner));
                };
                let mut buf = String::with_capacity(512);
                buf.push_str("<Roles>");
                for r in store.roles.values() {
                    write_role_xml(&mut buf, r);
                }
                buf.push_str("</Roles><IsTruncated>false</IsTruncated>");
                Ok(xml_resp("ListRoles", &rid, &buf))
            }

            // ---------------------------------------------------------------
            // Policy operations
            // ---------------------------------------------------------------
            "CreatePolicy" => {
                let name = match param(ctx, "PolicyName") {
                    Some(n) => n,
                    None => return Ok(iam_error("ValidationError", "PolicyName is required", 400)),
                };
                let path = param(ctx, "Path").unwrap_or("/");
                let document = param(ctx, "PolicyDocument").unwrap_or("").to_owned();
                let description = param(ctx, "Description").unwrap_or("").to_owned();
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
                    // "ANPA" + 16 uppercase hex chars from 8 UUID bytes — one allocation.
                    policy_id: format!("ANPA{}", uuid_hex_upper(8)),
                    policy_name: name.to_owned(),
                    arn: arn.clone(),
                    path: path.to_owned(),
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
                let Some(store) = self.store.get(account_id, region) else {
                    return Ok(iam_error(
                        "NoSuchEntity",
                        &format!("Policy {arn} not found"),
                        404,
                    ));
                };
                match store.policies.get(arn) {
                    None => Ok(iam_error(
                        "NoSuchEntity",
                        &format!("Policy {arn} not found"),
                        404,
                    )),
                    Some(p) => Ok(xml_resp("GetPolicy", &rid, &policy_xml(p))),
                }
            }

            "ListPolicies" => {
                let Some(store) = self.store.get(account_id, region) else {
                    let inner = "<Policies /><IsTruncated>false</IsTruncated>";
                    return Ok(xml_resp("ListPolicies", &rid, inner));
                };
                let mut buf = String::with_capacity(512);
                buf.push_str("<Policies>");
                for p in store.policies.values() {
                    write_policy_xml(&mut buf, p);
                }
                buf.push_str("</Policies><IsTruncated>false</IsTruncated>");
                Ok(xml_resp("ListPolicies", &rid, &buf))
            }

            "AttachUserPolicy" => {
                let user_name = match param(ctx, "UserName") {
                    Some(n) => n,
                    None => return Ok(iam_error("ValidationError", "UserName is required", 400)),
                };
                let policy_arn = match param(ctx, "PolicyArn") {
                    Some(a) => a.to_owned(),
                    None => return Ok(iam_error("ValidationError", "PolicyArn is required", 400)),
                };
                let mut store = self.store.get_or_create(account_id, region);
                match store.users.get_mut(user_name) {
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
                    Some(a) => a.to_owned(),
                    None => return Ok(iam_error("ValidationError", "PolicyArn is required", 400)),
                };
                let mut store = self.store.get_or_create(account_id, region);
                match store.roles.get_mut(role_name) {
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
                    Some(n) => n.to_owned(),
                    None => return Ok(iam_error("ValidationError", "PolicyName is required", 400)),
                };
                let policy_doc = param(ctx, "PolicyDocument").unwrap_or("").to_owned();
                let mut store = self.store.get_or_create(account_id, region);
                match store.roles.get_mut(role_name) {
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
                let path = param(ctx, "Path").unwrap_or("/");
                let mut store = self.store.get_or_create(account_id, region);
                if store.groups.contains_key(name) {
                    return Ok(iam_error(
                        "EntityAlreadyExists",
                        &format!("Group {name} already exists"),
                        409,
                    ));
                }
                let group = IamGroup {
                    // "AGPA" + 16 uppercase hex chars from 8 UUID bytes — one allocation.
                    group_id: format!("AGPA{}", uuid_hex_upper(8)),
                    arn: format!("arn:aws:iam::{account_id}:group{path}{name}"),
                    group_name: name.to_owned(),
                    path: path.to_owned(),
                    created: Utc::now(),
                    members: Vec::new(),
                    attached_policies: Vec::new(),
                };
                let xml = group_xml(&group);
                store.groups.insert(name.to_owned(), group);
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
                match store.groups.get_mut(group_name) {
                    None => Ok(iam_error(
                        "NoSuchEntity",
                        &format!("Group {group_name} not found"),
                        404,
                    )),
                    Some(g) => {
                        if !g.members.iter().any(|m| m == user_name) {
                            g.members.push(user_name.to_owned());
                        }
                        // Also update user's group list
                        if let Some(u) = store.users.get_mut(user_name)
                            && !u.groups.iter().any(|g| g == group_name)
                        {
                            u.groups.push(group_name.to_owned());
                        }
                        Ok(xml_no_result("AddUserToGroup", &rid))
                    }
                }
            }

            "ListGroups" => {
                let Some(store) = self.store.get(account_id, region) else {
                    let inner = "<Groups /><IsTruncated>false</IsTruncated>";
                    return Ok(xml_resp("ListGroups", &rid, inner));
                };
                let mut buf = String::with_capacity(512);
                buf.push_str("<Groups>");
                for g in store.groups.values() {
                    write_group_xml(&mut buf, g);
                }
                buf.push_str("</Groups><IsTruncated>false</IsTruncated>");
                Ok(xml_resp("ListGroups", &rid, &buf))
            }

            // ---------------------------------------------------------------
            // AssumeRole (also available via STS, but IAM can handle it too)
            // ---------------------------------------------------------------
            "AssumeRole" => {
                // Borrow param values as &str — no owned Strings needed for format args.
                let role_arn = param(ctx, "RoleArn").unwrap_or("");
                let session_name = param(ctx, "RoleSessionName").unwrap_or("session");
                let expiry = (Utc::now() + chrono::Duration::hours(1)).format("%Y-%m-%dT%H:%M:%SZ");
                // Generate the three UUID-derived tokens with one allocation each.
                let access_key_suffix = uuid_hex_upper(8); // ASIA + 16 uppercase hex
                let secret_key = uuid_hex_lower(16); // 32 lowercase hex
                let role_id_suffix = uuid_hex_upper(8); // AROA + 16 uppercase hex
                let creds_xml = format!(
                    "<Credentials>\
<AccessKeyId>ASIA{access_key_suffix}</AccessKeyId>\
<SecretAccessKey>{secret_key}</SecretAccessKey>\
<SessionToken>FQoGZXIvYXdzENr//</SessionToken>\
<Expiration>{expiry}</Expiration>\
</Credentials>\
<AssumedRoleUser>\
<AssumedRoleId>AROA{role_id_suffix}:{session_name}</AssumedRoleId>\
<Arn>{role_arn}/{session_name}</Arn>\
</AssumedRoleUser>"
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
