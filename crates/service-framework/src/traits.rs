use std::pin::Pin;

use async_trait::async_trait;
use bytes::Bytes;
use thiserror::Error;

use crate::SpooledBody;

/// The parsed request context passed to provider methods.
#[derive(Debug)]
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
    /// Spooled request body (for large payloads, may be on disk)
    pub spooled_body: Option<SpooledBody>,
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
            spooled_body: None,
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

/// The body of a dispatch response.
///
/// `Buffered` holds a complete `Bytes` payload in memory. `Streaming` holds
/// an async byte stream and optional content length — the gateway converts it
/// to a streaming HTTP response without buffering the whole body.
pub enum ResponseBody {
    /// A fully buffered response body.
    Buffered(Bytes),
    /// A streaming response body.
    Streaming {
        stream: Pin<Box<dyn futures_core::Stream<Item = Result<Bytes, std::io::Error>> + Send>>,
        content_length: Option<u64>,
    },
}

impl std::fmt::Debug for ResponseBody {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ResponseBody::Buffered(b) => f
                .debug_tuple("ResponseBody::Buffered")
                .field(&format!("{} bytes", b.len()))
                .finish(),
            ResponseBody::Streaming { content_length, .. } => f
                .debug_struct("ResponseBody::Streaming")
                .field("content_length", content_length)
                .finish(),
        }
    }
}

/// Allow constructing a `ResponseBody::Buffered` directly from `Bytes`.
impl From<Bytes> for ResponseBody {
    fn from(bytes: Bytes) -> Self {
        ResponseBody::Buffered(bytes)
    }
}

impl ResponseBody {
    /// Borrow the buffered bytes. Panics if this is a streaming body.
    ///
    /// Useful in tests where you know the response is always buffered.
    pub fn as_bytes(&self) -> &[u8] {
        match self {
            ResponseBody::Buffered(b) => b,
            ResponseBody::Streaming { .. } => {
                panic!("as_bytes() called on a streaming ResponseBody")
            }
        }
    }

    /// Consume this body and return all data as `Bytes`.
    ///
    /// For `Buffered`, this is a no-op move. For `Streaming`, this collects
    /// the entire stream into memory (use sparingly).
    pub async fn into_bytes(self) -> Result<Bytes, std::io::Error> {
        match self {
            ResponseBody::Buffered(b) => Ok(b),
            ResponseBody::Streaming { mut stream, .. } => {
                let mut buf = Vec::new();
                loop {
                    let next = std::future::poll_fn(|cx| stream.as_mut().poll_next(cx)).await;
                    match next {
                        Some(Ok(chunk)) => buf.extend_from_slice(&chunk),
                        Some(Err(e)) => return Err(e),
                        None => break,
                    }
                }
                Ok(Bytes::from(buf))
            }
        }
    }
}

/// A serialized response from a service provider dispatch.
#[derive(Debug)]
pub struct DispatchResponse {
    /// HTTP status code
    pub status_code: u16,
    /// Response body
    pub body: ResponseBody,
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
            body: ResponseBody::Buffered(Bytes::from(bytes)),
            content_type: "application/json".to_string(),
            headers: Vec::new(),
        })
    }

    pub fn ok_xml(xml: String) -> Self {
        Self {
            status_code: 200,
            body: ResponseBody::Buffered(Bytes::from(xml.into_bytes())),
            content_type: "text/xml".to_string(),
            headers: Vec::new(),
        }
    }

    /// Create a streaming response.
    pub fn streaming(
        stream: Pin<Box<dyn futures_core::Stream<Item = Result<Bytes, std::io::Error>> + Send>>,
        content_length: Option<u64>,
        content_type: impl Into<String>,
    ) -> Self {
        Self {
            status_code: 200,
            body: ResponseBody::Streaming {
                stream,
                content_length,
            },
            content_type: content_type.into(),
            headers: Vec::new(),
        }
    }

    pub fn not_implemented(operation: &str) -> DispatchError {
        DispatchError::NotImplemented(operation.to_string())
    }
}
