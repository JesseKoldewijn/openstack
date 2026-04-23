use std::collections::HashMap;

use openstack_service_framework::traits::{DispatchResponse, RequestContext, ServiceProvider};
use openstack_ses::SesProvider;

fn make_ctx(operation: &str, params: HashMap<String, String>) -> RequestContext {
    RequestContext {
        service: "ses".to_string(),
        operation: operation.to_string(),
        region: "us-east-1".to_string(),
        account_id: "000000000000".to_string(),
        request_body: serde_json::json!({}),
        raw_body: None,
        headers: Default::default(),
        path: "/".to_string(),
        method: "POST".to_string(),
        query_params: params,
        request_id: String::new(),
        spooled_body: None,
        body_reader: None,
    }
}

fn body_str(resp: &DispatchResponse) -> String {
    String::from_utf8_lossy(resp.body.as_bytes()).to_string()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_verify_email_identity() {
    let p = SesProvider::new();
    let mut params = HashMap::new();
    params.insert("EmailAddress".to_string(), "test@example.com".to_string());
    let resp = p
        .dispatch(&make_ctx("VerifyEmailIdentity", params))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
    assert_eq!(resp.content_type, "text/xml");
    let body = body_str(&resp);
    assert!(body.contains("VerifyEmailIdentityResponse"));
    assert!(body.contains("RequestId"));
}

#[tokio::test]
async fn test_list_identities() {
    let p = SesProvider::new();
    // Verify two emails
    let mut p1 = HashMap::new();
    p1.insert("EmailAddress".to_string(), "alice@example.com".to_string());
    p.dispatch(&make_ctx("VerifyEmailIdentity", p1))
        .await
        .unwrap();
    let mut p2 = HashMap::new();
    p2.insert("EmailAddress".to_string(), "bob@example.com".to_string());
    p.dispatch(&make_ctx("VerifyEmailIdentity", p2))
        .await
        .unwrap();

    let resp = p
        .dispatch(&make_ctx("ListIdentities", HashMap::new()))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
    let body = body_str(&resp);
    assert!(body.contains("<Identities>"));
    assert!(body.contains("alice@example.com"));
    assert!(body.contains("bob@example.com"));
}

#[tokio::test]
async fn test_verify_domain_identity() {
    let p = SesProvider::new();
    let mut params = HashMap::new();
    params.insert("Domain".to_string(), "example.com".to_string());
    let resp = p
        .dispatch(&make_ctx("VerifyDomainIdentity", params))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
    let body = body_str(&resp);
    assert!(body.contains("VerifyDomainIdentityResponse"));
    assert!(body.contains("<VerificationToken>example-com-verification-token</VerificationToken>"));

    let resp = p
        .dispatch(&make_ctx("ListIdentities", HashMap::new()))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
    let body = body_str(&resp);
    assert!(body.contains("example.com"));
}

#[tokio::test]
async fn test_verify_domain_identity_missing_domain() {
    let p = SesProvider::new();
    let resp = p
        .dispatch(&make_ctx("VerifyDomainIdentity", HashMap::new()))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 400);
    let body = body_str(&resp);
    assert!(body.contains("MissingParameter"));
    assert!(body.contains("Domain required"));
}

#[tokio::test]
async fn test_get_identity_verification_attributes() {
    let p = SesProvider::new();
    let mut verify = HashMap::new();
    verify.insert(
        "EmailAddress".to_string(),
        "verified@example.com".to_string(),
    );
    p.dispatch(&make_ctx("VerifyEmailIdentity", verify))
        .await
        .unwrap();

    let mut params = HashMap::new();
    params.insert(
        "Identities.member.1".to_string(),
        "verified@example.com".to_string(),
    );
    let resp = p
        .dispatch(&make_ctx("GetIdentityVerificationAttributes", params))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
    let body = body_str(&resp);
    assert!(body.contains("GetIdentityVerificationAttributesResponse"));
    assert!(body.contains("verified@example.com"));
    assert!(body.contains("Success"));
}

