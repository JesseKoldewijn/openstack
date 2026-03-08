use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use chrono::Utc;
use openstack_service_framework::traits::{
    DispatchError, DispatchResponse, RequestContext, ServiceProvider,
};
use openstack_state::AccountRegionBundle;
use uuid::Uuid;

use crate::store::{Identity, SesStore, StoredEmail};

pub struct SesProvider {
    store: Arc<AccountRegionBundle<SesStore>>,
}

impl SesProvider {
    pub fn new() -> Self {
        Self {
            store: Arc::new(AccountRegionBundle::new()),
        }
    }
}

impl Default for SesProvider {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Helpers — SES uses query protocol (XML responses)
// ---------------------------------------------------------------------------

const SES_NS: &str = "http://ses.amazonaws.com/doc/2010-12-01/";

fn xml_resp(action: &str, rid: &str, inner: &str) -> DispatchResponse {
    let xml = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
<{action}Response xmlns=\"{SES_NS}\">\
<{action}Result>{inner}</{action}Result>\
<ResponseMetadata><RequestId>{rid}</RequestId></ResponseMetadata>\
</{action}Response>"
    );
    DispatchResponse {
        status_code: 200,
        body: Bytes::from(xml.into_bytes()),
        content_type: "text/xml".to_string(),
        headers: Vec::new(),
    }
}

fn xml_no_result(action: &str, rid: &str) -> DispatchResponse {
    let xml = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
<{action}Response xmlns=\"{SES_NS}\">\
<ResponseMetadata><RequestId>{rid}</RequestId></ResponseMetadata>\
</{action}Response>"
    );
    DispatchResponse {
        status_code: 200,
        body: Bytes::from(xml.into_bytes()),
        content_type: "text/xml".to_string(),
        headers: Vec::new(),
    }
}

fn xml_error(code: &str, message: &str, status: u16) -> DispatchResponse {
    let xml = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
<ErrorResponse xmlns=\"{SES_NS}\">\
<Error><Code>{code}</Code><Message>{message}</Message></Error>\
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

fn str_param<'a>(ctx: &'a RequestContext, key: &str) -> Option<&'a str> {
    ctx.query_params
        .get(key)
        .map(|s| s.as_str())
        .or_else(|| ctx.request_body.get(key).and_then(|v| v.as_str()))
}

fn addresses_from_params(ctx: &RequestContext, prefix: &str) -> Vec<String> {
    let mut result = Vec::new();
    let mut idx = 1;
    loop {
        let key = format!("{prefix}.{idx}");
        if let Some(addr) = ctx.query_params.get(&key) {
            result.push(addr.clone());
        } else {
            break;
        }
        idx += 1;
    }
    result
}

// ---------------------------------------------------------------------------
// ServiceProvider
// ---------------------------------------------------------------------------

#[async_trait]
impl ServiceProvider for SesProvider {
    fn service_name(&self) -> &str {
        "ses"
    }

    async fn dispatch(&self, ctx: &RequestContext) -> Result<DispatchResponse, DispatchError> {
        let region = &ctx.region;
        let account_id = &ctx.account_id;
        let rid = req_id();

        match ctx.operation.as_str() {
            // ----------------------------------------------------------------
            // VerifyEmailIdentity
            // ----------------------------------------------------------------
            "VerifyEmailIdentity" => {
                let email = match str_param(ctx, "EmailAddress") {
                    Some(e) => e.to_string(),
                    None => return Ok(xml_error("MissingParameter", "EmailAddress required", 400)),
                };
                let identity = Identity {
                    identity: email.clone(),
                    verified: true,
                };
                let mut store = self.store.get_or_create(account_id, region);
                store.identities.insert(email, identity);
                Ok(xml_no_result("VerifyEmailIdentity", &rid))
            }

            // ----------------------------------------------------------------
            // ListIdentities
            // ----------------------------------------------------------------
            "ListIdentities" => {
                let store = self.store.get_or_create(account_id, region);
                let members: String = store
                    .identities
                    .keys()
                    .map(|id| format!("<member>{id}</member>"))
                    .collect();
                let inner = format!("<Identities>{members}</Identities>");
                Ok(xml_resp("ListIdentities", &rid, &inner))
            }

            // ----------------------------------------------------------------
            // SendEmail
            // ----------------------------------------------------------------
            "SendEmail" => {
                let source = match str_param(ctx, "Source") {
                    Some(s) => s.to_string(),
                    None => return Ok(xml_error("MissingParameter", "Source required", 400)),
                };
                let to = addresses_from_params(ctx, "Destination.ToAddresses.member");
                let cc = addresses_from_params(ctx, "Destination.CcAddresses.member");
                let bcc = addresses_from_params(ctx, "Destination.BccAddresses.member");
                let subject = str_param(ctx, "Message.Subject.Data")
                    .unwrap_or("")
                    .to_string();
                let body_text = str_param(ctx, "Message.Body.Text.Data")
                    .unwrap_or("")
                    .to_string();
                let body_html = str_param(ctx, "Message.Body.Html.Data")
                    .unwrap_or("")
                    .to_string();

                let message_id = Uuid::new_v4().to_string();
                let email = StoredEmail {
                    message_id: message_id.clone(),
                    source,
                    destination_to: to,
                    destination_cc: cc,
                    destination_bcc: bcc,
                    subject,
                    body_text,
                    body_html,
                    sent_at: Utc::now(),
                };
                let mut store = self.store.get_or_create(account_id, region);
                store.emails.insert(message_id.clone(), email);
                let inner = format!("<MessageId>{message_id}</MessageId>");
                Ok(xml_resp("SendEmail", &rid, &inner))
            }

            // ----------------------------------------------------------------
            // SendRawEmail
            // ----------------------------------------------------------------
            "SendRawEmail" => {
                let source = str_param(ctx, "Source").unwrap_or("unknown").to_string();
                let message_id = Uuid::new_v4().to_string();
                let raw_data = str_param(ctx, "RawMessage.Data").unwrap_or("").to_string();
                let email = StoredEmail {
                    message_id: message_id.clone(),
                    source,
                    destination_to: Vec::new(),
                    destination_cc: Vec::new(),
                    destination_bcc: Vec::new(),
                    subject: String::new(),
                    body_text: raw_data,
                    body_html: String::new(),
                    sent_at: Utc::now(),
                };
                let mut store = self.store.get_or_create(account_id, region);
                store.emails.insert(message_id.clone(), email);
                let inner = format!("<MessageId>{message_id}</MessageId>");
                Ok(xml_resp("SendRawEmail", &rid, &inner))
            }

            _ => Err(DispatchError::NotImplemented(ctx.operation.clone())),
        }
    }
}
