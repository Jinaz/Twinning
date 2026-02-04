//! Service Manager
//!
//! Central manager for RT and Non-RT services with hierarchical organization.
//! Handles service lifecycle, RT loop execution with per-service budget checking,
//! and coordination between RT and Non-RT layers.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use rtservices::{
    RtChecker, RtService,
    Properties, EngineEvent, RtContext,
    RtGuard, RtServiceStats, RtCheckConfig, update_stats,
};
use nonrtservices::NonRtService;

use crate::hierarchy::{ServiceHierarchy, ServiceNode};
use crate::registry::ServiceRegistry;

/// Manager configuration
#[derive(Debug, Clone)]
pub struct ManagerConfig {
    /// RT loop tick interval
    pub rt_tick_interval: Duration,
    /// RT CPU core to use (if any)
    pub rt_cpu_core: Option<usize>,
    /// RT FIFO priority (if any)
    pub rt_fifo_priority: Option<i32>,
    /// Enable RT features (may require root)
    pub enable_rt_features: bool,
    /// RT check configuration for overrun detection
    pub rt_check_config: RtCheckConfig,
}

impl Default for ManagerConfig {
    fn default() -> Self {
        Self {
            rt_tick_interval: Duration::from_millis(10),
            rt_cpu_core: None,
            rt_fifo_priority: None,
            enable_rt_features: false,
            rt_check_config: RtCheckConfig::default(),
        }
    }
}

/// Registration info for an RT service
#[derive(Debug, Clone)]
pub struct RtRegistration {
    /// Time budget for this service per tick
    pub budget: Duration,
}

/// Internal entry for an RT service with its stats
struct RtEntry {
    service: Box<dyn RtService>,
    registration: RtRegistration,
    stats: RtServiceStats,
}

/// Service Manager - coordinates all services
pub struct ServiceManager {
    /// Configuration
    config: ManagerConfig,
    /// RT service entries (owned, not behind async locks for RT safety)
    rt_entries: Vec<RtEntry>,
    /// Service registry (for Non-RT services)
    registry: Arc<ServiceRegistry>,
    /// Service hierarchy
    hierarchy: Arc<ServiceHierarchy>,
    /// RT checker for monitoring
    rt_checker: RtChecker,
    /// Shared properties (atomic, lock-free)
    properties: Arc<Properties>,
    /// Event channel sender
    event_tx: crossbeam_channel::Sender<EngineEvent>,
    /// Event channel receiver
    event_rx: crossbeam_channel::Receiver<EngineEvent>,
    /// Running state
    running: bool,
    /// Stop flag for RT thread
    stop_flag: Arc<AtomicBool>,
    /// RT thread handle
    rt_thread: Option<std::thread::JoinHandle<()>>,
}

impl ServiceManager {
    /// Create a new service manager
    pub fn new() -> Self {
        Self::with_config(ManagerConfig::default())
    }

    /// Create with specific configuration
    pub fn with_config(config: ManagerConfig) -> Self {
        let hierarchy = ServiceHierarchy::new();
        let (event_tx, event_rx) = crossbeam_channel::bounded(1024);

        Self {
            config,
            rt_entries: Vec::new(),
            registry: Arc::new(ServiceRegistry::new()),
            hierarchy: Arc::new(hierarchy),
            rt_checker: RtChecker::new(),
            properties: Arc::new(Properties::new()),
            event_tx,
            event_rx,
            running: false,
            stop_flag: Arc::new(AtomicBool::new(false)),
            rt_thread: None,
        }
    }

    /// Get shared properties
    pub fn props(&self) -> Arc<Properties> {
        self.properties.clone()
    }

    /// Get event sender (for injecting events)
    pub fn event_tx(&self) -> crossbeam_channel::Sender<EngineEvent> {
        self.event_tx.clone()
    }

    /// Get event receiver (for processing events in engine loop)
    pub fn event_rx(&self) -> crossbeam_channel::Receiver<EngineEvent> {
        self.event_rx.clone()
    }

    /// Get the service registry
    pub fn registry(&self) -> Arc<ServiceRegistry> {
        self.registry.clone()
    }

