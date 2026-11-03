//! Loading a copied database into an empty Postgres schema in one transaction.

use crate::error::DbResult;
use crate::lock::migration_key;
use crate::pool::Conn;
use crate::store::{PostgresOptions, PostgresStore, connection_config};
use bytes::BytesMut;
use oxabl_cache_core::ports::migration::{Batch, Cell, MIGRATED_TABLES, TableDigest};
use oxabl_cache_core::{Error, Result};
use tokio_postgres::types::{IsNull, ToSql, Type, to_sql_checked};

#[derive(Debug)]
struct Value<'a>(&'a Cell);

impl ToSql for Value<'_> {
    fn to_sql(
        &self,
        ty: &Type,
        out: &mut BytesMut,
    ) -> std::result::Result<IsNull, Box<dyn std::error::Error + Sync + Send>> {
        match self.0 {
            Cell::Null => Ok(IsNull::Yes),
            Cell::Int(value) => value.to_sql(ty, out),
            Cell::Bytes(bytes) => bytes.as_slice().to_sql(ty, out),
            Cell::Text(text) => text.as_str().to_sql(ty, out),
        }
    }

    fn accepts(ty: &Type) -> bool {
        matches!(*ty, Type::INT8 | Type::BYTEA | Type::TEXT)
    }

    to_sql_checked!();
}

/// An open transaction on a Postgres database whose tables are all empty. Dropping
/// it without [`PostgresImport::commit`] rolls everything back.
pub struct PostgresImport {
    conn: Conn,
}

impl PostgresImport {
    /// Brings the schema of the database up to date, then opens a transaction and
    /// refuses to continue unless every migrated table is empty.
    pub async fn begin(options: &PostgresOptions) -> Result<Self> {
        PostgresStore::connect(options).await?;
        let (config, tls) = connection_config(options)?;
        let conn = Conn::connect(&config, tls.as_ref()).await?;
        conn.client
            .batch_execute(&format!(
                "BEGIN ISOLATION LEVEL SERIALIZABLE; SELECT pg_advisory_xact_lock({})",
                migration_key()
            ))
            .await
            .db()?;
        for table in MIGRATED_TABLES {
            let rows: i64 = conn
                .client
                .query_one(&format!("SELECT count(*) FROM {table}"), &[])
                .await
                .db()?
                .try_get(0)
                .db()?;
            if rows != 0 {
                return Err(Error::bad_request(format!(
                    "the target is not empty: {table} holds {rows} rows"
                )));
            }
        }
        Ok(Self { conn })
    }

    /// Inserts a batch of rows.
    pub async fn insert(&self, table: &str, batch: &Batch) -> Result<()> {
        let width = batch.columns.len();
        let statement = |rows: usize| {
            let groups: Vec<String> = (0..rows)
                .map(|row| {
                    let slots: Vec<String> = (0..width)
                        .map(|col| format!("${}", row * width + col + 1))
                        .collect();
                    format!("({})", slots.join(", "))
                })
                .collect();
            format!(
                "INSERT INTO {table} ({}) VALUES {}",
                batch.columns.join(", "),
                groups.join(", ")
            )
        };
        for chunk in batch.rows.chunks(60_000 / width.max(1)) {
            let values: Vec<Value<'_>> = chunk.iter().flatten().map(Value).collect();
            let params: Vec<&(dyn ToSql + Sync)> = values
                .iter()
                .map(|value| value as &(dyn ToSql + Sync))
                .collect();
            self.conn
                .client
                .execute(&statement(chunk.len()), &params)
                .await
                .db()?;
        }
        Ok(())
    }

    /// The count and digest of what `table` now holds, read back from the database.
    pub async fn digest(&self, table: &str, columns: &[String]) -> Result<TableDigest> {
        let rows = self
            .conn
            .client
            .query(&format!("SELECT {} FROM {table}", columns.join(", ")), &[])
            .await
            .db()?;
        let mut digest = TableDigest::default();
        for row in &rows {
            let mut cells = Vec::with_capacity(columns.len());
            for (index, column) in row.columns().iter().enumerate() {
                cells.push(match *column.type_() {
                    Type::INT8 => row
                        .try_get::<_, Option<i64>>(index)
                        .db()?
                        .map_or(Cell::Null, Cell::Int),
                    Type::BYTEA => row
                        .try_get::<_, Option<Vec<u8>>>(index)
                        .db()?
                        .map_or(Cell::Null, Cell::Bytes),
                    Type::TEXT => row
                        .try_get::<_, Option<String>>(index)
                        .db()?
                        .map_or(Cell::Null, Cell::Text),
                    ref other => {
                        return Err(Error::internal(format!(
                            "{table}.{} has the unexpected type {other}",
                            column.name()
                        )));
                    }
                });
            }
            digest.add(&cells);
        }
        Ok(digest)
    }

    /// Commits the copy.
    pub async fn commit(self) -> Result<()> {
        self.conn.client.batch_execute("COMMIT").await.db()
    }
}
