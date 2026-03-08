/// Parses AWS SigV4 Authorization headers to extract auth context.
///
/// Format: AWS4-HMAC-SHA256 Credential=ACCESS_KEY/DATE/REGION/SERVICE/aws4_request,
///         SignedHeaders=..., Signature=...
#[derive(Debug, Default)]
pub struct SigV4Auth {
    pub access_key: String,
    pub region: String,
    pub service: String,
    pub date: String,
}

/// Parse a SigV4 Authorization header value.
/// Returns None if the header is not a valid SigV4 header.
pub fn parse_sigv4_auth(auth: &str) -> Option<SigV4Auth> {
    if !auth.starts_with("AWS4-HMAC-SHA256") {
        return None;
    }

    // Split on ',' and look for the Credential part in any segment,
    // including the first segment which contains the algorithm prefix.
    let credential = auth.split(',').find_map(|s| {
        let s = s.trim();
        // May appear as "AWS4-HMAC-SHA256 Credential=..." or just "Credential=..."
        if let Some(rest) = s.strip_prefix("Credential=") {
            Some(rest.trim())
        } else if let Some(rest) = s.find("Credential=").map(|i| &s[i + "Credential=".len()..]) {
            Some(rest.trim())
        } else {
            None
        }
    })?;

    // credential scope: ACCESS_KEY/DATE/REGION/SERVICE/aws4_request
    let parts: Vec<&str> = credential.split('/').collect();
    if parts.len() < 5 {
        return None;
    }

    Some(SigV4Auth {
        access_key: parts[0].to_string(),
        date: parts[1].to_string(),
        region: parts[2].to_string(),
        service: parts[3].to_string(),
    })
}

/// Derive the account ID from an access key.
/// Uses a deterministic mapping so unknown keys always map to the same account.
pub fn access_key_to_account_id(access_key: &str) -> String {
    // Well-known test access keys
    const DEFAULT_ACCOUNT: &str = "000000000000";
    const TEST_ACCESS_KEYS: &[(&str, &str)] = &[
        ("test", DEFAULT_ACCOUNT),
        ("mock", DEFAULT_ACCOUNT),
        ("AKIAIOSFODNN7EXAMPLE", DEFAULT_ACCOUNT),
    ];

    for (key, account) in TEST_ACCESS_KEYS {
        if access_key.starts_with(key) {
            return account.to_string();
        }
    }

    // Deterministic derivation for unknown keys: use hash of key
    derive_account_id_from_key(access_key)
}

fn derive_account_id_from_key(key: &str) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    key.hash(&mut hasher);
    let hash = hasher.finish();

    // Map to a 12-digit account ID
    format!("{:012}", hash % 1_000_000_000_000u64)
}

/// The default authorization context injected when no Authorization header is present.
pub const DEFAULT_ACCESS_KEY: &str = "test";
pub const DEFAULT_ACCOUNT_ID: &str = "000000000000";
pub const DEFAULT_REGION: &str = "us-east-1";

/// List of valid AWS region names for region validation.
pub const VALID_REGIONS: &[&str] = &[
    "us-east-1",
    "us-east-2",
    "us-west-1",
    "us-west-2",
    "eu-west-1",
    "eu-west-2",
    "eu-west-3",
    "eu-central-1",
    "eu-central-2",
    "eu-north-1",
    "eu-south-1",
    "eu-south-2",
    "ap-northeast-1",
    "ap-northeast-2",
    "ap-northeast-3",
    "ap-southeast-1",
    "ap-southeast-2",
    "ap-southeast-3",
    "ap-southeast-4",
    "ap-south-1",
    "ap-south-2",
    "ap-east-1",
    "sa-east-1",
    "ca-central-1",
    "ca-west-1",
    "me-south-1",
    "me-central-1",
    "af-south-1",
    "il-central-1",
    "us-gov-east-1",
    "us-gov-west-1",
    "cn-north-1",
    "cn-northwest-1",
];

pub fn is_valid_region(region: &str) -> bool {
    VALID_REGIONS.contains(&region)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_sigv4_auth() {
        let auth = "AWS4-HMAC-SHA256 Credential=AKID123/20260306/us-east-1/sqs/aws4_request, \
                   SignedHeaders=host;x-amz-date, Signature=abc123";
        let parsed = parse_sigv4_auth(auth).unwrap();
        assert_eq!(parsed.access_key, "AKID123");
        assert_eq!(parsed.region, "us-east-1");
        assert_eq!(parsed.service, "sqs");
        assert_eq!(parsed.date, "20260306");
    }

    #[test]
    fn test_parse_sigv4_invalid() {
        assert!(parse_sigv4_auth("Basic foo:bar").is_none());
        assert!(parse_sigv4_auth("").is_none());
    }

    #[test]
    fn test_access_key_to_account_id_default() {
        assert_eq!(access_key_to_account_id("test"), "000000000000");
        assert_eq!(
            access_key_to_account_id("AKIAIOSFODNN7EXAMPLE"),
            "000000000000"
        );
    }

    #[test]
    fn test_deterministic_account_id() {
        let id1 = access_key_to_account_id("UNKNOWNKEY123");
        let id2 = access_key_to_account_id("UNKNOWNKEY123");
        assert_eq!(id1, id2);
        assert_eq!(id1.len(), 12);
    }

    #[test]
    fn test_valid_regions() {
        assert!(is_valid_region("us-east-1"));
        assert!(is_valid_region("eu-west-1"));
        assert!(!is_valid_region("my-custom-region"));
    }
}
