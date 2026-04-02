use std::borrow::Cow;
use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt::Write as FmtWrite;
use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use chrono::{Datelike, Timelike, Utc};
use openstack_service_framework::traits::{
    DispatchError, DispatchResponse, RequestContext, ResponseBody, ServiceProvider,
};
use openstack_service_framework::xml::xml_escape;
use openstack_state::AccountRegionBundle;
use rand::rngs::SmallRng;
use rand::{RngExt, SeedableRng};

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
    let mut xml = String::with_capacity(150 + action.len() * 3 + request_id.len() + inner.len());
    write!(
        xml,
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
<{action}Response xmlns=\"https://iam.amazonaws.com/doc/2010-05-08/\">\
<{action}Result>{inner}</{action}Result>\
<ResponseMetadata><RequestId>{request_id}</RequestId></ResponseMetadata>\
</{action}Response>"
    )
    .expect("write to String is infallible");
    DispatchResponse {
        status_code: 200,
        body: ResponseBody::Buffered(Bytes::from(xml.into_bytes())),
        content_type: Cow::Borrowed("text/xml"),
        headers: Vec::new(),
    }
}

/// Build a full IAM XML response in a single allocation using a closure.
///
/// Writes the envelope prefix, calls `write_inner` to populate the result element,
/// then appends the envelope suffix — no intermediate `format!` copy.
fn xml_response_write(
    action: &str,
    request_id: &str,
    inner_capacity_hint: usize,
    write_inner: impl FnOnce(&mut String),
) -> DispatchResponse {
    let mut buf =
        String::with_capacity(150 + action.len() * 3 + request_id.len() + inner_capacity_hint);
    write!(
        buf,
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
<{action}Response xmlns=\"https://iam.amazonaws.com/doc/2010-05-08/\">\
<{action}Result>"
    )
    .expect("write to String is infallible");
    write_inner(&mut buf);
    write!(
        buf,
        "</{action}Result>\
<ResponseMetadata><RequestId>{request_id}</RequestId></ResponseMetadata>\
</{action}Response>"
    )
    .expect("write to String is infallible");
    DispatchResponse {
        status_code: 200,
        body: ResponseBody::Buffered(Bytes::from(buf.into_bytes())),
        content_type: Cow::Borrowed("text/xml"),
        headers: Vec::new(),
    }
}

fn xml_no_result(action: &str, request_id: &str) -> DispatchResponse {
    let mut xml = String::with_capacity(120 + action.len() * 2 + request_id.len());
    write!(
        xml,
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
<{action}Response xmlns=\"https://iam.amazonaws.com/doc/2010-05-08/\">\
<ResponseMetadata><RequestId>{request_id}</RequestId></ResponseMetadata>\
</{action}Response>"
    )
    .expect("write to String is infallible");
    DispatchResponse {
        status_code: 200,
        body: ResponseBody::Buffered(Bytes::from(xml.into_bytes())),
        content_type: Cow::Borrowed("text/xml"),
        headers: Vec::new(),
    }
}

fn iam_error(code: &str, message: &str, status: u16) -> DispatchResponse {
    let mut xml = String::with_capacity(120 + code.len() + message.len());
    write!(
        xml,
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
<ErrorResponse xmlns=\"https://iam.amazonaws.com/doc/2010-05-08/\">\
<Error><Type>Sender</Type><Code>{}</Code><Message>{}</Message></Error>\
</ErrorResponse>",
        xml_escape(code),
        xml_escape(message)
    )
    .expect("write to String is infallible");
    DispatchResponse {
        status_code: status,
        body: ResponseBody::Buffered(Bytes::from(xml.into_bytes())),
        content_type: Cow::Borrowed("text/xml"),
        headers: Vec::new(),
    }
}

// Thread-local fast RNG — seeded once per thread from the OS RNG, avoiding a
// getrandom syscall on every request.
thread_local! {
    static FAST_RNG: RefCell<SmallRng> = RefCell::new(SmallRng::from_rng(&mut rand::rng()));
}

/// Generate a UUID v4-formatted request ID using the thread-local fast RNG.
///
/// `SmallRng` is non-cryptographic but ~10× faster than `Uuid::new_v4()` which
/// hits the kernel CSPRNG on every call.
fn req_id() -> String {
    FAST_RNG.with(|rng| {
        let mut rng = rng.borrow_mut();
        let mut b = [0u8; 16];
        rng.fill(&mut b);
        // Set UUID v4 version and variant bits for RFC 4122 compliance.
        b[6] = (b[6] & 0x0f) | 0x40;
        b[8] = (b[8] & 0x3f) | 0x80;
        format!(
            "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
            b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7],
            b[8], b[9], b[10], b[11], b[12], b[13], b[14], b[15],
        )
    })
}

