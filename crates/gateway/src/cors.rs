use axum::http::{HeaderMap, HeaderValue, Method};
use openstack_config::Config;

/// Handles CORS for all requests.
pub struct CorsHandler {
    disable_headers: bool,
    #[allow(dead_code)]
    allowed_origins: Vec<String>,
    allowed_headers: Vec<String>,
}

impl CorsHandler {
    pub fn new(config: &Config) -> Self {
        let mut allowed_headers = vec![
            "Authorization".to_string(),
            "Content-Type".to_string(),
            "X-Amz-Date".to_string(),
            "X-Amz-Security-Token".to_string(),
            "X-Amz-Target".to_string(),
            "X-Amz-User-Agent".to_string(),
            "X-Amzn-RequestId".to_string(),
        ];
        allowed_headers.extend(config.cors.extra_allowed_headers.clone());

        Self {
            disable_headers: config.cors.disable_cors_headers,
            allowed_origins: config.cors.extra_allowed_origins.clone(),
            allowed_headers,
        }
    }

    /// Add CORS headers to an existing response's headers.
    pub fn add_cors_headers(&self, headers: &mut HeaderMap, origin: Option<&str>) {
        if self.disable_headers {
            return;
        }

        let origin_value = origin.unwrap_or("*");
        let _ = headers.insert(
            "access-control-allow-origin",
            HeaderValue::from_str(origin_value).unwrap_or(HeaderValue::from_static("*")),
        );
        let _ = headers.insert(
            "access-control-allow-methods",
            HeaderValue::from_static("HEAD,GET,PUT,POST,DELETE,OPTIONS,PATCH"),
        );
        let _ = headers.insert(
            "access-control-allow-headers",
            HeaderValue::from_str(&self.allowed_headers.join(","))
                .unwrap_or(HeaderValue::from_static("*")),
        );
        let _ = headers.insert(
            "access-control-expose-headers",
            HeaderValue::from_static("ETag,x-amz-version-id"),
        );
        let _ = headers.insert("access-control-max-age", HeaderValue::from_static("86400"));
    }

    /// Returns true if this is a CORS preflight request.
    pub fn is_preflight(method: &Method, headers: &HeaderMap) -> bool {
        *method == Method::OPTIONS
            && headers.contains_key("origin")
            && headers.contains_key("access-control-request-method")
    }
}
