use serde_json::{Map, Value};

use crate::protocol::ProtocolError;

/// Parse a query protocol request (form-urlencoded body with Action parameter).
/// Used by SQS, SNS, IAM, STS, CloudFormation.
pub fn parse_query_request(body: &[u8]) -> Result<(String, Value), ProtocolError> {
    let params: Vec<(String, String)> =
        serde_urlencoded::from_bytes(body).map_err(|e| ProtocolError::ParseError {
            protocol: "query".to_string(),
            message: e.to_string(),
        })?;

    let action = params
        .iter()
        .find(|(k, _)| k == "Action")
        .map(|(_, v)| v.clone())
        .ok_or_else(|| ProtocolError::ParseError {
            protocol: "query".to_string(),
            message: "Missing 'Action' parameter".to_string(),
        })?;

    // Build a JSON object from the parameters (excluding Action and Version)
    let mut map = Map::new();
    for (key, value) in &params {
        if key != "Action" && key != "Version" {
            // Handle nested structures like Attribute.1.Name / Attribute.1.Value
            insert_nested(&mut map, key, value);
        }
    }

    Ok((action, Value::Object(map)))
}

/// Insert a potentially-nested AWS query parameter into a JSON map.
/// Handles formats like: Attribute.1.Name=foo, Tag.member.1.Key=bar, etc.
fn insert_nested(map: &mut Map<String, Value>, key: &str, value: &str) {
    // Simple case: no dots, just insert directly
    if !key.contains('.') {
        map.insert(key.to_string(), Value::String(value.to_string()));
        return;
    }
    // For now, insert the raw key (full expansion is complex and service-specific)
    map.insert(key.to_string(), Value::String(value.to_string()));
}

/// Serialize a response using the XML query protocol format.
pub fn serialize_query_response(operation: &str, result: &Value, request_id: &str) -> String {
    let result_xml = json_to_xml(result);
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<{op}Response xmlns="https://sqs.amazonaws.com/doc/2012-11-05/">
  <{op}Result>
{result}
  </{op}Result>
  <ResponseMetadata>
    <RequestId>{request_id}</RequestId>
  </ResponseMetadata>
</{op}Response>"#,
        op = operation,
        result = result_xml,
        request_id = request_id
    )
}

fn json_to_xml(value: &Value) -> String {
    match value {
        Value::Object(map) => map
            .iter()
            .map(|(k, v)| format!("<{k}>{}</{k}>", json_to_xml(v), k = k))
            .collect::<Vec<_>>()
            .join("\n"),
        Value::Array(items) => items.iter().map(json_to_xml).collect::<Vec<_>>().join("\n"),
        Value::String(s) => escape_xml(s),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Null => String::new(),
    }
}

fn escape_xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_query_request() {
        let body = b"Action=CreateQueue&QueueName=test-queue&Version=2012-11-05";
        let (action, params) = parse_query_request(body).unwrap();
        assert_eq!(action, "CreateQueue");
        assert_eq!(params["QueueName"], "test-queue");
    }

    #[test]
    fn test_missing_action() {
        let body = b"QueueName=test-queue";
        assert!(parse_query_request(body).is_err());
    }
}