// ---------------------------------------------------------------------------
// User XML serializer
// ---------------------------------------------------------------------------

fn user_xml(u: &IamUser) -> String {
    let mut buf = String::with_capacity(256);
    write_user_xml(&mut buf, u);
    buf
}

/// Write a `<User>` element into `buf` using direct DateTime accessors to avoid
/// re-parsing the chrono format string on every call.
fn write_user_xml(buf: &mut String, u: &IamUser) {
    let dt = u.created;
    write!(
        buf,
        "<User>\
<Path>{path}</Path>\
<UserName>{name}</UserName>\
<UserId>{id}</UserId>\
<Arn>{arn}</Arn>\
<CreateDate>{yr:04}-{mo:02}-{dy:02}T{h:02}:{m:02}:{s:02}.{us:06}Z</CreateDate>\
</User>",
        path = xml_escape(&u.path),
        name = xml_escape(&u.user_name),
        id = xml_escape(&u.user_id),
        arn = xml_escape(&u.arn),
        yr = dt.year(),
        mo = dt.month(),
        dy = dt.day(),
        h = dt.hour(),
        m = dt.minute(),
        s = dt.second(),
        us = dt.nanosecond() / 1_000,
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
        xml_escape(&r.role_id),
        xml_escape(&r.role_name),
        xml_escape(&r.arn),
        xml_escape(&r.path),
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
        xml_escape(&p.policy_id),
        xml_escape(&p.policy_name),
        xml_escape(&p.arn),
        xml_escape(&p.path),
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
        xml_escape(&g.group_id),
        xml_escape(&g.group_name),
        xml_escape(&g.arn),
        xml_escape(&g.path),
        g.created.format("%Y-%m-%dT%H:%M:%SZ"),
    )
    .expect("write to String is infallible");
}

/// Encode `n` random bytes as uppercase hex (2n chars) using the thread-local fast RNG.
fn uuid_hex_upper(n: usize) -> String {
    FAST_RNG.with(|rng| {
        let mut rng = rng.borrow_mut();
        let mut s = String::with_capacity(n * 2);
        for _ in 0..n {
            write!(s, "{:02X}", rng.random::<u8>()).expect("write to String is infallible");
        }
        s
    })
}

/// Encode `n` random bytes as lowercase hex using a CSPRNG for credentials.
fn secure_hex_lower(n: usize) -> String {
    let mut bytes = vec![0u8; n];
    rand::rng().fill(&mut bytes);

    let mut s = String::with_capacity(n * 2);
    for b in bytes {
        write!(s, "{:02x}", b).expect("write to String is infallible");
    }
    s
}

