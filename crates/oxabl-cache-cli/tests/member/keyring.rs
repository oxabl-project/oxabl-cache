//! A passphrase kept in the operating system's keyring unlocks the identity file, so no prompt
//! and no passphrase file is needed; forgetting it brings the prompt back.

use oxabl_cache_e2e::secret_service::SecretService;

use crate::support::cli::run;
use crate::support::person::{Person, write_secret};
use crate::support::scenario::Owned;

/// The person's flags without the passphrase file.
fn without_passphrase(person: &Person) -> Vec<String> {
    let flags = person.flags();
    let at = flags
        .iter()
        .position(|f| f == "--passphrase-file")
        .expect("passphrase flag");
    flags
        .iter()
        .enumerate()
        .filter(|(i, _)| *i != at && *i != at + 1)
        .map(|(_, f)| f.clone())
        .collect()
}

fn status(person: &Person, env: &[(&str, &str)]) -> crate::support::cli::Output {
    let mut args = vec!["status".to_owned(), "--json".to_owned()];
    args.extend(without_passphrase(person));
    run(person.dir.path(), &args, env)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_remembered_passphrase_opens_the_cache_without_asking_until_it_is_forgotten() {
    let service = tokio::task::spawn_blocking(SecretService::start)
        .await
        .expect("join")
        .expect("a private secret service");
    let vars = service.environment();
    let env: Vec<(&str, &str)> = vars.iter().map(|(n, v)| (n.as_str(), v.as_str())).collect();
    let owned = Owned::start().await;
    let dev = owned
        .member("dev", "developer", &["read", "write:personal"])
        .await;

    status(&dev, &env).exits(2);

    let wrong = dev.file("wrong-passphrase");
    write_secret(&wrong, "not the passphrase");
    let mut refused = vec![
        "identity".to_owned(),
        "remember".to_owned(),
        "--json".to_owned(),
    ];
    refused.extend(without_passphrase(&dev));
    refused.extend(["--passphrase-file".to_owned(), wrong.display().to_string()]);
    run(dev.dir.path(), &refused, &env).exits(1);
    status(&dev, &env).exits(2);

    let remembered = dev.cli_with(&["identity", "remember"], &env).ok().json();
    assert_eq!(remembered["remembered"], true);
    let opened = status(&dev, &env).ok().json();
    assert_eq!(opened["on"], true, "{opened}");
    assert_eq!(opened["principal"]["label"], "dev");
    status(&dev, &[]).exits(2);

    let forgotten = dev.cli_with(&["identity", "forget"], &env).ok().json();
    assert_eq!(forgotten["remembered"], false);
    status(&dev, &env).exits(2);
    dev.cli_with(&["identity", "forget"], &env).ok();
}
