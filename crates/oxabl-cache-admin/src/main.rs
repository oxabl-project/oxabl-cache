//! The oxabl-cache-admin binary: operator command line over the admin API.

mod cli;
mod client;
mod commands;
mod config;
mod error;
mod ids;
mod output;
mod units;

use clap::Parser;
use cli::{CacheCommand, Cli, Command, TokenCommand, UsageCommand};
use client::Admin;
use config::Settings;
use error::Failure;
use std::process::ExitCode;

fn run(cli: Cli) -> Result<(), Failure> {
    let admin = Admin::new(Settings::load(cli.server, cli.token_file, cli.ca_file)?)?;
    let json = cli.json;
    match cli.command {
        Command::Tenant(command) => commands::tenant::run(&admin, json, command),
        Command::Token(TokenCommand::Create(args)) => commands::token::create(
            &admin,
            json,
            &args.tenant,
            &args.scopes,
            args.expires.as_deref(),
            &args.name,
        ),
        Command::OperatorToken(command) => commands::operator_token::run(&admin, json, command),
        Command::Usage(UsageCommand::Export { month, format }) => {
            commands::usage::export(&admin, &month, format, json)
        }
        Command::Cache(CacheCommand::Scrub { cache }) => {
            commands::cache::scrub(&admin, json, &cache)
        }
        Command::Health => commands::health::run(&admin, json),
    }
}

fn main() -> ExitCode {
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(error) => {
            let code = if error.use_stderr() {
                error::EXIT_USAGE
            } else {
                0
            };
            let _ = error.print();
            return ExitCode::from(code);
        }
    };
    let json = cli.json;
    match run(cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(failure) => {
            if json {
                eprintln!(
                    "{}",
                    serde_json::json!({ "error": failure.to_string(), "exit": failure.exit_code() })
                );
            } else {
                eprintln!("oxabl-cache-admin: {failure}");
            }
            ExitCode::from(failure.exit_code())
        }
    }
}
