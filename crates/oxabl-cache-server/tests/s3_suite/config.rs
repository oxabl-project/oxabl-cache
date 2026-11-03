//! The `[store]` section for kind s3 is strict, and its secrets live in files.

use std::fs;
use std::path::Path;
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_oxabl-cache-server");

fn check_store(name: &str, store: &str) -> String {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("dir");
    fs::write(dir.join("empty"), "").expect("empty file");
    let store = store.replace("{dir}", &dir.display().to_string());
    let config = format!(
        "[server]\nlisten = \"127.0.0.1:0\"\n[metadata]\nurl = \"sqlite://{dir}/meta.db\"\n\
         [store]\n{store}\n[keys]\nurl_key_file = \"{dir}/url.key\"\n",
        dir = dir.display(),
    );
    let path = dir.join("server.toml");
    fs::write(&path, config).expect("config");
    let output = Command::new(BIN)
        .arg("-c")
        .arg(&path)
        .arg("check-store")
        .output()
        .expect("run");
    let _ = fs::remove_dir_all(&dir);
    assert!(
        !output.status.success(),
        "{name}: the configuration was accepted"
    );
    String::from_utf8_lossy(&output.stderr).into_owned()
}

const COMPLETE: &str = "kind = \"s3\"\nendpoint = \"http://127.0.0.1:1\"\nregion = \"r\"\n\
                        bucket = \"a-bucket\"\npath_style = true\naccess_key_id_file = \"/nonexistent/id\"\n\
                        secret_access_key_file = \"/nonexistent/secret\"\n";

#[test]
fn an_incomplete_or_mixed_s3_section_is_refused_and_secrets_are_not_inline() {
    for (key, needle) in [
        ("endpoint", "store.endpoint"),
        ("region", "store.region"),
        ("bucket", "store.bucket"),
        ("access_key_id_file", "access_key_id_file"),
        ("secret_access_key_file", "secret_access_key_file"),
    ] {
        let without: String = COMPLETE
            .lines()
            .filter(|line| !line.starts_with(key))
            .map(|line| format!("{line}\n"))
            .collect();
        let stderr = check_store(&format!("s3-config-no-{key}"), &without);
        assert!(stderr.contains(needle), "{key}: {stderr}");
    }
    let inline = check_store(
        "s3-config-inline",
        &format!("{COMPLETE}secret_access_key = \"abc\"\n"),
    );
    assert!(inline.contains("secret_access_key"), "{inline}");
    let mixed = check_store("s3-config-root", &format!("{COMPLETE}root = \"/tmp/x\"\n"));
    assert!(mixed.contains("store.root"), "{mixed}");
    let bad_bucket = check_store(
        "s3-config-bucket",
        &COMPLETE.replace("a-bucket", "Not A Bucket"),
    );
    assert!(bad_bucket.contains("store.bucket"), "{bad_bucket}");
    let virtual_host = check_store(
        "s3-config-virtual-host",
        &COMPLETE.replace("path_style = true", "path_style = false"),
    );
    assert!(
        virtual_host.contains("store.endpoint"),
        "an IP address cannot carry a bucket subdomain: {virtual_host}"
    );
    let unreadable = check_store("s3-config-files", COMPLETE);
    assert!(unreadable.contains("cannot read"), "{unreadable}");
    let empty = check_store(
        "s3-config-empty",
        &COMPLETE
            .replace("/nonexistent/id", "{dir}/empty")
            .replace("/nonexistent/secret", "{dir}/empty"),
    );
    assert!(empty.contains("is empty"), "{empty}");
}
