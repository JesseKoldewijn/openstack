use std::borrow::Cow;
use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use openstack_service_framework::traits::{
    DispatchError, DispatchResponse, RequestContext, ResponseBody, ServiceProvider,
};
use openstack_service_framework::xml::xml_escape;
use openstack_state::AccountRegionBundle;
use uuid::Uuid;

use crate::store::{HealthCheck, HealthCheckConfig, HostedZone, ResourceRecordSet, Route53Store};

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
const ROUTE53_SUBMITTED_AT: &str = "2010-09-10T01:36:41.958000Z";

fn xml_ok(body: String) -> DispatchResponse {
    DispatchResponse {
        status_code: 200,
        body: ResponseBody::Buffered(Bytes::from(body.into_bytes())),
        content_type: Cow::Borrowed("application/xml"),
        headers: Vec::new(),
    }
}

fn xml_created(body: String, location: &str) -> DispatchResponse {
    DispatchResponse {
        status_code: 201,
        body: ResponseBody::Buffered(Bytes::from(body.into_bytes())),
        content_type: Cow::Borrowed("application/xml"),
        headers: vec![("Location".to_string(), location.to_string())],
    }
}

fn xml_error(code: &str, message: &str, status: u16) -> DispatchResponse {
    let body = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
<ErrorResponse xmlns=\"{ROUTE53_NS}\">\
<Error><Code>{}</Code><Message>{}</Message></Error>\
</ErrorResponse>",
        xml_escape(code),
        xml_escape(message)
    );
    DispatchResponse {
        status_code: status,
        body: ResponseBody::Buffered(Bytes::from(body.into_bytes())),
        content_type: Cow::Borrowed("text/xml"),
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

fn zone_xml(zone: &HostedZone) -> String {
    format!(
        "<HostedZone>\
<Id>/hostedzone/{}</Id>\
<Name>{}</Name>\
<CallerReference>{}</CallerReference>\
<Config><Comment>{}</Comment><PrivateZone>{}</PrivateZone></Config>\
<ResourceRecordSetCount>{}</ResourceRecordSetCount>\
</HostedZone>",
        zone.id,
        xml_escape(&zone.name),
        xml_escape(&zone.caller_reference),
        xml_escape(&zone.comment),
        zone.private_zone,
        zone.record_count
    )
}

fn health_check_xml(hc: &HealthCheck) -> String {
    let ip_xml = hc
        .config
        .ip_address
        .as_deref()
        .map(|ip| format!("<IPAddress>{}</IPAddress>", xml_escape(ip)))
        .unwrap_or_default();
    let path_xml = hc
        .config
        .resource_path
        .as_deref()
        .map(|p| format!("<ResourcePath>{}</ResourcePath>", xml_escape(p)))
        .unwrap_or_default();
    let fqdn_xml = hc
        .config
        .fully_qualified_domain_name
        .as_deref()
        .map(|f| format!("<FullyQualifiedDomainName>{}</FullyQualifiedDomainName>", xml_escape(f)))
        .unwrap_or_default();
    format!(
        "<Id>{}</Id>\
<CallerReference>{}</CallerReference>\
<HealthCheckConfig>\
{ip_xml}\
<Port>{}</Port>\
<Type>{}</Type>\
{path_xml}\
{fqdn_xml}\
<RequestInterval>{}</RequestInterval>\
<FailureThreshold>{}</FailureThreshold>\
</HealthCheckConfig>\
<HealthCheckVersion>{}</HealthCheckVersion>",
        xml_escape(&hc.id),
        xml_escape(&hc.caller_reference),
        hc.config.port,
        xml_escape(&hc.config.health_check_type),
        hc.config.request_interval,
        hc.config.failure_threshold,
        hc.health_check_version,
    )
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
                let raw = String::from_utf8_lossy(ctx.raw_body_bytes());
                let name_raw = xml_text(&raw, "Name").unwrap_or_default();
                let name = if name_raw.ends_with('.') {
                    name_raw
                } else {
                    format!("{name_raw}.")
                };
                let Some(caller_reference) = xml_text(&raw, "CallerReference") else {
                    return Ok(xml_error(
                        "InvalidInput",
                        "CallerReference is required",
                        400,
                    ));
                };
                let comment = xml_text(&raw, "Comment").unwrap_or_default();

                let zone_id = short_id();
                let zone = HostedZone {
                    id: zone_id.clone(),
                    name: name.clone(),
                    caller_reference: caller_reference.clone(),
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
<Name>{}</Name>\
<CallerReference>{}</CallerReference>\
<Config><Comment>{}</Comment><PrivateZone>false</PrivateZone></Config>\
<ResourceRecordSetCount>2</ResourceRecordSetCount>\
</HostedZone>\
<ChangeInfo><Id>/change/{rid}</Id><Status>INSYNC</Status><SubmittedAt>{ROUTE53_SUBMITTED_AT}</SubmittedAt></ChangeInfo>\
</CreateHostedZoneResponse>",
                    xml_escape(&name),
                    xml_escape(&caller_reference),
                    xml_escape(&comment)
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
                let zone_id = ctx.path.split('/').next_back().unwrap_or("").to_string();
                let mut store = self.store.get_or_create(account_id, ROUTE53_REGION);
                store.zones.remove(&zone_id);
                store.records.retain(|(zid, _, _), _| zid != &zone_id);

                let body = format!(
                    "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
<DeleteHostedZoneResponse xmlns=\"{ROUTE53_NS}\">\
<ChangeInfo><Id>/change/{rid}</Id><Status>INSYNC</Status><SubmittedAt>{ROUTE53_SUBMITTED_AT}</SubmittedAt></ChangeInfo>\
</DeleteHostedZoneResponse>"
                );
                Ok(xml_ok(body))
            }

            // ----------------------------------------------------------------
            // ListHostedZones  GET /2013-04-01/hostedzone
            // ----------------------------------------------------------------
            "ListHostedZones" => {
                let Some(store) = self.store.get(account_id, ROUTE53_REGION) else {
                    let body = format!(
                        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
<ListHostedZonesResponse xmlns=\"{ROUTE53_NS}\">\
<HostedZones></HostedZones>\
<IsTruncated>false</IsTruncated>\
<MaxItems>100</MaxItems>\
</ListHostedZonesResponse>"
                    );
                    return Ok(xml_ok(body));
                };
                let zones_xml: String = store.zones.values().map(zone_xml).collect();
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
            // GetHostedZone  GET /2013-04-01/hostedzone/{Id}
            // ----------------------------------------------------------------
            "GetHostedZone" => {
                let zone_id = ctx.path.split('/').next_back().unwrap_or("").to_string();
                let Some(store) = self.store.get(account_id, ROUTE53_REGION) else {
                    return Ok(xml_error(
                        "NoSuchHostedZone",
                        &format!("No hosted zone found with ID: {zone_id}"),
                        404,
                    ));
                };
                let Some(zone) = store.zones.get(&zone_id) else {
                    return Ok(xml_error(
                        "NoSuchHostedZone",
                        &format!("No hosted zone found with ID: {zone_id}"),
                        404,
                    ));
                };
                let body = format!(
                    "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
<GetHostedZoneResponse xmlns=\"{ROUTE53_NS}\">\
{}\
</GetHostedZoneResponse>",
                    zone_xml(zone)
                );
                Ok(xml_ok(body))
            }

            // ----------------------------------------------------------------
            // GetChange  GET /2013-04-01/change/{Id}
            // ----------------------------------------------------------------
            "GetChange" => {
                let change_id = ctx.path.split('/').next_back().unwrap_or("").to_string();
                let body = format!(
                    "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
<GetChangeResponse xmlns=\"{ROUTE53_NS}\">\
<ChangeInfo><Id>/change/{change_id}</Id><Status>INSYNC</Status><SubmittedAt>{ROUTE53_SUBMITTED_AT}</SubmittedAt></ChangeInfo>\
</GetChangeResponse>"
                );
                Ok(xml_ok(body))
            }

            // ----------------------------------------------------------------
            // ChangeResourceRecordSets  POST /2013-04-01/hostedzone/{Id}/rrset
            // ----------------------------------------------------------------
            "ChangeResourceRecordSets" => {
                let parts: Vec<&str> = ctx.path.split('/').collect();
                let zone_id = parts
                    .iter()
                    .enumerate()
                    .find(|(_, p)| *p == &"hostedzone")
                    .and_then(|(i, _)| parts.get(i + 1))
                    .copied()
                    .unwrap_or("")
                    .to_string();

                let raw = String::from_utf8_lossy(ctx.raw_body_bytes());
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
                            store.records.insert(key, rrset);
                        }
                    }
                }

                let body = format!(
                    "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
<ChangeResourceRecordSetsResponse xmlns=\"{ROUTE53_NS}\">\
<ChangeInfo><Id>/change/{rid}</Id><Status>INSYNC</Status><SubmittedAt>{ROUTE53_SUBMITTED_AT}</SubmittedAt></ChangeInfo>\
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

                let Some(store) = self.store.get(account_id, ROUTE53_REGION) else {
                    return Ok(xml_error(
                        "NoSuchHostedZone",
                        &format!("No hosted zone found with ID: {zone_id}"),
                        404,
                    ));
                };
                if !store.zones.contains_key(&zone_id) {
                    return Ok(xml_error(
                        "NoSuchHostedZone",
                        &format!("No hosted zone found with ID: {zone_id}"),
                        404,
                    ));
                }
                let rrsets_xml: String = store
                    .records
                    .iter()
                    .filter(|((zid, _, _), _)| zid == &zone_id)
                    .map(|((_, name, rtype), rrset)| {
                        let values_xml: String = rrset
                            .values
                            .iter()
                            .map(|v| {
                                format!(
                                    "<ResourceRecord><Value>{}</Value></ResourceRecord>",
                                    xml_escape(v)
                                )
                            })
                            .collect();
                        format!(
                            "<ResourceRecordSet>\
<Name>{}</Name>\
<Type>{}</Type>\
<TTL>{}</TTL>\
<ResourceRecords>{values_xml}</ResourceRecords>\
</ResourceRecordSet>",
                            xml_escape(name),
                            xml_escape(rtype),
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

            // ----------------------------------------------------------------
            // ListHostedZonesByName  GET /2013-04-01/hostedzonesbyname
            // ----------------------------------------------------------------
            "ListHostedZonesByName" => {
                let Some(store) = self.store.get(account_id, ROUTE53_REGION) else {
                    let body = format!(
                        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
<ListHostedZonesByNameResponse xmlns=\"{ROUTE53_NS}\">\
<HostedZones></HostedZones>\
<IsTruncated>false</IsTruncated>\
<MaxItems>100</MaxItems>\
</ListHostedZonesByNameResponse>"
                    );
                    return Ok(xml_ok(body));
                };
                let dns_name_filter = ctx
                    .query_params
                    .get("dnsname")
                    .map(|s| s.as_str())
                    .unwrap_or("");
                let mut zones: Vec<&HostedZone> = store
                    .zones
                    .values()
                    .filter(|z| {
                        dns_name_filter.is_empty()
                            || z.name
                                .trim_end_matches('.')
                                .ends_with(dns_name_filter.trim_end_matches('.'))
                    })
                    .collect();
                zones.sort_by(|a, b| a.name.cmp(&b.name));
                let zones_xml: String = zones.iter().map(|z| zone_xml(z)).collect();
                let body = format!(
                    "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
<ListHostedZonesByNameResponse xmlns=\"{ROUTE53_NS}\">\
<HostedZones>{zones_xml}</HostedZones>\
<IsTruncated>false</IsTruncated>\
<MaxItems>100</MaxItems>\
</ListHostedZonesByNameResponse>"
                );
                Ok(xml_ok(body))
            }

            // ----------------------------------------------------------------
            // GetHostedZoneCount  GET /2013-04-01/hostedzonecount
            // ----------------------------------------------------------------
            "GetHostedZoneCount" => {
                let count = self
                    .store
                    .get(account_id, ROUTE53_REGION)
                    .map(|s| s.zones.len())
                    .unwrap_or(0);
                let body = format!(
                    "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
<GetHostedZoneCountResponse xmlns=\"{ROUTE53_NS}\">\
<HostedZoneCount>{count}</HostedZoneCount>\
</GetHostedZoneCountResponse>"
                );
                Ok(xml_ok(body))
            }

            // ----------------------------------------------------------------
            // CreateHealthCheck  POST /2013-04-01/healthcheck
            // ----------------------------------------------------------------
            "CreateHealthCheck" => {
                let raw = String::from_utf8_lossy(ctx.raw_body_bytes());
                let caller_reference =
                    xml_text(&raw, "CallerReference").unwrap_or_else(req_id);
                let health_check_type =
                    xml_text(&raw, "Type").unwrap_or_else(|| "HTTP".to_string());
                let ip_address = xml_text(&raw, "IPAddress");
                let port: u16 = xml_text(&raw, "Port")
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(80);
                let resource_path = xml_text(&raw, "ResourcePath");
                let fqdn = xml_text(&raw, "FullyQualifiedDomainName");
                let request_interval: u32 = xml_text(&raw, "RequestInterval")
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(30);
                let failure_threshold: u32 = xml_text(&raw, "FailureThreshold")
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(3);

                let hc_id = short_id();
                let hc = HealthCheck {
                    id: hc_id.clone(),
                    caller_reference,
                    config: HealthCheckConfig {
                        ip_address,
                        port,
                        health_check_type,
                        resource_path,
                        fully_qualified_domain_name: fqdn,
                        request_interval,
                        failure_threshold,
                    },
                    health_check_version: 1,
                };

                let mut store = self.store.get_or_create(account_id, ROUTE53_REGION);
                store.health_checks.insert(hc_id.clone(), hc.clone());

                let body = format!(
                    "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
<CreateHealthCheckResponse xmlns=\"{ROUTE53_NS}\">\
<HealthCheck>{}</HealthCheck>\
</CreateHealthCheckResponse>",
                    health_check_xml(&hc)
                );
                Ok(xml_created(
                    body,
                    &format!("/2013-04-01/healthcheck/{hc_id}"),
                ))
            }

            // ----------------------------------------------------------------
            // GetHealthCheck  GET /2013-04-01/healthcheck/{Id}
            // ----------------------------------------------------------------
            "GetHealthCheck" => {
                let hc_id = ctx.path.split('/').next_back().unwrap_or("").to_string();
                let Some(store) = self.store.get(account_id, ROUTE53_REGION) else {
                    return Ok(xml_error(
                        "NoSuchHealthCheck",
                        &format!("No health check with ID: {hc_id}"),
                        404,
                    ));
                };
                match store.health_checks.get(&hc_id) {
                    Some(hc) => {
                        let body = format!(
                            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
<GetHealthCheckResponse xmlns=\"{ROUTE53_NS}\">\
<HealthCheck>{}</HealthCheck>\
</GetHealthCheckResponse>",
                            health_check_xml(hc)
                        );
                        Ok(xml_ok(body))
                    }
                    None => Ok(xml_error(
                        "NoSuchHealthCheck",
                        &format!("No health check with ID: {hc_id}"),
                        404,
                    )),
                }
            }

            // ----------------------------------------------------------------
            // DeleteHealthCheck  DELETE /2013-04-01/healthcheck/{Id}
            // ----------------------------------------------------------------
            "DeleteHealthCheck" => {
                let hc_id = ctx.path.split('/').next_back().unwrap_or("").to_string();
                let mut store = self.store.get_or_create(account_id, ROUTE53_REGION);
                if store.health_checks.remove(&hc_id).is_none() {
                    return Ok(xml_error(
                        "NoSuchHealthCheck",
                        &format!("No health check with ID: {hc_id}"),
                        404,
                    ));
                }
                let body = format!(
                    "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
<DeleteHealthCheckResponse xmlns=\"{ROUTE53_NS}\"></DeleteHealthCheckResponse>"
                );
                Ok(xml_ok(body))
            }

            // ----------------------------------------------------------------
            // ListHealthChecks  GET /2013-04-01/healthcheck
            // ----------------------------------------------------------------
            "ListHealthChecks" => {
                let Some(store) = self.store.get(account_id, ROUTE53_REGION) else {
                    let body = format!(
                        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
<ListHealthChecksResponse xmlns=\"{ROUTE53_NS}\">\
<HealthChecks></HealthChecks>\
<IsTruncated>false</IsTruncated>\
<MaxItems>100</MaxItems>\
</ListHealthChecksResponse>"
                    );
                    return Ok(xml_ok(body));
                };
                let hcs_xml: String = store
                    .health_checks
                    .values()
                    .map(|hc| format!("<HealthCheck>{}</HealthCheck>", health_check_xml(hc)))
                    .collect();
                let body = format!(
                    "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
<ListHealthChecksResponse xmlns=\"{ROUTE53_NS}\">\
<HealthChecks>{hcs_xml}</HealthChecks>\
<IsTruncated>false</IsTruncated>\
<MaxItems>100</MaxItems>\
</ListHealthChecksResponse>"
                );
                Ok(xml_ok(body))
            }

            // ----------------------------------------------------------------
            // ChangeTagsForResource  POST /2013-04-01/tags/{ResourceType}/{ResourceId}
            // ----------------------------------------------------------------
            "ChangeTagsForResource" => {
                // Path: /2013-04-01/tags/{resourcetype}/{resourceid}
                let parts: Vec<&str> = ctx.path.split('/').collect();
                let (resource_type, resource_id) = parts
                    .iter()
                    .enumerate()
                    .find(|(_, p)| *p == &"tags")
                    .map(|(i, _)| {
                        let rtype = parts.get(i + 1).copied().unwrap_or("");
                        let rid = parts.get(i + 2).copied().unwrap_or("");
                        (rtype.to_string(), rid.to_string())
                    })
                    .unwrap_or_default();

                let raw = String::from_utf8_lossy(ctx.raw_body_bytes());
                let mut store = self.store.get_or_create(account_id, ROUTE53_REGION);
                let tag_map = store
                    .tags
                    .entry((resource_type, resource_id))
                    .or_default();

                // Parse <AddTags><Tag><Key>...</Key><Value>...</Value></Tag></AddTags>
                let mut rest = raw.as_ref();
                while let Some(start) = rest.find("<Tag>") {
                    let chunk = &rest[start..];
                    let end = chunk.find("</Tag>").unwrap_or(chunk.len());
                    let tag_block = &chunk[..end];
                    if let (Some(key), Some(value)) =
                        (xml_text(tag_block, "Key"), xml_text(tag_block, "Value"))
                    {
                        tag_map.insert(key, value);
                    }
                    rest = &rest[start + end..];
                }

                // Parse <RemoveTagKeys><Key>...</Key></RemoveTagKeys>
                let mut remove_rest = raw.as_ref();
                if let Some(remove_start) = remove_rest.find("<RemoveTagKeys>") {
                    let remove_chunk = &remove_rest[remove_start..];
                    let remove_end = remove_chunk.find("</RemoveTagKeys>").unwrap_or(remove_chunk.len());
                    let remove_block = &remove_chunk[..remove_end];
                    remove_rest = remove_block;
                    while let Some(ks) = remove_rest.find("<Key>") {
                        let after = &remove_rest[ks + 5..];
                        if let Some(ke) = after.find("</Key>") {
                            let k = &after[..ke];
                            tag_map.remove(k);
                        }
                        remove_rest = &remove_rest[ks + 5..];
                    }
                }

                let body = format!(
                    "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
<ChangeTagsForResourceResponse xmlns=\"{ROUTE53_NS}\"></ChangeTagsForResourceResponse>"
                );
                Ok(xml_ok(body))
            }

            // ----------------------------------------------------------------
            // ListTagsForResource  GET /2013-04-01/tags/{ResourceType}/{ResourceId}
            // ----------------------------------------------------------------
            "ListTagsForResource" => {
                let parts: Vec<&str> = ctx.path.split('/').collect();
                let (resource_type, resource_id) = parts
                    .iter()
                    .enumerate()
                    .find(|(_, p)| *p == &"tags")
                    .map(|(i, _)| {
                        let rtype = parts.get(i + 1).copied().unwrap_or("");
                        let rid = parts.get(i + 2).copied().unwrap_or("");
                        (rtype.to_string(), rid.to_string())
                    })
                    .unwrap_or_default();

                let tags_xml = self
                    .store
                    .get(account_id, ROUTE53_REGION)
                    .and_then(|store| {
                        store
                            .tags
                            .get(&(resource_type.clone(), resource_id.clone()))
                            .map(|tags| {
                                tags.iter()
                                    .map(|(k, v)| {
                                        format!(
                                            "<Tag><Key>{}</Key><Value>{}</Value></Tag>",
                                            xml_escape(k),
                                            xml_escape(v)
                                        )
                                    })
                                    .collect::<String>()
                            })
                    })
                    .unwrap_or_default();

                let body = format!(
                    "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
<ListTagsForResourceResponse xmlns=\"{ROUTE53_NS}\">\
<ResourceTagSet>\
<ResourceType>{resource_type}</ResourceType>\
<ResourceId>{resource_id}</ResourceId>\
<Tags>{tags_xml}</Tags>\
</ResourceTagSet>\
</ListTagsForResourceResponse>"
                );
                Ok(xml_ok(body))
            }

            _ => Err(DispatchError::NotImplemented(ctx.operation.clone())),
        }
    }

    async fn storage_snapshot(&self) -> Option<serde_json::Value> {
        use serde_json::json;
        let mut zones = Vec::new();
        for entry in self.store.iter() {
            for zone in entry.value().zones.values() {
                zones.push(json!({
                    "id": zone.id, "kind": "hosted_zone",
                    "attributes": [
                        {"key": "name", "value": zone.name.clone()},
                        {"key": "record_count", "value": zone.record_count.to_string()},
                        {"key": "private", "value": zone.private_zone.to_string()},
                    ]
                }));
            }
        }
        Some(json!({ "kind": "route53", "hosted_zones": zones }))
    }
}
