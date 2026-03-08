use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use openstack_service_framework::traits::{
    DispatchError, DispatchResponse, RequestContext, ServiceProvider,
};
use openstack_state::AccountRegionBundle;
use uuid::Uuid;

use crate::store::{HostedZone, ResourceRecordSet, Route53Store};

pub struct Route53Provider {
    store: Arc<AccountRegionBundle<Route53Store>>,
}

impl Route53Provider {
    pub fn new() -> Self {
        Self {
            store: Arc::new(AccountRegionBundle::new()),
        }
    }
}

impl Default for Route53Provider {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Helpers — Route53 uses rest-xml protocol (XML body + REST paths)
// Route53 is global — use "us-east-1" as region key
// ---------------------------------------------------------------------------

const ROUTE53_REGION: &str = "us-east-1";
const ROUTE53_NS: &str = "https://route53.amazonaws.com/doc/2013-04-01/";

fn xml_ok(body: String) -> DispatchResponse {
    DispatchResponse {
        status_code: 200,
        body: Bytes::from(body.into_bytes()),
        content_type: "text/xml".to_string(),
        headers: Vec::new(),
    }
}

fn xml_created(body: String, location: &str) -> DispatchResponse {
    DispatchResponse {
        status_code: 201,
        body: Bytes::from(body.into_bytes()),
        content_type: "text/xml".to_string(),
        headers: vec![("Location".to_string(), location.to_string())],
    }
}

#[allow(dead_code)]
fn xml_error(code: &str, message: &str, status: u16) -> DispatchResponse {
    let body = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
<ErrorResponse xmlns=\"{ROUTE53_NS}\">\
<Error><Code>{code}</Code><Message>{message}</Message></Error>\
</ErrorResponse>"
    );
    DispatchResponse {
        status_code: status,
        body: Bytes::from(body.into_bytes()),
        content_type: "text/xml".to_string(),
        headers: Vec::new(),
    }
}

fn req_id() -> String {
    Uuid::new_v4().to_string()
}

fn short_id() -> String {
    Uuid::new_v4().to_string().replace('-', "")[..12].to_string()
}

/// Parse XML text content for a simple tag from raw body string
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

/// Parse ResourceRecordSet entries from ChangeResourceRecordSets XML body
fn parse_rrsets(xml: &str) -> Vec<(String, ResourceRecordSet)> {
    // Minimal parser: find <Change> blocks
    let mut results = Vec::new();
    let mut remaining = xml;
    while let Some(start) = remaining.find("<Change>") {
        let chunk = &remaining[start..];
        let end = chunk.find("</Change>").unwrap_or(chunk.len());
        let change = &chunk[..end];
        let action = xml_text(change, "Action").unwrap_or_default();
        let name = xml_text(change, "Name").unwrap_or_default();
        let rtype = xml_text(change, "Type").unwrap_or_default();
        let ttl: u64 = xml_text(change, "TTL")
            .and_then(|s| s.parse().ok())
            .unwrap_or(300);
        let mut values = Vec::new();
        let mut rest = change;
        while let Some(vstart) = rest.find("<Value>") {
            let after = &rest[vstart + 7..];
            if let Some(vend) = after.find("</Value>") {
                values.push(after[..vend].to_string());
            }
            rest = &rest[vstart + 7..];
        }
        results.push((
            action,
            ResourceRecordSet {
                name,
                record_type: rtype,
                ttl,
                values,
            },
        ));
        remaining = &remaining[start + end..];
    }
    results
}

// ---------------------------------------------------------------------------
// ServiceProvider
// ---------------------------------------------------------------------------

#[async_trait]
impl ServiceProvider for Route53Provider {
    fn service_name(&self) -> &str {
        "route53"
    }

