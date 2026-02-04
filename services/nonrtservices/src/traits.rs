//! Non-Real-Time Service Traits
//!
//! Defines the contract for services without strict timing constraints.
//! These services may perform blocking I/O, network calls, database operations, etc.

use async_trait::async_trait;
use rtservices::NonRtContext;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ServiceError {
    #[error("Service start failed: {0}")]
    StartFailed(String),
    #[error("Service stop failed: {0}")]
    StopFailed(String),
    #[error("Service not running")]
    NotRunning,
    #[error("Service already running")]
    AlreadyRunning,
    #[error("Service not found: {0}")]
    ServiceNotFound(String),
    #[error("Service failure: {0}")]
    ServiceFailure(String),
    #[error("Invalid request: {0}")]
    InvalidRequest(String),
    #[error("Internal error: {0}")]
    Internal(String),
}

pub type ServiceResult<T> = Result<T, ServiceError>;

/// Service status information
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ServiceStatus {
    pub name: String,
    pub running: bool,
    pub started_at: Option<chrono::DateTime<chrono::Utc>>,
    pub request_count: u64,
    pub error_count: u64,
}

impl ServiceStatus {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            running: false,
            started_at: None,
            request_count: 0,
            error_count: 0,
        }
    }
}

/// Non-Real-Time Service Trait
///
/// Services implementing this trait may perform blocking operations
/// and do not have strict timing guarantees.
#[async_trait]
pub trait NonRtService: Send + Sync {
    /// Unique name of the service
    fn name(&self) -> &str;

    /// Start the service
    async fn start(&mut self) -> ServiceResult<()>;

    /// Stop the service
    async fn stop(&mut self) -> ServiceResult<()>;

    /// Check if the service is healthy
    async fn health_check(&self) -> bool;

    /// Get service status
    fn status(&self) -> ServiceStatus;

    /// Call the service with a JSON request and Non-RT context.
    /// Default implementation returns an error; override in concrete services.
    fn call(
        &self,
        _req: serde_json::Value,
        _ctx: &NonRtContext,
    ) -> Result<serde_json::Value, ServiceError> {
        Err(ServiceError::Internal("call() not implemented".to_string()))
    }
}

/// Handle for interacting with a running service
pub struct ServiceHandle {
    pub name: String,
    pub started_at: chrono::DateTime<chrono::Utc>,
}

impl ServiceHandle {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            started_at: chrono::Utc::now(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_service_status() {
        let status = ServiceStatus::new("test_service");
        assert_eq!(status.name, "test_service");
        assert!(!status.running);
        assert!(status.started_at.is_none());
    }
}
