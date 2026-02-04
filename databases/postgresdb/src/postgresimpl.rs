use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use async_trait::async_trait;
use tokio::sync::{mpsc, oneshot, RwLock};

use crate::{
    databasemanager::Database,
    models::{
        postgrestypes::{ColumnDef, ColumnMeta, DataBinding, DbMetadata, PgDataType, TableMeta},
        transformation::SqlParam,
    },
    postgresclient::PostgresClient,
};

const METADATA_TABLE: &str = "dt_metadata";

#[derive(thiserror::Error, Debug)]
pub enum DbError {

    #[error("submit queue full (dropped)")]
    QueueFull,

    #[error("postgres error: {0}")]
    Postgres(#[from] tokio_postgres::Error),

    #[error("invalid identifier '{0}' (only [A-Za-z_][A-Za-z0-9_]* allowed)")]
    InvalidIdentifier(String),

    #[error("not found: {0}")]
    NotFound(String),

    #[error("already exists: {0}")]
    AlreadyExists(String),

    #[error("worker channel closed")]
    WorkerClosed,

    #[error("serde error: {0}")]
    Serde(#[from] serde_json::Error),
}

fn as_refs(params: &[SqlParam]) -> Vec<&(dyn tokio_postgres::types::ToSql + Sync)> {
    params
        .iter()
        .map(|p| (&*p.0) as &(dyn tokio_postgres::types::ToSql + Sync))
        .collect()
}

fn validate_ident(s: &str) -> Result<(), DbError> {
    let mut chars = s.chars();
    let first = chars
        .next()
        .ok_or_else(|| DbError::InvalidIdentifier(s.to_string()))?;
    let ok_first = first.is_ascii_alphabetic() || first == '_';
    let ok_rest = chars.all(|c| c.is_ascii_alphanumeric() || c == '_');

    if ok_first && ok_rest {
        Ok(())
    } else {
        Err(DbError::InvalidIdentifier(s.to_string()))
    }
}

fn join_columns(cols: &[ColumnDef]) -> Result<String, DbError> {
    let mut parts = Vec::with_capacity(cols.len());

    for c in cols {
        validate_ident(&c.name)?;

        let mut part = format!("{} {}", c.name, c.data_type.to_sql());

        if let Some(def) = &c.default_sql {
            part.push_str(" DEFAULT ");
            part.push_str(def);
        }

        if !c.nullable {
            part.push_str(" NOT NULL");
        }

        if c.primary_key {
            part.push_str(" PRIMARY KEY");
        }

        parts.push(part);
    }

    Ok(parts.join(", "))
}

async fn ensure_metadata_table(client: &PostgresClient) -> Result<(), DbError> {
    // Keep it simple: one row per db_name, store complete metadata as JSONB
    let sql = format!(
        r#"
        CREATE TABLE IF NOT EXISTS {t} (
            db_name    TEXT PRIMARY KEY,
            metadata   JSONB NOT NULL,
            updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
        )
        "#,
        t = METADATA_TABLE
    );
    client.execute(&sql, &[]).await?;
    Ok(())
}

async fn load_persisted_metadata(
    client: &PostgresClient,
    db_name: &str,
) -> Result<Option<DbMetadata>, DbError> {
    let sql = format!("SELECT metadata FROM {} WHERE db_name = $1", METADATA_TABLE);
    let rows = client.query(&sql, &[&db_name]).await?;
    if rows.is_empty() {
        return Ok(None);
    }
    let v: serde_json::Value = rows[0].get(0);
    let md: DbMetadata = serde_json::from_value(v)?;
    Ok(Some(md))
}

async fn persist_metadata(client: &PostgresClient, md: &DbMetadata) -> Result<(), DbError> {
    let sql = format!(
        r#"
        INSERT INTO {t} (db_name, metadata, updated_at)
        VALUES ($1, $2, now())
        ON CONFLICT (db_name)
        DO UPDATE SET metadata = EXCLUDED.metadata, updated_at = now()
        "#,
        t = METADATA_TABLE
    );

    let v = serde_json::to_value(md)?;
    client.execute(&sql, &[&md.db_name, &v]).await?;
    Ok(())
}

async fn introspect_schema(client: &PostgresClient) -> Result<HashMap<String, TableMeta>, DbError> {
    // Tables (public schema)
    let table_rows = client
        .query(
            r#"
            SELECT table_name
            FROM information_schema.tables
            WHERE table_schema = 'public'
              AND table_type = 'BASE TABLE'
            ORDER BY table_name
            "#,
            &[],
        )
        .await?;

    let mut tables: HashMap<String, TableMeta> = HashMap::new();

    for r in table_rows {
        let table_name: String = r.get("table_name");
        if table_name == METADATA_TABLE {
            continue;
        }

        // Primary key columns
        let pk_rows = client
            .query(
                r#"
                SELECT kcu.column_name
                FROM information_schema.table_constraints tc
                JOIN information_schema.key_column_usage kcu
                  ON tc.constraint_name = kcu.constraint_name
                 AND tc.table_schema = kcu.table_schema
                WHERE tc.table_schema = 'public'
                  AND tc.table_name = $1
                  AND tc.constraint_type = 'PRIMARY KEY'
                "#,
                &[&table_name],
            )
            .await?;

        let mut pk_set: HashSet<String> = HashSet::new();
        for p in pk_rows {
            let c: String = p.get("column_name");
            pk_set.insert(c);
        }

        // Columns
        let col_rows = client
            .query(
                r#"
                SELECT column_name, is_nullable, data_type, udt_name, column_default
                FROM information_schema.columns
                WHERE table_schema = 'public'
                  AND table_name = $1
                ORDER BY ordinal_position
                "#,
                &[&table_name],
            )
            .await?;

        let mut cols: HashMap<String, ColumnMeta> = HashMap::new();
        for c in col_rows {
            let name: String = c.get("column_name");
            let is_nullable: String = c.get("is_nullable"); // "YES"/"NO"
            let data_type: String = c.get("data_type");
            let udt_name: String = c.get("udt_name");
            let default_sql: Option<String> = c.get("column_default");

            let meta = ColumnMeta {
                name: name.clone(),
                data_type: PgDataType::from_information_schema(&data_type, &udt_name),
                nullable: is_nullable.to_ascii_uppercase() == "YES",
                primary_key: pk_set.contains(&name),
                default_sql,
            };

            cols.insert(name, meta);
        }

        tables.insert(
            table_name.clone(),
            TableMeta {
                name: table_name,
                columns: cols,
            },
        );
    }

    Ok(tables)
}

/// Remove bindings that point to tables/columns that don't exist in the current schema.
fn prune_bindings_against_schema(md: &mut DbMetadata) {
    md.bindings.retain(|_, b| {
        let t = match md.tables.get(&b.table) {
            Some(t) => t,
            None => return false,
        };
        match &b.column {
            Some(col) => t.columns.contains_key(col),
            None => true,
        }
    });
}

enum DbCommand {
        ExecuteNoAck {
        sql: String,
        params: Vec<SqlParam>,
    },
        Execute {
        sql: String,
        params: Vec<SqlParam>,
        resp: oneshot::Sender<Result<u64, DbError>>,
    },



    InsertRowNoAck {
        table: String,
        values: Vec<(String, SqlParam)>,
    },
    AddManagedId {
        id: String,
        resp: oneshot::Sender<Result<(), DbError>>,
    },
    RemoveManagedId {
        id: String,
        resp: oneshot::Sender<Result<(), DbError>>,
    },

    RegisterBinding {
        binding: DataBinding,
        resp: oneshot::Sender<Result<(), DbError>>,
    },
    UnregisterBinding {
        data_id: String,
        resp: oneshot::Sender<Result<(), DbError>>,
    },

    CreateTable {
        table: String,
        columns: Vec<ColumnDef>,
        resp: oneshot::Sender<Result<(), DbError>>,
    },
    DropTable {
        table: String,
        resp: oneshot::Sender<Result<(), DbError>>,
    },
    AddColumn {
        table: String,
        column: ColumnDef,
        resp: oneshot::Sender<Result<(), DbError>>,
    },
    DropColumn {
        table: String,
        column: String,
        resp: oneshot::Sender<Result<(), DbError>>,
    },

    InsertRow {
        table: String,
        values: Vec<(String, SqlParam)>,
        resp: oneshot::Sender<Result<u64, DbError>>,
    },

    Query {
        sql: String,
        params: Vec<SqlParam>,
        resp: oneshot::Sender<Result<Vec<tokio_postgres::Row>, DbError>>,
    },

    GetMetadata {
        resp: oneshot::Sender<Result<DbMetadata, DbError>>,
    },

    /// Re-introspect schema from DB, update metadata.tables, prune bindings, persist.
    RefreshSchema {
        resp: oneshot::Sender<Result<(), DbError>>,
    },
}

pub struct PostgresDatabase {
    name: String,
    managed_ids: Arc<RwLock<HashSet<String>>>,
    metadata: Arc<RwLock<DbMetadata>>,
    tx: mpsc::Sender<DbCommand>,
}

 impl PostgresDatabase {
    pub async fn new(name: impl Into<String>, client: PostgresClient) -> Self {
        let name = name.into();

        let managed_ids = Arc::new(RwLock::new(HashSet::new()));
        let metadata = Arc::new(RwLock::new(DbMetadata::new(&name)));

        let (tx, mut rx) = mpsc::channel::<DbCommand>(256);

        let managed_ids_worker = managed_ids.clone();
        let metadata_worker = metadata.clone();
        let db_name_for_worker = name.clone();

        tokio::spawn(async move {
            // ---- bootstrap: ensure table, load persisted bindings, introspect schema, merge, persist
            if let Err(e) = (async {
                ensure_metadata_table(&client).await?;

                // Load persisted metadata (if any)
                let persisted = load_persisted_metadata(&client, &db_name_for_worker).await?;

                // Introspect schema (source of truth)
                let schema_tables = introspect_schema(&client).await?;

                let mut md = DbMetadata::new(&db_name_for_worker);
                md.tables = schema_tables;

                // Keep persisted bindings (and any other persisted details), but prune against schema
                if let Some(p) = persisted {
                    md.bindings = p.bindings;
                }
                prune_bindings_against_schema(&mut md);

                // Push into memory + managed ids
                {
                    let mut m = metadata_worker.write().await;
                    *m = md.clone();
                }
                {
                    let mut g = managed_ids_worker.write().await;
                    g.clear();
                    for k in md.bindings.keys() {
                        g.insert(k.clone());
                    }
                }

                // Persist merged metadata so restarts always have a sane baseline
                persist_metadata(&client, &md).await?;
                Ok::<(), DbError>(())
            })
            .await
            {
                log::error!("PostgresDatabase bootstrap failed: {e}");
            }

            // ---- main actor loop
            while let Some(cmd) = rx.recv().await {
                match cmd {

                    DbCommand::ExecuteNoAck { sql, params } => {
                        let result: Result<(), DbError> = async {
                            let refs = as_refs(&params);
                            client.execute(&sql, &refs).await?;
                            Ok(())
                        }
                        .await;

                        if let Err(e) = result {
                            log::warn!("ExecuteNoAck failed: {e} | sql={}", sql);
                        }
                    }
                    
                    DbCommand::Execute { sql, params, resp } => {
                        let result = async {
                            let refs = as_refs(&params);
                            let n = client.execute(&sql, &refs).await?;
                            Ok(n)
                        }
                        .await;
                        let _ = resp.send(result);
                    }

                    DbCommand::AddManagedId { id, resp } => {
                        let mut g = managed_ids_worker.write().await;
                        g.insert(id);
                        let _ = resp.send(Ok(()));
                    }
                    DbCommand::RemoveManagedId { id, resp } => {
                        let mut g = managed_ids_worker.write().await;
                        g.remove(&id);

                        // remove binding if present + persist
                        let result = async {
                            let md_snap = {
                                let mut m = metadata_worker.write().await;
                                m.bindings.remove(&id);
                                m.clone()
                            };
                            persist_metadata(&client, &md_snap).await?;
                            Ok(())
                        }
                        .await;

                        let _ = resp.send(result);
                    }

                    DbCommand::RegisterBinding { binding, resp } => {
                        let result = async {
                            validate_ident(&binding.table)?;
                            if let Some(c) = &binding.column {
                                validate_ident(c)?;
                            }

                            let md_snap = {
                                let mut m = metadata_worker.write().await;
                                m.bindings.insert(binding.data_id.clone(), binding);
                                prune_bindings_against_schema(&mut m);
                                m.clone()
                            };

                            {
                                let mut g = managed_ids_worker.write().await;
                                g.clear();
                                for k in md_snap.bindings.keys() {
                                    g.insert(k.clone());
                                }
                            }

                            persist_metadata(&client, &md_snap).await?;
                            Ok(())
                        }
                        .await;
                        let _ = resp.send(result);
                    }

                    DbCommand::UnregisterBinding { data_id, resp } => {
                        let result = async {
                            let md_snap = {
                                let mut m = metadata_worker.write().await;
                                m.bindings.remove(&data_id);
                                m.clone()
                            };

                            {
                                let mut g = managed_ids_worker.write().await;
                                g.remove(&data_id);
                            }

                            persist_metadata(&client, &md_snap).await?;
                            Ok(())
                        }
                        .await;
                        let _ = resp.send(result);
                    }

                    DbCommand::CreateTable { table, columns, resp } => {
                        let result = async {
                            validate_ident(&table)?;
                            if table == METADATA_TABLE {
                                return Err(DbError::InvalidIdentifier(table));
                            }

                            let cols_sql = join_columns(&columns)?;
                            let sql = format!("CREATE TABLE IF NOT EXISTS {} ({})", table, cols_sql);
                            client.execute(&sql, &[]).await?;

                            // metadata update
                            let mut cols_map: HashMap<String, ColumnMeta> = HashMap::new();
                            for c in columns {
                                cols_map.insert(c.name.clone(), c.into());
                            }

                            let md_snap = {
                                let mut m = metadata_worker.write().await;
                                m.tables.insert(
                                    table.clone(),
                                    TableMeta { name: table.clone(), columns: cols_map },
                                );
                                prune_bindings_against_schema(&mut m);
                                m.clone()
                            };

                            persist_metadata(&client, &md_snap).await?;
                            Ok(())
                        }
                        .await;
                        let _ = resp.send(result);
                    }

                    DbCommand::DropTable { table, resp } => {
                        let result = async {
                            validate_ident(&table)?;
                            if table == METADATA_TABLE {
                                return Err(DbError::InvalidIdentifier(table));
                            }

                            let sql = format!("DROP TABLE IF EXISTS {} CASCADE", table);
                            client.execute(&sql, &[]).await?;

                            let md_snap = {
                                let mut m = metadata_worker.write().await;
                                m.tables.remove(&table);
                                m.bindings.retain(|_, b| b.table != table);
                                m.clone()
                            };

                            {
                                let mut g = managed_ids_worker.write().await;
                                g.clear();
                                for k in md_snap.bindings.keys() {
                                    g.insert(k.clone());
                                }
                            }

                            persist_metadata(&client, &md_snap).await?;
                            Ok(())
                        }
                        .await;
                        let _ = resp.send(result);
                    }

                    DbCommand::AddColumn { table, column, resp } => {
                        let result = async {
                            validate_ident(&table)?;
                            validate_ident(&column.name)?;
                            if table == METADATA_TABLE {
                                return Err(DbError::InvalidIdentifier(table));
                            }

                            let mut sql = format!(
                                "ALTER TABLE {} ADD COLUMN IF NOT EXISTS {} {}",
                                table,
                                column.name,
                                column.data_type.to_sql()
                            );

                            if let Some(def) = &column.default_sql {
                                sql.push_str(" DEFAULT ");
                                sql.push_str(def);
                            }
                            if !column.nullable {
                                sql.push_str(" NOT NULL");
                            }

                            client.execute(&sql, &[]).await?;

                            let md_snap = {
                                let mut m = metadata_worker.write().await;
                                let entry = m.tables.entry(table.clone()).or_insert_with(|| TableMeta {
                                    name: table.clone(),
                                    columns: HashMap::new(),
                                });
                                entry.columns.insert(column.name.clone(), column.into());
                                prune_bindings_against_schema(&mut m);
                                m.clone()
                            };

                            persist_metadata(&client, &md_snap).await?;
                            Ok(())
                        }
                        .await;
                        let _ = resp.send(result);
                    }

                    DbCommand::DropColumn { table, column, resp } => {
                        let result = async {
                            validate_ident(&table)?;
                            validate_ident(&column)?;
                            if table == METADATA_TABLE {
                                return Err(DbError::InvalidIdentifier(table));
                            }

                            let sql = format!("ALTER TABLE {} DROP COLUMN IF EXISTS {}", table, column);
                            client.execute(&sql, &[]).await?;

                            let md_snap = {
                                let mut m = metadata_worker.write().await;
                                if let Some(t) = m.tables.get_mut(&table) {
                                    t.columns.remove(&column);
                                }
                                // remove bindings pointing to this column
                                m.bindings.retain(|_, b| {
                                    if b.table != table {
                                        return true;
                                    }
                                    match (&b.column, &column) {
                                        (Some(c), col) => c != col,
                                        (None, _) => true,
                                    }
                                });
                                m.clone()
                            };

                            {
                                let mut g = managed_ids_worker.write().await;
                                g.clear();
                                for k in md_snap.bindings.keys() {
                                    g.insert(k.clone());
                                }
                            }

                            persist_metadata(&client, &md_snap).await?;
                            Ok(())
                        }
                        .await;
                        let _ = resp.send(result);
                    }

                    DbCommand::InsertRowNoAck { table, values } => {
                        // Fire-and-forget: do work, log errors, no reply.
                        let result: Result<(), DbError> = async {
                            validate_ident(&table)?;
                            if values.is_empty() {
                                return Ok(()); // nothing to do
                            }

                            let mut col_names = Vec::with_capacity(values.len());
                            let mut placeholders = Vec::with_capacity(values.len());
                            let mut boxed: Vec<SqlParam> = Vec::with_capacity(values.len());

                            for (i, (col, v)) in values.into_iter().enumerate() {
                                validate_ident(&col)?;
                                col_names.push(col);
                                placeholders.push(format!("${}", i + 1));
                                boxed.push(v);
                            }

                            let sql = format!(
                                "INSERT INTO {} ({}) VALUES ({})",
                                table,
                                col_names.join(", "),
                                placeholders.join(", ")
                            );

                            let refs: Vec<&(dyn tokio_postgres::types::ToSql + Sync)> = boxed
                                .iter()
                                .map(|p| (&*p.0) as &(dyn tokio_postgres::types::ToSql + Sync))
                                .collect();

                            client.execute(&sql, &refs).await?;
                            Ok(())
                        }
                        .await;

                        if let Err(e) = result {
                            log::warn!("InsertRowNoAck failed: {e}");
                        }
                    }


                    DbCommand::InsertRow { table, values, resp } => {
                        let result = async {
                            validate_ident(&table)?;
                            if values.is_empty() {
                                return Err(DbError::NotFound("no values to insert".into()));
                            }

                            let mut col_names = Vec::with_capacity(values.len());
                            let mut placeholders = Vec::with_capacity(values.len());
                            let mut boxed: Vec<SqlParam> = Vec::with_capacity(values.len());

                            for (i, (col, v)) in values.into_iter().enumerate() {
                                validate_ident(&col)?;
                                col_names.push(col);
                                placeholders.push(format!("${}", i + 1));
                                boxed.push(v);
                            }

                            let sql = format!(
                                "INSERT INTO {} ({}) VALUES ({})",
                                table,
                                col_names.join(", "),
                                placeholders.join(", ")
                            );

                            let refs: Vec<&(dyn tokio_postgres::types::ToSql + Sync)> = boxed
    .iter()
    .map(|p| (&*p.0) as &(dyn tokio_postgres::types::ToSql + Sync))
    .collect();


                            let n = client.execute(&sql, &refs).await?;
                            Ok(n)
                        }
                        .await;
                        let _ = resp.send(result);
                    }

                    DbCommand::Query { sql, params, resp } => {
                        let result = async {
                            let refs: Vec<&(dyn tokio_postgres::types::ToSql + Sync)> = params
    .iter()
    .map(|p| (&*p.0) as &(dyn tokio_postgres::types::ToSql + Sync))
    .collect();

                            client.query(&sql, &refs).await
                        }
                        .await;
                        let _ = resp.send(result);
                    }

                    DbCommand::GetMetadata { resp } => {
                        let snap = { metadata_worker.read().await.clone() };
                        let _ = resp.send(Ok(snap));
                    }

                    DbCommand::RefreshSchema { resp } => {
                        let result = async {
                            // Introspect
                            let schema_tables = introspect_schema(&client).await?;

                            // Apply
                            let md_snap = {
                                let mut m = metadata_worker.write().await;
                                m.tables = schema_tables;
                                prune_bindings_against_schema(&mut m);
                                m.clone()
                            };

                            // managed ids follow bindings
                            {
                                let mut g = managed_ids_worker.write().await;
                                g.clear();
                                for k in md_snap.bindings.keys() {
                                    g.insert(k.clone());
                                }
                            }

                            // persist
                            persist_metadata(&client, &md_snap).await?;
                            Ok(())
                        }
                        .await;

                        let _ = resp.send(result);
                    }
                }
            }
        });

        Self {
            name,
            managed_ids,
            metadata,
            tx,
        }
    }


        /// RT-friendly fire-and-forget SQL. Does not await.
    pub fn try_submit_execute(
        &self,
        sql: impl Into<String>,
        params: Vec<SqlParam>,
    ) -> Result<(), DbError> {
        let cmd = DbCommand::ExecuteNoAck {
            sql: sql.into(),
            params,
        };

        match self.tx.try_send(cmd) {
            Ok(()) => Ok(()),
            Err(tokio::sync::mpsc::error::TrySendError::Full(_)) => Err(DbError::QueueFull),
            Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => Err(DbError::WorkerClosed),
        }
    }

    /// Optional: awaiting execute returning affected rows.
    pub async fn execute(
        &self,
        sql: impl Into<String>,
        params: Vec<SqlParam>,
    ) -> Result<u64, DbError> {
        let (tx, rx) = oneshot::channel();
        self.tx
            .send(DbCommand::Execute {
                sql: sql.into(),
                params,
                resp: tx,
            })
            .await
            .map_err(|_| DbError::WorkerClosed)?;
        rx.await.map_err(|_| DbError::WorkerClosed)?
    }


    // ---- Public API ----

    pub async fn metadata_snapshot(&self) -> Result<DbMetadata, DbError> {
        let (tx, rx) = oneshot::channel();
        self.tx
            .send(DbCommand::GetMetadata { resp: tx })
            .await
            .map_err(|_| DbError::WorkerClosed)?;
        rx.await.map_err(|_| DbError::WorkerClosed)?
    }



    /// Re-introspect schema from Postgres and persist merged metadata.
    pub async fn refresh_schema_from_db(&self) -> Result<(), DbError> {
        let (tx, rx) = oneshot::channel();
        self.tx
            .send(DbCommand::RefreshSchema { resp: tx })
            .await
            .map_err(|_| DbError::WorkerClosed)?;
        rx.await.map_err(|_| DbError::WorkerClosed)?
    }

    pub async fn register_binding(
        &self,
        data_id: impl Into<String>,
        table: impl Into<String>,
        column: Option<impl Into<String>>,
    ) -> Result<(), DbError> {
        let binding = DataBinding {
            data_id: data_id.into(),
            table: table.into(),
            column: column.map(|c| c.into()),
        };

        let (tx, rx) = oneshot::channel();
        self.tx
            .send(DbCommand::RegisterBinding { binding, resp: tx })
            .await
            .map_err(|_| DbError::WorkerClosed)?;
        rx.await.map_err(|_| DbError::WorkerClosed)?
    }

    pub async fn unregister_binding(&self, data_id: impl Into<String>) -> Result<(), DbError> {
        let (tx, rx) = oneshot::channel();
        self.tx
            .send(DbCommand::UnregisterBinding {
                data_id: data_id.into(),
                resp: tx,
            })
            .await
            .map_err(|_| DbError::WorkerClosed)?;
        rx.await.map_err(|_| DbError::WorkerClosed)?
    }

    pub async fn create_table(
        &self,
        table: impl Into<String>,
        columns: Vec<ColumnDef>,
    ) -> Result<(), DbError> {
        let (tx, rx) = oneshot::channel();
        self.tx
            .send(DbCommand::CreateTable {
                table: table.into(),
                columns,
                resp: tx,
            })
            .await
            .map_err(|_| DbError::WorkerClosed)?;
        rx.await.map_err(|_| DbError::WorkerClosed)?
    }

    pub async fn drop_table(&self, table: impl Into<String>) -> Result<(), DbError> {
        let (tx, rx) = oneshot::channel();
        self.tx
            .send(DbCommand::DropTable {
                table: table.into(),
                resp: tx,
            })
            .await
            .map_err(|_| DbError::WorkerClosed)?;
        rx.await.map_err(|_| DbError::WorkerClosed)?
    }

    /// RT-safe: does NOT await. If the queue is full, returns QueueFull and the caller can drop the sample.
    pub fn try_submit_insert_row(
    &self,
    table: impl Into<String>,
    values: Vec<(String, SqlParam)>,
) -> Result<(), DbError> {
    match self.tx.try_send(DbCommand::InsertRowNoAck {
        table: table.into(),
        values,
    }) {
        Ok(()) => Ok(()),
        Err(tokio::sync::mpsc::error::TrySendError::Full(_)) => Err(DbError::QueueFull),
        Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => Err(DbError::WorkerClosed),
    }
}


    /// Best-effort: still non-awaiting for the caller.
    /// If the queue is full, it spawns a task that awaits capacity and then sends.
    /// (Use this if you prefer "eventually store" over "drop on overload".)
    pub fn submit_insert_row_best_effort(
        &self,
        table: impl Into<String>,
        values: Vec<(String, SqlParam)>,
    ) {
        let cmd = DbCommand::InsertRowNoAck {
            table: table.into(),
            values,
        };

        match self.tx.try_send(cmd) {
            Ok(_) => {}
            Err(tokio::sync::mpsc::error::TrySendError::Full(cmd)) => {
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let _ = tx.send(cmd).await;
        });
    }
    Err(tokio::sync::mpsc::error::TrySendError::Closed(_cmd)) => {
        // worker closed; ignore or log
    }
        }
    }

