use crate::catalog::{GuidedServiceSummary, ServiceCatalog};
use crate::models::ServiceEntry;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DashboardServiceCard {
    pub service: String,
    pub status: String,
    pub support_tier: String,
    pub protocol: String,
    pub flow_count: usize,
    pub coverage_quality: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DashboardHomeViewModel {
    pub cards: Vec<DashboardServiceCard>,
    pub total_services: usize,
    pub guided_services: usize,
}

/// Builds the dashboard view model containing sorted service cards and summary counts.
///
/// # Returns
/// A `DashboardHomeViewModel` with `cards` sorted by service name, `total_services` set to the number of cards, and `guided_services` set to the count of cards whose `support_tier` is `"guided"`.
///
/// # Examples
///
/// ```
/// // Construct a `ServiceCatalog` populated with services, then build the dashboard model.
/// let catalog = ServiceCatalog::default();
/// let model = build_dashboard_home_model(&catalog);
/// assert_eq!(model.total_services, model.cards.len());
/// ```
pub fn build_dashboard_home_model(catalog: &ServiceCatalog) -> DashboardHomeViewModel {
    let mut cards = catalog
        .all()
        .iter()
        .map(|svc| to_card(svc, catalog.guided_summary(&svc.name)))
        .collect::<Vec<_>>();

    cards.sort_by(|a, b| a.service.cmp(&b.service));

    let guided_services = cards.iter().filter(|c| c.support_tier == "guided").count();

    DashboardHomeViewModel {
        total_services: cards.len(),
        guided_services,
        cards,
    }
}

/// Create a DashboardServiceCard for a service, incorporating optional guided metadata.
///
/// The returned card copies the service's name, status, and support_tier. When `guided` is
/// provided, `protocol`, `flow_count`, and `coverage_quality` are taken from it; otherwise
/// `protocol` and `coverage_quality` are set to `"unknown"` and `flow_count` is set to `0`.
///
/// # Examples
///
/// ```
/// let service = ServiceEntry {
///     name: "s3".to_string(),
///     status: "active".to_string(),
///     support_tier: "guided".to_string(),
///     ..Default::default()
/// };
/// let guided = GuidedServiceSummary {
///     protocol: "rest_xml".to_string(),
///     flow_count: 2,
///     quality: "meets_l1".to_string(),
/// };
///
/// let card = to_card(&service, Some(&guided));
/// assert_eq!(card.service, "s3");
/// assert_eq!(card.protocol, "rest_xml");
/// assert_eq!(card.flow_count, 2);
/// assert_eq!(card.coverage_quality, "meets_l1");
/// ```
fn to_card(service: &ServiceEntry, guided: Option<&GuidedServiceSummary>) -> DashboardServiceCard {
    DashboardServiceCard {
        service: service.name.clone(),
        status: service.status.clone(),
        support_tier: service.support_tier.clone(),
        protocol: guided
            .map(|item| item.protocol.clone())
            .unwrap_or_else(|| "unknown".to_string()),
        flow_count: guided.map(|item| item.flow_count).unwrap_or(0),
        coverage_quality: guided
            .map(|item| item.quality.clone())
            .unwrap_or_else(|| "unknown".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{
        FlowCatalogEntry, FlowCatalogResponse, FlowCoverageEntry, FlowCoverageResponse,
        StudioServicesResponse,
    };

    #[test]
    fn dashboard_model_composes_catalog_and_guided_data() {
        let services = StudioServicesResponse {
            services: vec![
                ServiceEntry {
                    name: "s3".to_string(),
                    status: "running".to_string(),
                    support_tier: "guided".to_string(),
                },
                ServiceEntry {
                    name: "ec2".to_string(),
                    status: "available".to_string(),
                    support_tier: "raw".to_string(),
                },
            ],
        };

        let flow_catalog = FlowCatalogResponse {
            services: vec![FlowCatalogEntry {
                service: "s3".to_string(),
                manifest_version: "1.2".to_string(),
                protocol: "rest_xml".to_string(),
                flow_count: 2,
                maturity: "l1".to_string(),
            }],
        };

        let flow_coverage = FlowCoverageResponse {
            schema_version: "1.2".to_string(),
            summary: "ok".to_string(),
            services: vec![FlowCoverageEntry {
                service: "s3".to_string(),
                has_manifest: true,
                l1_flows: 1,
                total_flows: 2,
                quality: "meets_l1".to_string(),
            }],
        };

        let catalog = ServiceCatalog::from_response(services)
            .with_guided_metadata(flow_catalog, flow_coverage);

        let model = build_dashboard_home_model(&catalog);
        assert_eq!(model.total_services, 2);
        assert_eq!(model.guided_services, 1);
        assert_eq!(model.cards[1].service, "s3");
        assert_eq!(model.cards[1].flow_count, 2);
        assert_eq!(model.cards[1].coverage_quality, "meets_l1");
    }
}
