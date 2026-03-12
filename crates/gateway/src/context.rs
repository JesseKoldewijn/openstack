use std::collections::HashMap;

use bytes::Bytes;

/// Full context for an in-flight AWS request as it passes through the handler chain.
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
}

impl RequestContext {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(&name.to_lowercase()).map(|s| s.as_str())
    }

    pub fn to_service_request_context(
        &self,
    ) -> openstack_service_framework::traits::RequestContext {
        let mut headers = std::collections::HashMap::with_capacity(self.headers.len());
        for (k, v) in &self.headers {
            headers.insert(k.clone(), v.clone());
        }
        let mut query_params = std::collections::HashMap::with_capacity(self.query_params.len());
        for (k, v) in &self.query_params {
            query_params.insert(k.clone(), v.clone());
        }

        openstack_service_framework::traits::RequestContext {
            service: self.service.clone(),
            operation: self.operation.clone(),
            region: self.region.clone(),
            account_id: self.account_id.clone(),
            request_body: self.params.clone(),
            raw_body: self.raw_body.clone(),
            headers,
            path: self.path.clone(),
            method: self.method.clone(),
            query_params,
        }
    }
}
