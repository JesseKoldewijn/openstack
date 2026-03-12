use std::collections::HashMap;

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
        raw_body: Bytes::from(xml_body.as_bytes().to_vec()),
        headers: HashMap::new(),
        path: path.to_string(),
        method: method.to_string(),
        query_params: HashMap::new(),
        spooled_body: None,
    }
}

fn body_str(resp: &DispatchResponse) -> String {
    String::from_utf8_lossy(resp.body.as_bytes()).to_string()
}

/// Extract text content of a simple XML tag (first occurrence)
fn xml_text(xml: &str, tag: &str) -> Option<String> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let start = xml.find(&open)? + open.len();
    let end = xml.find(&close)?;
    if end >= start {
        Some(xml[start..end].to_string())
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_create_hosted_zone() {
    let p = Route53Provider::new();
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<CreateHostedZoneRequest xmlns="https://route53.amazonaws.com/doc/2013-04-01/">
  <Name>example.com</Name>
  <CallerReference>ref-1</CallerReference>
  <HostedZoneConfig>
    <Comment>Test zone</Comment>
  </HostedZoneConfig>
</CreateHostedZoneRequest>"#;

    let resp = p
        .dispatch(&make_ctx(
            "CreateHostedZone",
            xml,
            "/2013-04-01/hostedzone",
            "POST",
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 201);
    assert_eq!(resp.content_type, "text/xml");
    let body = body_str(&resp);
    assert!(body.contains("CreateHostedZoneResponse"));
    assert!(body.contains("example.com."));
    assert!(body.contains("<Id>/hostedzone/"));
    // Check Location header is set
    let loc_header = resp
        .headers
        .iter()
        .find(|(k, _)| k == "Location")
        .map(|(_, v)| v.as_str());
    assert!(loc_header.is_some());
    assert!(loc_header.unwrap().starts_with("/2013-04-01/hostedzone/"));
}

#[tokio::test]
async fn test_list_hosted_zones() {
    let p = Route53Provider::new();
    // Create two hosted zones
    let xml1 = r#"<CreateHostedZoneRequest><Name>zone1.com</Name><CallerReference>r1</CallerReference></CreateHostedZoneRequest>"#;
    let xml2 = r#"<CreateHostedZoneRequest><Name>zone2.com</Name><CallerReference>r2</CallerReference></CreateHostedZoneRequest>"#;
    p.dispatch(&make_ctx(
        "CreateHostedZone",
        xml1,
        "/2013-04-01/hostedzone",
        "POST",
    ))
    .await
    .unwrap();
    p.dispatch(&make_ctx(
        "CreateHostedZone",
        xml2,
        "/2013-04-01/hostedzone",
        "POST",
    ))
    .await
    .unwrap();

    let resp = p
        .dispatch(&make_ctx(
            "ListHostedZones",
            "",
            "/2013-04-01/hostedzone",
            "GET",
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
    let body = body_str(&resp);
    assert!(body.contains("ListHostedZonesResponse"));
    assert!(body.contains("zone1.com."));
    assert!(body.contains("zone2.com."));
}

#[tokio::test]
async fn test_delete_hosted_zone() {
    let p = Route53Provider::new();
    let xml = r#"<CreateHostedZoneRequest><Name>delete-me.com</Name><CallerReference>r3</CallerReference></CreateHostedZoneRequest>"#;
    let create_resp = p
        .dispatch(&make_ctx(
            "CreateHostedZone",
            xml,
            "/2013-04-01/hostedzone",
            "POST",
        ))
        .await
        .unwrap();
    let create_body = body_str(&create_resp);
    // Extract zone_id from <Id>/hostedzone/{id}</Id>
    let id_raw = xml_text(&create_body, "Id").unwrap();
    let zone_id = id_raw.trim_start_matches("/hostedzone/").to_string();

    let delete_path = format!("/2013-04-01/hostedzone/{zone_id}");
    let resp = p
        .dispatch(&make_ctx("DeleteHostedZone", "", &delete_path, "DELETE"))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
    let body = body_str(&resp);
    assert!(body.contains("DeleteHostedZoneResponse"));
    assert!(body.contains("INSYNC"));

    // List should be empty now
    let list_resp = p
        .dispatch(&make_ctx(
            "ListHostedZones",
            "",
            "/2013-04-01/hostedzone",
            "GET",
        ))
        .await
        .unwrap();
    let list_body = body_str(&list_resp);
    assert!(!list_body.contains("delete-me.com"));
}

#[tokio::test]
async fn test_change_resource_record_sets() {
    let p = Route53Provider::new();
    // Create a zone
    let xml = r#"<CreateHostedZoneRequest><Name>records.com</Name><CallerReference>r4</CallerReference></CreateHostedZoneRequest>"#;
    let create_resp = p
        .dispatch(&make_ctx(
            "CreateHostedZone",
            xml,
            "/2013-04-01/hostedzone",
            "POST",
        ))
        .await
        .unwrap();
    let create_body = body_str(&create_resp);
    let id_raw = xml_text(&create_body, "Id").unwrap();
    let zone_id = id_raw.trim_start_matches("/hostedzone/").to_string();

    // Upsert an A record
    let rrset_xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<ChangeResourceRecordSetsRequest xmlns="https://route53.amazonaws.com/doc/2013-04-01/">
  <ChangeBatch>
    <Changes>
      <Change>
        <Action>UPSERT</Action>
        <ResourceRecordSet>
          <Name>www.records.com</Name>
          <Type>A</Type>
          <TTL>300</TTL>
          <ResourceRecords>
            <ResourceRecord><Value>1.2.3.4</Value></ResourceRecord>
          </ResourceRecords>
        </ResourceRecordSet>
      </Change>
    </Changes>
  </ChangeBatch>
</ChangeResourceRecordSetsRequest>"#
        .to_string();
    let rrset_path = format!("/2013-04-01/hostedzone/{zone_id}/rrset");
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
    let body = body_str(&resp);
    assert!(body.contains("ChangeResourceRecordSetsResponse"));
    assert!(body.contains("INSYNC"));
}

#[tokio::test]
async fn test_list_resource_record_sets() {
    let p = Route53Provider::new();
    // Create a zone
    let xml = r#"<CreateHostedZoneRequest><Name>listrr.com</Name><CallerReference>r5</CallerReference></CreateHostedZoneRequest>"#;
    let create_resp = p
        .dispatch(&make_ctx(
            "CreateHostedZone",
            xml,
            "/2013-04-01/hostedzone",
            "POST",
        ))
        .await
        .unwrap();
    let create_body = body_str(&create_resp);
    let id_raw = xml_text(&create_body, "Id").unwrap();
    let zone_id = id_raw.trim_start_matches("/hostedzone/").to_string();

    // Upsert two records
    let upsert_xml = r#"<ChangeResourceRecordSetsRequest>
  <ChangeBatch><Changes>
    <Change>
      <Action>UPSERT</Action>
      <ResourceRecordSet>
        <Name>a.listrr.com</Name><Type>A</Type><TTL>60</TTL>
        <ResourceRecords><ResourceRecord><Value>10.0.0.1</Value></ResourceRecord></ResourceRecords>
      </ResourceRecordSet>
    </Change>
    <Change>
      <Action>UPSERT</Action>
      <ResourceRecordSet>
        <Name>b.listrr.com</Name><Type>CNAME</Type><TTL>120</TTL>
        <ResourceRecords><ResourceRecord><Value>example.com</Value></ResourceRecord></ResourceRecords>
      </ResourceRecordSet>
    </Change>
  </Changes></ChangeBatch>
</ChangeResourceRecordSetsRequest>"#.to_string();
    let rrset_path = format!("/2013-04-01/hostedzone/{zone_id}/rrset");
    p.dispatch(&make_ctx(
        "ChangeResourceRecordSets",
        &upsert_xml,
        &rrset_path,
        "POST",
    ))
    .await
    .unwrap();

    // List records
    let list_resp = p
        .dispatch(&make_ctx("ListResourceRecordSets", "", &rrset_path, "GET"))
        .await
        .unwrap();
    assert_eq!(list_resp.status_code, 200);
    let body = body_str(&list_resp);
    assert!(body.contains("ListResourceRecordSetsResponse"));
    assert!(body.contains("a.listrr.com"));
    assert!(body.contains("b.listrr.com"));
    assert!(body.contains("10.0.0.1"));
    assert!(body.contains("example.com"));
}
