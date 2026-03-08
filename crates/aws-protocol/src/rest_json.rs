use bytes::Bytes;
use serde_json::Value;

use crate::protocol::ProtocolError;

/// Parse a REST-JSON protocol request (RESTful path with JSON body).
/// Used by API Gateway, Lambda function URLs, and many newer AWS services.
pub fn parse_rest_json_request(
    _method: &str,
    _path: &str,
    body: &[u8],
    query_params: &std::collections::HashMap<String, String>,
) -> Result<Value, ProtocolError> {
    let mut params: Value = if body.is_empty() {
        Value::Object(Default::default())
    } else {
        serde_json::from_slice(body).map_err(|e| ProtocolError::ParseError {
            protocol: "rest-json".to_string(),
            message: e.to_string(),
        })?
    };

    // Merge query params into the params object
    if let Value::Object(map) = &mut params {
        for (k, v) in query_params {
            map.entry(k).or_insert_with(|| Value::String(v.clone()));
        }
    }

    Ok(params)
}

/// Serialize a REST-JSON protocol response.
pub fn serialize_rest_json_response(result: &Value) -> Bytes {
    Bytes::from(serde_json::to_vec(result).unwrap_or_default())
}
