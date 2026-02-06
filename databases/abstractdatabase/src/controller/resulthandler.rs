// src/resulthandler.rs
//! Result handling + error handling extension points.
//!
//! Goal:
//! - After a protocol/DB operation returns something (frames/rows/docs/ack/error),
//!   we want to ingest it and forward it to an "engine" (event system, workflow manager, etc.).
//! - The *how* is intentionally open: users plug in their own handlers.
//!
//! This module defines:
//! - Common result/error envelopes (db-agnostic)
//! - Handler traits (ResultHandler / ErrorHandler)
//! - A simple "EngineSink" trait (what forwarding means)
//! - Default no-op/log handlers
//!
//! You can integrate this with your Protocol/Connector by calling these handlers
//! after each operation or on session drop/reset failures.

use anyhow::Result;
use async_trait::async_trait;
use std::{fmt::Debug, sync::Arc, time::Duration};

use crate::model::serialization::{DataPointId, TimedValue, Value};

/// A database operation identifier (optional but very useful for routing).
#[derive(Clone, Debug)]
pub struct OperationMeta {
    pub op_id: String,                 // caller-chosen correlation id
    pub namespace: Option<String>,      // e.g., "telemetry", "erp", ...
    pub name: Option<String>,           // e.g., "read_latest", "write_points"
    pub timeout: Option<Duration>,
    pub tags: Vec<(String, String)>,    // free-form labels
}

impl OperationMeta {
    pub fn new(op_id: impl Into<String>) -> Self {
        Self {
            op_id: op_id.into(),
            namespace: None,
            name: None,
            timeout: None,
            tags: vec![],
        }
    }

    pub fn tag(mut self, k: impl Into<String>, v: impl Into<String>) -> Self {
        self.tags.push((k.into(), v.into()));
        self
    }
}

/// A generic "result" envelope your protocol layer can map to.
///
/// Keep it broad:
/// - Some DBs return rows/records
/// - Some return an ACK + stats
/// - Some return time-series points
/// - Some return a cursor/stream handle (handled elsewhere)
#[derive(Clone, Debug)]
pub enum DbResult {
    /// A single scalar (e.g., "SELECT 1", "COUNT(*)", etc.)
    Scalar(Value),

    /// A key/value object (document, row mapped to columns, etc.)
    Object(Vec<(String, Value)>),

    /// Tabular data: columns + rows.
    Table {
        columns: Vec<ColumnMeta>,
        rows: Vec<Vec<Value>>,
    },

    /// Time-series / datapoint updates (engine-friendly).
    Points(Vec<(DataPointId, TimedValue)>),

    /// Write acknowledgement / counts.
    Ack {
        affected: Option<u64>,
        message: Option<String>,
        extra: Vec<(String, Value)>,
    },

    /// Protocol-native payload (escape hatch).
    ///
    /// Users can downcast/interpret using their own types if they set `Native`.
    Native(Arc<dyn Debug + Send + Sync>),
}

/// Column metadata (optional: extend later with type info).
#[derive(Clone, Debug)]
pub struct ColumnMeta {
    pub name: String,
    pub declared_type: Option<String>, // db-specific type name if known
}

impl ColumnMeta {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            declared_type: None,
        }
    }
}

/// Error envelope. You can map protocol/driver errors into this.
#[derive(Clone, Debug)]
pub struct DbError {
    pub code: Option<String>,       // SQLSTATE, HTTP status as string, custom code...
    pub message: String,
    pub kind: DbErrorKind,
    pub retryable: bool,
    pub source: Option<Arc<dyn std::error::Error + Send + Sync>>,
}

impl DbError {
    pub fn new(message: impl Into<String>, kind: DbErrorKind) -> Self {
        Self {
            code: None,
            message: message.into(),
            kind,
            retryable: false,
            source: None,
        }
    }

    pub fn with_code(mut self, code: impl Into<String>) -> Self {
        self.code = Some(code.into());
        self
    }

    pub fn retryable(mut self, v: bool) -> Self {
        self.retryable = v;
        self
    }

    pub fn with_source<E>(mut self, err: E) -> Self
    where
        E: std::error::Error + Send + Sync + 'static,
    {
        self.source = Some(Arc::new(err));
        self
    }
}

