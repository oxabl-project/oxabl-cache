//! A small program over the public API for tests that need a process environment.
//!
//! `driver open <repository root>` reads the configuration, opens the cache and prints one JSON
//! line. `driver fetch <repository root> <stage> <count>` then fetches the keys `0..count` of
//! the test fixtures into the stage, printing `started` first and `done` last.

use std::path::PathBuf;

use oxabl_cache_client::{Cache, CompileKey, Config, FetchOptions, FetchRequest};

fn key(n: usize) -> CompileKey {
    CompileKey(*blake3::hash(format!("compile key {n}").as_bytes()).as_bytes())
}

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (Some(command), Some(root)) = (args.first(), args.get(1)) else {
        eprintln!("usage: driver open <root> | fetch <root> <stage> <count>");
        std::process::exit(2);
    };
    let config = match Config::from_repo(&PathBuf::from(root)) {
        Ok(config) => config,
        Err(error) => {
            println!(
                "{}",
                serde_json::json!({"config_error": format!("{error:?}")})
            );
            return;
        }
    };
    let cache = match Cache::open(config).await {
        Ok(cache) => cache,
        Err(error) => {
            println!(
                "{}",
                serde_json::json!({"open_error": format!("{error:?}")})
            );
            return;
        }
    };
    match command.as_str() {
        "open" => println!(
            "{}",
            serde_json::json!({"open": "ok", "mode": format!("{:?}", cache.mode())})
        ),
        "fetch" => {
            let stage = PathBuf::from(&args[2]);
            let count: usize = args[3].parse().expect("a count");
            let requests = (0..count)
                .map(|n| FetchRequest {
                    key: key(n),
                    expect: None,
                })
                .collect();
            println!("started");
            let mut fetch = cache.fetch(requests, FetchOptions::new(stage));
            while fetch.next().await.is_some() {}
            println!("done");
        }
        other => panic!("unknown command {other}"),
    }
}
