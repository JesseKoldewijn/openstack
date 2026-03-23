use std::collections::HashMap;

use crate::models::{
    FlowCatalogEntry, FlowCatalogResponse, FlowCoverageEntry, FlowCoverageResponse, ServiceEntry,
    StudioServicesResponse,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuidedServiceSummary {
    pub service: String,
    pub protocol: String,
    pub maturity: String,
    pub flow_count: usize,
    pub l1_flows: usize,
    pub quality: String,
}

#[derive(Debug, Clone, Default)]
pub struct ServiceCatalog {
    services: Vec<ServiceEntry>,
    guided: HashMap<String, GuidedServiceSummary>,
}

impl ServiceCatalog {
    pub fn from_response(response: StudioServicesResponse) -> Self {
        Self {
            services: response.services,
            guided: HashMap::new(),
        }
    }

    pub fn with_guided_metadata(
        mut self,
        catalog: FlowCatalogResponse,
        coverage: FlowCoverageResponse,
    ) -> Self {
        let by_service_coverage = coverage
            .services
            .into_iter()
            .map(|entry| (entry.service.clone(), entry))
            .collect::<HashMap<String, FlowCoverageEntry>>();

        for flow in catalog.services {
            let service_name = flow.service.clone();
            let summary = to_summary(flow, by_service_coverage.get(&service_name));
            self.guided.insert(summary.service.clone(), summary);
        }

        self
    }

    pub fn all(&self) -> &[ServiceEntry] {
        &self.services
    }

    pub fn by_tier<'a>(&'a self, tier: &'a str) -> impl Iterator<Item = &'a ServiceEntry> {
        self.services.iter().filter(move |s| s.support_tier == tier)
    }

    pub fn by_name(&self, name: &str) -> Option<&ServiceEntry> {
        self.services.iter().find(|s| s.name == name)
    }

    pub fn guided_summary(&self, service: &str) -> Option<&GuidedServiceSummary> {
        self.guided.get(service)
    }

    pub fn guided_services(&self) -> impl Iterator<Item = &GuidedServiceSummary> {
        self.guided.values()
    }
}

fn to_summary(
    flow: FlowCatalogEntry,
    coverage: Option<&FlowCoverageEntry>,
) -> GuidedServiceSummary {
    GuidedServiceSummary {
        service: flow.service,
        protocol: flow.protocol,
        maturity: flow.maturity,
        flow_count: flow.flow_count,
        l1_flows: coverage.map(|item| item.l1_flows).unwrap_or(0),
        quality: coverage
            .map(|item| item.quality.clone())
            .unwrap_or_else(|| "unknown".to_string()),
    }
}
