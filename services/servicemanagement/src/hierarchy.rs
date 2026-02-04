//! Hierarchical Plugin Pattern for Service Organization
//!
//! Implements TreeNode-based self-organization for services based on
//! the paper "Plugin-Based Systems with Self-Organized Hierarchical Presentation".
//!
//! This allows services to:
//! - Self-register in a hierarchical structure
//! - Define parent-child relationships
//! - Support priority-based ordering
//! - Enable dynamic reorganization

use std::sync::{Arc, Weak};
use tokio::sync::RwLock;

/// Service type classification
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ServiceType {
    /// Real-time service with bounded execution
    RealTime,
    /// Non-real-time service (async operations)
    NonRealTime,
    /// Container/group node (no actual service)
    Container,
}

/// Service node data in the hierarchy
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ServiceNode {
    /// Unique service identifier
    pub name: String,
    /// Service type classification
    pub service_type: ServiceType,
    /// Priority for ordering (higher = more important)
    pub priority: u32,
    /// Whether this service is enabled
    pub enabled: bool,
    /// Optional description
    pub description: Option<String>,
    /// Custom metadata
    pub metadata: serde_json::Value,
}

impl ServiceNode {
    pub fn new(name: &str, service_type: ServiceType) -> Self {
        Self {
            name: name.to_string(),
            service_type,
            priority: 0,
            enabled: true,
            description: None,
            metadata: serde_json::Value::Null,
        }
    }

    pub fn with_priority(mut self, priority: u32) -> Self {
        self.priority = priority;
        self
    }

    pub fn with_description(mut self, description: &str) -> Self {
        self.description = Some(description.to_string());
        self
    }

    pub fn container(name: &str) -> Self {
        Self::new(name, ServiceType::Container)
    }

    pub fn rt(name: &str) -> Self {
        Self::new(name, ServiceType::RealTime)
    }

    pub fn non_rt(name: &str) -> Self {
        Self::new(name, ServiceType::NonRealTime)
    }
}

/// Tree node for hierarchical service organization
pub struct TreeNode {
    /// Node data
    pub data: ServiceNode,
    /// Child nodes
    pub children: RwLock<Vec<Arc<TreeNode>>>,
    /// Weak reference to parent (prevents cycles)
    pub parent: RwLock<Option<Weak<TreeNode>>>,
}

impl TreeNode {
    pub fn new(data: ServiceNode) -> Arc<Self> {
        Arc::new(Self {
            data,
            children: RwLock::new(Vec::new()),
            parent: RwLock::new(None),
        })
    }

    /// Add a child node
    pub async fn add_child(self: &Arc<Self>, child: Arc<TreeNode>) {
        // Set parent reference
        {
            let mut parent = child.parent.write().await;
            *parent = Some(Arc::downgrade(self));
        }

        // Add to children list (sorted by priority, descending)
        let mut children = self.children.write().await;
        let priority = child.data.priority;

        // Find insertion point to maintain sorted order
        let pos = children
            .iter()
            .position(|c| c.data.priority < priority)
            .unwrap_or(children.len());

        children.insert(pos, child);
    }

    /// Remove a child by name
    pub async fn remove_child(&self, name: &str) -> Option<Arc<TreeNode>> {
        let mut children = self.children.write().await;
        if let Some(pos) = children.iter().position(|c| c.data.name == name) {
            let child = children.remove(pos);
            // Clear parent reference
            {
                let mut parent = child.parent.write().await;
                *parent = None;
            }
            Some(child)
        } else {
            None
        }
    }

    /// Find a child by name
    pub async fn find_child(&self, name: &str) -> Option<Arc<TreeNode>> {
        let children = self.children.read().await;
        children.iter().find(|c| c.data.name == name).cloned()
    }

    /// Get parent node
    pub async fn get_parent(&self) -> Option<Arc<TreeNode>> {
        let parent = self.parent.read().await;
        parent.as_ref().and_then(|p| p.upgrade())
    }

    /// Check if this node is a leaf (no children)
    pub async fn is_leaf(&self) -> bool {
        let children = self.children.read().await;
        children.is_empty()
    }

    /// Get all children
    pub async fn get_children(&self) -> Vec<Arc<TreeNode>> {
        let children = self.children.read().await;
        children.clone()
    }

    /// Find a node by path (e.g., "root/realtime/position_sync")
    pub async fn find_by_path(self: &Arc<Self>, path: &str) -> Option<Arc<TreeNode>> {
        let parts: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();

        let mut current = self.clone();
        for part in parts {
            if current.data.name == part {
                continue;
            }
            match current.find_child(part).await {
                Some(child) => current = child,
                None => return None,
            }
        }
        Some(current)
    }

    /// Traverse tree depth-first, calling visitor on each node
    pub async fn traverse<F>(&self, visitor: &mut F)
    where
        F: FnMut(&ServiceNode, usize) + Send,
    {
        self.traverse_internal(visitor, 0).await;
    }

    #[async_recursion::async_recursion]
    async fn traverse_internal<F>(&self, visitor: &mut F, depth: usize)
    where
        F: FnMut(&ServiceNode, usize) + Send,
    {
        visitor(&self.data, depth);
        let children = self.children.read().await;
        for child in children.iter() {
            child.traverse_internal(visitor, depth + 1).await;
        }
    }

