use std::collections::HashMap;

use serde_json::Value;
use thiserror::Error;

use crate::guided_manifest::{NormalizedOperation, ProtocolClass};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdapterRequest {
    pub method: String,
    pub path: String,
    pub query: HashMap<String, String>,
    pub headers: HashMap<String, String>,
    pub body: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdapterResponse {
    pub status: u16,
    pub headers: HashMap<String, String>,
    pub body: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdapterError {
    pub code: String,
    pub message: String,
    pub retryable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdapterResult {
    pub request: AdapterRequest,
    pub captures: HashMap<String, String>,
}

#[derive(Debug, Error)]
pub enum AdapterExecError {
    #[error("unsupported protocol adapter")]
    UnsupportedProtocol,
    #[error("invalid JSON response body: {0}")]
    InvalidJsonBody(serde_json::Error),
    #[error("invalid XML capture path: {0}")]
    InvalidXmlCapturePath(String),
}

/// Dispatches execution to the protocol-specific adapter and returns the constructed request and captured values.
///
/// This function selects the adapter implementation for the given protocol and delegates building an AdapterRequest
/// and extracting captures from the response according to that protocol's semantics.
///
/// # Returns
///
/// `Ok(AdapterResult)` containing the constructed `AdapterRequest` and a map of captured values on success, or
/// `Err(AdapterExecError)` if the selected adapter fails (e.g., invalid JSON capture paths or unsupported protocol).
///
/// # Examples
///
/// ```no_run
/// use std::collections::HashMap;
/// // Construct minimal inputs (fields shown for clarity; use appropriate constructors in real code)
/// let protocol = ProtocolClass::RestJson;
/// let operation = NormalizedOperation {
///     method: "POST".to_string(),
///     path: "/".to_string(),
///     query: HashMap::new(),
///     headers: HashMap::new(),
///     body: Some("{}".to_string()),
/// };
/// let response = AdapterResponse {
///     status: 200,
///     headers: HashMap::new(),
///     body: "{}".to_string(),
/// };
/// let capture_sources: HashMap<String, String> = HashMap::new();
///
/// let result = execute_protocol_adapter(protocol, &operation, &response, &capture_sources);
/// match result {
///     Ok(adapter_result) => {
///         // use adapter_result.request and adapter_result.captures
///     }
///     Err(e) => {
///         // handle adapter execution error
///     }
/// }
/// ```
pub fn execute_protocol_adapter(
    protocol: ProtocolClass,
    operation: &NormalizedOperation,
    response: &AdapterResponse,
    capture_sources: &HashMap<String, String>,
) -> Result<AdapterResult, AdapterExecError> {
    match protocol {
        ProtocolClass::Query => execute_query(operation, response, capture_sources),
        ProtocolClass::JsonTarget => execute_json_target(operation, response, capture_sources),
        ProtocolClass::RestXml => execute_rest_xml(operation, response, capture_sources),
        ProtocolClass::RestJson => execute_rest_json(operation, response, capture_sources),
    }
}

/// Map an adapter response into a normalized AdapterError when the response is an error.
///
/// Returns `Some(AdapterError)` when `response.status` is 400 or greater; `None` otherwise.
/// The returned error's `retryable` flag is `true` for status 429 or any 5xx status. The
/// error `code` is determined by the `protocol`.
///
/// # Examples
///
/// ```
/// use std::collections::HashMap;
///
/// // construct a minimal AdapterResponse for the example
/// let response = crate::protocol_adapters::AdapterResponse {
///     status: 503,
///     headers: HashMap::new(),
///     body: "service unavailable".to_string(),
/// };
///
/// let err = crate::protocol_adapters::normalize_error(
///     crate::protocol_adapters::ProtocolClass::RestJson,
///     &response,
/// ).unwrap();
///
/// assert_eq!(err.code, "rest_json_error");
/// assert_eq!(err.message, "service unavailable");
/// assert!(err.retryable);
/// ```
pub fn normalize_error(
    protocol: ProtocolClass,
    response: &AdapterResponse,
) -> Option<AdapterError> {
    if response.status < 400 {
        return None;
    }

    let retryable = response.status == 429 || response.status >= 500;
    let code = match protocol {
        ProtocolClass::Query => "query_error",
        ProtocolClass::JsonTarget => "json_target_error",
        ProtocolClass::RestXml => "rest_xml_error",
        ProtocolClass::RestJson => "rest_json_error",
    }
    .to_string();

    Some(AdapterError {
        code,
        message: response.body.clone(),
        retryable,
    })
}

/// Builds a Query-protocol AdapterRequest from the given operation, ensures a default
/// Content-Type of `application/x-www-form-urlencoded` when missing, and extracts captures
/// from the response body using XML/tag or query-style rules.
///
/// Returns an AdapterResult containing the constructed request and a map of captured values.
///
/// # Examples
///
/// ```
/// use std::collections::HashMap;
///
/// // Minimal operation and response setup
/// let mut headers = HashMap::new();
/// let operation = NormalizedOperation {
///     method: "POST".to_string(),
///     path: "/".to_string(),
///     query: HashMap::new(),
///     headers: headers.clone(),
///     body: Some("{}".to_string()),
/// };
/// let response = AdapterResponse {
///     status: 200,
///     headers: HashMap::new(),
///     body: "<QueueUrl>abc</QueueUrl>".to_string(),
/// };
/// let capture_sources: HashMap<String, String> = [("queue_url".to_string(), "QueueUrl".to_string())].into_iter().collect();
///
/// let result = execute_query(&operation, &response, &capture_sources).unwrap();
/// // default content-type is set
/// assert_eq!(result.request.headers.get("content-type").map(String::as_str), Some("application/x-www-form-urlencoded"));
/// // capture extracted from XML body
/// assert_eq!(result.captures.get("queue_url").map(String::as_str), Some("abc"));
/// ```
fn execute_query(
    operation: &NormalizedOperation,
    response: &AdapterResponse,
    capture_sources: &HashMap<String, String>,
) -> Result<AdapterResult, AdapterExecError> {
    let mut headers = operation.headers.clone();
    headers
        .entry("content-type".to_string())
        .or_insert_with(|| "application/x-www-form-urlencoded".to_string());

    let request = AdapterRequest {
        method: operation.method.clone(),
        path: operation.path.clone(),
        query: operation.query.clone(),
        headers,
        body: operation.body.clone(),
    };

    let captures = capture_query_or_xml_body(&response.body, capture_sources);
    Ok(AdapterResult { request, captures })
}

/// Builds an AdapterRequest for the JSON target protocol, ensures protocol-specific default headers, and captures values from the response JSON body.
///
/// The request returned in the result has `content-type` set to `application/x-amz-json-1.1` and `x-amz-target` set to `OpenStackStudio.Execute` when those headers are not present on the provided operation.
///
/// # Returns
///
/// `AdapterResult` containing the constructed `AdapterRequest` and a map of captured values extracted from the response body.
///
/// # Examples
///
/// ```
/// use std::collections::HashMap;
///
/// // prepare a minimal operation and response
/// let mut op = NormalizedOperation {
///     method: "POST".to_string(),
///     path: "/".to_string(),
///     query: HashMap::new(),
///     headers: HashMap::new(),
///     body: Some("{\"StreamARN\":{\"value\":\"arn:aws:kinesis:...\"}}".to_string()),
/// };
/// let resp = AdapterResponse {
///     status: 200,
///     headers: HashMap::new(),
///     body: op.body.clone().unwrap(),
/// };
/// let mut sources = HashMap::new();
/// sources.insert("stream_arn".to_string(), "StreamARN.value".to_string());
///
/// let result = execute_json_target(&op, &resp, &sources).unwrap();
/// assert_eq!(result.request.headers.get("x-amz-target").unwrap(), "OpenStackStudio.Execute");
/// assert!(result.captures.contains_key("stream_arn"));
/// ```
fn execute_json_target(
    operation: &NormalizedOperation,
    response: &AdapterResponse,
    capture_sources: &HashMap<String, String>,
) -> Result<AdapterResult, AdapterExecError> {
    let mut headers = operation.headers.clone();
    headers
        .entry("content-type".to_string())
        .or_insert_with(|| "application/x-amz-json-1.1".to_string());
    headers
        .entry("x-amz-target".to_string())
        .or_insert_with(|| "OpenStackStudio.Execute".to_string());

    let request = AdapterRequest {
        method: operation.method.clone(),
        path: operation.path.clone(),
        query: operation.query.clone(),
        headers,
        body: operation.body.clone(),
    };

    let captures = capture_json_body(&response.body, capture_sources)?;
    Ok(AdapterResult { request, captures })
}

/// Builds an AdapterRequest from the provided operation and extracts captures from an XML-like response body.
///
/// # Examples
///
/// ```
/// use std::collections::HashMap;
///
/// let operation = NormalizedOperation {
///     method: "POST".into(),
///     path: "/".into(),
///     query: HashMap::new(),
///     headers: HashMap::new(),
///     body: Some("{}".into()),
/// };
///
/// let response = AdapterResponse {
///     status: 200,
///     headers: HashMap::new(),
///     body: "<QueueUrl>abc</QueueUrl>".into(),
/// };
///
/// let mut capture_sources = HashMap::new();
/// capture_sources.insert("queue_url".into(), "QueueUrl".into());
///
/// let result = execute_rest_xml(&operation, &response, &capture_sources).unwrap();
/// assert_eq!(result.request.path, "/");
/// assert_eq!(result.captures.get("queue_url").map(String::as_str), Some("abc"));
/// ```
///
/// # Returns
///
/// `Ok(AdapterResult)` containing the constructed request and any captured values extracted from the response body.
fn execute_rest_xml(
    operation: &NormalizedOperation,
    response: &AdapterResponse,
    capture_sources: &HashMap<String, String>,
) -> Result<AdapterResult, AdapterExecError> {
    let request = AdapterRequest {
        method: operation.method.clone(),
        path: operation.path.clone(),
        query: operation.query.clone(),
        headers: operation.headers.clone(),
        body: operation.body.clone(),
    };

    let captures = capture_query_or_xml_body(&response.body, capture_sources);
    Ok(AdapterResult { request, captures })
}

/// Constructs the AdapterRequest for a REST-JSON operation, ensuring a default
/// `Content-Type: application/json` header when absent, and captures values from
/// the JSON response body.
///
/// # Returns
/// An `AdapterResult` containing the constructed request and a map of captured
/// values, or an `AdapterExecError::InvalidJsonBody` if the response body is not
/// valid JSON.
///
/// # Examples
///
/// ```
/// // Build minimal operation and response (types come from the surrounding crate)
/// let mut op = NormalizedOperation {
///     method: "POST".to_string(),
///     path: "/".to_string(),
///     query: std::collections::HashMap::new(),
///     headers: std::collections::HashMap::new(),
///     body: Some("{}".to_string()),
/// };
/// let resp = AdapterResponse {
///     status: 200,
///     headers: std::collections::HashMap::new(),
///     body: r#"{"id":"123"}"#.to_string(),
/// };
/// let capture_sources = std::collections::HashMap::from([("id".to_string(), "id".to_string())]);
///
/// let result = execute_rest_json(&op, &resp, &capture_sources).unwrap();
/// assert_eq!(result.request.headers.get("content-type").unwrap(), "application/json");
/// assert_eq!(result.captures.get("id").unwrap(), "123");
/// ```
fn execute_rest_json(
    operation: &NormalizedOperation,
    response: &AdapterResponse,
    capture_sources: &HashMap<String, String>,
) -> Result<AdapterResult, AdapterExecError> {
    let mut headers = operation.headers.clone();
    headers
        .entry("content-type".to_string())
        .or_insert_with(|| "application/json".to_string());

    let request = AdapterRequest {
        method: operation.method.clone(),
        path: operation.path.clone(),
        query: operation.query.clone(),
        headers,
        body: operation.body.clone(),
    };

    let captures = capture_json_body(&response.body, capture_sources)?;
    Ok(AdapterResult { request, captures })
}

/// Extracts values from an XML- or query-like response body using tag names provided in `capture_sources`.
///
/// `capture_sources` maps capture names to XML tag names to search for in `body`. For each entry,
/// if a matching `<tag>...</tag>` pair is found, the inner text is inserted into the returned map
/// under the capture name.
///
/// # Examples
///
/// ```
/// use std::collections::HashMap;
///
/// let body = r#"<Response><QueueUrl>abc</QueueUrl></Response>"#;
/// let mut sources = HashMap::new();
/// sources.insert("queue_url".to_string(), "QueueUrl".to_string());
///
/// let captures = capture_query_or_xml_body(body, &sources);
/// assert_eq!(captures.get("queue_url").map(|s| s.as_str()), Some("abc"));
/// ```
fn capture_query_or_xml_body(
    body: &str,
    capture_sources: &HashMap<String, String>,
) -> HashMap<String, String> {
    let mut captures = HashMap::new();
    for (name, source) in capture_sources {
        if let Some(value) = capture_xml_tag(body, source) {
            captures.insert(name.clone(), value);
        }
    }
    captures
}

/// Extracts named values from a JSON payload using dot-separated paths.
///
/// Parses `body` as JSON and, for each entry in `capture_sources` (name -> dot-separated path),
/// traverses JSON objects by the path segments. If the path resolves to a string, that string is
/// inserted into the result under `name`. If it resolves to any other JSON value, the value's
/// string representation is inserted. Paths that are empty or don't resolve to a value are
/// skipped. Returns `AdapterExecError::InvalidJsonBody` if `body` is not valid JSON.
///
/// # Examples
///
/// ```
/// use std::collections::HashMap;
///
/// let body = r#"{"a":{"b":"v"},"x":1}"#;
/// let mut sources = HashMap::new();
/// sources.insert("val".to_string(), "a.b".to_string());
/// sources.insert("num".to_string(), "x".to_string());
///
/// let captures = crate::capture_json_body(body, &sources).unwrap();
/// assert_eq!(captures.get("val").map(String::as_str), Some("v"));
/// assert_eq!(captures.get("num").map(String::as_str), Some("1"));
/// ```
fn capture_json_body(
    body: &str,
    capture_sources: &HashMap<String, String>,
) -> Result<HashMap<String, String>, AdapterExecError> {
    let payload: Value = serde_json::from_str(body).map_err(AdapterExecError::InvalidJsonBody)?;
    let mut captures = HashMap::new();

    for (name, source) in capture_sources {
        let segments = source
            .split('.')
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>();
        if segments.is_empty() {
            continue;
        }

        let mut current = &payload;
        for segment in &segments {
            match current {
                Value::Object(map) => match map.get(*segment) {
                    Some(next) => current = next,
                    None => {
                        current = &Value::Null;
                        break;
                    }
                },
                _ => {
                    current = &Value::Null;
                    break;
                }
            }
        }

        match current {
            Value::Null => {}
            Value::String(s) => {
                captures.insert(name.clone(), s.clone());
            }
            other => {
                captures.insert(name.clone(), other.to_string());
            }
        }
    }

    Ok(captures)
}

/// Extracts the inner text of the first occurrence of an XML-like tag from the given body.
///
/// The function looks for a matching `<tag>...</tag>` pair and returns the text between them.
///
/// # Examples
///
/// ```
/// let body = "<Response><QueueUrl>abc</QueueUrl></Response>";
/// assert_eq!(capture_xml_tag(body, "QueueUrl"), Some("abc".to_string()));
/// assert_eq!(capture_xml_tag(body, "Missing"), None);
/// ```
fn capture_xml_tag(body: &str, tag: &str) -> Option<String> {
    let start_marker = format!("<{tag}>");
    let end_marker = format!("</{tag}>");

    let start = body.find(&start_marker)? + start_marker.len();
    let end = body[start..].find(&end_marker)? + start;
    Some(body[start..end].to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Creates a basic `NormalizedOperation` configured for tests: POST to `/` with empty headers and query and a `"{}"` body.
    ///
    /// # Examples
    ///
    /// ```
    /// let op = operation();
    /// assert_eq!(op.method, "POST");
    /// assert_eq!(op.path, "/");
    /// assert!(op.headers.is_empty());
    /// assert!(op.query.is_empty());
    /// assert_eq!(op.body.as_deref(), Some("{}"));
    /// ```
    fn operation() -> NormalizedOperation {
        NormalizedOperation {
            method: "POST".to_string(),
            path: "/".to_string(),
            headers: HashMap::new(),
            query: HashMap::new(),
            body: Some("{}".to_string()),
        }
    }

    #[test]
    fn query_adapter_sets_form_content_type_and_extracts_xml_capture() {
        let mut capture_sources = HashMap::new();
        capture_sources.insert("queue_url".to_string(), "QueueUrl".to_string());

        let response = AdapterResponse {
            status: 200,
            headers: HashMap::new(),
            body: "<CreateQueueResponse><QueueUrl>abc</QueueUrl></CreateQueueResponse>".to_string(),
        };

        let result = execute_protocol_adapter(
            ProtocolClass::Query,
            &operation(),
            &response,
            &capture_sources,
        )
        .expect("query adapter should succeed");

        assert_eq!(
            result
                .request
                .headers
                .get("content-type")
                .map(String::as_str),
            Some("application/x-www-form-urlencoded")
        );
        assert_eq!(
            result.captures.get("queue_url").map(String::as_str),
            Some("abc")
        );
    }

    #[test]
    fn json_target_adapter_sets_target_header_and_extracts_json_capture() {
        let mut capture_sources = HashMap::new();
        capture_sources.insert(
            "stream_arn".to_string(),
            "StreamDescription.StreamARN".to_string(),
        );

        let response = AdapterResponse {
            status: 200,
            headers: HashMap::new(),
            body: "{\"StreamDescription\": {\"StreamARN\": \"arn:aws:kinesis:...\"}}".to_string(),
        };

        let result = execute_protocol_adapter(
            ProtocolClass::JsonTarget,
            &operation(),
            &response,
            &capture_sources,
        )
        .expect("json target adapter should succeed");

        assert_eq!(
            result
                .request
                .headers
                .get("x-amz-target")
                .map(String::as_str),
            Some("OpenStackStudio.Execute")
        );
        assert_eq!(
            result.captures.get("stream_arn").map(String::as_str),
            Some("arn:aws:kinesis:...")
        );
    }

    #[test]
    fn rest_json_adapter_normalizes_errors_with_retryability() {
        let response = AdapterResponse {
            status: 503,
            headers: HashMap::new(),
            body: "service unavailable".to_string(),
        };

        let error = normalize_error(ProtocolClass::RestJson, &response)
            .expect("error response should be normalized");

        assert_eq!(error.code, "rest_json_error");
        assert!(error.retryable);
    }
}
