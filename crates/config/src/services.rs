use std::collections::{HashMap, HashSet};

/// Configuration for which services are enabled and their provider overrides.
#[derive(Debug, Clone)]
pub struct ServicesConfig {
    /// If Some, only these services are enabled. If None, all are enabled.
    enabled: Option<HashSet<String>>,
    /// Provider overrides: service_name -> provider_name
    overrides: HashMap<String, String>,
}

impl ServicesConfig {
    /// All services enabled, no overrides.
    pub fn all() -> Self {
        Self {
            enabled: None,
            overrides: HashMap::new(),
        }
    }

    /// Only the given services are enabled.
    pub fn only(services: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self {
            enabled: Some(
                services
                    .into_iter()
                    .map(|s| s.into().to_lowercase())
                    .collect(),
            ),
            overrides: HashMap::new(),
        }
    }

    pub fn from_env() -> Self {
        let enabled = std::env::var("SERVICES").ok().map(|v| {
            v.split(',')
                .map(|s| s.trim().to_lowercase())
                .filter(|s| !s.is_empty())
                .collect()
        });

        // Parse all PROVIDER_OVERRIDE_<SERVICE> env vars
        let overrides = std::env::vars()
            .filter_map(|(key, value)| {
                key.strip_prefix("PROVIDER_OVERRIDE_")
                    .map(|service| (service.to_lowercase(), value))
            })
            .collect();

        Self { enabled, overrides }
    }

    /// Returns true if the given service is enabled.
    pub fn is_enabled(&self, service: &str) -> bool {
        match &self.enabled {
            Some(set) => set.contains(&service.to_lowercase()),
            None => true,
        }
    }

    /// Returns the provider override for a service, if any.
    pub fn get_override(&self, service: &str) -> Option<&str> {
        self.overrides
            .get(&service.to_lowercase())
            .map(|s| s.as_str())
    }

    /// Returns the set of explicitly enabled services, or None if all are enabled.
    pub fn enabled_services(&self) -> Option<&HashSet<String>> {
        self.enabled.as_ref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_all_services_enabled_by_default() {
        let config = ServicesConfig {
            enabled: None,
            overrides: HashMap::new(),
        };
        assert!(config.is_enabled("s3"));
        assert!(config.is_enabled("sqs"));
        assert!(config.is_enabled("dynamodb"));
    }

    #[test]
    fn test_restricted_services() {
        let config = ServicesConfig {
            enabled: Some(["s3".to_string(), "sqs".to_string()].into()),
            overrides: HashMap::new(),
        };
        assert!(config.is_enabled("s3"));
        assert!(config.is_enabled("sqs"));
        assert!(!config.is_enabled("dynamodb"));
        assert!(!config.is_enabled("lambda"));
    }

    #[test]
    fn test_provider_override() {
        let config = ServicesConfig {
            enabled: None,
            overrides: [("sqs".to_string(), "v2".to_string())].into(),
        };
        assert_eq!(config.get_override("sqs"), Some("v2"));
        assert_eq!(config.get_override("SQS"), Some("v2"));
        assert_eq!(config.get_override("s3"), None);
    }
}