    /// Collect all leaf nodes
    pub async fn collect_leaves(&self) -> Vec<ServiceNode> {
        let mut leaves = Vec::new();
        self.collect_leaves_internal(&mut leaves).await;
        leaves
    }

    #[async_recursion::async_recursion]
    async fn collect_leaves_internal(&self, leaves: &mut Vec<ServiceNode>) {
        let children = self.children.read().await;
        if children.is_empty() {
            leaves.push(self.data.clone());
        } else {
            for child in children.iter() {
                child.collect_leaves_internal(leaves).await;
            }
        }
    }

    /// Get all services of a specific type
    pub async fn get_services_by_type(&self, service_type: ServiceType) -> Vec<ServiceNode> {
        let mut services = Vec::new();
        self.collect_by_type_internal(service_type, &mut services).await;
        services
    }

    #[async_recursion::async_recursion]
    async fn collect_by_type_internal(&self, service_type: ServiceType, services: &mut Vec<ServiceNode>) {
        if self.data.service_type == service_type && self.data.enabled {
            services.push(self.data.clone());
        }
        let children = self.children.read().await;
        for child in children.iter() {
            child.collect_by_type_internal(service_type, services).await;
        }
    }
}

/// Service Hierarchy - manages the entire service tree
pub struct ServiceHierarchy {
    root: Arc<TreeNode>,
}

impl ServiceHierarchy {
    /// Create a new hierarchy with a root container
    pub fn new() -> Self {
        let root = TreeNode::new(ServiceNode::container("root"));
        Self { root }
    }

    /// Get the root node
    pub fn root(&self) -> Arc<TreeNode> {
        self.root.clone()
    }

    /// Register a service at a given path
    pub async fn register(&self, path: &str, service: ServiceNode) -> bool {
        let parent_path = path.rsplit_once('/').map(|(p, _)| p).unwrap_or("");

        let parent = if parent_path.is_empty() {
            self.root.clone()
        } else {
            match self.root.find_by_path(parent_path).await {
                Some(p) => p,
                None => {
                    log::warn!("Parent path '{}' not found for service '{}'", parent_path, service.name);
                    return false;
                }
            }
        };

        let node = TreeNode::new(service);
        parent.add_child(node).await;
        true
    }

    /// Create a container at a given path
    pub async fn create_container(&self, path: &str, name: &str) -> bool {
        let container = ServiceNode::container(name);
        self.register(path, container).await
    }

    /// Find a service by full path
    pub async fn find(&self, path: &str) -> Option<Arc<TreeNode>> {
        self.root.find_by_path(path).await
    }

    /// Get all RT services
    pub async fn get_rt_services(&self) -> Vec<ServiceNode> {
        self.root.get_services_by_type(ServiceType::RealTime).await
    }

    /// Get all Non-RT services
    pub async fn get_non_rt_services(&self) -> Vec<ServiceNode> {
        self.root.get_services_by_type(ServiceType::NonRealTime).await
    }

    /// Print the hierarchy tree (for debugging)
    pub async fn print_tree(&self) {
        println!("Service Hierarchy:");
        self.root.traverse(&mut |node, depth| {
            let indent = "  ".repeat(depth);
            let type_str = match node.service_type {
                ServiceType::RealTime => "[RT]",
                ServiceType::NonRealTime => "[NonRT]",
                ServiceType::Container => "[Container]",
            };
            let enabled = if node.enabled { "" } else { " (disabled)" };
            println!("{}{} {} (priority: {}){}", indent, type_str, node.name, node.priority, enabled);
        }).await;
    }
}

impl Default for ServiceHierarchy {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_tree_node_creation() {
        let node = TreeNode::new(ServiceNode::rt("test_service").with_priority(10));
        assert_eq!(node.data.name, "test_service");
        assert_eq!(node.data.priority, 10);
        assert!(node.is_leaf().await);
    }

    #[tokio::test]
    async fn test_hierarchy() {
        let hierarchy = ServiceHierarchy::new();

        // Create containers
        hierarchy.create_container("root", "realtime").await;
        hierarchy.create_container("root", "nonrealtime").await;

        // Register services
        hierarchy.register("root/realtime", ServiceNode::rt("position_sync").with_priority(100)).await;
        hierarchy.register("root/realtime", ServiceNode::rt("speed_sync").with_priority(50)).await;
        hierarchy.register("root/nonrealtime", ServiceNode::non_rt("rest_api").with_priority(10)).await;

        // Check RT services
        let rt_services = hierarchy.get_rt_services().await;
        assert_eq!(rt_services.len(), 2);
        // Should be sorted by priority
        assert_eq!(rt_services[0].name, "position_sync");
        assert_eq!(rt_services[1].name, "speed_sync");

        // Check Non-RT services
        let non_rt_services = hierarchy.get_non_rt_services().await;
        assert_eq!(non_rt_services.len(), 1);
        assert_eq!(non_rt_services[0].name, "rest_api");
    }
}
