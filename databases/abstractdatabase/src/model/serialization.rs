// src/serialization.rs
//! Runtime (de)serialization model for database datapoints.
//!
//! Goal:
//! - Represent a "schema-ish" view of datapoints (names, ids, types, units, tags, current value, etc.)
//! - Allow adding new datapoints on the fly (dynamic registry).
//! - Keep it database-agnostic (works for SQL tables, time-series measurements, key/value stores, document DBs).
//!
//! This module focuses on *models* and a small in-memory registry.
//! Actual encoding (JSON/CBOR/MsgPack/DB-native) can be layered on top later.

use std::{
    collections::{BTreeMap, HashMap},
    fmt::Debug,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{anyhow, Result};
use tokio::sync::RwLock;

/// A stable id for a datapoint.
/// Use something deterministic (hash of namespace+name) or an externally assigned id.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct DataPointId(pub String);

impl DataPointId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }
}

/// A fully qualified name (optional).
/// Example: "machine.1.temperature" or "db.schema.table.column".
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct QualifiedName(pub String);

impl QualifiedName {
    pub fn new(name: impl Into<String>) -> Self {
        Self(name.into())
    }
}

/// Describes the value type a datapoint carries.
/// Keep this small and broadly mappable to DB types.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ValueType {
    Null,
    Bool,
    I64,
    U64,
    F64,
    String,
    Bytes,

    /// Useful for time-series DBs.
    TimestampMillis,

    /// JSON-like nested values.
    Array(Box<ValueType>),
    Object,

    /// For databases with rich numeric types (DECIMAL/NUMERIC).
    Decimal { precision: u32, scale: u32 },

    /// For DB-specific / user-defined types.
    Custom(String),
}

/// A runtime value (dynamic).
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    I64(i64),
    U64(u64),
    F64(f64),
    String(String),
    Bytes(Vec<u8>),
    TimestampMillis(i64),
    Array(Vec<Value>),
    Object(BTreeMap<String, Value>),
}

impl Value {
    pub fn value_type(&self) -> ValueType {
        match self {
            Value::Null => ValueType::Null,
            Value::Bool(_) => ValueType::Bool,
            Value::I64(_) => ValueType::I64,
            Value::U64(_) => ValueType::U64,
            Value::F64(_) => ValueType::F64,
            Value::String(_) => ValueType::String,
            Value::Bytes(_) => ValueType::Bytes,
            Value::TimestampMillis(_) => ValueType::TimestampMillis,
            Value::Array(v) => {
                // best-effort: infer homogeneous type if possible, else Object
                if let Some(first) = v.first() {
                    ValueType::Array(Box::new(first.value_type()))
                } else {
                    ValueType::Array(Box::new(ValueType::Null))
                }
            }
            Value::Object(_) => ValueType::Object,
        }
    }
}

/// Common metadata for a datapoint.
#[derive(Clone, Debug)]
pub struct DataPointMeta {
    pub id: DataPointId,
    pub name: QualifiedName,

    /// The declared value type (expected).
    pub declared_type: ValueType,

    /// Optional: unit (°C, bar, ms, %, etc.)
    pub unit: Option<String>,

    /// Optional: human-readable description.
    pub description: Option<String>,

    /// Optional: tags for routing/lookup (e.g., machine=1, line=A, region=eu).
    pub tags: BTreeMap<String, String>,

    /// Optional: "source" mapping (table/column/measurement/field).
    pub source: Option<SourceRef>,

    /// Optional: constraints (min/max, enum values).
    pub constraints: Option<Constraints>,
}

/// Where this datapoint lives in an underlying DB model.
/// Kept generic so you can map SQL / time-series / doc DB.
#[derive(Clone, Debug)]
pub enum SourceRef {
    SqlColumn {
        schema: Option<String>,
        table: String,
        column: String,
    },
    TimeSeriesField {
        measurement: String,
        field: String,
        tag_keys: Vec<String>,
    },
    KeyValue {
        key: String,
    },
    DocumentPath {
        collection: String,
        path: String, // e.g., "$.a.b[0].c"
    },
    Custom(BTreeMap<String, String>),
}

#[derive(Clone, Debug)]
pub struct Constraints {
    pub min: Option<Value>,
    pub max: Option<Value>,
    pub allowed: Option<Vec<Value>>,
}

/// A datapoint plus a "current value" snapshot.
/// Useful for caches / live dashboards / last-known state.
#[derive(Clone, Debug)]
pub struct DataPointState {
    pub meta: DataPointMeta,
    pub current: Option<TimedValue>,
}

/// Value with a timestamp (millis since epoch).
#[derive(Clone, Debug, PartialEq)]
pub struct TimedValue {
    pub ts_millis: i64,
    pub value: Value,
}

impl TimedValue {
    pub fn now(value: Value) -> Self {
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64;
        Self { ts_millis: ts, value }
    }
}

