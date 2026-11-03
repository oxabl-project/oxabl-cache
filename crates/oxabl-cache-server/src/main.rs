//! `oxabl-cache-server`: one binary for every deployment shape.

mod assembler;
mod cli;
mod commands;
mod config;
mod keys;
mod listener;
mod meta;
mod scheduler;
mod serve;
mod store;
mod tls;
mod wiring;

use cli::Command;
use config::Config;
use std::process::ExitCode;

fn fail(message: &str) -> ExitCode {
    eprintln!("oxabl-cache-server: {message}");
    ExitCode::FAILURE
}

#[tokio::main]
async fn main() -> ExitCode {
    let invocation = match cli::parse(std::env::args().skip(1).collect()) {
        Ok(invocation) => invocation,
        Err(usage) => {
            eprintln!("{usage}");
            return ExitCode::from(2);
        }
    };
    match &invocation.command {
        Command::Help => {
            println!("{}", cli::usage());
            return ExitCode::SUCCESS;
        }
        Command::Unimplemented(name) => {
            eprintln!("oxabl-cache-server: {name} is not implemented in this build");
            return ExitCode::from(2);
        }
        _ => {}
    }
    let config = match Config::load(&invocation.config) {
        Ok(config) => config,
        Err(message) => return fail(&message),
    };
    let outcome = match invocation.command {
        Command::Serve => serve::run(config).await,
        Command::Init => commands::init(&config).await,
        Command::Scrub { cache } => commands::scrub(&config, cache.as_deref()).await,
        Command::CheckStore => commands::check_store(&config).await,
        Command::Unimplemented(_) | Command::Help => Ok(()),
    };
    match outcome {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => fail(&message),
    }
}
