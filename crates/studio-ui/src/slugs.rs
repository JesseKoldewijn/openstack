/// Canonical service slug normalization.
///
/// AWS operations use several slug naming conventions that differ between:
/// - guided manifest `"service"` fields (e.g. `"events"`, `"states"`)
/// - provider `service_name()` returns (e.g. `"eventbridge"`, `"stepfunctions"`)
/// - AWS SDK service names (e.g. `"eventbridge"`, `"states"`)
///
/// This module provides a single authoritative mapping so the Studio API,
/// operation catalog, storage inspector, and SPA all agree.
use std::collections::HashMap;

/// Normalize any known alias to the canonical provider slug.
///
/// Examples:
/// - `"events"` → `"eventbridge"`
/// - `"states"` → `"stepfunctions"`
/// - `"secretsmanager"` → `"secretsmanager"` (no-op)
pub fn to_provider_slug(slug: &str) -> &str {
    match slug {
        "events"         => "eventbridge",
        "states"         => "stepfunctions",
        "awsevents"      => "eventbridge",
        "monitoring"     => "cloudwatch",
        "logs"           => "cloudwatch",
        "email"          => "ses",
        "email-smtp"     => "ses",
        "elasticmapreduce" => "emr",
        _                => slug,
    }
}

/// Normalize any known alias to the canonical manifest slug.
///
/// Manifest slugs follow the service's AWS endpoint prefix convention,
/// not the provider name.  Most are identical to provider slugs.
/// The exceptions are documented below.
pub fn to_manifest_slug(slug: &str) -> &str {
    match slug {
        "eventbridge"    => "events",
        "stepfunctions"  => "states",
        _                => slug,
    }
}

/// Returns all known alias mappings as (alias, canonical) pairs.
/// Useful for building lookup tables in the SPA and API handlers.
pub fn alias_map() -> HashMap<&'static str, &'static str> {
    let mut m = HashMap::new();
    m.insert("events",           "eventbridge");
    m.insert("states",           "stepfunctions");
    m.insert("awsevents",        "eventbridge");
    m.insert("monitoring",       "cloudwatch");
    m.insert("logs",             "cloudwatch");
    m.insert("email",            "ses");
    m.insert("email-smtp",       "ses");
    m
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_to_provider() {
        assert_eq!(to_provider_slug("events"), "eventbridge");
        assert_eq!(to_provider_slug("states"), "stepfunctions");
        assert_eq!(to_provider_slug("s3"), "s3");
        assert_eq!(to_provider_slug("sqs"), "sqs");
        assert_eq!(to_provider_slug("monitoring"), "cloudwatch");
    }

    #[test]
    fn provider_to_manifest() {
        assert_eq!(to_manifest_slug("eventbridge"), "events");
        assert_eq!(to_manifest_slug("stepfunctions"), "states");
        assert_eq!(to_manifest_slug("s3"), "s3");
        assert_eq!(to_manifest_slug("cloudwatch"), "cloudwatch");
    }

    #[test]
    fn round_trip_stable() {
        // Known aliases normalize consistently.
        for (alias, canonical) in alias_map() {
            assert_eq!(to_provider_slug(alias), canonical);
        }
    }
}