    pub async fn add_column(&self, table: impl Into<String>, column: ColumnDef) -> Result<(), DbError> {
        let (tx, rx) = oneshot::channel();
        self.tx
            .send(DbCommand::AddColumn {
                table: table.into(),
                column,
                resp: tx,
            })
            .await
            .map_err(|_| DbError::WorkerClosed)?;
        rx.await.map_err(|_| DbError::WorkerClosed)?
    }

    pub async fn drop_column(&self, table: impl Into<String>, column: impl Into<String>) -> Result<(), DbError> {
        let (tx, rx) = oneshot::channel();
        self.tx
            .send(DbCommand::DropColumn {
                table: table.into(),
                column: column.into(),
                resp: tx,
            })
            .await
            .map_err(|_| DbError::WorkerClosed)?;
        rx.await.map_err(|_| DbError::WorkerClosed)?
    }

    pub async fn insert_row(&self, table: impl Into<String>, values: Vec<(String, SqlParam)>) -> Result<u64, DbError> {
        let (tx, rx) = oneshot::channel();
        self.tx
            .send(DbCommand::InsertRow {
                table: table.into(),
                values,
                resp: tx,
            })
            .await
            .map_err(|_| DbError::WorkerClosed)?;
        rx.await.map_err(|_| DbError::WorkerClosed)?
    }

