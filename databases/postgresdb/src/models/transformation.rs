use tokio_postgres::types::ToSql;

/// Owned parameter wrapper so we can build dynamic SQL with dynamic binds.
pub struct SqlParam(pub Box<dyn ToSql + Sync + Send>);

impl SqlParam {
    pub fn i64(v: i64) -> Self { Self(Box::new(v)) }
    pub fn i32(v: i32) -> Self { Self(Box::new(v)) }
    pub fn f64(v: f64) -> Self { Self(Box::new(v)) }
    pub fn bool(v: bool) -> Self { Self(Box::new(v)) }
    pub fn text(v: impl Into<String>) -> Self { Self(Box::new(v.into())) }
    pub fn json(v: serde_json::Value) -> Self { Self(Box::new(v)) }
    pub fn uuid(v: uuid::Uuid) -> Self { Self(Box::new(v)) }
    pub fn timestamptz(v: chrono::DateTime<chrono::Utc>) -> Self { Self(Box::new(v)) }
}