#[tokio::test]
async fn test_get_identity_verification_attributes_missing_identity_returns_empty_result() {
    let p = SesProvider::new();
    let resp = p
        .dispatch(&make_ctx(
            "GetIdentityVerificationAttributes",
            HashMap::new(),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
    let body = body_str(&resp);
    assert!(body.contains("GetIdentityVerificationAttributesResponse"));
    assert!(
        body.contains("<VerificationAttributes></VerificationAttributes>")
            || body.contains("<VerificationAttributes />")
    );
}

#[tokio::test]
async fn test_send_email() {
    let p = SesProvider::new();
    let mut params = HashMap::new();
    params.insert("Source".to_string(), "sender@example.com".to_string());
    params.insert(
        "Destination.ToAddresses.member.1".to_string(),
        "recipient@example.com".to_string(),
    );
    params.insert(
        "Message.Subject.Data".to_string(),
        "Hello World".to_string(),
    );
    params.insert(
        "Message.Body.Text.Data".to_string(),
        "This is the body.".to_string(),
    );
    let resp = p.dispatch(&make_ctx("SendEmail", params)).await.unwrap();
    assert_eq!(resp.status_code, 200);
    let body = body_str(&resp);
    assert!(body.contains("<MessageId>"));
    assert!(body.contains("SendEmailResponse"));
}

#[tokio::test]
async fn test_send_email_multiple_recipients() {
    let p = SesProvider::new();
    let mut params = HashMap::new();
    params.insert("Source".to_string(), "noreply@example.com".to_string());
    params.insert(
        "Destination.ToAddresses.member.1".to_string(),
        "a@example.com".to_string(),
    );
    params.insert(
        "Destination.ToAddresses.member.2".to_string(),
        "b@example.com".to_string(),
    );
    params.insert(
        "Destination.CcAddresses.member.1".to_string(),
        "c@example.com".to_string(),
    );
    params.insert("Message.Subject.Data".to_string(), "Bulk Email".to_string());
    params.insert(
        "Message.Body.Html.Data".to_string(),
        "<h1>Hello</h1>".to_string(),
    );
    let resp = p.dispatch(&make_ctx("SendEmail", params)).await.unwrap();
    assert_eq!(resp.status_code, 200);
    let body = body_str(&resp);
    assert!(body.contains("<MessageId>"));
}

#[tokio::test]
async fn test_send_raw_email() {
    let p = SesProvider::new();
    let mut params = HashMap::new();
    params.insert("Source".to_string(), "raw@example.com".to_string());
    params.insert(
        "RawMessage.Data".to_string(),
        "From: raw@example.com\r\nTo: dest@example.com\r\nSubject: Raw\r\n\r\nBody".to_string(),
    );
    let resp = p.dispatch(&make_ctx("SendRawEmail", params)).await.unwrap();
    assert_eq!(resp.status_code, 200);
    let body = body_str(&resp);
    assert!(body.contains("<MessageId>"));
    assert!(body.contains("SendRawEmailResponse"));
}

#[tokio::test]
async fn test_delete_identity() {
    let p = SesProvider::new();
    let mut verify = HashMap::new();
    verify.insert("EmailAddress".to_string(), "gone@example.com".to_string());
    p.dispatch(&make_ctx("VerifyEmailIdentity", verify))
        .await
        .unwrap();

    let mut delete = HashMap::new();
    delete.insert("Identity".to_string(), "gone@example.com".to_string());
    let resp = p
        .dispatch(&make_ctx("DeleteIdentity", delete))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
    let body = body_str(&resp);
    assert!(body.contains("DeleteIdentityResponse"));

    let resp = p
        .dispatch(&make_ctx("ListIdentities", HashMap::new()))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
    let body = body_str(&resp);
    assert!(!body.contains("gone@example.com"));
}

#[tokio::test]
async fn test_delete_identity_missing_identity() {
    let p = SesProvider::new();
    let resp = p
        .dispatch(&make_ctx("DeleteIdentity", HashMap::new()))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 400);
    let body = body_str(&resp);
    assert!(body.contains("MissingParameter"));
    assert!(body.contains("Identity required"));
}

#[tokio::test]
async fn test_verify_email_identity_missing_address() {
    let p = SesProvider::new();
    let resp = p
        .dispatch(&make_ctx("VerifyEmailIdentity", HashMap::new()))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 400);
    let body = body_str(&resp);
    assert!(body.contains("MissingParameter"));
}

// ---------------------------------------------------------------------------
// GetSendQuota
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_get_send_quota_empty() {
    let p = SesProvider::new();
    let resp = p
        .dispatch(&make_ctx("GetSendQuota", HashMap::new()))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
    let body = body_str(&resp);
    assert!(body.contains("GetSendQuotaResponse"));
    assert!(body.contains("<Max24HourSend>50000.0</Max24HourSend>"));
    assert!(body.contains("<SentLast24Hours>0</SentLast24Hours>"));
}

