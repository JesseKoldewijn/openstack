/// The `ServiceSkeleton` is a helper that service providers can use to
/// map operation names to handler functions with consistent error handling.
///
/// Each service implements its own dispatch table.
use crate::traits::{DispatchError, DispatchResponse};

/// Helper to return a `NotImplemented` error for an unimplemented operation.
pub fn not_implemented(operation: &str) -> Result<DispatchResponse, DispatchError> {
    Err(DispatchError::NotImplemented(format!(
        "Operation '{}' is not yet implemented",
        operation
    )))
}

/// Helper to extract a required JSON field from the request body.
pub fn get_required_field<'a>(
    body: &'a serde_json::Value,
    field: &str,
) -> Result<&'a serde_json::Value, DispatchError> {
    body.get(field)
        .ok_or_else(|| DispatchError::ProviderError(format!("Missing required field: {}", field)))
}

/// Helper to extract an optional string field from the request body.
pub fn get_string_field(body: &serde_json::Value, field: &str) -> Option<String> {
    body.get(field)
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}
