//! One window per person: a second launch hands its activation token to the running app and
//! exits, so the window comes to the front instead of a twin opening.

use std::io::{Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::time::Duration;

fn socket() -> Option<PathBuf> {
    let runtime = std::env::var_os("XDG_RUNTIME_DIR").filter(|v| !v.is_empty())?;
    Some(PathBuf::from(runtime).join("oxabl-cache-desktop.sock"))
}

fn token() -> String {
    ["XDG_ACTIVATION_TOKEN", "DESKTOP_STARTUP_ID"]
        .iter()
        .find_map(|name| std::env::var(name).ok().filter(|v| !v.is_empty()))
        .unwrap_or_default()
}

/// Asks a running app to come forward. True when one answered, so this launch should end.
pub fn forward_to_running() -> bool {
    let Some(path) = socket() else { return false };
    let Ok(mut stream) = UnixStream::connect(&path) else {
        return false;
    };
    let _ = stream.set_write_timeout(Some(Duration::from_millis(500)));
    stream.write_all(token().as_bytes()).is_ok()
}

/// Listens for later launches and calls `activate` with each one's token.
pub fn listen(activate: impl Fn(String) + Send + 'static) {
    let Some(path) = socket() else { return };
    let _ = std::fs::remove_file(&path);
    let Ok(listener) = UnixListener::bind(&path) else {
        return;
    };
    let _ = std::thread::Builder::new()
        .name("oxabl-cache-activation".into())
        .spawn(move || {
            for mut stream in listener.incoming().flatten() {
                let mut token = String::new();
                let _ = stream.set_read_timeout(Some(Duration::from_millis(500)));
                let _ = (&mut stream).take(4096).read_to_string(&mut token);
                activate(token);
            }
        });
}