/// Encode `n` random bytes as uppercase hex using a CSPRNG for credentials.
fn secure_hex_upper(n: usize) -> String {
    let mut bytes = vec![0u8; n];
    rand::rng().fill(&mut bytes);

    let mut s = String::with_capacity(n * 2);
    for b in bytes {
        write!(s, "{:02X}", b).expect("write to String is infallible");
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
        // Reuse the gateway's request ID when available; fall back to local
        // generation for unit tests that construct RequestContext directly.
        let rid: Cow<'_, str> = if ctx.request_id.is_empty() {
            Cow::Owned(req_id())
        } else {
            Cow::Borrowed(&ctx.request_id)
        };
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
                    // "AIDA" + 16 uppercase hex chars from 8 random bytes.
                    user_id: format!("AIDA{}", uuid_hex_upper(8)),
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
                // Serialize directly inside the DashMap read lock — the lock
                // is shared (multiple concurrent readers), so holding it during
                // serialization is safe and avoids extra allocations.
                match self.store.get(account_id, region) {
                    None => {
                        return Ok(xml_resp(
                            "ListUsers",
                            &rid,
                            "<Users /><IsTruncated>false</IsTruncated>",
                        ));
                    }
                    Some(store) => {
                        // Users are stored in a BTreeMap, so iteration is
                        // already sorted by UserName with no per-request sort.
                        let user_count = store.users.len();
                        Ok(xml_response_write(
                            "ListUsers",
                            &rid,
                            8 + user_count * 260,
                            |buf| {
                                buf.push_str("<Users>");
                                for u in store.users.values() {
                                    write_user_xml(buf, u);
                                }
                                buf.push_str("</Users><IsTruncated>false</IsTruncated>");
                            },
                        ))
                        // DashMap read lock released here after serialization.
                    }
                }
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

            "ListRoles" => match self.store.get(account_id, region) {
                None => Ok(xml_resp(
                    "ListRoles",
                    &rid,
                    "<Roles /><IsTruncated>false</IsTruncated>",
                )),
                Some(store) => {
                    let role_count = store.roles.len();
                    Ok(xml_response_write(
                        "ListRoles",
                        &rid,
                        8 + role_count * 400,
                        |buf| {
                            buf.push_str("<Roles>");
                            for r in store.roles.values() {
                                write_role_xml(buf, r);
                            }
                            buf.push_str("</Roles><IsTruncated>false</IsTruncated>");
                        },
                    ))
                }
            },

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

            "ListPolicies" => match self.store.get(account_id, region) {
                None => Ok(xml_resp(
                    "ListPolicies",
                    &rid,
                    "<Policies /><IsTruncated>false</IsTruncated>",
                )),
                Some(store) => {
                    let policy_count = store.policies.len();
                    Ok(xml_response_write(
                        "ListPolicies",
                        &rid,
                        10 + policy_count * 260,
                        |buf| {
                            buf.push_str("<Policies>");
                            for p in store.policies.values() {
                                write_policy_xml(buf, p);
                            }
                            buf.push_str("</Policies><IsTruncated>false</IsTruncated>");
                        },
                    ))
                }
            },

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
                if !store.users.contains_key(user_name) {
                    return Ok(iam_error(
                        "NoSuchEntity",
                        &format!("User {user_name} not found"),
                        404,
                    ));
                }
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

            "ListGroups" => match self.store.get(account_id, region) {
                None => Ok(xml_resp(
                    "ListGroups",
                    &rid,
                    "<Groups /><IsTruncated>false</IsTruncated>",
                )),
                Some(store) => {
                    let group_count = store.groups.len();
                    Ok(xml_response_write(
                        "ListGroups",
                        &rid,
                        9 + group_count * 260,
                        |buf| {
                            buf.push_str("<Groups>");
                            for g in store.groups.values() {
                                write_group_xml(buf, g);
                            }
                            buf.push_str("</Groups><IsTruncated>false</IsTruncated>");
                        },
                    ))
                }
            },

            // ---------------------------------------------------------------
            // AssumeRole (also available via STS, but IAM can handle it too)
            // ---------------------------------------------------------------
            "AssumeRole" => {
                // Borrow param values as &str — no owned Strings needed for format args.
                let role_arn = param(ctx, "RoleArn").unwrap_or("");
                let session_name = param(ctx, "RoleSessionName").unwrap_or("session");
                let expiry = (Utc::now() + chrono::Duration::hours(1)).format("%Y-%m-%dT%H:%M:%SZ");
                // Generate the three UUID-derived tokens with one allocation each.
                let access_key_suffix = secure_hex_upper(8); // ASIA + 16 uppercase hex
                let secret_key = secure_hex_lower(16); // 32 lowercase hex
                let role_id_suffix = secure_hex_upper(8); // AROA + 16 uppercase hex
                let session_token = secure_hex_lower(32);
                let creds_xml = format!(
                    "<Credentials>\
<AccessKeyId>ASIA{access_key_suffix}</AccessKeyId>\
<SecretAccessKey>{secret_key}</SecretAccessKey>\
<SessionToken>{session_token}</SessionToken>\
<Expiration>{expiry}</Expiration>\
</Credentials>\
<AssumedRoleUser>\
<AssumedRoleId>AROA{role_id_suffix}:{}</AssumedRoleId>\
<Arn>{}/{}</Arn>\
</AssumedRoleUser>",
                    xml_escape(session_name),
                    xml_escape(role_arn),
                    xml_escape(session_name)
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

    async fn storage_snapshot(&self) -> Option<serde_json::Value> {
        use serde_json::json;
        let mut users = Vec::new();
        let mut roles = Vec::new();
        let mut policies = Vec::new();
        for entry in self.store.iter() {
            let store = entry.value();
            for user in store.users.values() {
                users.push(json!({
                    "id": user.arn, "kind": "user",
                    "attributes": [{"key": "name", "value": user.user_name.clone()}]
                }));
            }
            for role in store.roles.values() {
                roles.push(json!({
                    "id": role.arn, "kind": "role",
                    "attributes": [{"key": "name", "value": role.role_name.clone()}]
                }));
            }
            for policy in store.policies.values() {
                policies.push(json!({
                    "id": policy.arn, "kind": "policy",
                    "attributes": [{"key": "name", "value": policy.policy_name.clone()}]
                }));
            }
        }
        Some(json!({ "kind": "iam", "users": users, "roles": roles, "policies": policies }))
    }
}
