//! The command line: `oxabl-cache-server [--config FILE] <subcommand> [options]`.

use std::path::PathBuf;

const USAGE: &str = "usage: oxabl-cache-server [--config FILE] <subcommand>

subcommands:
  serve                    run the server
  init                     create the schema and key files, print the first operator token once
  scrub [--cache c_...]    re-verify the packs of one cache, or of every cache
  check-store              print the capabilities of the configured object store
  backup --to FILE         write a consistent copy of the SQLite metadata database (online)
  migrate --from sqlite://PATH --to postgres://URL [--password-file F] [--ca-file F]
                           copy the metadata of a SQLite database into an empty Postgres one";

const DEFAULT_CONFIG: &str = "/etc/oxabl-cache/server.toml";

/// What to run.
#[derive(Debug)]
pub(crate) enum Command {
    Serve,
    Init,
    Scrub { cache: Option<String> },
    CheckStore,
    Backup { to: PathBuf },
    Migrate(Migration),
    Help,
}

/// The arguments of `migrate`.
#[derive(Debug)]
pub(crate) struct Migration {
    pub from: String,
    pub to: String,
    pub password_file: Option<PathBuf>,
    pub ca_file: Option<PathBuf>,
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
        Some("backup") => {
            let mut to = None;
            while let Some(arg) = rest.next() {
                if arg == "--to" {
                    to = Some(PathBuf::from(rest.next().ok_or("--to needs a file")?));
                } else {
                    return Err(format!("unknown backup option {arg}\n{USAGE}"));
                }
            }
            let to = to.ok_or_else(|| format!("backup needs --to FILE\n{USAGE}"))?;
            return Ok(Invocation {
                config,
                command: Command::Backup { to },
            });
        }
        Some("migrate") => {
            let (mut from, mut to) = (None, None);
            let (mut password_file, mut ca_file) = (None, None);
            while let Some(arg) = rest.next() {
                let mut value = |what: &str| rest.next().ok_or(format!("{arg} needs {what}"));
                match arg.as_str() {
                    "--from" => from = Some(value("a sqlite:// URL")?),
                    "--to" => to = Some(value("a postgres:// URL")?),
                    "--password-file" => password_file = Some(PathBuf::from(value("a file")?)),
                    "--ca-file" => ca_file = Some(PathBuf::from(value("a file")?)),
                    _ => return Err(format!("unknown migrate option {arg}\n{USAGE}")),
                }
            }
            let (Some(from), Some(to)) = (from, to) else {
                return Err(format!("migrate needs --from and --to\n{USAGE}"));
            };
            return Ok(Invocation {
                config,
                command: Command::Migrate(Migration {
                    from,
                    to,
                    password_file,
                    ca_file,
                }),
            });
        }
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
    if let Some(extra) = rest.next() {
        return Err(format!("unexpected argument {extra}\n{USAGE}"));
    }
    Ok(Invocation { config, command })
}

/// The usage text.
pub(crate) fn usage() -> &'static str {
    USAGE
}
