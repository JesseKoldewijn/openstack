/// Performance tests for SES provider.
///
/// These cover the identity-verification and email-submission hot paths.
use std::collections::HashMap;
use std::time::Instant;

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

#[tokio::test]
async fn perf_verify_email_identity_throughput() {
    let p = SesProvider::new();
    let n = 100usize;

    let start = Instant::now();
    for i in 0..n {
        let mut params = HashMap::new();
        params.insert(
            "EmailAddress".to_string(),
            format!("perf-{i:03}@example.com"),
        );
        let resp = p
            .dispatch(&make_ctx("VerifyEmailIdentity", params))
            .await
            .unwrap();
        assert_eq!(resp.status_code, 200, "{}", body_str(&resp));
    }
    let elapsed = start.elapsed();

    assert!(
        elapsed.as_millis() < 1500,
        "VerifyEmailIdentity x{n} took {}ms — expected <1500ms",
        elapsed.as_millis()
    );
}

#[tokio::test]
async fn perf_send_email_throughput() {
    let p = SesProvider::new();
    let mut verify = HashMap::new();
    verify.insert("EmailAddress".to_string(), "sender@example.com".to_string());
    p.dispatch(&make_ctx("VerifyEmailIdentity", verify))
        .await
        .unwrap();

    let n = 200usize;
    let start = Instant::now();
    for i in 0..n {
        let mut params = HashMap::new();
        params.insert("Source".to_string(), "sender@example.com".to_string());
        params.insert(
            "Destination.ToAddresses.member.1".to_string(),
            format!("dest-{i:03}@example.com"),
        );
        params.insert("Message.Subject.Data".to_string(), format!("subj-{i:03}"));
        params.insert("Message.Body.Text.Data".to_string(), "hello".to_string());
        let resp = p.dispatch(&make_ctx("SendEmail", params)).await.unwrap();
        assert_eq!(resp.status_code, 200, "{}", body_str(&resp));
    }
    let elapsed = start.elapsed();

    assert!(
        elapsed.as_millis() < 2000,
        "SendEmail x{n} took {}ms — expected <2000ms",
        elapsed.as_millis()
    );
}

#[tokio::test]
async fn perf_list_identities_many() {
    let p = SesProvider::new();
    let n = 100usize;
    for i in 0..n {
        let mut params = HashMap::new();
        params.insert(
            "EmailAddress".to_string(),
            format!("list-{i:03}@example.com"),
        );
        let resp = p
            .dispatch(&make_ctx("VerifyEmailIdentity", params))
            .await
            .unwrap();
        assert_eq!(resp.status_code, 200);
    }

    let start = Instant::now();
    let resp = p
        .dispatch(&make_ctx("ListIdentities", HashMap::new()))
        .await
        .unwrap();
    let elapsed = start.elapsed();
    assert_eq!(resp.status_code, 200);
    let xml = body_str(&resp);
    assert!(xml.contains("list-000@example.com"));
    assert!(xml.contains("list-099@example.com"));

    assert!(
        elapsed.as_millis() < 500,
        "ListIdentities({n}) took {}ms — expected <500ms",
        elapsed.as_millis()
    );
}
