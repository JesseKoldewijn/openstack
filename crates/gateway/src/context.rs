use std::collections::HashMap;

use bytes::Bytes;
use openstack_service_framework::SpooledBody;

/// Full context for an in-flight AWS request as it passes through the handler chain.
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
    /// The access key ID from the Authorization header
    pub access_key: String,
    /// The parsed AWS protocol used
    pub protocol: openstack_aws_protocol::AwsProtocol,
    /// Parsed request parameters as a unified JSON value
    pub params: serde_json::Value,
    /// Raw request body bytes
    pub raw_body: Bytes,
    /// Request headers (keys lowercased)
    pub headers: HashMap<String, String>,
    /// URL path
    pub path: String,
    /// HTTP method
    pub method: String,
    /// Parsed query string parameters
    pub query_params: HashMap<String, String>,
    /// Unique request ID for tracing
    pub request_id: String,
    /// Spooled request body (for large payloads, may be on disk)
    pub spooled_body: Option<SpooledBody>,
}

impl RequestContext {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(&name.to_lowercase()).map(|s| s.as_str())
    }

    /// Convert to the service-framework `RequestContext`, consuming self
    /// (because `SpooledBody` is not `Clone`).
    pub fn to_service_request_context(self) -> openstack_service_framework::traits::RequestContext {
        openstack_service_framework::traits::RequestContext {
            service: self.service,
            operation: self.operation,
            region: self.region,
            account_id: self.account_id,
            request_body: self.params,
            raw_body: self.raw_body,
            headers: self.headers,
            path: self.path,
            method: self.method,
            query_params: self.query_params,
            spooled_body: self.spooled_body,
        }
    }
}
