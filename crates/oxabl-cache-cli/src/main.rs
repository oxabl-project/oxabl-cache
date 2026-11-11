//! The oxabl-cache member command line: identities, roster, rotation, login.

mod cli;
mod commands;
mod context;
mod failure;
mod output;
mod secrets;

use std::process::ExitCode;

use clap::Parser;

use cli::{Cli, Command, DictCommand, IdentityCommand, OnboardCommand};
use context::Context;
use failure::{EXIT_USAGE, Failure};

async fn run(cli: Cli) -> Result<(), Failure> {
    let context = Context::new(&cli)?;
    match cli.command {
        Command::Identity(IdentityCommand::New(args)) => commands::identity::new(&context, args),
        Command::Identity(IdentityCommand::Remember) => {
            commands::identity::remember(&context).await
        }
        Command::Identity(IdentityCommand::Forget) => commands::identity::forget(&context),
        Command::Init(args) => commands::init::run(&context, args).await,
        Command::Member(command) => commands::member::run(&context, command).await,
        Command::Rotate { purge } => commands::member::rotate(&context, purge).await,
        Command::Owner(command) => commands::owner::run(&context, command).await,
        Command::Token(command) => commands::token::run(&context, command).await,
        Command::Onboard(OnboardCommand::Ci(args)) => commands::onboard::ci(&context, args).await,
        Command::Invite(args) => commands::invite::run(&context, args),
        Command::Login(args) => commands::login::run(&context, args).await,
        Command::Logout => commands::login::run_logout(&context).await,
        Command::Status => commands::status::run(&context).await,
        Command::Dict(command @ DictCommand::Train { .. }) => {
            commands::dict::run(&context, command).await
        }
        Command::Verify(args) => commands::verify::run(&context, args).await,
        Command::Fetch(args) => commands::fetch::run(&context, args).await,
        Command::Put(args) => commands::put::run(&context, args).await,
    }
}

fn main() -> ExitCode {
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(error) => {
            let code = if error.use_stderr() { EXIT_USAGE } else { 0 };
            let _ = error.print();
            return ExitCode::from(code);
        }
    };
    let json = cli.json;
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("oxabl-cache: cannot start the runtime: {error}");
            return ExitCode::FAILURE;
        }
    };
    match runtime.block_on(run(cli)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(failure) => {
            if json {
                eprintln!(
                    "{}",
                    serde_json::json!({"error": failure.to_string(), "exit": failure.exit_code()})
                );
            } else {
                eprintln!("oxabl-cache: {failure}");
            }
            ExitCode::from(failure.exit_code())
        }
    }
}
