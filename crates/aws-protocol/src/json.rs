use bytes::Bytes;
use serde_json::Value;

use crate::protocol::ProtocolError;

/// Parse a JSON protocol request (JSON body with X-Amz-Target header).
/// Used by DynamoDB, Kinesis, Lambda, etc.
pub fn parse_json_request(
    body: &[u8],
    target: Option<&str>,
) -> Result<(String, Value), ProtocolError> {
    let params: Value = if body.is_empty() {
        Value::Object(Default::default())
    } else {
        serde_json::from_slice(body).map_err(|e| ProtocolError::ParseError {
            protocol: "json".to_string(),
            message: e.to_string(),
        })?
    };

    // Extract operation from X-Amz-Target (format: "ServiceName_Version.OperationName")
    let operation = target
        .and_then(|t| t.split('.').next_back())
        .unwrap_or("")
        .to_string();

    if operation.is_empty() {
        return Err(ProtocolError::ParseError {
            protocol: "json".to_string(),
            message: "Missing X-Amz-Target header or invalid format".to_string(),
        });
    }

    Ok((operation, params))
}

/// Serialize a JSON protocol response.
pub fn serialize_json_response(result: &Value) -> Bytes {
    Bytes::from(serde_json::to_vec(result).unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_json_request() {
        let body = br#"{"TableName": "test"}"#;
        let (op, params) = parse_json_request(body, Some("DynamoDB_20120810.GetItem")).unwrap();
        assert_eq!(op, "GetItem");
        assert_eq!(params["TableName"], "test");
    }

    #[test]
    fn test_parse_json_empty_body() {
        let (op, params) = parse_json_request(b"", Some("DynamoDB_20120810.ListTables")).unwrap();
        assert_eq!(op, "ListTables");
        assert!(params.is_object());
    }

    #[test]
    fn test_missing_target() {
        assert!(parse_json_request(b"{}", None).is_err());
    }
}
