//! Row values and digests the metadata adapters exchange when a database is copied.

/// One column value, in the types both adapters store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Cell {
    /// SQL `NULL`.
    Null,
    /// An integer column.
    Int(i64),
    /// A byte-string column.
    Bytes(Vec<u8>),
    /// A text column.
    Text(String),
}

/// A run of rows of one table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Batch {
    /// Column names, in the order of every row.
    pub columns: Vec<String>,
    /// The rows.
    pub rows: Vec<Vec<Cell>>,
}

/// The tables a migration copies, in an order that satisfies the foreign keys.
pub const MIGRATED_TABLES: &[&str] = &[
    "tenants",
    "caches",
    "cache_owner_keys",
    "users",
    "grants",
    "tokens",
    "token_grants",
    "sessions",
    "session_grants",
    "rosters",
    "roster_signers",
    "packs",
    "segments",
    "entries",
    "entry_access",
    "dictionary_pointers",
    "usage_ledger",
    "ledger_nodes",
    "ledger_days",
    "nodes",
    "scrub_queue",
    "operator_tokens",
    "oidc_issuers",
    "oidc_issuer_claims",
    "workload_policies",
    "workload_policy_claims",
    "workload_policy_grants",
];

/// Tables a migration leaves to the target: its schema version and its leader epoch.
pub const UNMIGRATED_TABLES: &[&str] = &["schema_migrations", "gc_leader"];

/// The row count and an order-independent digest of a table's content.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TableDigest {
    /// Rows added.
    pub rows: u64,
    combined: [u8; 32],
}

impl TableDigest {
    /// Adds one row; the order of rows does not matter.
    pub fn add(&mut self, row: &[Cell]) {
        let mut hasher = blake3::Hasher::new();
        for cell in row {
            match cell {
                Cell::Null => {
                    hasher.update(&[0]);
                }
                Cell::Int(value) => {
                    hasher.update(&[1]).update(&value.to_be_bytes());
                }
                Cell::Bytes(bytes) => {
                    hasher
                        .update(&[2])
                        .update(&(bytes.len() as u64).to_be_bytes());
                    hasher.update(bytes);
                }
                Cell::Text(text) => {
                    hasher
                        .update(&[3])
                        .update(&(text.len() as u64).to_be_bytes());
                    hasher.update(text.as_bytes());
                }
            }
        }
        let hash = hasher.finalize();
        for (slot, byte) in self.combined.iter_mut().zip(hash.as_bytes()) {
            *slot ^= byte;
        }
        self.rows += 1;
    }

    /// The digest as lowercase hex.
    pub fn hex(&self) -> String {
        self.combined
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }
}