#[tokio::test]
async fn test_get_send_quota_after_sends() {
    let p = SesProvider::new();
    for i in 0..3u8 {
        let mut params = HashMap::new();
        params.insert("Source".to_string(), "s@example.com".to_string());
        params.insert(
            "Destination.ToAddresses.member.1".to_string(),
            format!("d{i}@example.com"),
        );
        params.insert("Message.Subject.Data".to_string(), "hi".to_string());
        p.dispatch(&make_ctx("SendEmail", params)).await.unwrap();
    }
    let resp = p
        .dispatch(&make_ctx("GetSendQuota", HashMap::new()))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
    let body = body_str(&resp);
    assert!(body.contains("<SentLast24Hours>3</SentLast24Hours>"));
}

// ---------------------------------------------------------------------------
// GetSendStatistics
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_get_send_statistics_empty() {
    let p = SesProvider::new();
    let resp = p
        .dispatch(&make_ctx("GetSendStatistics", HashMap::new()))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
    let body = body_str(&resp);
    assert!(body.contains("GetSendStatisticsResponse"));
    assert!(body.contains("<SendDataPoints"));
}

#[tokio::test]
async fn test_get_send_statistics_after_sends() {
    let p = SesProvider::new();
    let mut params = HashMap::new();
    params.insert("Source".to_string(), "s@example.com".to_string());
    params.insert(
        "Destination.ToAddresses.member.1".to_string(),
        "d@example.com".to_string(),
    );
    params.insert("Message.Subject.Data".to_string(), "stat test".to_string());
    p.dispatch(&make_ctx("SendEmail", params)).await.unwrap();

    let resp = p
        .dispatch(&make_ctx("GetSendStatistics", HashMap::new()))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
    let body = body_str(&resp);
    assert!(body.contains("<DeliveryAttempts>1</DeliveryAttempts>"));
}

// ---------------------------------------------------------------------------
// Template CRUD
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_create_and_get_template() {
    let p = SesProvider::new();
    let mut params = HashMap::new();
    params.insert("Template.TemplateName".to_string(), "welcome".to_string());
    params.insert(
        "Template.SubjectPart".to_string(),
        "Welcome {{name}}!".to_string(),
    );
    params.insert(
        "Template.HtmlPart".to_string(),
        "<h1>Hi {{name}}</h1>".to_string(),
    );
    params.insert("Template.TextPart".to_string(), "Hi {{name}}".to_string());
    let resp = p
        .dispatch(&make_ctx("CreateTemplate", params))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
    let body = body_str(&resp);
    assert!(body.contains("CreateTemplateResponse"));

    let mut get_params = HashMap::new();
    get_params.insert("TemplateName".to_string(), "welcome".to_string());
    let resp = p
        .dispatch(&make_ctx("GetTemplate", get_params))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
    let body = body_str(&resp);
    assert!(body.contains("GetTemplateResponse"));
    assert!(body.contains("<TemplateName>welcome</TemplateName>"));
    assert!(body.contains("Welcome {{name}}!"));
}

#[tokio::test]
async fn test_create_template_duplicate_fails() {
    let p = SesProvider::new();
    let mut params = HashMap::new();
    params.insert("Template.TemplateName".to_string(), "dup".to_string());
    p.dispatch(&make_ctx("CreateTemplate", params.clone()))
        .await
        .unwrap();
    let resp = p
        .dispatch(&make_ctx("CreateTemplate", params))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 400);
    let body = body_str(&resp);
    assert!(body.contains("AlreadyExists"));
}

#[tokio::test]
async fn test_get_template_not_found() {
    let p = SesProvider::new();
    let mut params = HashMap::new();
    params.insert("TemplateName".to_string(), "nonexistent".to_string());
    let resp = p.dispatch(&make_ctx("GetTemplate", params)).await.unwrap();
    assert_eq!(resp.status_code, 400);
    let body = body_str(&resp);
    assert!(body.contains("TemplateDoesNotExist"));
}

#[tokio::test]
async fn test_list_templates() {
    let p = SesProvider::new();
    for name in ["tmpl-a", "tmpl-b"] {
        let mut params = HashMap::new();
        params.insert("Template.TemplateName".to_string(), name.to_string());
        p.dispatch(&make_ctx("CreateTemplate", params))
            .await
            .unwrap();
    }
    let resp = p
        .dispatch(&make_ctx("ListTemplates", HashMap::new()))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
    let body = body_str(&resp);
    assert!(body.contains("tmpl-a"));
    assert!(body.contains("tmpl-b"));
}

