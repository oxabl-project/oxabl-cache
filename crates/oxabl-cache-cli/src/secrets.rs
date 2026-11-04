//! Secrets come from a terminal, a file named by a flag or the `OXABL_CACHE_*` variables the
//! library reads, never from arguments.

use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use nix::sys::termios::{self, LocalFlags, SetArg, Termios};
use oxabl_cache_client::SecretString;
use zeroize::Zeroizing;

use crate::failure::Failure;

const MAX_FILE_BYTES: u64 = 4096;
const PASSPHRASE_VARIABLE: &str = "OXABL_CACHE_IDENTITY_PASSPHRASE";

/// The identity file's passphrase from the environment, if set and not empty.
pub fn environment_passphrase() -> Option<SecretString> {
    std::env::var(PASSPHRASE_VARIABLE)
        .ok()
        .filter(|value| !value.is_empty())
        .map(SecretString::from)
}

/// The content of a secret file: a regular file of at most 4 KiB that no group or other user
/// can access.
pub fn read_secret_file(path: &Path) -> Result<Zeroizing<String>, Failure> {
    let unusable = |why: &str| Failure::usage(format!("{}: {why}", path.display()));
    let file = File::open(path).map_err(|e| unusable(&e.to_string()))?;
    let meta = file.metadata().map_err(|e| unusable(&e.to_string()))?;
    if !meta.is_file() || meta.len() > MAX_FILE_BYTES {
        return Err(unusable("not a regular file of at most 4 KiB"));
    }
    if meta.permissions().mode() & 0o077 != 0 {
        return Err(unusable("readable by group or others; chmod 600 it"));
    }
    let mut text = Zeroizing::new(String::new());
    file.take(MAX_FILE_BYTES)
        .read_to_string(&mut text)
        .map_err(|_| unusable("not UTF-8 text"))?;
    Ok(text)
}

/// A passphrase file holds the passphrase and at most one trailing newline.
pub fn read_passphrase_file(path: &Path) -> Result<SecretString, Failure> {
    let text = read_secret_file(path)?;
    let passphrase = text
        .strip_suffix('\n')
        .map(|t| t.strip_suffix('\r').unwrap_or(t))
        .unwrap_or(&text);
    if passphrase.is_empty() {
        return Err(Failure::usage(format!(
            "{}: the passphrase is empty",
            path.display()
        )));
    }
    Ok(SecretString::from(passphrase.to_owned()))
}

/// A token file holds one token.
pub fn read_token_file(path: &Path) -> Result<SecretString, Failure> {
    let text = read_secret_file(path)?;
    let token = text.trim();
    if token.is_empty() || token.contains(char::is_whitespace) {
        return Err(Failure::usage(format!(
            "{}: the file must hold exactly one token",
            path.display()
        )));
    }
    Ok(SecretString::from(token.to_owned()))
}

/// Whether a controlling terminal can be prompted.
pub fn has_terminal() -> bool {
    OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/tty")
        .is_ok()
}

struct Silenced<'a> {
    tty: &'a File,
    original: Termios,
}

impl Drop for Silenced<'_> {
    fn drop(&mut self) {
        let _ = termios::tcsetattr(self.tty, SetArg::TCSAFLUSH, &self.original);
    }
}

/// Reads one line from the controlling terminal with echo off.
pub fn prompt(label: &str) -> Option<SecretString> {
    let tty = OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/tty")
        .ok()?;
    let original = termios::tcgetattr(&tty).ok()?;
    let mut silent = original.clone();
    silent.local_flags.remove(LocalFlags::ECHO);
    termios::tcsetattr(&tty, SetArg::TCSAFLUSH, &silent).ok()?;
    let _restore = Silenced {
        tty: &tty,
        original,
    };
    let mut out = &tty;
    write!(out, "{label}: ").ok()?;
    out.flush().ok()?;
    let mut line = Zeroizing::new(String::new());
    let read = BufReader::new(&tty).read_line(&mut line).ok()?;
    let _ = writeln!(out);
    let secret = line.trim_end_matches(['\n', '\r']);
    (read > 0 && !secret.is_empty()).then(|| SecretString::from(secret.to_owned()))
}

/// Prompts twice and returns the passphrase only when both entries agree.
pub fn prompt_new(label: &str) -> Option<SecretString> {
    use oxabl_cache_client::ExposeSecret;
    let first = prompt(label)?;
    let second = prompt(&format!("{label} (again)"))?;
    (first.expose_secret() == second.expose_secret()).then_some(first)
}
