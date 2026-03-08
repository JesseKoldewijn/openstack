use std::collections::HashMap;

use bytes::Bytes;
use openstack_service_framework::traits::{DispatchResponse, RequestContext, ServiceProvider};
use openstack_ses::SesProvider;

fn make_ctx(operation: &str, params: HashMap<String, String>) -> RequestContext {
    RequestContext {
        service: "ses".to_string(),
        operation: operation.to_string(),
        region: "us-east-1".to_string(),
        account_id: "000000000000".to_string(),
        request_body: serde_json::json!({}),
        raw_body: Bytes::new(),
        headers: HashMap::new(),
        path: "/".to_string(),
        method: "POST".to_string(),
        query_params: params,
    }
}

fn body_str(resp: &DispatchResponse) -> String {
    String::from_utf8_lossy(&resp.body).to_string()
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
