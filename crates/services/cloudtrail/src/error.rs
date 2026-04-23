use thiserror::Error;

#[derive(Debug, Error)]
pub enum CloudTrailError {
    #[error("internal error: {0}")]
    Internal(String),
    #[error("not found: {0}")]
    NotFound(String),
    #[error("already exists: {0}")]
    AlreadyExists(String),
}

impl From<CloudTrailError> for openstack_service_framework::traits::DispatchError {
    fn from(e: CloudTrailError) -> Self {
        openstack_service_framework::traits::DispatchError::ProviderError(e.to_string())
    }
}
