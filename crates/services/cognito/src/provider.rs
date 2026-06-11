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

use crate::store::{CognitoStore, User, UserAttribute, UserPool, UserPoolClient, UserStatus};

pub struct CognitoProvider {
    store: Arc<AccountRegionBundle<CognitoStore>>,
}

impl CognitoProvider {
    pub fn new() -> Self {
        Self {
            store: Arc::new(AccountRegionBundle::new()),
        }
    }
}

impl Default for CognitoProvider {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Helpers — Cognito uses JSON protocol (application/x-amz-json-1.1)
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
        content_type: Cow::Borrowed("application/x-amz-json-1.1"),
        headers: Vec::new(),
    }
}

fn str_param(ctx: &RequestContext, key: &str) -> Option<String> {
    ctx.request_body
        .get(key)
        .and_then(|v| v.as_str())
        .map(String::from)
}

fn short_id() -> String {
    Uuid::new_v4().to_string().replace('-', "")[..12].to_string()
}

fn pool_arn(account_id: &str, region: &str, pool_id: &str) -> String {
    format!("arn:aws:cognito-idp:{region}:{account_id}:userpool/{pool_id}")
}

fn pool_json(p: &UserPool) -> Value {
    json!({
        "Id": p.id,
        "Name": p.name,
        "Arn": p.arn,
        "Status": p.status,
        "CreationDate": p.creation_date.timestamp(),
        "LastModifiedDate": p.last_modified_date.timestamp(),
        "MfaConfiguration": p.mfa_configuration,
        "UsernameAttributes": p.username_attributes,
        "AutoVerifiedAttributes": p.auto_verified_attributes,
    })
}

fn client_json(c: &UserPoolClient) -> Value {
    json!({
        "ClientId": c.client_id,
        "ClientName": c.client_name,
        "UserPoolId": c.user_pool_id,
        "ClientSecret": c.client_secret,
        "ExplicitAuthFlows": c.explicit_auth_flows,
        "CallbackURLs": c.callback_urls,
        "LogoutURLs": c.logout_urls,
        "CreationDate": c.creation_date.timestamp(),
        "LastModifiedDate": c.last_modified_date.timestamp(),
    })
}

fn user_json(u: &User) -> Value {
    let attrs: Vec<Value> = u
        .attributes
        .iter()
        .map(|a| json!({ "Name": a.name, "Value": a.value }))
        .collect();
    json!({
        "Username": u.username,
        "UserStatus": u.user_status.as_str(),
        "Enabled": u.enabled,
        "Attributes": attrs,
        "UserCreateDate": u.user_create_date.timestamp(),
        "UserLastModifiedDate": u.user_last_modified_date.timestamp(),
    })
}

// ---------------------------------------------------------------------------
// ServiceProvider
// ---------------------------------------------------------------------------

#[async_trait]
impl ServiceProvider for CognitoProvider {
    fn service_name(&self) -> &str {
        "cognito-idp"
    }

