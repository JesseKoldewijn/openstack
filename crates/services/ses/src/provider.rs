use std::borrow::Cow;
use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use chrono::Utc;
use openstack_service_framework::traits::{
    DispatchError, DispatchResponse, RequestContext, ResponseBody, ServiceProvider,
};
use openstack_state::AccountRegionBundle;
use uuid::Uuid;

use crate::store::{EmailTemplate, Identity, SesStore, StoredEmail};

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
        body: ResponseBody::Buffered(Bytes::from(xml.into_bytes())),
        content_type: Cow::Borrowed("text/xml"),
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
        body: ResponseBody::Buffered(Bytes::from(xml.into_bytes())),
        content_type: Cow::Borrowed("text/xml"),
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
            // VerifyDomainIdentity
            // ----------------------------------------------------------------
            "VerifyDomainIdentity" => {
                let domain = match str_param(ctx, "Domain") {
                    Some(d) => d.to_string(),
                    None => return Ok(xml_error("MissingParameter", "Domain required", 400)),
                };
                let identity = Identity {
                    identity: domain.clone(),
                    verified: true,
                };
                let mut store = self.store.get_or_create(account_id, region);
                store.identities.insert(domain.clone(), identity);
                let token = format!("{}-verification-token", domain.replace('.', "-"));
                let inner = format!("<VerificationToken>{token}</VerificationToken>");
                Ok(xml_resp("VerifyDomainIdentity", &rid, &inner))
            }

            // ----------------------------------------------------------------
            // ListIdentities
            // ----------------------------------------------------------------
            "ListIdentities" => {
                let Some(store) = self.store.get(account_id, region) else {
                    let inner = "<Identities />";
                    return Ok(xml_resp("ListIdentities", &rid, inner));
                };
                let members: String = store
                    .identities
                    .keys()
                    .map(|id| format!("<member>{id}</member>"))
                    .collect();
                let identities = if members.is_empty() {
                    "<Identities />".to_string()
                } else {
                    format!("<Identities>{members}</Identities>")
                };
                let inner = identities;
                Ok(xml_resp("ListIdentities", &rid, &inner))
            }

            // ----------------------------------------------------------------
            // GetIdentityVerificationAttributes
            // ----------------------------------------------------------------
            "GetIdentityVerificationAttributes" => {
                let identities = addresses_from_params(ctx, "Identities.member");
                let attrs = self
                    .store
                    .get(account_id, region)
                    .map(|store| {
                        identities
                            .iter()
                            .filter_map(|identity| {
                                store.identities.get(identity).map(|id| {
                                    format!(
                                        "<entry><key>{}</key><value><VerificationStatus>{}</VerificationStatus></value></entry>",
                                        identity,
                                        if id.verified { "Success" } else { "Pending" }
                                    )
                                })
                            })
                            .collect::<String>()
                    })
                    .unwrap_or_default();
                let inner = format!("<VerificationAttributes>{attrs}</VerificationAttributes>");
                Ok(xml_resp("GetIdentityVerificationAttributes", &rid, &inner))
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

            // ----------------------------------------------------------------
            // DeleteIdentity
            // ----------------------------------------------------------------
            "DeleteIdentity" => {
                let identity = match str_param(ctx, "Identity") {
                    Some(i) => i.to_string(),
                    None => return Ok(xml_error("MissingParameter", "Identity required", 400)),
                };
                let mut store = self.store.get_or_create(account_id, region);
                store.identities.remove(&identity);
                Ok(xml_no_result("DeleteIdentity", &rid))
            }

            // ----------------------------------------------------------------
            // GetSendQuota
            // ----------------------------------------------------------------
            "GetSendQuota" => {
                let sent_count = self
                    .store
                    .get(account_id, region)
                    .map(|s| s.emails.len() as f64)
                    .unwrap_or(0.0);
                let inner = format!(
                    "<Max24HourSend>50000.0</Max24HourSend>\
<MaxSendRate>14.0</MaxSendRate>\
<SentLast24Hours>{sent_count}</SentLast24Hours>"
                );
                Ok(xml_resp("GetSendQuota", &rid, &inner))
            }

            // ----------------------------------------------------------------
            // GetSendStatistics
            // ----------------------------------------------------------------
            "GetSendStatistics" => {
                let data_points = self
                    .store
                    .get(account_id, region)
                    .map(|store| {
                        // Group emails by hour bucket for realistic stats
                        let total = store.emails.len() as u64;
                        if total == 0 {
                            return String::new();
                        }
                        format!(
                            "<member>\
<Timestamp>{}</Timestamp>\
<DeliveryAttempts>{total}</DeliveryAttempts>\
<Bounces>0</Bounces>\
<Complaints>0</Complaints>\
<Rejects>0</Rejects>\
</member>",
                            Utc::now().format("%Y-%m-%dT%H:00:00Z")
                        )
                    })
                    .unwrap_or_default();
                let inner = format!("<SendDataPoints>{data_points}</SendDataPoints>");
                Ok(xml_resp("GetSendStatistics", &rid, &inner))
            }

            // ----------------------------------------------------------------
            // CreateTemplate
            // ----------------------------------------------------------------
            "CreateTemplate" => {
                let template_name = match str_param(ctx, "Template.TemplateName") {
                    Some(n) => n.to_string(),
                    None => {
                        return Ok(xml_error(
                            "MissingParameter",
                            "Template.TemplateName required",
                            400,
                        ));
                    }
                };
                let subject_part = str_param(ctx, "Template.SubjectPart")
                    .unwrap_or("")
                    .to_string();
                let html_part = str_param(ctx, "Template.HtmlPart").unwrap_or("").to_string();
                let text_part = str_param(ctx, "Template.TextPart").unwrap_or("").to_string();

                let mut store = self.store.get_or_create(account_id, region);
                if store.templates.contains_key(&template_name) {
                    return Ok(xml_error(
                        "AlreadyExists",
                        &format!("Template {template_name} already exists"),
                        400,
                    ));
                }
                store.templates.insert(
                    template_name.clone(),
                    EmailTemplate {
                        template_name,
                        subject_part,
                        html_part,
                        text_part,
                        created_at: Utc::now(),
                    },
                );
                Ok(xml_no_result("CreateTemplate", &rid))
            }

            // ----------------------------------------------------------------
            // DeleteTemplate
            // ----------------------------------------------------------------
            "DeleteTemplate" => {
                let template_name = match str_param(ctx, "TemplateName") {
                    Some(n) => n.to_string(),
                    None => {
                        return Ok(xml_error(
                            "MissingParameter",
                            "TemplateName required",
                            400,
                        ));
                    }
                };
                let mut store = self.store.get_or_create(account_id, region);
                store.templates.remove(&template_name);
                Ok(xml_no_result("DeleteTemplate", &rid))
            }

            // ----------------------------------------------------------------
            // GetTemplate
            // ----------------------------------------------------------------
            "GetTemplate" => {
                let template_name = match str_param(ctx, "TemplateName") {
                    Some(n) => n.to_string(),
                    None => {
                        return Ok(xml_error(
                            "MissingParameter",
                            "TemplateName required",
                            400,
                        ));
                    }
                };
                let Some(store) = self.store.get(account_id, region) else {
                    return Ok(xml_error(
                        "TemplateDoesNotExist",
                        &format!("Template {template_name} does not exist"),
                        400,
                    ));
                };
                match store.templates.get(&template_name) {
                    Some(t) => {
                        let inner = format!(
                            "<Template>\
<TemplateName>{}</TemplateName>\
<SubjectPart>{}</SubjectPart>\
<HtmlPart>{}</HtmlPart>\
<TextPart>{}</TextPart>\
</Template>",
                            t.template_name, t.subject_part, t.html_part, t.text_part
                        );
                        Ok(xml_resp("GetTemplate", &rid, &inner))
                    }
                    None => Ok(xml_error(
                        "TemplateDoesNotExist",
                        &format!("Template {template_name} does not exist"),
                        400,
                    )),
                }
            }

            // ----------------------------------------------------------------
            // ListTemplates
            // ----------------------------------------------------------------
            "ListTemplates" => {
                let Some(store) = self.store.get(account_id, region) else {
                    return Ok(xml_resp(
                        "ListTemplates",
                        &rid,
                        "<TemplatesMetadata />",
                    ));
                };
                let members: String = store
                    .templates
                    .values()
                    .map(|t| {
                        format!(
                            "<member><Name>{}</Name><CreatedTimestamp>{}</CreatedTimestamp></member>",
                            t.template_name,
                            t.created_at.format("%Y-%m-%dT%H:%M:%SZ")
                        )
                    })
                    .collect();
                let inner = if members.is_empty() {
                    "<TemplatesMetadata />".to_string()
                } else {
                    format!("<TemplatesMetadata>{members}</TemplatesMetadata>")
                };
                Ok(xml_resp("ListTemplates", &rid, &inner))
            }

            // ----------------------------------------------------------------
            // SendTemplatedEmail
            // ----------------------------------------------------------------
            "SendTemplatedEmail" => {
                let source = match str_param(ctx, "Source") {
                    Some(s) => s.to_string(),
                    None => return Ok(xml_error("MissingParameter", "Source required", 400)),
                };
                let template_name = match str_param(ctx, "Template") {
                    Some(t) => t.to_string(),
                    None => return Ok(xml_error("MissingParameter", "Template required", 400)),
                };
                let to = addresses_from_params(ctx, "Destination.ToAddresses.member");
                let cc = addresses_from_params(ctx, "Destination.CcAddresses.member");
                let bcc = addresses_from_params(ctx, "Destination.BccAddresses.member");

                let (subject, html_part, text_part) = {
                    let Some(store) = self.store.get(account_id, region) else {
                        return Ok(xml_error(
                            "TemplateDoesNotExist",
                            &format!("Template {template_name} does not exist"),
                            400,
                        ));
                    };
                    match store.templates.get(&template_name) {
                        Some(t) => (
                            t.subject_part.clone(),
                            t.html_part.clone(),
                            t.text_part.clone(),
                        ),
                        None => {
                            return Ok(xml_error(
                                "TemplateDoesNotExist",
                                &format!("Template {template_name} does not exist"),
                                400,
                            ));
                        }
                    }
                };

                let message_id = Uuid::new_v4().to_string();
                let email = StoredEmail {
                    message_id: message_id.clone(),
                    source,
                    destination_to: to,
                    destination_cc: cc,
                    destination_bcc: bcc,
                    subject,
                    body_text: text_part,
                    body_html: html_part,
                    sent_at: Utc::now(),
                };
                let mut store = self.store.get_or_create(account_id, region);
                store.emails.insert(message_id.clone(), email);
                let inner = format!("<MessageId>{message_id}</MessageId>");
                Ok(xml_resp("SendTemplatedEmail", &rid, &inner))
            }

            // ----------------------------------------------------------------
            // SetIdentityFeedbackForwardingEnabled
            // ----------------------------------------------------------------
            "SetIdentityFeedbackForwardingEnabled" => {
                let identity = match str_param(ctx, "Identity") {
                    Some(i) => i.to_string(),
                    None => return Ok(xml_error("MissingParameter", "Identity required", 400)),
                };
                let enabled = str_param(ctx, "ForwardingEnabled")
                    .map(|v| v.eq_ignore_ascii_case("true"))
                    .unwrap_or(true);
                let mut store = self.store.get_or_create(account_id, region);
                let attrs = store
                    .notification_attrs
                    .entry(identity)
                    .or_default();
                attrs.forwarding_enabled = enabled;
                Ok(xml_no_result("SetIdentityFeedbackForwardingEnabled", &rid))
            }

            // ----------------------------------------------------------------
            // GetIdentityNotificationAttributes
            // ----------------------------------------------------------------
            "GetIdentityNotificationAttributes" => {
                let identities = addresses_from_params(ctx, "Identities.member");
                let entries = self
                    .store
                    .get(account_id, region)
                    .map(|store| {
                        identities
                            .iter()
                            .map(|id| {
                                let attrs = store
                                    .notification_attrs
                                    .get(id)
                                    .cloned()
                                    .unwrap_or_default();
                                let bounce = attrs
                                    .bounce_topic
                                    .as_deref()
                                    .map(|t| format!("<BounceTopic>{t}</BounceTopic>"))
                                    .unwrap_or_default();
                                let complaint = attrs
                                    .complaint_topic
                                    .as_deref()
                                    .map(|t| format!("<ComplaintTopic>{t}</ComplaintTopic>"))
                                    .unwrap_or_default();
                                let delivery = attrs
                                    .delivery_topic
                                    .as_deref()
                                    .map(|t| format!("<DeliveryTopic>{t}</DeliveryTopic>"))
                                    .unwrap_or_default();
                                format!(
                                    "<entry><key>{id}</key><value>\
{bounce}{complaint}{delivery}\
<ForwardingEnabled>{}</ForwardingEnabled>\
</value></entry>",
                                    attrs.forwarding_enabled
                                )
                            })
                            .collect::<String>()
                    })
                    .unwrap_or_default();
                let inner =
                    format!("<NotificationAttributes>{entries}</NotificationAttributes>");
                Ok(xml_resp("GetIdentityNotificationAttributes", &rid, &inner))
            }

            _ => Err(DispatchError::NotImplemented(ctx.operation.clone())),
        }
    }

    async fn storage_snapshot(&self) -> Option<serde_json::Value> {
        use serde_json::json;
        let mut identities = Vec::new();
        for entry in self.store.iter() {
            for id in entry.value().identities.values() {
                identities.push(json!({
                    "id": id.identity, "kind": "identity",
                    "attributes": [{"key": "verified", "value": id.verified.to_string()}]
                }));
            }
        }
        Some(json!({ "kind": "ses", "identities": identities }))
    }
}
