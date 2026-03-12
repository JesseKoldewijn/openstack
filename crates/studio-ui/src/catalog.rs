use crate::models::{ServiceEntry, StudioServicesResponse};

#[derive(Debug, Clone, Default)]
pub struct ServiceCatalog {
    services: Vec<ServiceEntry>,
}

impl ServiceCatalog {
    pub fn from_response(response: StudioServicesResponse) -> Self {
        Self {
            services: response.services,
        }
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
}
