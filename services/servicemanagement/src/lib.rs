//! Service Management Crate
//!
//! Provides central service management with:
//! - Hierarchical plugin pattern for service organization
//! - Service registry for RT and Non-RT services
//! - Service manager for lifecycle coordination
//!
//! # Example
//!
//! ```rust,ignore
//! use servicemanagement::{ServiceManager, ManagerConfig};
//! use rtservices::services::PositionSyncService;
//! use nonrtservices::RestApiService;
//!
//! #[tokio::main]
//! async fn main() {
//!     let manager = ServiceManager::new();
//!
//!     // Register services
//!     manager.register_rt_service(Box::new(position_sync)).await;
//!     manager.register_non_rt_service(Box::new(rest_api)).await;
//!
//!     // Start all services
//!     manager.start_all().await.unwrap();
//!
//!     // Print hierarchy
//!     manager.print_hierarchy().await;
//! }
//! ```

pub mod hierarchy;
pub mod registry;
pub mod manager;
pub mod dispatcher;
pub mod sample_engine;

// Re-exports
pub use hierarchy::{ServiceHierarchy, ServiceNode, ServiceType, TreeNode};
pub use registry::ServiceRegistry;
pub use manager::{ServiceManager, ManagerConfig, ManagerStatus, RtRegistration};
pub use dispatcher::NonRtDispatcher;
pub use sample_engine::SampleEngine;
