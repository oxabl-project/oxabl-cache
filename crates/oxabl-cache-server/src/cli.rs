//! The command line: `oxabl-cache-server [--config FILE] <subcommand> [options]`.

use std::path::PathBuf;

const USAGE: &str = "usage: oxabl-cache-server [--config FILE] <subcommand>

subcommands:
  serve                    run the server
  init                     create the schema and key files, print the first operator token once
  scrub [--cache c_...]    re-verify the packs of one cache, or of every cache
  check-store              print the capabilities of the configured object store
  migrate, backup          not implemented in this build";

const DEFAULT_CONFIG: &str = "/etc/oxabl-cache/server.toml";

/// What to run.
#[derive(Debug)]
pub(crate) enum Command {
    Serve,
    Init,
    Scrub { cache: Option<String> },
    CheckStore,
    Unimplemented(&'static str),
    Help,
}

/// A parsed command line.
#[derive(Debug)]
pub(crate) struct Invocation {
    pub config: PathBuf,
    pub command: Command,
}

/// Parses the arguments after the program name; the error is a usage message.
pub(crate) fn parse(args: Vec<String>) -> Result<Invocation, String> {
    let mut config = PathBuf::from(DEFAULT_CONFIG);
    let mut rest = Vec::new();
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--config" | "-c" => {
                config = args
                    .next()
                    .ok_or_else(|| format!("{arg} needs a file\n{USAGE}"))?
                    .into();
            }
            "--help" | "-h" => {
                return Ok(Invocation {
                    config,
                    command: Command::Help,
                });
            }
            _ => rest.push(arg),
        }
    }
    let mut rest = rest.into_iter();
    let command = match rest.next().as_deref() {
        Some("serve") => Command::Serve,
        Some("init") => Command::Init,
        Some("check-store") => Command::CheckStore,
        Some("migrate") => Command::Unimplemented("migrate"),
        Some("backup") => Command::Unimplemented("backup"),
        Some("scrub") => {
            let mut cache = None;
            while let Some(arg) = rest.next() {
                if arg == "--cache" {
                    cache = Some(rest.next().ok_or("--cache needs a cache id")?);
                } else {
                    return Err(format!("unknown scrub option {arg}\n{USAGE}"));
                }
            }
            return Ok(Invocation {
                config,
                command: Command::Scrub { cache },
            });
        }
        Some("help") => Command::Help,
        Some(other) => return Err(format!("unknown subcommand {other}\n{USAGE}")),
        None => return Err(USAGE.to_owned()),
    };
    if let Some(extra) = rest.next()
        && !matches!(command, Command::Unimplemented(_))
    {
        return Err(format!("unexpected argument {extra}\n{USAGE}"));
    }
    Ok(Invocation { config, command })
}

/// The usage text.
pub(crate) fn usage() -> &'static str {
    USAGE
}
