//! RT Services Crate
//!
//! Provides real-time service infrastructure with:
//! - OS-level RT verification (CPU pinning, SCHED_FIFO, mlockall)
//! - RT Service traits with bounded execution time
//! - Jitter monitoring and statistics
//!
//! # Example
//!
//! ```rust,ignore
//! use rtservices::{RtChecker, RtService, RtConfig};
//! use rtservices::services::PositionSyncService;
//!
//! // Setup RT environment
//! let mut checker = RtChecker::new();
//! checker.apply_rt_settings(0, 50)?; // CPU 0, priority 50
//!
//! // Create and run service
//! let mut service = PositionSyncService::new("pos_sync", source, 10_000);
//! service.init()?;
//! service.tick()?;
//! ```

pub mod rt_checker;
pub mod traits;
pub mod services;
pub mod engine_event;
pub mod properties;
pub mod context;
pub mod rt_guard;

// Re-exports
pub use rt_checker::{RtChecker, RtError, RtResult, JitterStats, RtStats};
pub use traits::{RtConfig, RtService, RtServiceExt};
pub use engine_event::EngineEvent;
pub use properties::Properties;
pub use context::{RtContext, NonRtContext};
pub use rt_guard::{RtGuard, RtServiceStats, RtCheckConfig, update_stats};