/// A “schema” is just a set of datapoint metas (and optional relationships).
/// It can be updated at runtime.
#[derive(Clone, Debug, Default)]
pub struct Schema {
    pub namespace: Option<String>,
    pub datapoints: Vec<DataPointMeta>,
    pub version: Option<String>,
}

/// Dynamic registry for datapoints.
/// - Create/update datapoints at runtime
/// - Get by id or name
/// - Update current values
#[derive(Clone, Debug, Default)]
pub struct DataPointRegistry {
    inner: Arc<RwLock<RegistryInner>>,
}

#[derive(Debug, Default)]
struct RegistryInner {
    by_id: HashMap<DataPointId, DataPointState>,
    by_name: HashMap<QualifiedName, DataPointId>,
}

impl DataPointRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register or update a datapoint definition.
    /// If id already exists, we update metadata (and keep current value).
    pub async fn upsert_meta(&self, meta: DataPointMeta) -> Result<()> {
        let mut inner = self.inner.write().await;

        let id = meta.id.clone();
        let name = meta.name.clone();

        match inner.by_id.get_mut(&id) {
            Some(existing) => {
                let current = existing.current.clone();
                *existing = DataPointState { meta, current };
            }
            None => {
                inner.by_id.insert(
                    id.clone(),
                    DataPointState {
                        meta,
                        current: None,
                    },
                );
            }
        }

        inner.by_name.insert(name, id);
        Ok(())
    }

    /// Convenience: register from schema.
    pub async fn apply_schema(&self, schema: Schema) -> Result<()> {
        for dp in schema.datapoints {
            self.upsert_meta(dp).await?;
        }
        Ok(())
    }

    pub async fn get_by_id(&self, id: &DataPointId) -> Option<DataPointState> {
        let inner = self.inner.read().await;
        inner.by_id.get(id).cloned()
    }

    pub async fn get_by_name(&self, name: &QualifiedName) -> Option<DataPointState> {
        let inner = self.inner.read().await;
        let id = inner.by_name.get(name)?.clone();
        inner.by_id.get(&id).cloned()
    }

    /// Update current value; if datapoint doesn't exist and `auto_define` is true,
    /// it will create a new datapoint definition on the fly.
    pub async fn set_value(
        &self,
        id: DataPointId,
        value: TimedValue,
        auto_define: bool,
        default_name: Option<QualifiedName>,
        default_tags: BTreeMap<String, String>,
    ) -> Result<()> {
        let mut inner = self.inner.write().await;

        if let Some(dp) = inner.by_id.get_mut(&id) {
            // optional: type check
            let declared = &dp.meta.declared_type;
            let actual = value.value.value_type();
            if !type_compatible(declared, &actual) {
                return Err(anyhow!(
                    "type mismatch for {:?}: declared {:?}, got {:?}",
                    dp.meta.name,
                    declared,
                    actual
                ));
            }
            dp.current = Some(value);
            return Ok(());
        }

        if !auto_define {
            return Err(anyhow!("unknown datapoint id {:?}", id));
        }

        let name = default_name.unwrap_or_else(|| QualifiedName::new(id.0.clone()));
        let meta = DataPointMeta {
            id: id.clone(),
            name: name.clone(),
            declared_type: value.value.value_type(),
            unit: None,
            description: None,
            tags: default_tags,
            source: None,
            constraints: None,
        };

        inner.by_name.insert(name, id.clone());
        inner.by_id.insert(
            id,
            DataPointState {
                meta,
                current: Some(value),
            },
        );

        Ok(())
    }

    /// List all datapoints (metadata + current).
    pub async fn list(&self) -> Vec<DataPointState> {
        let inner = self.inner.read().await;
        inner.by_id.values().cloned().collect()
    }
}

/// Basic compatibility rules.
/// You can make this stricter/looser (e.g., allow I64 -> F64).
pub fn type_compatible(declared: &ValueType, actual: &ValueType) -> bool {
    match (declared, actual) {
        (ValueType::I64, ValueType::I64) => true,
        (ValueType::U64, ValueType::U64) => true,
        (ValueType::F64, ValueType::F64) => true,
        (ValueType::Bool, ValueType::Bool) => true,
        (ValueType::String, ValueType::String) => true,
        (ValueType::Bytes, ValueType::Bytes) => true,
        (ValueType::TimestampMillis, ValueType::TimestampMillis) => true,
        (ValueType::Object, ValueType::Object) => true,
        (ValueType::Array(dt), ValueType::Array(at)) => type_compatible(dt, at),

        // Allow null for anything
        (_, ValueType::Null) => true,

        // Optional: allow widening conversions
        (ValueType::F64, ValueType::I64) => true,
        (ValueType::F64, ValueType::U64) => true,

        _ => false,
    }
}