    pub async fn query_raw(&self, sql: impl Into<String>, params: Vec<SqlParam>) -> Result<Vec<tokio_postgres::Row>, DbError> {
        let (tx, rx) = oneshot::channel();
        self.tx
            .send(DbCommand::Query {
                sql: sql.into(),
                params,
                resp: tx,
            })
            .await
            .map_err(|_| DbError::WorkerClosed)?;
        rx.await.map_err(|_| DbError::WorkerClosed)?
    }

    pub fn default_timeseries_columns(payload_type: PgDataType) -> Vec<ColumnDef> {
        vec![
            ColumnDef {
                name: "id".into(),
                data_type: PgDataType::Uuid,
                nullable: false,
                primary_key: true,
                default_sql: None,
            },
            ColumnDef {
                name: "ts".into(),
                data_type: PgDataType::TimestampTz,
                nullable: false,
                primary_key: false,
                default_sql: Some("now()".into()),
            },
            ColumnDef {
                name: "payload".into(),
                data_type: payload_type,
                nullable: false,
                primary_key: false,
                default_sql: None,
            },
        ]
    }
}

#[async_trait]
impl Database for PostgresDatabase {
    fn name(&self) -> &str {
        &self.name
    }

    async fn list_managed_ids(&self) -> Vec<String> {
        let g = self.managed_ids.read().await;
        g.iter().cloned().collect()
    }
}
