use serde_json::json;

/// Crate/workspace semantic version from Cargo metadata.
pub fn pkg_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// Build tag injected at compile time by CI/workflows.
///
/// Examples:
/// - stable tag: `v1.2.3`
/// - rc channel: `rc`
/// - pr preview: `v1.2.3-rc-4.pr-123`
pub fn build_tag() -> Option<&'static str> {
    option_env!("OPENSTACK_BUILD_TAG").and_then(|v| {
        let trimmed = v.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed)
        }
    })
}

/// Build git sha injected at compile time by CI/workflows.
pub fn build_sha() -> Option<&'static str> {
    option_env!("OPENSTACK_BUILD_SHA").and_then(|v| {
        let trimmed = v.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed)
        }
    })
}

/// Human-friendly full version string for CLI/logging.
///
/// Examples:
/// - `0.1.0`
/// - `0.1.0 (v1.0.0-rc-1.pr-123)`
/// - `0.1.0 (v1.0.0-rc-1.pr-123, 8d6aa4f)`
pub fn display_version() -> String {
    match (build_tag(), build_sha()) {
        (Some(tag), Some(sha)) => format!("{} ({tag}, {sha})", pkg_version()),
        (Some(tag), None) => format!("{} ({tag})", pkg_version()),
        (None, Some(sha)) => format!("{} ({sha})", pkg_version()),
        (None, None) => pkg_version().to_string(),
    }
}

/// Structured build metadata for API responses.
pub fn build_info_json() -> serde_json::Value {
    json!({
        "tag": build_tag(),
        "sha": build_sha(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_version_always_contains_pkg_version() {
        assert!(display_version().starts_with(pkg_version()));
    }
}