#[tokio::test]
async fn test_delete_template() {
    let p = SesProvider::new();
    let mut params = HashMap::new();
    params.insert("Template.TemplateName".to_string(), "to-delete".to_string());
    p.dispatch(&make_ctx("CreateTemplate", params))
        .await
        .unwrap();

    let mut del_params = HashMap::new();
    del_params.insert("TemplateName".to_string(), "to-delete".to_string());
    let resp = p
        .dispatch(&make_ctx("DeleteTemplate", del_params))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);

    let mut get_params = HashMap::new();
    get_params.insert("TemplateName".to_string(), "to-delete".to_string());
    let resp = p
        .dispatch(&make_ctx("GetTemplate", get_params))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 400);
    let body = body_str(&resp);
    assert!(body.contains("TemplateDoesNotExist"));
}

// ---------------------------------------------------------------------------
// SendTemplatedEmail
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_send_templated_email() {
    let p = SesProvider::new();
    let mut t_params = HashMap::new();
    t_params.insert("Template.TemplateName".to_string(), "greet".to_string());
    t_params.insert(
        "Template.SubjectPart".to_string(),
        "Hello there".to_string(),
    );
    t_params.insert(
        "Template.HtmlPart".to_string(),
        "<p>Greetings</p>".to_string(),
    );
    p.dispatch(&make_ctx("CreateTemplate", t_params))
        .await
        .unwrap();

    let mut params = HashMap::new();
    params.insert("Source".to_string(), "from@example.com".to_string());
    params.insert("Template".to_string(), "greet".to_string());
    params.insert(
        "Destination.ToAddresses.member.1".to_string(),
        "to@example.com".to_string(),
    );
    let resp = p
        .dispatch(&make_ctx("SendTemplatedEmail", params))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
    let body = body_str(&resp);
    assert!(body.contains("SendTemplatedEmailResponse"));
    assert!(body.contains("<MessageId>"));
}

#[tokio::test]
async fn test_send_templated_email_missing_template() {
    let p = SesProvider::new();
    let mut params = HashMap::new();
    params.insert("Source".to_string(), "from@example.com".to_string());
    params.insert("Template".to_string(), "nonexistent".to_string());
    params.insert(
        "Destination.ToAddresses.member.1".to_string(),
        "to@example.com".to_string(),
    );
    let resp = p
        .dispatch(&make_ctx("SendTemplatedEmail", params))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 400);
    let body = body_str(&resp);
    assert!(body.contains("TemplateDoesNotExist"));
}

// ---------------------------------------------------------------------------
// Notification attributes
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_set_identity_feedback_forwarding_enabled() {
    let p = SesProvider::new();
    let mut verify = HashMap::new();
    verify.insert("EmailAddress".to_string(), "notif@example.com".to_string());
    p.dispatch(&make_ctx("VerifyEmailIdentity", verify))
        .await
        .unwrap();

    let mut params = HashMap::new();
    params.insert("Identity".to_string(), "notif@example.com".to_string());
    params.insert("ForwardingEnabled".to_string(), "false".to_string());
    let resp = p
        .dispatch(&make_ctx("SetIdentityFeedbackForwardingEnabled", params))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
    let body = body_str(&resp);
    assert!(body.contains("SetIdentityFeedbackForwardingEnabledResponse"));
}

#[tokio::test]
async fn test_get_identity_notification_attributes() {
    let p = SesProvider::new();
    let mut verify = HashMap::new();
    verify.insert("EmailAddress".to_string(), "notif2@example.com".to_string());
    p.dispatch(&make_ctx("VerifyEmailIdentity", verify))
        .await
        .unwrap();

    let mut params = HashMap::new();
    params.insert(
        "Identities.member.1".to_string(),
        "notif2@example.com".to_string(),
    );
    let resp = p
        .dispatch(&make_ctx("GetIdentityNotificationAttributes", params))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
    let body = body_str(&resp);
    assert!(body.contains("GetIdentityNotificationAttributesResponse"));
    assert!(body.contains("notif2@example.com"));
    assert!(body.contains("<ForwardingEnabled>true</ForwardingEnabled>"));
}