    /// Get the service hierarchy
    pub fn hierarchy(&self) -> Arc<ServiceHierarchy> {
        self.hierarchy.clone()
    }

    /// Create a NonRtDispatcher backed by the registry's Non-RT service map.
    pub fn nonrt_dispatcher(&self, capacity: usize) -> crate::dispatcher::NonRtDispatcher {
        let services = self.registry.non_rt_services_map();
        crate::dispatcher::NonRtDispatcher::new(capacity, services)
    }

    /// Set RT check configuration
    pub fn set_rtcheck_config(&mut self, cfg: RtCheckConfig) {
        self.config.rt_check_config = cfg;
    }

    /// Register an RT service with its registration info
    pub fn register_rt(&mut self, service: Box<dyn RtService>, registration: RtRegistration) {
        let name = service.name().to_string();
        log::info!("Registered RT service: {} (budget={:?})", name, registration.budget);
        self.rt_entries.push(RtEntry {
            service,
            registration,
            stats: RtServiceStats::default(),
        });
    }

    /// Register a Non-RT service
    pub async fn register_nonrt(&self, service: Box<dyn NonRtService>) {
        let name = service.name().to_string();
        self.registry.register_non_rt(service).await;

        let node = ServiceNode::non_rt(&name);
        self.hierarchy.register("root", node).await;
    }

    /// Initialize default hierarchy structure
    pub async fn init_default_hierarchy(&self) {
        self.hierarchy.create_container("root", "realtime").await;
        self.hierarchy.create_container("root", "nonrealtime").await;
        self.hierarchy.create_container("root", "system").await;
    }

    /// Execute one RT tick: iterate all RT services with budget checking.
    /// This is meant to be called from a dedicated RT thread.
    pub fn tick_rt(&mut self) {
        let ctx = RtContext {
            props: &self.properties,
            event_tx: &self.event_tx,
        };

        for entry in &mut self.rt_entries {
            if entry.stats.disabled {
                continue;
            }

            let guard = RtGuard::new();
            if let Err(e) = entry.service.tick(&ctx) {
                log::warn!("RT service '{}' tick error: {}", entry.service.name(), e);
            }
            let elapsed = guard.elapsed();

            update_stats(
                &mut entry.stats,
                elapsed,
                entry.registration.budget,
                &self.config.rt_check_config,
            );
        }
    }

    /// Get a snapshot of RT service stats
    pub fn rt_stats_snapshot(&self) -> Vec<(String, RtServiceStats, Duration)> {
        self.rt_entries
            .iter()
            .map(|e| {
                (
                    e.service.name().to_string(),
                    e.stats.clone(),
                    e.registration.budget,
                )
            })
            .collect()
    }

    /// Start all services
    pub async fn start_all(&mut self) -> Result<(), String> {
        if self.running {
            return Err("Manager already running".to_string());
        }

        log::info!("Starting Service Manager...");

        // Apply RT settings if enabled
        if self.config.enable_rt_features {
            if let Some(core) = self.config.rt_cpu_core {
                if let Err(e) = self.rt_checker.pin_to_cpu(core) {
                    log::warn!("Failed to pin CPU: {}", e);
                }
            }
            if let Some(priority) = self.config.rt_fifo_priority {
                if let Err(e) = self.rt_checker.set_fifo_priority(priority) {
                    log::warn!("Failed to set FIFO priority: {}", e);
                }
            }
            if let Err(e) = self.rt_checker.lock_memory() {
                log::warn!("Failed to lock memory: {}", e);
            }
        }

        // Start Non-RT services first
        let non_rt_services = self.registry.all_non_rt().await;
        for service in non_rt_services {
            let mut svc = service.write().await;
            if let Err(e) = svc.start().await {
                log::error!("Failed to start Non-RT service '{}': {}", svc.name(), e);
            } else {
                log::info!("Started Non-RT service: {}", svc.name());
            }
        }

        // Initialize RT services
        for entry in &mut self.rt_entries {
            if let Err(e) = entry.service.init() {
                log::error!("Failed to init RT service '{}': {}", entry.service.name(), e);
            } else {
                log::info!("Initialized RT service: {}", entry.service.name());
            }
        }

        self.running = true;
        self.stop_flag.store(false, Ordering::Release);

        log::info!("Service Manager started");
        Ok(())
    }

