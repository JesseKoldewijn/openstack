/// Performance tests for ACM provider.
///
/// These focus on the lightweight certificate control-plane operations we
/// emulate in-memory: request, describe, and tagging.
use std::collections::HashMap;
use std::time::Instant;

use bytes::Bytes;
use openstack_acm::AcmProvider;
use openstack_service_framework::traits::{DispatchResponse, RequestContext, ServiceProvider};
use serde_json::{Value, json};

fn make_ctx(operation: &str, body: Value) -> RequestContext {
    RequestContext {
        service: "acm".to_string(),
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

fn body(resp: &DispatchResponse) -> Value {
    serde_json::from_slice(resp.body.as_bytes()).expect("response body is valid JSON")
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
    assert_eq!(resp.status_code, 200);
    body(&resp)["CertificateArn"].as_str().unwrap().to_string()
}

#[tokio::test]
async fn perf_request_certificate_throughput() {
    let p = AcmProvider::new();
    let n = 100usize;

    let start = Instant::now();
    for i in 0..n {
        let resp = p
            .dispatch(&make_ctx(
                "RequestCertificate",
                json!({
                    "DomainName": format!("perf-{i:03}.example.com"),
                    "SubjectAlternativeNames": [format!("www.perf-{i:03}.example.com")],
                }),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status_code, 200);
    }
    let elapsed = start.elapsed();

    assert!(
        elapsed.as_millis() < 1500,
        "RequestCertificate x{n} took {}ms — expected <1500ms",
        elapsed.as_millis()
    );
}

#[tokio::test]
async fn perf_describe_certificate_many() {
    let p = AcmProvider::new();
    let n = 100usize;
    let mut arns = Vec::with_capacity(n);
    for i in 0..n {
        arns.push(request_cert(&p, &format!("describe-{i:03}.example.com")).await);
    }

    let start = Instant::now();
    for arn in &arns {
        let resp = p
            .dispatch(&make_ctx(
                "DescribeCertificate",
                json!({ "CertificateArn": arn }),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status_code, 200);
    }
    let elapsed = start.elapsed();

    assert!(
        elapsed.as_millis() < 1000,
        "DescribeCertificate x{n} took {}ms — expected <1000ms",
        elapsed.as_millis()
    );
}

#[tokio::test]
async fn perf_tagging_round_trip() {
    let p = AcmProvider::new();
    let arn = request_cert(&p, "perf-tags.example.com").await;

    let tags: Vec<Value> = (0..50)
        .map(|i| json!({ "Key": format!("key-{i:02}"), "Value": format!("val-{i:02}") }))
        .collect();

    let start = Instant::now();
    let add_resp = p
        .dispatch(&make_ctx(
            "AddTagsToCertificate",
            json!({
                "CertificateArn": arn,
                "Tags": tags,
            }),
        ))
        .await
        .unwrap();
    assert_eq!(add_resp.status_code, 200);

    let list_resp = p
        .dispatch(&make_ctx(
            "ListTagsForCertificate",
            json!({ "CertificateArn": arn }),
        ))
        .await
        .unwrap();
    assert_eq!(list_resp.status_code, 200);
    let elapsed = start.elapsed();
    assert_eq!(body(&list_resp)["Tags"].as_array().unwrap().len(), 50);

    assert!(
        elapsed.as_millis() < 500,
        "ACM tag round-trip took {}ms — expected <500ms",
        elapsed.as_millis()
    );
}
