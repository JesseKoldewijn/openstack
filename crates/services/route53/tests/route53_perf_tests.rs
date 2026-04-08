/// Performance tests for Route53 provider.
///
/// These exercise the core hosted-zone and record-set control-plane paths.
use std::collections::HashMap;
use std::time::Instant;

use bytes::Bytes;
use openstack_route53::Route53Provider;
use openstack_service_framework::traits::{DispatchResponse, RequestContext, ServiceProvider};

fn make_ctx(operation: &str, xml_body: &str, path: &str, method: &str) -> RequestContext {
    RequestContext {
        service: "route53".to_string(),
        operation: operation.to_string(),
        region: "us-east-1".to_string(),
        account_id: "000000000000".to_string(),
        request_body: serde_json::json!({}),
        raw_body: Some(Bytes::from(xml_body.as_bytes().to_vec())),
        headers: Default::default(),
        path: path.to_string(),
        method: method.to_string(),
        query_params: HashMap::new(),
        request_id: String::new(),
        spooled_body: None,
        body_reader: None,
    }
}

fn body_str(resp: &DispatchResponse) -> String {
    String::from_utf8_lossy(resp.body.as_bytes()).to_string()
}

fn xml_text(xml: &str, tag: &str) -> Option<String> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let start = xml.find(&open)? + open.len();
    let end = xml.find(&close)?;
    (end >= start).then(|| xml[start..end].to_string())
}

async fn create_zone(p: &Route53Provider, name: &str, caller_ref: &str) -> String {
    let xml = format!(
        "<CreateHostedZoneRequest><Name>{name}</Name><CallerReference>{caller_ref}</CallerReference></CreateHostedZoneRequest>"
    );
    let resp = p
        .dispatch(&make_ctx(
            "CreateHostedZone",
            &xml,
            "/2013-04-01/hostedzone",
            "POST",
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 201);
    let body = body_str(&resp);
    xml_text(&body, "Id")
        .unwrap()
        .trim_start_matches("/hostedzone/")
        .to_string()
}

#[tokio::test]
async fn perf_create_hosted_zone_throughput() {
    let p = Route53Provider::new();
    let n = 100usize;

    let start = Instant::now();
    for i in 0..n {
        let xml = format!(
            "<CreateHostedZoneRequest><Name>perf-{i:03}.example.com</Name><CallerReference>ref-{i:03}</CallerReference></CreateHostedZoneRequest>"
        );
        let resp = p
            .dispatch(&make_ctx(
                "CreateHostedZone",
                &xml,
                "/2013-04-01/hostedzone",
                "POST",
            ))
            .await
            .unwrap();
        assert_eq!(resp.status_code, 201);
    }
    let elapsed = start.elapsed();

    assert!(
        elapsed.as_millis() < 2000,
        "CreateHostedZone x{n} took {}ms — expected <2000ms",
        elapsed.as_millis()
    );
}

#[tokio::test]
async fn perf_change_rrsets_round_trip() {
    let p = Route53Provider::new();
    let zone_id = create_zone(&p, "perf-rrsets.example.com", "rr-ref").await;
    let path = format!("/2013-04-01/hostedzone/{zone_id}/rrset");

    let rrset_xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<ChangeResourceRecordSetsRequest xmlns="https://route53.amazonaws.com/doc/2013-04-01/">
  <ChangeBatch>
    <Changes>
      <Change>
        <Action>UPSERT</Action>
        <ResourceRecordSet>
          <Name>www.perf-rrsets.example.com</Name>
          <Type>A</Type>
          <TTL>300</TTL>
          <ResourceRecords>
            <ResourceRecord><Value>1.2.3.4</Value></ResourceRecord>
          </ResourceRecords>
        </ResourceRecordSet>
      </Change>
    </Changes>
  </ChangeBatch>
</ChangeResourceRecordSetsRequest>"#;

    let start = Instant::now();
    for _ in 0..100 {
        let resp = p
            .dispatch(&make_ctx(
                "ChangeResourceRecordSets",
                rrset_xml,
                &path,
                "POST",
            ))
            .await
            .unwrap();
        assert_eq!(resp.status_code, 200);
    }
    let elapsed = start.elapsed();

    assert!(
        elapsed.as_millis() < 1500,
        "ChangeResourceRecordSets x100 took {}ms — expected <1500ms",
        elapsed.as_millis()
    );
}

#[tokio::test]
async fn perf_list_rrsets_many() {
    let p = Route53Provider::new();
    let zone_id = create_zone(&p, "perf-list.example.com", "list-ref").await;
    let rrset_path = format!("/2013-04-01/hostedzone/{zone_id}/rrset");

    for i in 0..100usize {
        let rrset_xml = format!(
            "<ChangeResourceRecordSetsRequest><ChangeBatch><Changes><Change><Action>UPSERT</Action><ResourceRecordSet><Name>rec-{i:03}.perf-list.example.com</Name><Type>A</Type><TTL>300</TTL><ResourceRecords><ResourceRecord><Value>1.2.3.{}</Value></ResourceRecord></ResourceRecords></ResourceRecordSet></Change></Changes></ChangeBatch></ChangeResourceRecordSetsRequest>",
            i % 255
        );
        let resp = p
            .dispatch(&make_ctx(
                "ChangeResourceRecordSets",
                &rrset_xml,
                &rrset_path,
                "POST",
            ))
            .await
            .unwrap();
        assert_eq!(resp.status_code, 200);
    }

    let start = Instant::now();
    let resp = p
        .dispatch(&make_ctx("ListResourceRecordSets", "", &rrset_path, "GET"))
        .await
        .unwrap();
    let elapsed = start.elapsed();
    assert_eq!(resp.status_code, 200);
    let xml = body_str(&resp);
    assert!(xml.contains("rec-000.perf-list.example.com"));
    assert!(xml.contains("rec-099.perf-list.example.com"));

    assert!(
        elapsed.as_millis() < 500,
        "ListResourceRecordSets(100) took {}ms — expected <500ms",
        elapsed.as_millis()
    );
}
