//! Starting a server on a SQLite file, whatever backend the test target runs.

use super::server::Options;

/// Options that put the server on a SQLite file whatever backend the target runs: the
/// tools under test work on SQLite databases. `name` is the name the world starts under.
pub fn on_sqlite(name: &str, options: Options) -> Options {
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("{name}-{}", std::process::id()));
    Options {
        metadata_url: Some(format!("sqlite://{}/meta.db", dir.display())),
        ..options
    }
}
