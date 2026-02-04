use std::{collections::HashMap, sync::Arc};
use tokio::sync::RwLock;

use async_trait::async_trait;

use crate::postgresimpl::DbError;

#[async_trait]
pub trait Database: Send + Sync {
    fn name(&self) -> &str;

    /// Unique identifiers (data IDs) that this DB claims to manage.
    async fn list_managed_ids(&self) -> Vec<String>;
}

pub struct DatabaseManager {
    dbs: RwLock<HashMap<String, Arc<dyn Database>>>,
}

impl DatabaseManager {
    pub fn new() -> Self {
        Self {
            dbs: RwLock::new(HashMap::new()),
        }
    }

    pub async fn add_db(&self, db: Arc<dyn Database>) -> Result<(), DbError> {
        let mut guard = self.dbs.write().await;
        let name = db.name().to_string();

        if guard.contains_key(&name) {
            return Err(DbError::AlreadyExists(format!(
                "DB '{}' already registered",
                name
            )));
        }

        guard.insert(name, db);
        Ok(())
    }

    pub async fn remove_db(&self, name: &str) -> Option<Arc<dyn Database>> {
        let mut guard = self.dbs.write().await;
        guard.remove(name)
    }

    pub async fn get_db(&self, name: &str) -> Option<Arc<dyn Database>> {
        let guard = self.dbs.read().await;
        guard.get(name).cloned()
    }

    pub async fn list_db_names(&self) -> Vec<String> {
        let guard = self.dbs.read().await;
        guard.keys().cloned().collect()
    }
}
