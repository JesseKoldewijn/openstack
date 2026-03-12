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
    /// Creates a ServiceCatalog from a StudioServicesResponse.
    ///
    /// The catalog's `services` vector is taken from the provided response and the `guided` map is initialized empty.
    ///
    /// # Parameters
    ///
    /// - `response`: The `StudioServicesResponse` whose `services` are moved into the resulting catalog.
    ///
    /// # Returns
    ///
    /// A `ServiceCatalog` containing the response's services and an empty guided metadata map.
    ///
    /// # Examples
    ///
    /// ```
    /// let resp = StudioServicesResponse { services: vec![/* ... */] };
    /// let catalog = ServiceCatalog::from_response(resp);
    /// assert_eq!(catalog.guided().count(), 0);
    /// ```
    pub fn from_response(response: StudioServicesResponse) -> Self {
        Self {
            services: response.services,
            guided: HashMap::new(),
        }
    }

    /// Enriches the catalog with guided metadata derived from a flow catalog and coverage report.
    ///
    /// This consumes a flow catalog and a coverage response, computes a per-service guided summary
    /// for each flow in the catalog, and stores those summaries in the catalog's `guided` map
    /// keyed by service name. Coverage entries are matched to flows by service name; when coverage
    /// for a service is absent, default values are used in the summary.
    ///
    /// # Returns
    ///
    /// Self with guided metadata populated for services present in the provided flow catalog.
    ///
    /// # Examples
    ///
    /// ```
    /// // Given existing responses: `services_resp`, `flow_catalog`, `flow_coverage`
    /// let base = ServiceCatalog::from_response(services_resp);
    /// let enriched = base.with_guided_metadata(flow_catalog, flow_coverage);
    /// // guided summaries are now available
    /// let _count = enriched.guided_services().count();
    /// ```
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

    /// List all services in the catalog.
    ///
    /// # Examples
    ///
    /// ```
    /// let catalog = ServiceCatalog::default();
    /// assert!(catalog.all().is_empty());
    /// ```
    ///
    /// # Returns
    ///
    /// A slice of `ServiceEntry` containing every service stored in the catalog.
    pub fn all(&self) -> &[ServiceEntry] {
        &self.services
    }

    /// Iterates over services whose `support_tier` equals the provided `tier`.
    ///
    /// # Returns
    ///
    /// An iterator yielding references to `ServiceEntry` items that match the given tier.
    ///
    /// # Examples
    ///
    /// ```
    /// let catalog = ServiceCatalog::default();
    /// let count = catalog.by_tier("gold").count();
    /// assert_eq!(count, 0);
    /// ```
    pub fn by_tier<'a>(&'a self, tier: &'a str) -> impl Iterator<Item = &'a ServiceEntry> {
        self.services.iter().filter(move |s| s.support_tier == tier)
    }

    /// Finds a service entry by its name.
    ///
    /// # Parameters
    ///
    /// - `name`: the service name to look up.
    ///
    /// # Returns
    ///
    /// `Some(&ServiceEntry)` for the first service whose `name` equals the provided `name`, `None` if no match is found.
    ///
    /// # Examples
    ///
    /// ```
    /// // Assuming `ServiceEntry` implements `Default` and has a `name` field:
    /// let svc = ServiceEntry { name: "users".into(), ..Default::default() };
    /// let catalog = ServiceCatalog { services: vec![svc], guided: Default::default() };
    /// assert!(catalog.by_name("users").is_some());
    /// assert!(catalog.by_name("payments").is_none());
    /// ```
    pub fn by_name(&self, name: &str) -> Option<&ServiceEntry> {
        self.services.iter().find(|s| s.name == name)
    }

    /// Retrieves the guided metadata summary for a service by name.
    ///
    /// # Returns
    ///
    /// `Some(&GuidedServiceSummary)` if a summary exists for the given service name, `None` otherwise.
    ///
    /// # Examples
    ///
    /// ```
    /// let catalog = ServiceCatalog {
    ///     services: vec![],
    ///     guided: {
    ///         let mut m = std::collections::HashMap::new();
    ///         m.insert("payments".to_string(), GuidedServiceSummary {
    ///             service: "payments".into(),
    ///             protocol: "http".into(),
    ///             maturity: "stable".into(),
    ///             flow_count: 3,
    ///             l1_flows: 1,
    ///             quality: "high".into(),
    ///         });
    ///         m
    ///     },
    /// };
    ///
    /// let summary = catalog.guided_summary("payments");
    /// assert!(summary.is_some());
    /// assert_eq!(summary.unwrap().service, "payments");
    /// ```
    pub fn guided_summary(&self, service: &str) -> Option<&GuidedServiceSummary> {
        self.guided.get(service)
    }

    /// Iterates over all guided service summaries.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::collections::HashMap;
    ///
    /// let mut guided = HashMap::new();
    /// guided.insert(
    ///     "svc".to_string(),
    ///     GuidedServiceSummary {
    ///         service: "svc".into(),
    ///         protocol: "http".into(),
    ///         maturity: "stable".into(),
    ///         flow_count: 1,
    ///         l1_flows: 0,
    ///         quality: "unknown".into(),
    ///     },
    /// );
    ///
    /// let catalog = ServiceCatalog {
    ///     services: Vec::new(),
    ///     guided,
    /// };
    ///
    /// let summaries: Vec<&GuidedServiceSummary> = catalog.guided_services().collect();
    /// assert_eq!(summaries.len(), 1);
    /// assert_eq!(summaries[0].service, "svc");
    /// ```
    pub fn guided_services(&self) -> impl Iterator<Item = &GuidedServiceSummary> {
        self.guided.values()
    }
}

/// Create a `GuidedServiceSummary` for a service flow, applying coverage defaults when coverage is absent.
///
/// When `coverage` is `None`, `l1_flows` is set to `0` and `quality` is set to `"unknown"`.
///
/// # Examples
///
/// ```
/// let flow = FlowCatalogEntry {
///     service: "payments".to_string(),
///     protocol: "http".to_string(),
///     maturity: "stable".to_string(),
///     flow_count: 3,
/// };
///
/// let summary_no_coverage = to_summary(flow.clone(), None);
/// assert_eq!(summary_no_coverage.service, "payments");
/// assert_eq!(summary_no_coverage.l1_flows, 0);
/// assert_eq!(summary_no_coverage.quality, "unknown");
///
/// let coverage = FlowCoverageEntry {
///     service: "payments".to_string(),
///     l1_flows: 2,
///     quality: "good".to_string(),
/// };
///
/// let summary_with_coverage = to_summary(flow, Some(&coverage));
/// assert_eq!(summary_with_coverage.l1_flows, 2);
/// assert_eq!(summary_with_coverage.quality, "good");
/// ```
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
