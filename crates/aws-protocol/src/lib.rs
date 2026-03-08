pub mod ec2;
pub mod error;
pub mod json;
pub mod protocol;
pub mod query;
pub mod rest_json;
pub mod rest_xml;

pub use error::serialize_error;
pub use protocol::{AwsProtocol, ParsedRequest, ProtocolError, SerializedResponse};
