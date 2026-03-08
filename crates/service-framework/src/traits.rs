use async_trait::async_trait;
use bytes::Bytes;
use thiserror::Error;

/// The parsed request context passed to provider methods.
#[derive(Debug, Clone)]
pub struct RequestContext {
    /// Target AWS service (e.g., "s3", "sqs")
    pub service: String,
    /// AWS operation name (e.g., "CreateQueue")
    pub operation: String,
    /// AWS region (e.g., "us-east-1")
    pub region: String,
    /// AWS account ID (e.g., "000000000000")
    pub account_id: String,
    /// The parsed request body (protocol-specific)
    pub request_body: serde_json::Value,
    /// Raw request bytes (for protocols that need it)
    pub raw_body: Bytes,
    /// Request headers (key lowercased)
    pub headers: std::collections::HashMap<String, String>,
    /// URL path
    pub path: String,
    /// HTTP method
    pub method: String,
    /// Query string parameters
    pub query_params: std::collections::HashMap<String, String>,
}

impl RequestContext {
    pub fn new(
        service: impl Into<String>,
        operation: impl Into<String>,
        region: impl Into<String>,
        account_id: impl Into<String>,
    ) -> Self {
        Self {
            service: service.into(),
            operation: operation.into(),
            region: region.into(),
            account_id: account_id.into(),
            request_body: serde_json::Value::Null,
            raw_body: Bytes::new(),
            headers: Default::default(),
            path: String::new(),
            method: String::new(),
            query_params: Default::default(),
        }
    }
}

/// Error returned when dispatching a request to a provider fails.
#[derive(Debug, Error)]
pub enum DispatchError {
    #[error("operation not implemented: {0}")]
    NotImplemented(String),
    #[error("service not found: {0}")]
    ServiceNotFound(String),
    #[error("service unavailable: {0}")]
    ServiceUnavailable(String),
    #[error("provider error: {0}")]
    ProviderError(String),
    #[error("serialization error: {0}")]
    SerializationError(String),
}

/// The base trait all service providers must implement.
#[async_trait]
pub trait ServiceProvider: Send + Sync {
    /// Returns the canonical service name (e.g., "s3", "sqs").
    fn service_name(&self) -> &str;

    /// Returns a human-readable provider name (e.g., "default", "v2").
    fn provider_name(&self) -> &str {
        "default"
    }

    /// Called when the service is first started.
    async fn start(&self) -> Result<(), anyhow::Error> {
        Ok(())
    }

    /// Called when the service is being stopped.
    async fn stop(&self) -> Result<(), anyhow::Error> {
        Ok(())
    }

    /// Health check for this provider. Returns Ok if healthy.
    async fn check(&self) -> Result<(), anyhow::Error> {
        Ok(())
    }

    /// Dispatch an operation to this provider.
    /// Returns the serialized HTTP response body and status code.
    async fn dispatch(&self, ctx: &RequestContext) -> Result<DispatchResponse, DispatchError>;
}

/// A serialized response from a service provider dispatch.
#[derive(Debug)]
pub struct DispatchResponse {
    /// HTTP status code
    pub status_code: u16,
    /// Response body bytes
    pub body: Bytes,
    /// Response content type
    pub content_type: String,
    /// Additional response headers
    pub headers: Vec<(String, String)>,
}

impl DispatchResponse {
    pub fn ok_json(body: impl serde::Serialize) -> Result<Self, DispatchError> {
        let bytes = serde_json::to_vec(&body)
            .map_err(|e| DispatchError::SerializationError(e.to_string()))?;
        Ok(Self {
            status_code: 200,
            body: Bytes::from(bytes),
            content_type: "application/json".to_string(),
            headers: Vec::new(),
        })
    }

    pub fn ok_xml(xml: String) -> Self {
        Self {
            status_code: 200,
            body: Bytes::from(xml.into_bytes()),
            content_type: "text/xml".to_string(),
            headers: Vec::new(),
        }
    }

    pub fn not_implemented(operation: &str) -> DispatchError {
        DispatchError::NotImplemented(operation.to_string())
    }
}
