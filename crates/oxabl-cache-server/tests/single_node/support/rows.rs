//! Row counts read straight from the metadata database, as an operator with direct access would.

use super::backend::Backend;
use super::server::Server;
use rusqlite::{Connection, OpenFlags};

/// A byte-string literal of `bytes` in the SQL of the backend under test.
pub fn blob(bytes: &[u8]) -> String {
    let hex: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
    match crate::BACKEND {
        Backend::Sqlite => format!("x'{hex}'"),
        Backend::Postgres => format!("decode('{hex}', 'hex')"),
    }
}

fn postgres_url(server: &Server) -> String {
    std::fs::read_to_string(server.dir.join("server.toml"))
        .expect("configuration")
        .lines()
        .find_map(|line| line.strip_prefix("url = \""))
        .and_then(|rest| rest.strip_suffix('"'))
        .expect("metadata url")
        .to_owned()
}

/// The single number `sql` selects.
pub fn count(server: &Server, sql: &str) -> i64 {
    match crate::BACKEND {
        Backend::Sqlite => {
            let db = Connection::open_with_flags(
                server.dir.join("meta.db"),
                OpenFlags::SQLITE_OPEN_READ_ONLY,
            )
            .expect("open metadata");
            db.busy_timeout(std::time::Duration::from_secs(10))
                .expect("busy timeout");
            db.query_row(sql, [], |row| row.get(0)).expect("count")
        }
        Backend::Postgres => {
            let url = postgres_url(server);
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("runtime")
                .block_on(async {
                    let (client, connection) = tokio_postgres::connect(&url, tokio_postgres::NoTls)
                        .await
                        .expect("connect");
                    tokio::spawn(connection);
                    let row = client.query_one(sql, &[]).await.expect("count");
                    row.try_get::<_, i64>(0).expect("number")
                })
        }
    }
}

/// Rows of `table` whose `column` equals `bytes`.
pub fn rows_of(server: &Server, table: &str, column: &str, bytes: &[u8]) -> i64 {
    count(
        server,
        &format!(
            "SELECT count(*) FROM {table} WHERE {column} = {}",
            blob(bytes)
        ),
    )
}
