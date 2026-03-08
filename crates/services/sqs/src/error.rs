use thiserror::Error;

#[derive(Debug, Error)]
pub enum SqsError {
    #[error("internal error: {0}")]
    Internal(String),
    #[error("not found: {0}")]
    NotFound(String),
    #[error("already exists: {0}")]
    AlreadyExists(String),
    #[error("invalid parameter: {0}")]
    InvalidParameter(String),
    #[error("not implemented: {0}")]
    NotImplemented(String),
}

impl From<SqsError> for openstack_service_framework::traits::DispatchError {
    fn from(e: SqsError) -> Self {
        openstack_service_framework::traits::DispatchError::ProviderError(e.to_string())
    }
}
