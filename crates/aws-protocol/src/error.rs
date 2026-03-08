use bytes::Bytes;

use crate::protocol::AwsProtocol;

/// Serialize an AWS error response for any protocol type.
pub fn serialize_error(
    protocol: &AwsProtocol,
    code: &str,
    message: &str,
    http_status: u16,
    request_id: &str,
) -> (u16, Bytes, &'static str) {
    match protocol {
        AwsProtocol::Query | AwsProtocol::Ec2 => {
            let xml = format!(
                r#"<?xml version="1.0" encoding="UTF-8"?>
<ErrorResponse>
  <Error>
    <Type>Sender</Type>
    <Code>{code}</Code>
    <Message>{message}</Message>
  </Error>
  <RequestId>{request_id}</RequestId>
</ErrorResponse>"#
            );
            (http_status, Bytes::from(xml.into_bytes()), "text/xml")
        }
        AwsProtocol::Json => {
            let body = serde_json::json!({
                "__type": code,
                "message": message,
            });
            (
                http_status,
                Bytes::from(serde_json::to_vec(&body).unwrap_or_default()),
                "application/x-amz-json-1.1",
            )
        }
        AwsProtocol::RestJson => {
            let body = serde_json::json!({
                "code": code,
                "message": message,
            });
            (
                http_status,
                Bytes::from(serde_json::to_vec(&body).unwrap_or_default()),
                "application/json",
            )
        }
        AwsProtocol::RestXml => {
            let xml = format!(
                r#"<?xml version="1.0" encoding="UTF-8"?>
<Error>
  <Code>{code}</Code>
  <Message>{message}</Message>
  <RequestId>{request_id}</RequestId>
</Error>"#
            );
            (
                http_status,
                Bytes::from(xml.into_bytes()),
                "application/xml",
            )
        }
    }
}

/// Well-known AWS error codes and their default HTTP status codes.
pub fn error_http_status(code: &str) -> u16 {
    match code {
        "AccessDeniedException" | "AccessDenied" => 403,
        "AuthFailure" | "InvalidSignatureException" => 403,
        "InvalidParameterValue"
        | "InvalidParameterException"
        | "ValidationException"
        | "MalformedQueryString" => 400,
        "NotFoundException"
        | "NoSuchBucket"
        | "NoSuchKey"
        | "QueueDoesNotExist"
        | "ResourceNotFoundException" => 404,
        "AlreadyExistsException"
        | "BucketAlreadyOwnedByYou"
        | "QueueAlreadyExists"
        | "ResourceInUseException" => 409,
        "ThrottlingException" | "RequestLimitExceeded" => 429,
        "InternalError" | "InternalFailure" | "ServiceUnavailable" => 500,
        "NotImplemented" | "UnsupportedOperation" => 501,
        _ => 400,
    }
}
