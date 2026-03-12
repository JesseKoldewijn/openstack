use std::collections::HashMap;

use bytes::Bytes;
use openstack_acm::AcmProvider;
use openstack_service_framework::traits::{RequestContext, ServiceProvider};
use serde_json::{Value, json};

fn make_ctx(operation: &str, body: Value) -> RequestContext {
    RequestContext {
        service: "acm".to_string(),
        operation: operation.to_string(),
        region: "us-east-1".to_string(),
        account_id: "000000000000".to_string(),
        request_body: body.clone(),
        raw_body: Bytes::from(serde_json::to_vec(&body).unwrap()),
        headers: HashMap::new(),
        path: "/".to_string(),
        method: "POST".to_string(),
        query_params: HashMap::new(),
        spooled_body: None,
    }
}

fn body(resp: &openstack_service_framework::traits::DispatchResponse) -> Value {
    serde_json::from_slice(resp.body.as_bytes()).expect("response body is valid JSON")
}

fn body_str(resp: &openstack_service_framework::traits::DispatchResponse) -> String {
    String::from_utf8_lossy(resp.body.as_bytes()).to_string()
}

async fn request_cert(p: &AcmProvider, domain: &str) -> String {
    let resp = p
        .dispatch(&make_ctx(
            "RequestCertificate",
            json!({
                "DomainName": domain,
                "SubjectAlternativeNames": [format!("www.{domain}")],
            }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200, "{}", body_str(&resp));
    body(&resp)["CertificateArn"].as_str().unwrap().to_string()
}

#[tokio::test]
async fn test_request_and_describe_certificate() {
    let p = AcmProvider::new();
    let arn = request_cert(&p, "example.com").await;
    assert!(arn.contains("arn:aws:acm:"));

    let resp = p
        .dispatch(&make_ctx(
            "DescribeCertificate",
            json!({
                "CertificateArn": arn,
            }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200, "{}", body_str(&resp));
    let b = body(&resp);
    assert_eq!(b["Certificate"]["DomainName"], "example.com");
    assert_eq!(b["Certificate"]["Status"], "ISSUED");
}

#[tokio::test]
async fn test_list_certificates() {
    let p = AcmProvider::new();
    request_cert(&p, "a.com").await;
    request_cert(&p, "b.com").await;

    let resp = p
        .dispatch(&make_ctx("ListCertificates", json!({})))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
    let certs = body(&resp)["CertificateSummaryList"]
        .as_array()
        .unwrap()
        .clone();
    assert!(certs.len() >= 2);
}

#[tokio::test]
async fn test_delete_certificate() {
    let p = AcmProvider::new();
    let arn = request_cert(&p, "delete-me.com").await;

    let resp = p
        .dispatch(&make_ctx(
            "DeleteCertificate",
            json!({
                "CertificateArn": arn,
            }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200, "{}", body_str(&resp));

    let resp = p
        .dispatch(&make_ctx(
            "DescribeCertificate",
            json!({
                "CertificateArn": arn,
            }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 400);
    assert!(body_str(&resp).contains("ResourceNotFoundException"));
}

#[tokio::test]
async fn test_certificate_with_sans() {
    let p = AcmProvider::new();
    let resp = p
        .dispatch(&make_ctx(
            "RequestCertificate",
            json!({
                "DomainName": "multi.com",
                "SubjectAlternativeNames": ["api.multi.com", "admin.multi.com"],
            }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
    let arn = body(&resp)["CertificateArn"].as_str().unwrap().to_string();

    let resp = p
        .dispatch(&make_ctx(
            "DescribeCertificate",
            json!({ "CertificateArn": arn }),
        ))
        .await
        .unwrap();
    let sans = body(&resp)["Certificate"]["SubjectAlternativeNames"]
        .as_array()
        .unwrap()
        .clone();
    assert_eq!(sans.len(), 2);
}

#[tokio::test]
async fn test_add_tags() {
    let p = AcmProvider::new();
    let arn = request_cert(&p, "tagged.com").await;

    let resp = p
        .dispatch(&make_ctx(
            "AddTagsToCertificate",
            json!({
                "CertificateArn": arn,
                "Tags": [{ "Key": "env", "Value": "prod" }],
            }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200, "{}", body_str(&resp));

    let resp = p
        .dispatch(&make_ctx(
            "ListTagsForCertificate",
            json!({ "CertificateArn": arn }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
    let tags = body(&resp)["Tags"].as_array().unwrap().clone();
    assert_eq!(tags.len(), 1);
    assert_eq!(tags[0]["Key"], "env");
}

#[tokio::test]
async fn test_describe_not_found() {
    let p = AcmProvider::new();
    let resp = p
        .dispatch(&make_ctx(
            "DescribeCertificate",
            json!({
                "CertificateArn": "arn:aws:acm:us-east-1:000000000000:certificate/nonexistent",
            }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 400);
    assert!(body_str(&resp).contains("ResourceNotFoundException"));
}
