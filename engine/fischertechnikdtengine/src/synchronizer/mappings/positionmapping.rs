use std::sync::Arc;

use async_trait::async_trait;

use crate::synchronizer::synchronizer::{Mapping, SyncError};

#[derive(Debug, Clone, Copy)]
pub struct Position {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

#[derive(thiserror::Error, Debug)]
pub enum MappingIoError {
    #[error("gateway read failed: {0}")]
    Gateway(String),
    #[error("db write failed: {0}")]
    DbWrite(String),
}

#[async_trait]
pub trait GatewayPort: Send + Sync {
    async fn get_position(&self) -> Result<Position, MappingIoError>;
}

pub trait DatabasePort: Send + Sync {
    /// RT-friendly: non-awaiting best-effort write.
    fn try_set_position(&self, pos: Position) -> Result<(), MappingIoError>;
}

pub struct PositionMapping {
    name: String,
    gateway: Arc<dyn GatewayPort>,
    db: Arc<dyn DatabasePort>,
}

impl PositionMapping {
    pub fn new(
        name: impl Into<String>,
        gateway: Arc<dyn GatewayPort>,
        db: Arc<dyn DatabasePort>,
    ) -> Self {
        Self {
            name: name.into(),
            gateway,
            db,
        }
    }
}

#[async_trait]
impl Mapping for PositionMapping {
    fn name(&self) -> &str {
        &self.name
    }

    async fn sync_gateway_to_db(&self) -> Result<(), SyncError> {
        let pos = self
            .gateway
            .get_position()
            .await
            .map_err(|e| SyncError::Mapping(format!("{} (gateway): {e}", self.name)))?;

        self.db
            .try_set_position(pos)
            .map_err(|e| SyncError::Mapping(format!("{} (db): {e}", self.name)))?;

        Ok(())
    }
}
