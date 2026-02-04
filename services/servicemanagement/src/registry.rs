//! Service Registry
//!
//! Manages registration and lookup of RT and Non-RT services.

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use rtservices::RtService;
use nonrtservices::NonRtService;

/// Registry for managing service instances
pub struct ServiceRegistry {
    /// Registered RT services
    rt_services: RwLock<HashMap<String, Arc<RwLock<Box<dyn RtService>>>>>,
    /// Registered Non-RT services (Arc-wrapped so it can be shared with NonRtDispatcher)
    non_rt_services: Arc<RwLock<HashMap<String, Arc<RwLock<Box<dyn NonRtService>>>>>>,
}

impl ServiceRegistry {
    pub fn new() -> Self {
        Self {
            rt_services: RwLock::new(HashMap::new()),
            non_rt_services: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Get a shared reference to the Non-RT services map (for use with NonRtDispatcher).
    pub fn non_rt_services_map(&self) -> Arc<RwLock<HashMap<String, Arc<RwLock<Box<dyn NonRtService>>>>>> {
        self.non_rt_services.clone()
    }

    /// Register an RT service
    pub async fn register_rt(&self, service: Box<dyn RtService>) {
        let name = service.name().to_string();
        let mut services = self.rt_services.write().await;

        if services.contains_key(&name) {
            log::warn!("RT service '{}' already registered, replacing", name);
        }

        services.insert(name.clone(), Arc::new(RwLock::new(service)));
        log::info!("Registered RT service: {}", name);
    }

    /// Register a Non-RT service
    pub async fn register_non_rt(&self, service: Box<dyn NonRtService>) {
        let name = service.name().to_string();
        let mut services = self.non_rt_services.write().await;

        if services.contains_key(&name) {
            log::warn!("Non-RT service '{}' already registered, replacing", name);
        }

        services.insert(name.clone(), Arc::new(RwLock::new(service)));
        log::info!("Registered Non-RT service: {}", name);
    }

    /// Get an RT service by name
    pub async fn get_rt(&self, name: &str) -> Option<Arc<RwLock<Box<dyn RtService>>>> {
        let services = self.rt_services.read().await;
        services.get(name).cloned()
    }

    /// Get a Non-RT service by name
    pub async fn get_non_rt(&self, name: &str) -> Option<Arc<RwLock<Box<dyn NonRtService>>>> {
        let services = self.non_rt_services.read().await;
        services.get(name).cloned()
    }

    /// Unregister an RT service
    pub async fn unregister_rt(&self, name: &str) -> bool {
        let mut services = self.rt_services.write().await;
        if services.remove(name).is_some() {
            log::info!("Unregistered RT service: {}", name);
            true
        } else {
            false
        }
    }

    /// Unregister a Non-RT service
    pub async fn unregister_non_rt(&self, name: &str) -> bool {
        let mut services = self.non_rt_services.write().await;
        if services.remove(name).is_some() {
            log::info!("Unregistered Non-RT service: {}", name);
            true
        } else {
            false
        }
    }

    /// Get all RT service names
    pub async fn list_rt(&self) -> Vec<String> {
        let services = self.rt_services.read().await;
        services.keys().cloned().collect()
    }

    /// Get all Non-RT service names
    pub async fn list_non_rt(&self) -> Vec<String> {
        let services = self.non_rt_services.read().await;
        services.keys().cloned().collect()
    }

    /// Get total service count
    pub async fn count(&self) -> (usize, usize) {
        let rt_count = self.rt_services.read().await.len();
        let non_rt_count = self.non_rt_services.read().await.len();
        (rt_count, non_rt_count)
    }

    /// Get all RT service instances (for iteration)
    pub async fn all_rt(&self) -> Vec<Arc<RwLock<Box<dyn RtService>>>> {
        let services = self.rt_services.read().await;
        services.values().cloned().collect()
    }

    /// Get all Non-RT service instances
    pub async fn all_non_rt(&self) -> Vec<Arc<RwLock<Box<dyn NonRtService>>>> {
        let services = self.non_rt_services.read().await;
        services.values().cloned().collect()
    }
}

impl Default for ServiceRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rtservices::{RtConfig, RtResult};

    struct MockRtService {
        name: String,
        config: RtConfig,
    }

    impl RtService for MockRtService {
        fn name(&self) -> &str {
            &self.name
        }

        fn config(&self) -> &RtConfig {
            &self.config
        }

        fn tick(&mut self, _ctx: &rtservices::RtContext<'_>) -> RtResult<()> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn test_registry() {
        let registry = ServiceRegistry::new();

        let service = MockRtService {
            name: "test_rt".to_string(),
            config: RtConfig::default(),
        };

        registry.register_rt(Box::new(service)).await;

        assert!(registry.get_rt("test_rt").await.is_some());
        assert!(registry.get_rt("nonexistent").await.is_none());

        let names = registry.list_rt().await;
        assert_eq!(names.len(), 1);
        assert!(names.contains(&"test_rt".to_string()));

        let (rt_count, non_rt_count) = registry.count().await;
        assert_eq!(rt_count, 1);
        assert_eq!(non_rt_count, 0);
    }
}
