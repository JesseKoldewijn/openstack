use std::collections::HashMap;

use bytes::Bytes;
use openstack_cognito::CognitoProvider;
use openstack_service_framework::traits::{RequestContext, ServiceProvider};
use serde_json::{Value, json};

fn make_ctx(operation: &str, body: Value) -> RequestContext {
    RequestContext {
        service: "cognito-idp".to_string(),
        operation: operation.to_string(),
        region: "us-east-1".to_string(),
        account_id: "000000000000".to_string(),
        request_body: body.clone(),
        raw_body: Some(Bytes::from(serde_json::to_vec(&body).unwrap())),
        headers: Default::default(),
        path: "/".to_string(),
        method: "POST".to_string(),
        query_params: HashMap::new(),
        request_id: String::new(),
        spooled_body: None,
        body_reader: None,
    }
}

fn body_json(resp: &openstack_service_framework::traits::DispatchResponse) -> Value {
    serde_json::from_slice(resp.body.as_bytes()).expect("valid JSON")
}

// ---------------------------------------------------------------------------
// User Pools
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_create_user_pool() {
    let p = CognitoProvider::new();
    let resp = p
        .dispatch(&make_ctx(
            "CreateUserPool",
            json!({
                "PoolName": "my-pool",
                "MfaConfiguration": "OFF",
                "AutoVerifiedAttributes": ["email"],
            }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
    assert_eq!(resp.content_type, "application/x-amz-json-1.1");
    let b = body_json(&resp);
    assert_eq!(b["UserPool"]["Name"], "my-pool");
    assert_eq!(b["UserPool"]["Status"], "Active");
    let id = b["UserPool"]["Id"].as_str().unwrap();
    assert!(id.starts_with("us-east-1_"));
    let arn = b["UserPool"]["Arn"].as_str().unwrap();
    assert!(arn.contains("userpool/"));
}

#[tokio::test]
async fn test_describe_user_pool() {
    let p = CognitoProvider::new();
    let create_resp = p
        .dispatch(&make_ctx(
            "CreateUserPool",
            json!({ "PoolName": "desc-pool" }),
        ))
        .await
        .unwrap();
    let pool_id = body_json(&create_resp)["UserPool"]["Id"]
        .as_str()
        .unwrap()
        .to_string();

    let resp = p
        .dispatch(&make_ctx(
            "DescribeUserPool",
            json!({ "UserPoolId": pool_id }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
    let b = body_json(&resp);
    assert_eq!(b["UserPool"]["Name"], "desc-pool");
    assert_eq!(b["UserPool"]["Id"], pool_id);
}

#[tokio::test]
async fn test_describe_user_pool_not_found() {
    let p = CognitoProvider::new();
    let resp = p
        .dispatch(&make_ctx(
            "DescribeUserPool",
            json!({ "UserPoolId": "us-east-1_nonexistent" }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 400);
    let b = body_json(&resp);
    assert!(
        b["__type"]
            .as_str()
            .unwrap()
            .contains("ResourceNotFoundException")
    );
}

#[tokio::test]
async fn test_list_user_pools() {
    let p = CognitoProvider::new();
    for name in ["pool-x", "pool-y"] {
        p.dispatch(&make_ctx("CreateUserPool", json!({ "PoolName": name })))
            .await
            .unwrap();
    }

    let resp = p
        .dispatch(&make_ctx("ListUserPools", json!({ "MaxResults": 10 })))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
    let b = body_json(&resp);
    assert_eq!(b["UserPools"].as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn test_delete_user_pool() {
    let p = CognitoProvider::new();
    let create_resp = p
        .dispatch(&make_ctx(
            "CreateUserPool",
            json!({ "PoolName": "del-pool" }),
        ))
        .await
        .unwrap();
    let pool_id = body_json(&create_resp)["UserPool"]["Id"]
        .as_str()
        .unwrap()
        .to_string();

    let resp = p
        .dispatch(&make_ctx(
            "DeleteUserPool",
            json!({ "UserPoolId": pool_id }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);

    let list_resp = p
        .dispatch(&make_ctx("ListUserPools", json!({ "MaxResults": 10 })))
        .await
        .unwrap();
    assert_eq!(
        body_json(&list_resp)["UserPools"].as_array().unwrap().len(),
        0
    );
}

#[tokio::test]
async fn test_update_user_pool() {
    let p = CognitoProvider::new();
    let create_resp = p
        .dispatch(&make_ctx(
            "CreateUserPool",
            json!({ "PoolName": "upd-pool" }),
        ))
        .await
        .unwrap();
    let pool_id = body_json(&create_resp)["UserPool"]["Id"]
        .as_str()
        .unwrap()
        .to_string();

    let resp = p
        .dispatch(&make_ctx(
            "UpdateUserPool",
            json!({ "UserPoolId": pool_id, "MfaConfiguration": "ON" }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);

    let desc_resp = p
        .dispatch(&make_ctx(
            "DescribeUserPool",
            json!({ "UserPoolId": pool_id }),
        ))
        .await
        .unwrap();
    assert_eq!(body_json(&desc_resp)["UserPool"]["MfaConfiguration"], "ON");
}

// ---------------------------------------------------------------------------
// App Clients
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_create_and_describe_client() {
    let p = CognitoProvider::new();
    let pool_resp = p
        .dispatch(&make_ctx(
            "CreateUserPool",
            json!({ "PoolName": "client-pool" }),
        ))
        .await
        .unwrap();
    let pool_id = body_json(&pool_resp)["UserPool"]["Id"]
        .as_str()
        .unwrap()
        .to_string();

    let resp = p
        .dispatch(&make_ctx(
            "CreateUserPoolClient",
            json!({
                "UserPoolId": pool_id,
                "ClientName": "my-client",
                "GenerateSecret": true,
                "ExplicitAuthFlows": ["ALLOW_USER_PASSWORD_AUTH"],
            }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
    let b = body_json(&resp);
    assert_eq!(b["UserPoolClient"]["ClientName"], "my-client");
    let client_id = b["UserPoolClient"]["ClientId"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(b["UserPoolClient"]["ClientSecret"].is_string());

    let desc_resp = p
        .dispatch(&make_ctx(
            "DescribeUserPoolClient",
            json!({ "UserPoolId": pool_id, "ClientId": client_id }),
        ))
        .await
        .unwrap();
    assert_eq!(desc_resp.status_code, 200);
    assert_eq!(
        body_json(&desc_resp)["UserPoolClient"]["ClientName"],
        "my-client"
    );
}

#[tokio::test]
async fn test_list_user_pool_clients() {
    let p = CognitoProvider::new();
    let pool_resp = p
        .dispatch(&make_ctx(
            "CreateUserPool",
            json!({ "PoolName": "list-clients-pool" }),
        ))
        .await
        .unwrap();
    let pool_id = body_json(&pool_resp)["UserPool"]["Id"]
        .as_str()
        .unwrap()
        .to_string();

    for name in ["client-a", "client-b"] {
        p.dispatch(&make_ctx(
            "CreateUserPoolClient",
            json!({ "UserPoolId": pool_id, "ClientName": name }),
        ))
        .await
        .unwrap();
    }

    let resp = p
        .dispatch(&make_ctx(
            "ListUserPoolClients",
            json!({ "UserPoolId": pool_id, "MaxResults": 10 }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
    assert_eq!(
        body_json(&resp)["UserPoolClients"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
}

#[tokio::test]
async fn test_delete_client() {
    let p = CognitoProvider::new();
    let pool_resp = p
        .dispatch(&make_ctx(
            "CreateUserPool",
            json!({ "PoolName": "del-client-pool" }),
        ))
        .await
        .unwrap();
    let pool_id = body_json(&pool_resp)["UserPool"]["Id"]
        .as_str()
        .unwrap()
        .to_string();
    let client_resp = p
        .dispatch(&make_ctx(
            "CreateUserPoolClient",
            json!({ "UserPoolId": pool_id, "ClientName": "del-client" }),
        ))
        .await
        .unwrap();
    let client_id = body_json(&client_resp)["UserPoolClient"]["ClientId"]
        .as_str()
        .unwrap()
        .to_string();

    let resp = p
        .dispatch(&make_ctx(
            "DeleteUserPoolClient",
            json!({ "UserPoolId": pool_id, "ClientId": client_id }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
}

// ---------------------------------------------------------------------------
// Admin User Management
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_admin_create_and_get_user() {
    let p = CognitoProvider::new();
    let pool_resp = p
        .dispatch(&make_ctx(
            "CreateUserPool",
            json!({ "PoolName": "user-pool" }),
        ))
        .await
        .unwrap();
    let pool_id = body_json(&pool_resp)["UserPool"]["Id"]
        .as_str()
        .unwrap()
        .to_string();

    let resp = p
        .dispatch(&make_ctx(
            "AdminCreateUser",
            json!({
                "UserPoolId": pool_id,
                "Username": "alice",
                "UserAttributes": [
                    {"Name": "email", "Value": "alice@example.com"},
                    {"Name": "email_verified", "Value": "true"},
                ],
                "TemporaryPassword": "Temp1234!",
            }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
    let b = body_json(&resp);
    assert_eq!(b["User"]["Username"], "alice");
    assert_eq!(b["User"]["UserStatus"], "FORCE_CHANGE_PASSWORD");

    let get_resp = p
        .dispatch(&make_ctx(
            "AdminGetUser",
            json!({ "UserPoolId": pool_id, "Username": "alice" }),
        ))
        .await
        .unwrap();
    assert_eq!(get_resp.status_code, 200);
    let gb = body_json(&get_resp);
    assert_eq!(gb["Username"], "alice");
    let attrs = gb["Attributes"].as_array().unwrap();
    assert!(
        attrs
            .iter()
            .any(|a| a["Name"] == "email" && a["Value"] == "alice@example.com")
    );
}

#[tokio::test]
async fn test_admin_create_user_duplicate_fails() {
    let p = CognitoProvider::new();
    let pool_resp = p
        .dispatch(&make_ctx(
            "CreateUserPool",
            json!({ "PoolName": "dup-user-pool" }),
        ))
        .await
        .unwrap();
    let pool_id = body_json(&pool_resp)["UserPool"]["Id"]
        .as_str()
        .unwrap()
        .to_string();

    let user_body = json!({ "UserPoolId": pool_id, "Username": "bob" });
    p.dispatch(&make_ctx("AdminCreateUser", user_body.clone()))
        .await
        .unwrap();
    let resp = p
        .dispatch(&make_ctx("AdminCreateUser", user_body))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 400);
    assert!(
        body_json(&resp)["__type"]
            .as_str()
            .unwrap()
            .contains("UsernameExistsException")
    );
}

#[tokio::test]
async fn test_list_users() {
    let p = CognitoProvider::new();
    let pool_resp = p
        .dispatch(&make_ctx(
            "CreateUserPool",
            json!({ "PoolName": "list-users-pool" }),
        ))
        .await
        .unwrap();
    let pool_id = body_json(&pool_resp)["UserPool"]["Id"]
        .as_str()
        .unwrap()
        .to_string();

    for user in ["u1", "u2", "u3"] {
        p.dispatch(&make_ctx(
            "AdminCreateUser",
            json!({ "UserPoolId": pool_id, "Username": user }),
        ))
        .await
        .unwrap();
    }

    let resp = p
        .dispatch(&make_ctx("ListUsers", json!({ "UserPoolId": pool_id })))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
    assert_eq!(body_json(&resp)["Users"].as_array().unwrap().len(), 3);
}

#[tokio::test]
async fn test_admin_delete_user() {
    let p = CognitoProvider::new();
    let pool_resp = p
        .dispatch(&make_ctx(
            "CreateUserPool",
            json!({ "PoolName": "del-user-pool" }),
        ))
        .await
        .unwrap();
    let pool_id = body_json(&pool_resp)["UserPool"]["Id"]
        .as_str()
        .unwrap()
        .to_string();
    p.dispatch(&make_ctx(
        "AdminCreateUser",
        json!({ "UserPoolId": pool_id, "Username": "to-delete" }),
    ))
    .await
    .unwrap();

    let del_resp = p
        .dispatch(&make_ctx(
            "AdminDeleteUser",
            json!({ "UserPoolId": pool_id, "Username": "to-delete" }),
        ))
        .await
        .unwrap();
    assert_eq!(del_resp.status_code, 200);

    let get_resp = p
        .dispatch(&make_ctx(
            "AdminGetUser",
            json!({ "UserPoolId": pool_id, "Username": "to-delete" }),
        ))
        .await
        .unwrap();
    assert_eq!(get_resp.status_code, 400);
    assert!(
        body_json(&get_resp)["__type"]
            .as_str()
            .unwrap()
            .contains("UserNotFoundException")
    );
}

#[tokio::test]
async fn test_admin_set_user_password() {
    let p = CognitoProvider::new();
    let pool_resp = p
        .dispatch(&make_ctx(
            "CreateUserPool",
            json!({ "PoolName": "pw-pool" }),
        ))
        .await
        .unwrap();
    let pool_id = body_json(&pool_resp)["UserPool"]["Id"]
        .as_str()
        .unwrap()
        .to_string();
    p.dispatch(&make_ctx(
        "AdminCreateUser",
        json!({ "UserPoolId": pool_id, "Username": "pw-user", "TemporaryPassword": "Temp1!" }),
    ))
    .await
    .unwrap();

    let resp = p
        .dispatch(&make_ctx(
            "AdminSetUserPassword",
            json!({
                "UserPoolId": pool_id,
                "Username": "pw-user",
                "Password": "NewPass1!",
                "Permanent": true,
            }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);

    // Status should now be CONFIRMED
    let get_resp = p
        .dispatch(&make_ctx(
            "AdminGetUser",
            json!({ "UserPoolId": pool_id, "Username": "pw-user" }),
        ))
        .await
        .unwrap();
    assert_eq!(body_json(&get_resp)["UserStatus"], "CONFIRMED");
}

#[tokio::test]
async fn test_admin_enable_disable_user() {
    let p = CognitoProvider::new();
    let pool_resp = p
        .dispatch(&make_ctx(
            "CreateUserPool",
            json!({ "PoolName": "en-dis-pool" }),
        ))
        .await
        .unwrap();
    let pool_id = body_json(&pool_resp)["UserPool"]["Id"]
        .as_str()
        .unwrap()
        .to_string();
    p.dispatch(&make_ctx(
        "AdminCreateUser",
        json!({ "UserPoolId": pool_id, "Username": "toggle-user" }),
    ))
    .await
    .unwrap();

    let dis_resp = p
        .dispatch(&make_ctx(
            "AdminDisableUser",
            json!({ "UserPoolId": pool_id, "Username": "toggle-user" }),
        ))
        .await
        .unwrap();
    assert_eq!(dis_resp.status_code, 200);

    let get_resp = p
        .dispatch(&make_ctx(
            "AdminGetUser",
            json!({ "UserPoolId": pool_id, "Username": "toggle-user" }),
        ))
        .await
        .unwrap();
    assert_eq!(body_json(&get_resp)["Enabled"], false);

    let en_resp = p
        .dispatch(&make_ctx(
            "AdminEnableUser",
            json!({ "UserPoolId": pool_id, "Username": "toggle-user" }),
        ))
        .await
        .unwrap();
    assert_eq!(en_resp.status_code, 200);

    let get_resp2 = p
        .dispatch(&make_ctx(
            "AdminGetUser",
            json!({ "UserPoolId": pool_id, "Username": "toggle-user" }),
        ))
        .await
        .unwrap();
    assert_eq!(body_json(&get_resp2)["Enabled"], true);
}

#[tokio::test]
async fn test_admin_update_user_attributes() {
    let p = CognitoProvider::new();
    let pool_resp = p
        .dispatch(&make_ctx(
            "CreateUserPool",
            json!({ "PoolName": "attr-pool" }),
        ))
        .await
        .unwrap();
    let pool_id = body_json(&pool_resp)["UserPool"]["Id"]
        .as_str()
        .unwrap()
        .to_string();
    p.dispatch(&make_ctx(
        "AdminCreateUser",
        json!({
            "UserPoolId": pool_id,
            "Username": "attr-user",
            "UserAttributes": [{"Name": "email", "Value": "old@example.com"}],
        }),
    ))
    .await
    .unwrap();

    let resp = p
        .dispatch(&make_ctx(
            "AdminUpdateUserAttributes",
            json!({
                "UserPoolId": pool_id,
                "Username": "attr-user",
                "UserAttributes": [{"Name": "email", "Value": "new@example.com"}],
            }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);

    let get_resp = p
        .dispatch(&make_ctx(
            "AdminGetUser",
            json!({ "UserPoolId": pool_id, "Username": "attr-user" }),
        ))
        .await
        .unwrap();
    let attrs = body_json(&get_resp)["Attributes"]
        .as_array()
        .unwrap()
        .clone();
    assert!(
        attrs
            .iter()
            .any(|a| a["Name"] == "email" && a["Value"] == "new@example.com")
    );
}

// ---------------------------------------------------------------------------
// AdminInitiateAuth
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_admin_initiate_auth_success() {
    let p = CognitoProvider::new();
    let pool_resp = p
        .dispatch(&make_ctx(
            "CreateUserPool",
            json!({ "PoolName": "auth-pool" }),
        ))
        .await
        .unwrap();
    let pool_id = body_json(&pool_resp)["UserPool"]["Id"]
        .as_str()
        .unwrap()
        .to_string();
    p.dispatch(&make_ctx(
        "AdminCreateUser",
        json!({ "UserPoolId": pool_id, "Username": "auth-user" }),
    ))
    .await
    .unwrap();
    p.dispatch(&make_ctx(
        "AdminSetUserPassword",
        json!({
            "UserPoolId": pool_id,
            "Username": "auth-user",
            "Password": "Correct1!",
            "Permanent": true,
        }),
    ))
    .await
    .unwrap();

    let resp = p
        .dispatch(&make_ctx(
            "AdminInitiateAuth",
            json!({
                "UserPoolId": pool_id,
                "ClientId": "dummy-client",
                "AuthFlow": "USER_PASSWORD_AUTH",
                "AuthParameters": {
                    "USERNAME": "auth-user",
                    "PASSWORD": "Correct1!",
                },
            }),
        ))
        .await
        .unwrap();
    assert_eq!(
        resp.status_code,
        200,
        "{}",
        String::from_utf8_lossy(resp.body.as_bytes())
    );
    let b = body_json(&resp);
    assert!(b["AuthenticationResult"]["AccessToken"].is_string());
    assert!(b["AuthenticationResult"]["IdToken"].is_string());
    assert_eq!(b["AuthenticationResult"]["TokenType"], "Bearer");
}

#[tokio::test]
async fn test_admin_initiate_auth_wrong_password() {
    let p = CognitoProvider::new();
    let pool_resp = p
        .dispatch(&make_ctx(
            "CreateUserPool",
            json!({ "PoolName": "auth-fail-pool" }),
        ))
        .await
        .unwrap();
    let pool_id = body_json(&pool_resp)["UserPool"]["Id"]
        .as_str()
        .unwrap()
        .to_string();
    p.dispatch(&make_ctx(
        "AdminCreateUser",
        json!({ "UserPoolId": pool_id, "Username": "fail-user" }),
    ))
    .await
    .unwrap();
    p.dispatch(&make_ctx(
        "AdminSetUserPassword",
        json!({
            "UserPoolId": pool_id,
            "Username": "fail-user",
            "Password": "CorrectPass1!",
            "Permanent": true,
        }),
    ))
    .await
    .unwrap();

    let resp = p
        .dispatch(&make_ctx(
            "AdminInitiateAuth",
            json!({
                "UserPoolId": pool_id,
                "ClientId": "dummy-client",
                "AuthFlow": "USER_PASSWORD_AUTH",
                "AuthParameters": {
                    "USERNAME": "fail-user",
                    "PASSWORD": "WrongPass1!",
                },
            }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 400);
    assert!(
        body_json(&resp)["__type"]
            .as_str()
            .unwrap()
            .contains("NotAuthorizedException")
    );
}
