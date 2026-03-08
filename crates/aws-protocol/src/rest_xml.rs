use bytes::Bytes;
use serde_json::Value;

use crate::protocol::ProtocolError;

/// Parse a REST-XML protocol request (RESTful path with XML body).
/// Used by S3 and Route53.
pub fn parse_rest_xml_request(
    method: &str,
    path: &str,
    body: &[u8],
    query_params: &std::collections::HashMap<String, String>,
) -> Result<Value, ProtocolError> {
    let mut params = serde_json::json!({
        "Method": method,
        "Path": path,
    });

    // Add query params
    if let Value::Object(map) = &mut params {
        for (k, v) in query_params {
            map.insert(k.clone(), Value::String(v.clone()));
        }
    }

    // XML body parsing (if present)
    if !body.is_empty() {
        // Store raw XML body for services that need it directly (S3)
        if let Value::Object(map) = &mut params {
            map.insert(
                "__xml_body".to_string(),
                Value::String(String::from_utf8_lossy(body).to_string()),
            );
        }
    }

    Ok(params)
}

/// Serialize a REST-XML response.
pub fn serialize_rest_xml_response(xml: &str) -> Bytes {
    Bytes::from(xml.to_string().into_bytes())
}

/// Build an S3 error XML response.
pub fn s3_error_xml(code: &str, message: &str, request_id: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<Error>
  <Code>{code}</Code>
  <Message>{message}</Message>
  <RequestId>{request_id}</RequestId>
</Error>"#
    )
}