    async fn dispatch(&self, ctx: &RequestContext) -> Result<DispatchResponse, DispatchError> {
        let region = &ctx.region;
        let account_id = &ctx.account_id;

        match ctx.operation.as_str() {
            // ----------------------------------------------------------------
            // CreateUserPool
            // ----------------------------------------------------------------
            "CreateUserPool" => {
                let pool_name = match str_param(ctx, "PoolName") {
                    Some(n) => n,
                    None => {
                        return Ok(json_error(
                            "InvalidParameterException",
                            "PoolName is required",
                            400,
                        ));
                    }
                };
                let pool_id = format!("{region}_{}", short_id());
                let arn = pool_arn(account_id, region, &pool_id);
                let now = Utc::now();
                let mfa = str_param(ctx, "MfaConfiguration").unwrap_or_else(|| "OFF".to_string());
                let username_attrs: Vec<String> = ctx
                    .request_body
                    .get("UsernameAttributes")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str().map(String::from))
                            .collect()
                    })
                    .unwrap_or_default();
                let auto_verified: Vec<String> = ctx
                    .request_body
                    .get("AutoVerifiedAttributes")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str().map(String::from))
                            .collect()
                    })
                    .unwrap_or_default();

                let pool = UserPool {
                    id: pool_id.clone(),
                    name: pool_name,
                    arn,
                    status: "Active".to_string(),
                    creation_date: now,
                    last_modified_date: now,
                    mfa_configuration: mfa,
                    email_verification_subject: "Your verification code".to_string(),
                    email_verification_message: "Your verification code is {####}.".to_string(),
                    username_attributes: username_attrs,
                    auto_verified_attributes: auto_verified,
                };
                let mut store = self.store.get_or_create(account_id, region);
                store.user_pools.insert(pool_id, pool.clone());
                Ok(json_ok(json!({ "UserPool": pool_json(&pool) })))
            }

            // ----------------------------------------------------------------
            // DeleteUserPool
            // ----------------------------------------------------------------
            "DeleteUserPool" => {
                let pool_id = match str_param(ctx, "UserPoolId") {
                    Some(id) => id,
                    None => {
                        return Ok(json_error(
                            "InvalidParameterException",
                            "UserPoolId is required",
                            400,
                        ));
                    }
                };
                let mut store = self.store.get_or_create(account_id, region);
                if store.user_pools.remove(&pool_id).is_none() {
                    return Ok(json_error(
                        "ResourceNotFoundException",
                        &format!("User pool {pool_id} not found"),
                        400,
                    ));
                }
                // Also remove clients and users belonging to this pool
                store.clients.retain(|_, c| c.user_pool_id != pool_id);
                store.users.retain(|(pid, _), _| pid != &pool_id);
                Ok(json_ok(json!({})))
            }

            // ----------------------------------------------------------------
            // DescribeUserPool
            // ----------------------------------------------------------------
            "DescribeUserPool" => {
                let pool_id = match str_param(ctx, "UserPoolId") {
                    Some(id) => id,
                    None => {
                        return Ok(json_error(
                            "InvalidParameterException",
                            "UserPoolId is required",
                            400,
                        ));
                    }
                };
                let Some(store) = self.store.get(account_id, region) else {
                    return Ok(json_error(
                        "ResourceNotFoundException",
                        &format!("User pool {pool_id} not found"),
                        400,
                    ));
                };
                match store.user_pools.get(&pool_id) {
                    Some(pool) => Ok(json_ok(json!({ "UserPool": pool_json(pool) }))),
                    None => Ok(json_error(
                        "ResourceNotFoundException",
                        &format!("User pool {pool_id} not found"),
                        400,
                    )),
                }
            }

            // ----------------------------------------------------------------
            // ListUserPools
            // ----------------------------------------------------------------
            "ListUserPools" => {
                let pools: Vec<Value> = self
                    .store
                    .get(account_id, region)
                    .map(|store| store.user_pools.values().map(pool_json).collect())
                    .unwrap_or_default();
                Ok(json_ok(json!({ "UserPools": pools, "NextToken": null })))
            }

            // ----------------------------------------------------------------
            // UpdateUserPool
            // ----------------------------------------------------------------
            "UpdateUserPool" => {
                let pool_id = match str_param(ctx, "UserPoolId") {
                    Some(id) => id,
                    None => {
                        return Ok(json_error(
                            "InvalidParameterException",
                            "UserPoolId is required",
                            400,
                        ));
                    }
                };
                let mut store = self.store.get_or_create(account_id, region);
                match store.user_pools.get_mut(&pool_id) {
                    Some(pool) => {
                        if let Some(mfa) = str_param(ctx, "MfaConfiguration") {
                            pool.mfa_configuration = mfa;
                        }
                        pool.last_modified_date = Utc::now();
                        Ok(json_ok(json!({})))
                    }
                    None => Ok(json_error(
                        "ResourceNotFoundException",
                        &format!("User pool {pool_id} not found"),
                        400,
                    )),
                }
            }

            // ----------------------------------------------------------------
            // CreateUserPoolClient
            // ----------------------------------------------------------------
            "CreateUserPoolClient" => {
                let pool_id = match str_param(ctx, "UserPoolId") {
                    Some(id) => id,
                    None => {
                        return Ok(json_error(
                            "InvalidParameterException",
                            "UserPoolId is required",
                            400,
                        ));
                    }
                };
                let client_name = match str_param(ctx, "ClientName") {
                    Some(n) => n,
                    None => {
                        return Ok(json_error(
                            "InvalidParameterException",
                            "ClientName is required",
                            400,
                        ));
                    }
                };
                let generate_secret = ctx
                    .request_body
                    .get("GenerateSecret")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let client_secret = if generate_secret {
                    Some(Uuid::new_v4().to_string().replace('-', ""))
                } else {
                    None
                };
                let explicit_auth_flows: Vec<String> = ctx
                    .request_body
                    .get("ExplicitAuthFlows")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str().map(String::from))
                            .collect()
                    })
                    .unwrap_or_default();

                let client_id = short_id();
                let now = Utc::now();
                let client = UserPoolClient {
                    client_id: client_id.clone(),
                    client_name,
                    user_pool_id: pool_id.clone(),
                    client_secret,
                    explicit_auth_flows,
                    allowed_o_auth_flows: Vec::new(),
                    allowed_o_auth_scopes: Vec::new(),
                    callback_urls: ctx
                        .request_body
                        .get("CallbackURLs")
                        .and_then(|v| v.as_array())
                        .map(|arr| {
                            arr.iter()
                                .filter_map(|v| v.as_str().map(String::from))
                                .collect()
                        })
                        .unwrap_or_default(),
                    logout_urls: ctx
                        .request_body
                        .get("LogoutURLs")
                        .and_then(|v| v.as_array())
                        .map(|arr| {
                            arr.iter()
                                .filter_map(|v| v.as_str().map(String::from))
                                .collect()
                        })
                        .unwrap_or_default(),
                    creation_date: now,
                    last_modified_date: now,
                };

                // Verify pool exists
                {
                    let Some(store) = self.store.get(account_id, region) else {
                        return Ok(json_error(
                            "ResourceNotFoundException",
                            &format!("User pool {pool_id} not found"),
                            400,
                        ));
                    };
                    if !store.user_pools.contains_key(&pool_id) {
                        return Ok(json_error(
                            "ResourceNotFoundException",
                            &format!("User pool {pool_id} not found"),
                            400,
                        ));
                    }
                }
                let mut store = self.store.get_or_create(account_id, region);
                store.clients.insert(client_id, client.clone());
                Ok(json_ok(json!({ "UserPoolClient": client_json(&client) })))
            }

            // ----------------------------------------------------------------
            // DeleteUserPoolClient
            // ----------------------------------------------------------------
            "DeleteUserPoolClient" => {
                let client_id = match str_param(ctx, "ClientId") {
                    Some(id) => id,
                    None => {
                        return Ok(json_error(
                            "InvalidParameterException",
                            "ClientId is required",
                            400,
                        ));
                    }
                };
                let mut store = self.store.get_or_create(account_id, region);
                if store.clients.remove(&client_id).is_none() {
                    return Ok(json_error(
                        "ResourceNotFoundException",
                        &format!("Client {client_id} not found"),
                        400,
                    ));
                }
                Ok(json_ok(json!({})))
            }

            // ----------------------------------------------------------------
            // DescribeUserPoolClient
            // ----------------------------------------------------------------
            "DescribeUserPoolClient" => {
                let client_id = match str_param(ctx, "ClientId") {
                    Some(id) => id,
                    None => {
                        return Ok(json_error(
                            "InvalidParameterException",
                            "ClientId is required",
                            400,
                        ));
                    }
                };
                let Some(store) = self.store.get(account_id, region) else {
                    return Ok(json_error(
                        "ResourceNotFoundException",
                        &format!("Client {client_id} not found"),
                        400,
                    ));
                };
                match store.clients.get(&client_id) {
                    Some(c) => Ok(json_ok(json!({ "UserPoolClient": client_json(c) }))),
                    None => Ok(json_error(
                        "ResourceNotFoundException",
                        &format!("Client {client_id} not found"),
                        400,
                    )),
                }
            }

            // ----------------------------------------------------------------
            // ListUserPoolClients
            // ----------------------------------------------------------------
            "ListUserPoolClients" => {
                let pool_id = match str_param(ctx, "UserPoolId") {
                    Some(id) => id,
                    None => {
                        return Ok(json_error(
                            "InvalidParameterException",
                            "UserPoolId is required",
                            400,
                        ));
                    }
                };
                let clients: Vec<Value> = self
                    .store
                    .get(account_id, region)
                    .map(|store| {
                        store
                            .clients
                            .values()
                            .filter(|c| c.user_pool_id == pool_id)
                            .map(|c| json!({ "ClientId": c.client_id, "ClientName": c.client_name, "UserPoolId": c.user_pool_id }))
                            .collect()
                    })
                    .unwrap_or_default();
                Ok(json_ok(
                    json!({ "UserPoolClients": clients, "NextToken": null }),
                ))
            }

            // ----------------------------------------------------------------
            // AdminCreateUser
            // ----------------------------------------------------------------
            "AdminCreateUser" => {
                let pool_id = match str_param(ctx, "UserPoolId") {
                    Some(id) => id,
                    None => {
                        return Ok(json_error(
                            "InvalidParameterException",
                            "UserPoolId is required",
                            400,
                        ));
                    }
                };
                let username = match str_param(ctx, "Username") {
                    Some(u) => u,
                    None => {
                        return Ok(json_error(
                            "InvalidParameterException",
                            "Username is required",
                            400,
                        ));
                    }
                };
                let user_attributes: Vec<UserAttribute> = ctx
                    .request_body
                    .get("UserAttributes")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|a| {
                                let name = a.get("Name")?.as_str()?.to_string();
                                let value = a.get("Value")?.as_str()?.to_string();
                                Some(UserAttribute { name, value })
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                let temp_password = str_param(ctx, "TemporaryPassword");

                let key = (pool_id.clone(), username.clone());
                {
                    let Some(store) = self.store.get(account_id, region) else {
                        return Ok(json_error(
                            "ResourceNotFoundException",
                            &format!("User pool {pool_id} not found"),
                            400,
                        ));
                    };
                    if !store.user_pools.contains_key(&pool_id) {
                        return Ok(json_error(
                            "ResourceNotFoundException",
                            &format!("User pool {pool_id} not found"),
                            400,
                        ));
                    }
                    if store.users.contains_key(&key) {
                        return Ok(json_error(
                            "UsernameExistsException",
                            &format!("User {username} already exists"),
                            400,
                        ));
                    }
                }
                let now = Utc::now();
                let user = User {
                    username: username.clone(),
                    user_pool_id: pool_id,
                    attributes: user_attributes,
                    user_status: if temp_password.is_some() {
                        UserStatus::ForceChangePassword
                    } else {
                        UserStatus::Confirmed
                    },
                    enabled: true,
                    user_create_date: now,
                    user_last_modified_date: now,
                    password: temp_password,
                };
                let mut store = self.store.get_or_create(account_id, region);
                store.users.insert(key, user.clone());
                Ok(json_ok(json!({ "User": user_json(&user) })))
            }

            // ----------------------------------------------------------------
            // AdminDeleteUser
            // ----------------------------------------------------------------
            "AdminDeleteUser" => {
                let pool_id = match str_param(ctx, "UserPoolId") {
                    Some(id) => id,
                    None => {
                        return Ok(json_error(
                            "InvalidParameterException",
                            "UserPoolId is required",
                            400,
                        ));
                    }
                };
                let username = match str_param(ctx, "Username") {
                    Some(u) => u,
                    None => {
                        return Ok(json_error(
                            "InvalidParameterException",
                            "Username is required",
                            400,
                        ));
                    }
                };
                let mut store = self.store.get_or_create(account_id, region);
                let key = (pool_id.clone(), username.clone());
                if store.users.remove(&key).is_none() {
                    return Ok(json_error(
                        "UserNotFoundException",
                        &format!("User {username} not found in pool {pool_id}"),
                        400,
                    ));
                }
                Ok(json_ok(json!({})))
            }

            // ----------------------------------------------------------------
            // AdminGetUser
            // ----------------------------------------------------------------
            "AdminGetUser" => {
                let pool_id = match str_param(ctx, "UserPoolId") {
                    Some(id) => id,
                    None => {
                        return Ok(json_error(
                            "InvalidParameterException",
                            "UserPoolId is required",
                            400,
                        ));
                    }
                };
                let username = match str_param(ctx, "Username") {
                    Some(u) => u,
                    None => {
                        return Ok(json_error(
                            "InvalidParameterException",
                            "Username is required",
                            400,
                        ));
                    }
                };
                let Some(store) = self.store.get(account_id, region) else {
                    return Ok(json_error(
                        "UserNotFoundException",
                        &format!("User {username} not found"),
                        400,
                    ));
                };
                let key = (pool_id.clone(), username.clone());
                match store.users.get(&key) {
                    Some(u) => Ok(json_ok(user_json(u))),
                    None => Ok(json_error(
                        "UserNotFoundException",
                        &format!("User {username} not found in pool {pool_id}"),
                        400,
                    )),
                }
            }

            // ----------------------------------------------------------------
            // ListUsers
            // ----------------------------------------------------------------
            "ListUsers" => {
                let pool_id = match str_param(ctx, "UserPoolId") {
                    Some(id) => id,
                    None => {
                        return Ok(json_error(
                            "InvalidParameterException",
                            "UserPoolId is required",
                            400,
                        ));
                    }
                };
                let users: Vec<Value> = self
                    .store
                    .get(account_id, region)
                    .map(|store| {
                        store
                            .users
                            .iter()
                            .filter(|((pid, _), _)| pid == &pool_id)
                            .map(|(_, u)| user_json(u))
                            .collect()
                    })
                    .unwrap_or_default();
                Ok(json_ok(json!({ "Users": users, "PaginationToken": null })))
            }

            // ----------------------------------------------------------------
            // AdminSetUserPassword
            // ----------------------------------------------------------------
            "AdminSetUserPassword" => {
                let pool_id = match str_param(ctx, "UserPoolId") {
                    Some(id) => id,
                    None => {
                        return Ok(json_error(
                            "InvalidParameterException",
                            "UserPoolId is required",
                            400,
                        ));
                    }
                };
                let username = match str_param(ctx, "Username") {
                    Some(u) => u,
                    None => {
                        return Ok(json_error(
                            "InvalidParameterException",
                            "Username is required",
                            400,
                        ));
                    }
                };
                let password = match str_param(ctx, "Password") {
                    Some(p) => p,
                    None => {
                        return Ok(json_error(
                            "InvalidParameterException",
                            "Password is required",
                            400,
                        ));
                    }
                };
                let permanent = ctx
                    .request_body
                    .get("Permanent")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let mut store = self.store.get_or_create(account_id, region);
                let key = (pool_id.clone(), username.clone());
                match store.users.get_mut(&key) {
                    Some(user) => {
                        user.password = Some(password);
                        if permanent {
                            user.user_status = UserStatus::Confirmed;
                        }
                        user.user_last_modified_date = Utc::now();
                        Ok(json_ok(json!({})))
                    }
                    None => Ok(json_error(
                        "UserNotFoundException",
                        &format!("User {username} not found in pool {pool_id}"),
                        400,
                    )),
                }
            }

            // ----------------------------------------------------------------
            // AdminEnableUser / AdminDisableUser
            // ----------------------------------------------------------------
            "AdminEnableUser" | "AdminDisableUser" => {
                let enabled = ctx.operation == "AdminEnableUser";
                let pool_id = match str_param(ctx, "UserPoolId") {
                    Some(id) => id,
                    None => {
                        return Ok(json_error(
                            "InvalidParameterException",
                            "UserPoolId is required",
                            400,
                        ));
                    }
                };
                let username = match str_param(ctx, "Username") {
                    Some(u) => u,
                    None => {
                        return Ok(json_error(
                            "InvalidParameterException",
                            "Username is required",
                            400,
                        ));
                    }
                };
                let mut store = self.store.get_or_create(account_id, region);
                let key = (pool_id.clone(), username.clone());
                match store.users.get_mut(&key) {
                    Some(user) => {
                        user.enabled = enabled;
                        user.user_last_modified_date = Utc::now();
                        Ok(json_ok(json!({})))
                    }
                    None => Ok(json_error(
                        "UserNotFoundException",
                        &format!("User {username} not found in pool {pool_id}"),
                        400,
                    )),
                }
            }

            // ----------------------------------------------------------------
            // AdminUpdateUserAttributes
            // ----------------------------------------------------------------
            "AdminUpdateUserAttributes" => {
                let pool_id = match str_param(ctx, "UserPoolId") {
                    Some(id) => id,
                    None => {
                        return Ok(json_error(
                            "InvalidParameterException",
                            "UserPoolId is required",
                            400,
                        ));
                    }
                };
                let username = match str_param(ctx, "Username") {
                    Some(u) => u,
                    None => {
                        return Ok(json_error(
                            "InvalidParameterException",
                            "Username is required",
                            400,
                        ));
                    }
                };
                let updates: Vec<(String, String)> = ctx
                    .request_body
                    .get("UserAttributes")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|a| {
                                let name = a.get("Name")?.as_str()?.to_string();
                                let value = a.get("Value")?.as_str()?.to_string();
                                Some((name, value))
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                let mut store = self.store.get_or_create(account_id, region);
                let key = (pool_id.clone(), username.clone());
                match store.users.get_mut(&key) {
                    Some(user) => {
                        for (name, value) in updates {
                            if let Some(attr) = user.attributes.iter_mut().find(|a| a.name == name)
                            {
                                attr.value = value;
                            } else {
                                user.attributes
                                    .push(crate::store::UserAttribute { name, value });
                            }
                        }
                        user.user_last_modified_date = Utc::now();
                        Ok(json_ok(json!({})))
                    }
                    None => Ok(json_error(
                        "UserNotFoundException",
                        &format!("User {username} not found in pool {pool_id}"),
                        400,
                    )),
                }
            }

            // ----------------------------------------------------------------
            // AdminInitiateAuth (basic USER_PASSWORD_AUTH)
            // ----------------------------------------------------------------
            "AdminInitiateAuth" => {
                let pool_id = match str_param(ctx, "UserPoolId") {
                    Some(id) => id,
                    None => {
                        return Ok(json_error(
                            "InvalidParameterException",
                            "UserPoolId is required",
                            400,
                        ));
                    }
                };
                let auth_params = ctx
                    .request_body
                    .get("AuthParameters")
                    .cloned()
                    .unwrap_or_default();
                let username = auth_params
                    .get("USERNAME")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let password = auth_params
                    .get("PASSWORD")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();

                let Some(store) = self.store.get(account_id, region) else {
                    return Ok(json_error(
                        "UserNotFoundException",
                        &format!("User {username} not found"),
                        400,
                    ));
                };
                let key = (pool_id.clone(), username.clone());
                match store.users.get(&key) {
                    Some(user) => {
                        if !user.enabled {
                            return Ok(json_error(
                                "UserNotConfirmedException",
                                "User is disabled",
                                400,
                            ));
                        }
                        let stored_password = user.password.as_deref().unwrap_or("");
                        if !password.is_empty() && stored_password != password {
                            return Ok(json_error(
                                "NotAuthorizedException",
                                "Incorrect username or password",
                                400,
                            ));
                        }
                        let access_token = Uuid::new_v4().to_string();
                        let id_token = Uuid::new_v4().to_string();
                        let refresh_token = Uuid::new_v4().to_string();
                        Ok(json_ok(json!({
                            "AuthenticationResult": {
                                "AccessToken": access_token,
                                "IdToken": id_token,
                                "RefreshToken": refresh_token,
                                "TokenType": "Bearer",
                                "ExpiresIn": 3600,
                            },
                            "ChallengeName": null,
                        })))
                    }
                    None => Ok(json_error(
                        "UserNotFoundException",
                        &format!("User {username} not found in pool {pool_id}"),
                        400,
                    )),
                }
            }

            _ => Err(DispatchError::NotImplemented(ctx.operation.clone())),
        }
    }

    async fn storage_snapshot(&self) -> Option<serde_json::Value> {
        use serde_json::json;
        let mut pools = Vec::new();
        for entry in self.store.iter() {
            for pool in entry.value().user_pools.values() {
                pools.push(json!({
                    "id": pool.id, "kind": "user_pool",
                    "attributes": [
                        {"key": "name", "value": pool.name.clone()},
                        {"key": "status", "value": pool.status.clone()},
                    ]
                }));
            }
        }
        Some(json!({ "kind": "cognito-idp", "user_pools": pools }))
    }
}
