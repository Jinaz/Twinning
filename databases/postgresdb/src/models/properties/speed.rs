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

pub const TABLE_SPEED: &str = "dt_speed";

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Speed {
    pub value: f64,
}

impl Speed {
    pub fn new(value: f64) -> Self {
        Self { value }
    }
}

pub fn speed_schema() -> Vec<ColumnDef> {
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
            let mut c = ColumnDef::new("value", PgDataType::Float8);
            c.nullable = false;
            c
        },
    ]
}

#[async_trait]
pub trait SpeedProperty: Send + Sync {
    async fn ensure_speed_schema(&self) -> Result<(), DbError>;
    fn try_set_speed(&self, entity_id: &str, speed: Speed) -> Result<(), DbError>;
    async fn get_speed(&self, entity_id: &str) -> Result<Option<Speed>, DbError>;
    async fn get_speed_with_ts(&self, entity_id: &str) -> Result<Option<(Speed, DateTime<Utc>)>, DbError>;
}

#[async_trait]
impl SpeedProperty for PostgresDatabase {
    async fn ensure_speed_schema(&self) -> Result<(), DbError> {
        self.create_table(TABLE_SPEED, speed_schema()).await
    }

    fn try_set_speed(&self, entity_id: &str, speed: Speed) -> Result<(), DbError> {
        let sql = format!(
            r#"
            INSERT INTO {t} (entity_id, ts, value)
            VALUES ($1, now(), $2)
            ON CONFLICT (entity_id)
            DO UPDATE SET ts = EXCLUDED.ts, value = EXCLUDED.value
            "#,
            t = TABLE_SPEED
        );

        self.try_submit_execute(
            sql,
            vec![SqlParam::text(entity_id), SqlParam::f64(speed.value)],
        )
    }

    async fn get_speed(&self, entity_id: &str) -> Result<Option<Speed>, DbError> {
        let sql = format!("SELECT value FROM {} WHERE entity_id = $1", TABLE_SPEED);
        let rows = self.query_raw(sql, vec![SqlParam::text(entity_id)]).await?;
        if rows.is_empty() {
            return Ok(None);
        }
        let r = &rows[0];
        Ok(Some(Speed { value: r.get("value") }))
    }

    async fn get_speed_with_ts(&self, entity_id: &str) -> Result<Option<(Speed, DateTime<Utc>)>, DbError> {
        let sql = format!("SELECT value, ts FROM {} WHERE entity_id = $1", TABLE_SPEED);
        let rows = self.query_raw(sql, vec![SqlParam::text(entity_id)]).await?;
        if rows.is_empty() {
            return Ok(None);
        }
        let r = &rows[0];
        let speed = Speed { value: r.get("value") };
        let ts: DateTime<Utc> = r.get("ts");
        Ok(Some((speed, ts)))
    }
}