    async fn dispatch(&self, ctx: &RequestContext) -> Result<DispatchResponse, DispatchError> {
        let account_id = &ctx.account_id;
        let rid = req_id();

        match ctx.operation.as_str() {
            // ----------------------------------------------------------------
            // CreateHostedZone  POST /2013-04-01/hostedzone
            // ----------------------------------------------------------------
            "CreateHostedZone" => {
                let raw = String::from_utf8_lossy(&ctx.raw_body);
                let name_raw = xml_text(&raw, "Name").unwrap_or_default();
                // Normalize: ensure trailing dot
                let name = if name_raw.ends_with('.') {
                    name_raw
                } else {
                    format!("{name_raw}.")
                };
                let comment = xml_text(&raw, "Comment").unwrap_or_default();

                let zone_id = short_id();
                let zone = HostedZone {
                    id: zone_id.clone(),
                    name: name.clone(),
                    comment: comment.clone(),
                    private_zone: false,
                    record_count: 2,
                };

                let mut store = self.store.get_or_create(account_id, ROUTE53_REGION);
                store.zones.insert(zone_id.clone(), zone);

                let body = format!(
                    "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
<CreateHostedZoneResponse xmlns=\"{ROUTE53_NS}\">\
<HostedZone>\
<Id>/hostedzone/{zone_id}</Id>\
<Name>{name}</Name>\
<Config><Comment>{comment}</Comment><PrivateZone>false</PrivateZone></Config>\
<ResourceRecordSetCount>2</ResourceRecordSetCount>\
</HostedZone>\
<ChangeInfo><Id>/{rid}</Id><Status>INSYNC</Status></ChangeInfo>\
</CreateHostedZoneResponse>"
                );
                Ok(xml_created(
                    body,
                    &format!("/2013-04-01/hostedzone/{zone_id}"),
                ))
            }

            // ----------------------------------------------------------------
            // DeleteHostedZone  DELETE /2013-04-01/hostedzone/{Id}
            // ----------------------------------------------------------------
            "DeleteHostedZone" => {
                // Extract zone ID from path: /2013-04-01/hostedzone/{id}
                let zone_id = ctx.path.split('/').next_back().unwrap_or("").to_string();
                let mut store = self.store.get_or_create(account_id, ROUTE53_REGION);
                store.zones.remove(&zone_id);
                store.records.retain(|(zid, _, _), _| zid != &zone_id);

                let body = format!(
                    "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
<DeleteHostedZoneResponse xmlns=\"{ROUTE53_NS}\">\
<ChangeInfo><Id>/{rid}</Id><Status>INSYNC</Status></ChangeInfo>\
</DeleteHostedZoneResponse>"
                );
                Ok(xml_ok(body))
            }

            // ----------------------------------------------------------------
            // ListHostedZones  GET /2013-04-01/hostedzone
            // ----------------------------------------------------------------
            "ListHostedZones" => {
                let store = self.store.get_or_create(account_id, ROUTE53_REGION);
                let zones_xml: String = store
                    .zones
                    .values()
                    .map(|z| {
                        format!(
                            "<HostedZone>\
<Id>/hostedzone/{}</Id>\
<Name>{}</Name>\
<Config><Comment>{}</Comment><PrivateZone>{}</PrivateZone></Config>\
<ResourceRecordSetCount>{}</ResourceRecordSetCount>\
</HostedZone>",
                            z.id, z.name, z.comment, z.private_zone, z.record_count
                        )
                    })
                    .collect();
                let body = format!(
                    "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
<ListHostedZonesResponse xmlns=\"{ROUTE53_NS}\">\
<HostedZones>{zones_xml}</HostedZones>\
<IsTruncated>false</IsTruncated>\
<MaxItems>100</MaxItems>\
</ListHostedZonesResponse>"
                );
                Ok(xml_ok(body))
            }

            // ----------------------------------------------------------------
            // ChangeResourceRecordSets  POST /2013-04-01/hostedzone/{Id}/rrset
            // ----------------------------------------------------------------
            "ChangeResourceRecordSets" => {
                // Path: /2013-04-01/hostedzone/{id}/rrset
                let parts: Vec<&str> = ctx.path.split('/').collect();
                // find "hostedzone" segment
                let zone_id = parts
                    .iter()
                    .enumerate()
                    .find(|(_, p)| *p == &"hostedzone")
                    .and_then(|(i, _)| parts.get(i + 1))
                    .copied()
                    .unwrap_or("")
                    .to_string();

                let raw = String::from_utf8_lossy(&ctx.raw_body);
                let changes = parse_rrsets(&raw);

                let mut store = self.store.get_or_create(account_id, ROUTE53_REGION);
                for (action, rrset) in changes {
                    let key = (
                        zone_id.clone(),
                        rrset.name.clone(),
                        rrset.record_type.clone(),
                    );
                    match action.as_str() {
                        "DELETE" => {
                            store.records.remove(&key);
                        }
                        _ => {
                            // CREATE | UPSERT
                            store.records.insert(key, rrset);
                        }
                    }
                }

                let body = format!(
                    "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
<ChangeResourceRecordSetsResponse xmlns=\"{ROUTE53_NS}\">\
<ChangeInfo><Id>/{rid}</Id><Status>INSYNC</Status></ChangeInfo>\
</ChangeResourceRecordSetsResponse>"
                );
                Ok(xml_ok(body))
            }

            // ----------------------------------------------------------------
            // ListResourceRecordSets  GET /2013-04-01/hostedzone/{Id}/rrset
            // ----------------------------------------------------------------
            "ListResourceRecordSets" => {
                let parts: Vec<&str> = ctx.path.split('/').collect();
                let zone_id = parts
                    .iter()
                    .enumerate()
                    .find(|(_, p)| *p == &"hostedzone")
                    .and_then(|(i, _)| parts.get(i + 1))
                    .copied()
                    .unwrap_or("")
                    .to_string();

                let store = self.store.get_or_create(account_id, ROUTE53_REGION);
                let rrsets_xml: String = store
                    .records
                    .iter()
                    .filter(|((zid, _, _), _)| zid == &zone_id)
                    .map(|((_, name, rtype), rrset)| {
                        let values_xml: String = rrset
                            .values
                            .iter()
                            .map(|v| format!("<ResourceRecord><Value>{v}</Value></ResourceRecord>"))
                            .collect();
                        format!(
                            "<ResourceRecordSet>\
<Name>{name}</Name>\
<Type>{rtype}</Type>\
<TTL>{}</TTL>\
<ResourceRecords>{values_xml}</ResourceRecords>\
</ResourceRecordSet>",
                            rrset.ttl
                        )
                    })
                    .collect();

                let body = format!(
                    "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
<ListResourceRecordSetsResponse xmlns=\"{ROUTE53_NS}\">\
<ResourceRecordSets>{rrsets_xml}</ResourceRecordSets>\
<IsTruncated>false</IsTruncated>\
<MaxItems>100</MaxItems>\
</ListResourceRecordSetsResponse>"
                );
                Ok(xml_ok(body))
            }

            _ => Err(DispatchError::NotImplemented(ctx.operation.clone())),
        }
    }
}