    /// Stop all services
    pub async fn stop_all(&mut self) -> Result<(), String> {
        if !self.running {
            return Err("Manager not running".to_string());
        }

        log::info!("Stopping Service Manager...");

        // Signal RT thread to stop
        self.stop_flag.store(true, Ordering::Release);
        if let Some(handle) = self.rt_thread.take() {
            let _ = handle.join();
        }

        // Shutdown RT services
        for entry in &mut self.rt_entries {
            if let Err(e) = entry.service.shutdown() {
                log::error!("Failed to shutdown RT service '{}': {}", entry.service.name(), e);
            } else {
                log::info!("Shutdown RT service: {}", entry.service.name());
            }
        }

        // Stop Non-RT services
        let non_rt_services = self.registry.all_non_rt().await;
        for service in non_rt_services {
            let mut svc = service.write().await;
            if let Err(e) = svc.stop().await {
                log::error!("Failed to stop Non-RT service '{}': {}", svc.name(), e);
            } else {
                log::info!("Stopped Non-RT service: {}", svc.name());
            }
        }

        self.running = false;
        log::info!("Service Manager stopped");
        Ok(())
    }

    /// Check if manager is running
    pub fn is_running(&self) -> bool {
        self.running
    }

    /// Get RT jitter statistics
    pub fn jitter_stats(&self) -> rtservices::JitterStats {
        self.rt_checker.jitter_stats().clone()
    }

    /// Get status summary
    pub async fn status(&self) -> ManagerStatus {
        let non_rt_count = self.registry.all_non_rt().await.len();
        let rt_status = self.rt_checker.status_report();

        ManagerStatus {
            running: self.running,
            rt_service_count: self.rt_entries.len(),
            non_rt_service_count: non_rt_count,
            rt_status,
        }
    }

    /// Print service hierarchy
    pub async fn print_hierarchy(&self) {
        self.hierarchy.print_tree().await;
    }
}

impl Default for ServiceManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Manager status information
#[derive(Debug, Clone, serde::Serialize)]
pub struct ManagerStatus {
    pub running: bool,
    pub rt_service_count: usize,
    pub non_rt_service_count: usize,
    pub rt_status: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use rtservices::{RtConfig, RtResult, RtContext};

    struct TestRtService {
        name: String,
        config: RtConfig,
        tick_count: u32,
    }

    impl RtService for TestRtService {
        fn name(&self) -> &str {
            &self.name
        }

        fn config(&self) -> &RtConfig {
            &self.config
        }

        fn tick(&mut self, _ctx: &RtContext<'_>) -> RtResult<()> {
            self.tick_count += 1;
            Ok(())
        }
    }

    #[tokio::test]
    async fn test_service_manager() {
        let mut manager = ServiceManager::new();

        let service = TestRtService {
            name: "test".to_string(),
            config: RtConfig::default(),
            tick_count: 0,
        };

        manager.register_rt(
            Box::new(service),
            RtRegistration {
                budget: Duration::from_millis(5),
            },
        );

        let status = manager.status().await;
        assert!(!status.running);
        assert_eq!(status.rt_service_count, 1);
    }

    #[test]
    fn test_tick_rt() {
        let mut manager = ServiceManager::new();

        let service = TestRtService {
            name: "test".to_string(),
            config: RtConfig::default(),
            tick_count: 0,
        };

        manager.register_rt(
            Box::new(service),
            RtRegistration {
                budget: Duration::from_millis(5),
            },
        );

        manager.tick_rt();
        manager.tick_rt();
        manager.tick_rt();

        let stats = manager.rt_stats_snapshot();
        assert_eq!(stats.len(), 1);
        assert_eq!(stats[0].1.calls, 3);
        assert_eq!(stats[0].1.overruns, 0);
    }
}
