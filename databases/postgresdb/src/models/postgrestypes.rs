use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum PgDataType {
    Bool,
    Int4,
    Int8,
    Float8,
    Text,
    Uuid,
    TimestampTz,
    Jsonb,

    /// For types not covered above (e.g., enums, numeric, varchar, custom domains)
    Custom(String),
}

impl PgDataType {
    /// Convert to SQL type name for DDL generation.
    pub fn to_sql(&self) -> String {
        match self {
            PgDataType::Bool => "BOOLEAN".into(),
            PgDataType::Int4 => "INT4".into(),
            PgDataType::Int8 => "INT8".into(),
            PgDataType::Float8 => "FLOAT8".into(),
            PgDataType::Text => "TEXT".into(),
            PgDataType::Uuid => "UUID".into(),
            PgDataType::TimestampTz => "TIMESTAMPTZ".into(),
            PgDataType::Jsonb => "JSONB".into(),
            PgDataType::Custom(s) => s.clone(),
        }
    }

    /// Best-effort mapping from information_schema columns.
    ///
    /// `data_type` examples: "integer", "bigint", "text", "timestamp with time zone", "USER-DEFINED"
    /// `udt_name` examples: "int4", "int8", "text", "timestamptz", "jsonb", "uuid", enum name
    pub fn from_information_schema(data_type: &str, udt_name: &str) -> Self {
        let dt = data_type.to_ascii_lowercase();
        let udt = udt_name.to_ascii_lowercase();

        match (dt.as_str(), udt.as_str()) {
            ("boolean", _) => PgDataType::Bool,
            ("integer", _) => PgDataType::Int4,
            ("bigint", _) => PgDataType::Int8,
            ("double precision", _) => PgDataType::Float8,
            ("text", _) => PgDataType::Text,
            ("uuid", _) => PgDataType::Uuid,
            ("timestamp with time zone", _) => PgDataType::TimestampTz,
            ("jsonb", _) => PgDataType::Jsonb,

            // Sometimes udt_name is the canonical one even if data_type varies
            (_, "bool") => PgDataType::Bool,
            (_, "int4") => PgDataType::Int4,
            (_, "int8") => PgDataType::Int8,
            (_, "float8") => PgDataType::Float8,
            (_, "text") => PgDataType::Text,
            (_, "uuid") => PgDataType::Uuid,
            (_, "timestamptz") => PgDataType::TimestampTz,
            (_, "jsonb") => PgDataType::Jsonb,

            // fallback: store UDT name (works for enums/domains if used in DDL)
            _ => PgDataType::Custom(udt_name.to_string()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ColumnDef {
    pub name: String,
    pub data_type: PgDataType,
    pub nullable: bool,
    pub primary_key: bool,
    /// Raw SQL default expression (e.g., "now()", "'abc'", "0")
    pub default_sql: Option<String>,
}

impl ColumnDef {
    pub fn new(name: impl Into<String>, data_type: PgDataType) -> Self {
        Self {
            name: name.into(),
            data_type,
            nullable: true,
            primary_key: false,
            default_sql: None,
        }
    }
}

/// ---- Metadata tracking ----

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ColumnMeta {
    pub name: String,
    pub data_type: PgDataType,
    pub nullable: bool,
    pub primary_key: bool,
    pub default_sql: Option<String>,
}

impl From<ColumnDef> for ColumnMeta {
    fn from(d: ColumnDef) -> Self {
        Self {
            name: d.name,
            data_type: d.data_type,
            nullable: d.nullable,
            primary_key: d.primary_key,
            default_sql: d.default_sql,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TableMeta {
    pub name: String,
    /// column_name -> meta
    pub columns: HashMap<String, ColumnMeta>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataBinding {
    pub data_id: String,
    pub table: String,
    pub column: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DbMetadata {
    pub db_name: String,
    pub tables: HashMap<String, TableMeta>,
    pub bindings: HashMap<String, DataBinding>,
}

impl DbMetadata {
    pub fn new(db_name: impl Into<String>) -> Self {
        Self {
            db_name: db_name.into(),
            tables: HashMap::new(),
            bindings: HashMap::new(),
        }
    }
}
