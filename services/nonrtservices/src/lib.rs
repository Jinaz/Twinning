//! Non-RT Services Crate
//!
//! Provides non-real-time service infrastructure:
//! - Non-RT Service traits for async operations
//! - REST API service with Axum
//! - Database writer service
//!
//! # Example
//!
//! ```rust,ignore
//! use nonrtservices::{NonRtService, RestApiService};
//!
//! #[tokio::main]
//! async fn main() {
//!     let mut api = RestApiService::new("0.0.0.0:8080");
//!     api.start().await.unwrap();
//! }
//! ```

pub mod traits;
pub mod rest_api;
pub mod services;

// Re-exports
pub use traits::{NonRtService, ServiceError, ServiceResult, ServiceStatus, ServiceHandle};
pub use rest_api::{RestApiService, ApiState, create_router};
