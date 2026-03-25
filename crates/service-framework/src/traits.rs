use std::borrow::Cow;
use std::pin::Pin;
use std::sync::Mutex;

use async_trait::async_trait;
use bytes::Bytes;
use http::HeaderMap;
use thiserror::Error;

use crate::SpooledBody;

/// Type alias for the boxed async body reader passed from the gateway to
/// providers for S3 object-body requests.
///
/// Using `Box<dyn AsyncRead + Send + Unpin>` allows the gateway to pass any
/// async byte source (e.g. the raw axum body stream) without leaking axum
/// types into the service-framework crate.
pub type BodyReader = Box<dyn tokio::io::AsyncRead + Send + Unpin>;

/// The parsed request context passed to provider methods.
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
    /// Raw request bytes (for protocols that need them).
    ///
    /// `None` for S3 PutObject / UploadPart — the binary object payload is
    /// never materialised eagerly; use `spooled_body` instead.
    /// For all other protocols this is `Some(bytes)` populated by the gateway.
    pub raw_body: Option<Bytes>,
    /// Request headers (key lowercased)
    pub headers: HeaderMap,
    /// URL path
    pub path: String,
    /// HTTP method
    pub method: String,
    /// Query string parameters
    pub query_params: std::collections::HashMap<String, String>,
    /// Unique request ID for tracing (generated once by the gateway).
    pub request_id: String,
    /// Spooled request body (for large payloads, may be on disk).
    ///
    /// Wrapped in a `Mutex` so that it can be locked and consumed
    /// (via `SpooledBody::into_reader()`) even when the provider receives
    /// `ctx: &RequestContext`.
    pub spooled_body: Option<Mutex<SpooledBody>>,
    /// Streaming body reader for S3 object-body requests (PutObject, UploadPart).
    ///
    /// When the gateway sets this field it has bypassed the intermediate
    /// `SpooledBody` disk spool, allowing providers to read object data
    /// directly from the network stream and write it to persistent storage
    /// in a single pass — eliminating one full disk write per large-object PUT.
    ///
    /// Wrapped in `tokio::sync::Mutex` so the provider can hold the lock
    /// across `.await` points while streaming to disk.
    ///
    /// Priority order in providers: `body_reader` → `spooled_body` → `raw_body`.
    pub body_reader: Option<tokio::sync::Mutex<BodyReader>>,
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
            raw_body: None,
            headers: Default::default(),
            path: String::new(),
            method: String::new(),
            query_params: Default::default(),
            request_id: String::new(),
            spooled_body: None,
            body_reader: None,
        }
    }

    /// Return a slice of the raw body bytes, or an empty slice if not present.
    ///
    /// Providers that need the raw bytes should call this instead of
    /// accessing `raw_body` directly so that future lazy-materialisation
    /// changes do not break them.
    pub fn raw_body_bytes(&self) -> &[u8] {
        self.raw_body.as_deref().unwrap_or(b"")
    }
}

impl std::fmt::Debug for RequestContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RequestContext")
            .field("service", &self.service)
            .field("operation", &self.operation)
            .field("region", &self.region)
            .field("account_id", &self.account_id)
            .field("request_body", &self.request_body)
            .field("raw_body", &self.raw_body.as_ref().map(|b| b.len()))
            .field("path", &self.path)
            .field("method", &self.method)
            .field("query_params", &self.query_params)
            .field("request_id", &self.request_id)
            .field(
                "spooled_body",
                &self.spooled_body.as_ref().map(|_| "<SpooledBody>"),
            )
            .field(
                "body_reader",
                &self.body_reader.as_ref().map(|_| "<BodyReader>"),
            )
            .finish()
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
    pub content_type: Cow<'static, str>,
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
            content_type: Cow::Borrowed("application/json"),
            headers: Vec::new(),
        })
    }

    pub fn ok_xml(xml: String) -> Self {
        Self {
            status_code: 200,
            body: ResponseBody::Buffered(Bytes::from(xml.into_bytes())),
            content_type: Cow::Borrowed("text/xml"),
            headers: Vec::new(),
        }
    }

    /// Create a streaming response.
    pub fn streaming(
        stream: Pin<Box<dyn futures_core::Stream<Item = Result<Bytes, std::io::Error>> + Send>>,
        content_length: Option<u64>,
        content_type: Cow<'static, str>,
    ) -> Self {
        Self {
            status_code: 200,
            body: ResponseBody::Streaming {
                stream,
                content_length,
            },
            content_type,
            headers: Vec::new(),
        }
    }

    pub fn not_implemented(operation: &str) -> DispatchError {
        DispatchError::NotImplemented(operation.to_string())
    }
}
