use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::{
    models::{
        postgrestypes::{ColumnDef, PgDataType},
        transformation::SqlParam,
    },
    postgresimpl::{DbError, PostgresDatabase},
};

pub const TABLE_POSITION: &str = "dt_position";

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Position {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl Position {
    pub fn new(x: f64, y: f64, z: f64) -> Self {
        Self { x, y, z }
    }
}

pub fn position_schema() -> Vec<ColumnDef> {
    vec![
        {
            let mut c = ColumnDef::new("entity_id", PgDataType::Text);
            c.nullable = false;
            c.primary_key = true;
            c
        },
        {
            let mut c = ColumnDef::new("ts", PgDataType::TimestampTz);
            c.nullable = false;
            c.default_sql = Some("now()".into());
            c
        },
        {
            let mut c = ColumnDef::new("x", PgDataType::Float8);
            c.nullable = false;
            c
        },
        {
            let mut c = ColumnDef::new("y", PgDataType::Float8);
            c.nullable = false;
            c
        },
        {
            let mut c = ColumnDef::new("z", PgDataType::Float8);
            c.nullable = false;
            c
        },
    ]
}

#[async_trait]
pub trait PositionProperty: Send + Sync {
    async fn ensure_position_schema(&self) -> Result<(), DbError>;

    /// RT-friendly: fire-and-forget upsert (no await).
    fn try_set_position(&self, entity_id: &str, pos: Position) -> Result<(), DbError>;

    async fn get_position(&self, entity_id: &str) -> Result<Option<Position>, DbError>;

    async fn get_position_with_ts(
        &self,
        entity_id: &str,
    ) -> Result<Option<(Position, DateTime<Utc>)>, DbError>;
}

#[async_trait]
impl PositionProperty for PostgresDatabase {
    async fn ensure_position_schema(&self) -> Result<(), DbError> {
        self.create_table(TABLE_POSITION, position_schema()).await
    }

    fn try_set_position(&self, entity_id: &str, pos: Position) -> Result<(), DbError> {
        // Upsert latest state
        let sql = format!(
            r#"
            INSERT INTO {t} (entity_id, ts, x, y, z)
            VALUES ($1, now(), $2, $3, $4)
            ON CONFLICT (entity_id)
            DO UPDATE SET ts = EXCLUDED.ts, x = EXCLUDED.x, y = EXCLUDED.y, z = EXCLUDED.z
            "#,
            t = TABLE_POSITION
        );

        self.try_submit_execute(
            sql,
            vec![
                SqlParam::text(entity_id),
                SqlParam::f64(pos.x),
                SqlParam::f64(pos.y),
                SqlParam::f64(pos.z),
            ],
        )
    }

    async fn get_position(&self, entity_id: &str) -> Result<Option<Position>, DbError> {
        let sql = format!("SELECT x, y, z FROM {} WHERE entity_id = $1", TABLE_POSITION);
        let rows = self.query_raw(sql, vec![SqlParam::text(entity_id)]).await?;
        if rows.is_empty() {
            return Ok(None);
        }
        let r = &rows[0];
        Ok(Some(Position {
            x: r.get("x"),
            y: r.get("y"),
            z: r.get("z"),
        }))
    }

    async fn get_position_with_ts(
        &self,
        entity_id: &str,
    ) -> Result<Option<(Position, DateTime<Utc>)>, DbError> {
        let sql = format!(
            "SELECT x, y, z, ts FROM {} WHERE entity_id = $1",
            TABLE_POSITION
        );
        let rows = self.query_raw(sql, vec![SqlParam::text(entity_id)]).await?;
        if rows.is_empty() {
            return Ok(None);
        }
        let r = &rows[0];
        let pos = Position {
            x: r.get("x"),
            y: r.get("y"),
            z: r.get("z"),
        };
        let ts: DateTime<Utc> = r.get("ts");
        Ok(Some((pos, ts)))
    }
}
