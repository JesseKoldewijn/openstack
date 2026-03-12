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
