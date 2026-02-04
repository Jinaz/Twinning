use std::sync::Arc;

use tokio_postgres::{Client, NoTls, Row};
use crate::postgresimpl::DbError;

#[derive(Clone)]
pub struct PostgresClient {
    client: Arc<Client>,
}

impl PostgresClient {
    pub async fn connect(pg_url: &str) -> Result<Self, DbError> {
        // ✅ Don't annotate the connection type; let inference handle Socket vs TcpStream.
        let (client, connection) = tokio_postgres::connect(pg_url, NoTls)
            .await
            .map_err(DbError::Postgres)?;

        // Drive the connection in the background
        tokio::spawn(async move {
            if let Err(e) = connection.await {
                log::error!("Postgres connection error: {e}");
            }
        });

        Ok(Self {
            client: Arc::new(client),
        })
    }

    pub async fn execute(
        &self,
        sql: &str,
        params: &[&(dyn tokio_postgres::types::ToSql + Sync)],
    ) -> Result<u64, DbError> {
        self.client.execute(sql, params).await.map_err(DbError::Postgres)
    }

    pub async fn query(
        &self,
        sql: &str,
        params: &[&(dyn tokio_postgres::types::ToSql + Sync)],
    ) -> Result<Vec<Row>, DbError> {
        self.client.query(sql, params).await.map_err(DbError::Postgres)
    }
}
