use serde_json::{Map, Value};

use crate::protocol::ProtocolError;

/// Parse an EC2 protocol request (variant of query protocol used by EC2).
pub fn parse_ec2_request(body: &[u8]) -> Result<(String, Value), ProtocolError> {
    let params: Vec<(String, String)> =
        serde_urlencoded::from_bytes(body).map_err(|e| ProtocolError::ParseError {
            protocol: "ec2".to_string(),
            message: e.to_string(),
        })?;

    let action = params
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("Action"))
        .map(|(_, v)| v.clone())
        .ok_or_else(|| ProtocolError::ParseError {
            protocol: "ec2".to_string(),
            message: "Missing 'Action' parameter".to_string(),
        })?;

    let mut map = Map::new();
    for (key, value) in &params {
        if !key.eq_ignore_ascii_case("Action") && !key.eq_ignore_ascii_case("Version") {
            map.insert(key.clone(), Value::String(value.clone()));
        }
    }

    Ok((action, Value::Object(map)))
}

/// Serialize an EC2 protocol response (XML with EC2-specific namespace).
pub fn serialize_ec2_response(operation: &str, result: &Value, request_id: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<{op}Response xmlns="http://ec2.amazonaws.com/doc/2016-11-15/">
  <requestId>{request_id}</requestId>
  {result}
</{op}Response>"#,
        op = operation,
        request_id = request_id,
        result = json_to_ec2_xml(result),
    )
}

fn json_to_ec2_xml(value: &Value) -> String {
    match value {
        Value::Object(map) => map
            .iter()
            .map(|(k, v)| {
                // Convert camelCase to EC2 XML element naming
                format!("<{k}>{}</{k}>", json_to_ec2_xml(v), k = k)
            })
            .collect::<Vec<_>>()
            .join("\n  "),
        Value::Array(items) => items
            .iter()
            .map(|v| format!("<item>{}</item>", json_to_ec2_xml(v)))
            .collect::<Vec<_>>()
            .join("\n  "),
        Value::String(s) => s.replace('<', "&lt;").replace('>', "&gt;"),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Null => String::new(),
    }
}