#[derive(Clone, Debug)]
pub enum DbErrorKind {
    Transport,
    Timeout,
    Authentication,
    Authorization,
    NotFound,
    Conflict,
    ConstraintViolation,
    Serialization,
    Deserialization,
    Protocol,
    Server,
    Client,
    Unknown,
}

/// What does “forward to the engine” mean?
/// You define it by implementing this trait.
#[async_trait]
pub trait EngineSink: Send + Sync + Debug + 'static {
    async fn emit_result(&self, meta: OperationMeta, result: DbResult) -> Result<()>;
    async fn emit_error(&self, meta: OperationMeta, error: DbError) -> Result<()>;
}

/// A hook for successful results.
/// Implementations can:
/// - transform DbResult (e.g., map rows to datapoints)
/// - update a registry
/// - forward to EngineSink
/// - fan-out to multiple sinks
#[async_trait]
pub trait ResultHandler: Send + Sync + Debug + 'static {
    async fn handle_result(
        &self,
        meta: &OperationMeta,
        result: DbResult,
        engine: &dyn EngineSink,
    ) -> Result<()>;
}

/// A hook for errors.
/// Implementations can:
/// - classify retryability
/// - enrich with metadata/tags
/// - forward to EngineSink
/// - trigger circuit breaker, alerts, etc.
#[async_trait]
pub trait ErrorHandler: Send + Sync + Debug + 'static {
    async fn handle_error(
        &self,
        meta: &OperationMeta,
        error: DbError,
        engine: &dyn EngineSink,
    ) -> Result<()>;
}

/// Convenience: bundle both handlers.
#[derive(Clone, Debug)]
pub struct Handlers {
    pub result: Arc<dyn ResultHandler>,
    pub error: Arc<dyn ErrorHandler>,
}

impl Handlers {
    pub fn new(result: Arc<dyn ResultHandler>, error: Arc<dyn ErrorHandler>) -> Self {
        Self { result, error }
    }
}

/// -------------------------
/// Default handlers
/// -------------------------

/// Default: forward everything as-is.
#[derive(Debug, Default)]
pub struct ForwardResultHandler;

#[async_trait]
impl ResultHandler for ForwardResultHandler {
    async fn handle_result(
        &self,
        meta: &OperationMeta,
        result: DbResult,
        engine: &dyn EngineSink,
    ) -> Result<()> {
        engine.emit_result(meta.clone(), result).await
    }
}

/// Default: forward errors as-is.
#[derive(Debug, Default)]
pub struct ForwardErrorHandler;

#[async_trait]
impl ErrorHandler for ForwardErrorHandler {
    async fn handle_error(
        &self,
        meta: &OperationMeta,
        error: DbError,
        engine: &dyn EngineSink,
    ) -> Result<()> {
        engine.emit_error(meta.clone(), error).await
    }
}

/// Default: do nothing (useful for tests / silent mode).
#[derive(Debug, Default)]
pub struct NoopEngineSink;

#[async_trait]
impl EngineSink for NoopEngineSink {
    async fn emit_result(&self, _meta: OperationMeta, _result: DbResult) -> Result<()> {
        Ok(())
    }
    async fn emit_error(&self, _meta: OperationMeta, _error: DbError) -> Result<()> {
        Ok(())
    }
}

/// -------------------------
/// Example: a simple in-process engine sink using tokio mpsc
/// -------------------------

#[derive(Debug)]
pub enum EngineEvent {
    Result { meta: OperationMeta, result: DbResult },
    Error { meta: OperationMeta, error: DbError },
}

#[derive(Clone, Debug)]
pub struct ChannelEngineSink {
    tx: tokio::sync::mpsc::Sender<EngineEvent>,
}

impl ChannelEngineSink {
    pub fn new(tx: tokio::sync::mpsc::Sender<EngineEvent>) -> Self {
        Self { tx }
    }
}

#[async_trait]
impl EngineSink for ChannelEngineSink {
    async fn emit_result(&self, meta: OperationMeta, result: DbResult) -> Result<()> {
        let _ = self.tx.send(EngineEvent::Result { meta, result }).await;
        Ok(())
    }

    async fn emit_error(&self, meta: OperationMeta, error: DbError) -> Result<()> {
        let _ = self.tx.send(EngineEvent::Error { meta, error }).await;
        Ok(())
    }
}
