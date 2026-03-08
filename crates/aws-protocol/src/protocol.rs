use bytes::Bytes;
use thiserror::Error;

/// The five AWS API protocol types.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AwsProtocol {
    /// Used by SQS, CloudFormation, SNS, IAM, STS, EC2
    Query,
    /// Used by DynamoDB, Kinesis, Lambda, and others
    Json,
    /// Used by API Gateway, Lambda function URLs, IAM, and others (RESTful with JSON body)
    RestJson,
    /// Used by S3 (RESTful with XML body)
    RestXml,
    /// Used by EC2 (variant of query protocol)
    Ec2,
}

impl AwsProtocol {
    pub fn from_service(service: &str) -> Self {
        match service {
            "s3" => AwsProtocol::RestXml,
            "sqs" | "sns" | "iam" | "sts" | "cloudformation" => AwsProtocol::Query,
            "ec2" => AwsProtocol::Ec2,
            "dynamodb" | "kinesis" | "firehose" | "secretsmanager" | "ssm" | "kms"
            | "cloudwatch" => AwsProtocol::Json,
            _ => AwsProtocol::RestJson,
        }
    }

    pub fn content_type(&self) -> &'static str {
        match self {
            AwsProtocol::Query | AwsProtocol::Ec2 => "text/xml",
            AwsProtocol::Json => "application/x-amz-json-1.1",
            AwsProtocol::RestJson => "application/json",
            AwsProtocol::RestXml => "application/xml",
        }
    }
}

/// A parsed AWS request, ready for dispatch.
#[derive(Debug)]
pub struct ParsedRequest {
    pub service: String,
    pub operation: String,
    pub protocol: AwsProtocol,
    /// The parsed parameters as a JSON value for uniform handling
    pub params: serde_json::Value,
    /// Raw body bytes
    pub raw_body: Bytes,
}

/// A serialized HTTP response.
#[derive(Debug)]
pub struct SerializedResponse {
    pub status_code: u16,
    pub body: Bytes,
    pub content_type: String,
    pub headers: Vec<(String, String)>,
}

impl SerializedResponse {
    pub fn ok(body: Bytes, content_type: impl Into<String>) -> Self {
        Self {
            status_code: 200,
            body,
            content_type: content_type.into(),
            headers: Vec::new(),
        }
    }
}

#[derive(Debug, Error)]
pub enum ProtocolError {
    #[error("failed to parse {protocol} request: {message}")]
    ParseError { protocol: String, message: String },
    #[error("failed to serialize {protocol} response: {message}")]
    SerializeError { protocol: String, message: String },
    #[error("unsupported protocol: {0}")]
    UnsupportedProtocol(String),
}
