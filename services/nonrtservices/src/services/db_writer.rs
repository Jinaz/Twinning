//! Database Writer Service
//!
//! Async service for non-critical database write operations.
//! Uses a channel-based architecture for batching and async processing.

use async_trait::async_trait;
use tokio::sync::mpsc;

use crate::traits::{NonRtService, ServiceError, ServiceResult, ServiceStatus};

/// Write operation types
#[derive(Debug, Clone)]
pub enum WriteOp {
    /// Insert a new record
    Insert {
        table: String,
        data: serde_json::Value,
    },
    /// Update an existing record
    Update {
        table: String,
        id: String,
        data: serde_json::Value,
    },
    /// Delete a record
    Delete { table: String, id: String },
    /// Custom SQL
    Custom { sql: String },
}

/// Database Writer Service
///
/// Processes database write operations asynchronously through a channel.
/// This decouples the write request from the actual database operation.
pub struct DbWriterService {
    name: String,
    status: ServiceStatus,
    /// Channel for sending write operations
    write_tx: Option<mpsc::Sender<WriteOp>>,
    /// Batch size for processing
    batch_size: usize,
    /// Channel capacity
    channel_capacity: usize,
    /// Shutdown signal
    shutdown_tx: Option<tokio::sync::oneshot::Sender<()>>,
}

impl DbWriterService {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            status: ServiceStatus::new(name),
            write_tx: None,
            batch_size: 100,
            channel_capacity: 10_000,
            shutdown_tx: None,
        }
    }

    pub fn with_batch_size(mut self, size: usize) -> Self {
        self.batch_size = size;
        self
    }

    pub fn with_channel_capacity(mut self, capacity: usize) -> Self {
        self.channel_capacity = capacity;
        self
    }

    /// Submit a write operation (non-blocking if channel not full)
    pub fn try_submit(&self, op: WriteOp) -> ServiceResult<()> {
        match &self.write_tx {
            Some(tx) => tx.try_send(op).map_err(|e| {
                ServiceError::Internal(format!("Channel send failed: {}", e))
            }),
            None => Err(ServiceError::NotRunning),
        }
    }

    /// Submit a write operation (async, waits if channel full)
    pub async fn submit(&self, op: WriteOp) -> ServiceResult<()> {
        match &self.write_tx {
            Some(tx) => tx.send(op).await.map_err(|e| {
                ServiceError::Internal(format!("Channel send failed: {}", e))
            }),
            None => Err(ServiceError::NotRunning),
        }
    }

    /// Get the sender for external use
    pub fn sender(&self) -> Option<mpsc::Sender<WriteOp>> {
        self.write_tx.clone()
    }
}

#[async_trait]
impl NonRtService for DbWriterService {
    fn name(&self) -> &str {
        &self.name
    }

    async fn start(&mut self) -> ServiceResult<()> {
        if self.status.running {
            return Err(ServiceError::AlreadyRunning);
        }

        // Create channel
        let (tx, mut rx) = mpsc::channel::<WriteOp>(self.channel_capacity);
        self.write_tx = Some(tx);

        // Create shutdown channel
        let (shutdown_tx, mut shutdown_rx) = tokio::sync::oneshot::channel();
        self.shutdown_tx = Some(shutdown_tx);

        let batch_size = self.batch_size;
        let service_name = self.name.clone();

        // Spawn worker task
        tokio::spawn(async move {
            let mut batch: Vec<WriteOp> = Vec::with_capacity(batch_size);

            loop {
                tokio::select! {
                    // Check for shutdown
                    _ = &mut shutdown_rx => {
                        // Process remaining items in batch
                        if !batch.is_empty() {
                            Self::process_batch(&service_name, &batch).await;
                        }
                        log::info!("{}: Worker shutdown", service_name);
                        break;
                    }
                    // Receive write operations
                    op = rx.recv() => {
                        match op {
                            Some(write_op) => {
                                batch.push(write_op);

                                // Process batch when full
                                if batch.len() >= batch_size {
                                    Self::process_batch(&service_name, &batch).await;
                                    batch.clear();
                                }
                            }
                            None => {
                                // Channel closed
                                if !batch.is_empty() {
                                    Self::process_batch(&service_name, &batch).await;
                                }
                                break;
                            }
                        }
                    }
                }
            }
        });

        self.status.running = true;
        self.status.started_at = Some(chrono::Utc::now());

        log::info!("{}: Service started", self.name);
        Ok(())
    }

    async fn stop(&mut self) -> ServiceResult<()> {
        if !self.status.running {
            return Err(ServiceError::NotRunning);
        }

        // Send shutdown signal
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }

        // Drop the sender to close the channel
        self.write_tx = None;

        self.status.running = false;
        log::info!("{}: Service stopped", self.name);
        Ok(())
    }

    async fn health_check(&self) -> bool {
        self.status.running && self.write_tx.is_some()
    }

    fn status(&self) -> ServiceStatus {
        self.status.clone()
    }
}

impl DbWriterService {
    /// Process a batch of write operations
    async fn process_batch(service_name: &str, batch: &[WriteOp]) {
        log::debug!("{}: Processing batch of {} operations", service_name, batch.len());

        for op in batch {
            // In a real implementation, this would execute the database operations
            // For now, we just log them
            match op {
                WriteOp::Insert { table, data } => {
                    log::trace!("{}: INSERT into {} - {:?}", service_name, table, data);
                }
                WriteOp::Update { table, id, data } => {
                    log::trace!("{}: UPDATE {} WHERE id={} - {:?}", service_name, table, id, data);
                }
                WriteOp::Delete { table, id } => {
                    log::trace!("{}: DELETE FROM {} WHERE id={}", service_name, table, id);
                }
                WriteOp::Custom { sql } => {
                    log::trace!("{}: CUSTOM SQL: {}", service_name, sql);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_db_writer_lifecycle() {
        let mut service = DbWriterService::new("test_writer");

        // Start service
        service.start().await.unwrap();
        assert!(service.status().running);

        // Submit some operations
        service
            .submit(WriteOp::Insert {
                table: "test".to_string(),
                data: serde_json::json!({"foo": "bar"}),
            })
            .await
            .unwrap();

        // Stop service
        service.stop().await.unwrap();
        assert!(!service.status().running);
    }
}
