use thiserror::Error;

#[derive(Debug, Error)]
pub enum ElastiCacheError {
    #[error("internal error: {0}")]
    Internal(String),
    #[error("not found: {0}")]
    NotFound(String),
    #[error("already exists: {0}")]
    AlreadyExists(String),
}

impl From<ElastiCacheError> for openstack_service_framework::traits::DispatchError {
    fn from(e: ElastiCacheError) -> Self {
        openstack_service_framework::traits::DispatchError::ProviderError(e.to_string())
    }
}
