use async_trait::async_trait;

#[derive(thiserror::Error, Debug)]
pub enum SyncError {
    #[error("mapping failed: {0}")]
    Mapping(String),
}

#[async_trait]
pub trait Mapping: Send + Sync {
    fn name(&self) -> &str;

    /// Gateway -> DB (your primary direction for now).
    async fn sync_gateway_to_db(&self) -> Result<(), SyncError>;
}

pub struct Synchronizer {
    mappings: Vec<Box<dyn Mapping>>,
}

impl Synchronizer {
    pub fn new(mappings: Vec<Box<dyn Mapping>>) -> Self {
        Self { mappings }
    }

    /// Run all mappings once. Keep it simple; later you can add time budgets / priorities.
    pub async fn tick(&self) {
        for m in &self.mappings {
            if let Err(e) = m.sync_gateway_to_db().await {
                log::warn!("synchronizer mapping '{}' failed: {e}", m.name());
            }
        }
    }
}
