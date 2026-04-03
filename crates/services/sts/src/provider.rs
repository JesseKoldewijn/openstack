use std::borrow::Cow;

use async_trait::async_trait;
use base64::{Engine as _, engine::general_purpose::STANDARD as B64};
use bytes::Bytes;
use chrono::Utc;
use openstack_service_framework::traits::{
    DispatchError, DispatchResponse, RequestContext, ResponseBody, ServiceProvider,
};
use openstack_service_framework::xml::xml_escape;
use uuid::Uuid;

pub struct StsProvider;

impl StsProvider {
    pub fn new() -> Self {
        Self
    }
}

impl Default for StsProvider {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// XML helpers — STS uses query protocol (XML responses)
// ---------------------------------------------------------------------------

fn xml_resp(action: &str, request_id: &str, inner: &str) -> DispatchResponse {
    let xml = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
<{action}Response xmlns=\"https://sts.amazonaws.com/doc/2011-06-15/\">\
<{action}Result>{inner}</{action}Result>\
<ResponseMetadata><RequestId>{request_id}</RequestId></ResponseMetadata>\
</{action}Response>"
    );
    DispatchResponse {
        status_code: 200,
        body: ResponseBody::Buffered(Bytes::from(xml.into_bytes())),
        content_type: Cow::Borrowed("text/xml"),
        headers: Vec::new(),
    }
}

fn sts_error(code: &str, message: &str, status: u16) -> DispatchResponse {
    let xml = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
<ErrorResponse xmlns=\"https://sts.amazonaws.com/doc/2011-06-15/\">\
<Error><Type>Sender</Type><Code>{code}</Code><Message>{message}</Message></Error>\
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

fn param(ctx: &RequestContext, key: &str) -> Option<String> {
    ctx.query_params.get(key).cloned().or_else(|| {
        if let Some(obj) = ctx.request_body.as_object() {
            obj.get(key).and_then(|v| v.as_str()).map(String::from)
        } else {
            None
        }
    })
}

fn temp_credentials_xml(prefix: &str) -> String {
    let expiry = (Utc::now() + chrono::Duration::hours(1)).format("%Y-%m-%dT%H:%M:%SZ");
    format!(
        "<Credentials>\
<AccessKeyId>ASIA{}</AccessKeyId>\
<SecretAccessKey>{}</SecretAccessKey>\
<SessionToken>FQoGZXIvYXdzE{}</SessionToken>\
<Expiration>{expiry}</Expiration>\
</Credentials>",
        &Uuid::new_v4().to_string().replace('-', "")[..16].to_uppercase(),
        &Uuid::new_v4().to_string().replace('-', ""),
        prefix,
    )
}

// ---------------------------------------------------------------------------
// ServiceProvider
// ---------------------------------------------------------------------------

#[async_trait]
impl ServiceProvider for StsProvider {
    fn service_name(&self) -> &str {
        "sts"
    }

    async fn dispatch(&self, ctx: &RequestContext) -> Result<DispatchResponse, DispatchError> {
        let op = ctx.operation.as_str();
        let rid = req_id();
        let account_id = &ctx.account_id;
        // STS is global — pin to us-east-1
        match op {
            "GetCallerIdentity" => {
                let inner = format!(
                    "<UserId>AKIAIOSFODNN7EXAMPLE</UserId>\
<Account>{account_id}</Account>\
<Arn>arn:aws:iam::{account_id}:root</Arn>"
                );
                Ok(xml_resp("GetCallerIdentity", &rid, &inner))
            }

            "AssumeRole" => {
                let role_arn = param(ctx, "RoleArn")
                    .unwrap_or_else(|| format!("arn:aws:iam::{account_id}:role/default"));
                let role_name = role_arn
                    .rsplit('/')
                    .next()
                    .filter(|s| !s.is_empty())
                    .unwrap_or("default")
                    .to_string();
                let session_name =
                    param(ctx, "RoleSessionName").unwrap_or_else(|| "session".to_string());
                let creds = temp_credentials_xml("nr//");
                let role_id_suffix =
                    &Uuid::new_v4().to_string().replace('-', "")[..16].to_uppercase();
                let inner = format!(
                    "{creds}\
<AssumedRoleUser>\
<AssumedRoleId>AROA{role_id_suffix}:{session_name}</AssumedRoleId>\
<Arn>arn:aws:sts::{account_id}:assumed-role/{role_name}/{session_name}</Arn>\
</AssumedRoleUser>"
                );
                Ok(xml_resp("AssumeRole", &rid, &inner))
            }

            "GetSessionToken" => {
                let creds = temp_credentials_xml("st//");
                Ok(xml_resp("GetSessionToken", &rid, &creds))
            }

            "GetAccessKeyInfo" => {
                // Return the account ID for the given access key.
                let inner = format!("<Account>{account_id}</Account>");
                Ok(xml_resp("GetAccessKeyInfo", &rid, &inner))
            }

            "DecodeAuthorizationMessage" => {
                // Base64-decode the EncodedMessage and return it as DecodedMessage.
                // In AWS this contains a JSON policy evaluation context; here we decode
                // whatever was passed so Sign/Verify round-trips work correctly.
                let encoded = match param(ctx, "EncodedMessage") {
                    Some(v) if !v.is_empty() => v,
                    _ => {
                        return Ok(sts_error(
                            "InvalidAuthorizationMessage",
                            "EncodedMessage is required or invalid",
                            400,
                        ));
                    }
                };
                let decoded = match B64.decode(encoded.as_bytes()) {
                    Ok(b) => match String::from_utf8(b) {
                        Ok(s) => s,
                        Err(_) => {
                            return Ok(sts_error(
                                "InvalidAuthorizationMessage",
                                "EncodedMessage is required or invalid",
                                400,
                            ));
                        }
                    },
                    Err(_) => {
                        return Ok(sts_error(
                            "InvalidAuthorizationMessage",
                            "EncodedMessage is required or invalid",
                            400,
                        ));
                    }
                };
                // XML-escape the decoded JSON before embedding it in XML.
                let inner = format!("<DecodedMessage>{}</DecodedMessage>", xml_escape(&decoded));
                Ok(xml_resp("DecodeAuthorizationMessage", &rid, &inner))
            }

            _ => Ok(sts_error(
                "NotImplemented",
                &format!("Operation not implemented: {op}"),
                501,
            )),
        }
    }

    // STS is stateless — no persistent resources to inspect.
    async fn storage_snapshot(&self) -> Option<serde_json::Value> {
        None
    }
}
