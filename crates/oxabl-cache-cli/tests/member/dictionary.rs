//! `dict train`: a dictionary trained from outputs gains on files it has not seen, and is
//! published only by a `ci` member when the gain is worth it.

use oxabl_cache_client::Mode;
use oxabl_cache_e2e::workdir::TestDir;

use crate::support::cache::{assert_installed, fetch, store};
use crate::support::scenario::Owned;
use crate::support::sets;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_ci_member_publishes_a_dictionary_that_shrinks_what_it_uploads_next() {
    let owned = Owned::start().await;
    let ci = owned.member("ci", "ci", &["read", "write:trusted"]).await;
    let dev = owned
        .member("dev", "developer", &["read", "write:personal"])
        .await;
    let work = TestDir::new("work").expect("work");
    let repo = owned.world.repo.path();

    let before = ci
        .as_member()
        .open(repo, Mode::Ci, &work.path().join("s1"))
        .await
        .expect("open");
    let plain = sets::make_text(&work.path().join("w1"), "plain", 40, 8_000);
    let baseline = store(&before, &plain).await;

    let outputs = TestDir::new("outputs").expect("outputs");
    sets::write_outputs(outputs.path(), "train", 120, 8_000, true);
    let outputs_arg = outputs.path().display().to_string();
    let trained = ci
        .cli(&[
            "dict",
            "train",
            "--outputs",
            &outputs_arg,
            "--publish",
            "--mode",
            "ci",
        ])
        .ok()
        .json();
    assert!(trained["gain"].as_f64() >= Some(1.3), "{trained}");
    assert_eq!(trained["published"]["pointer_version"], 1);
    assert_eq!(trained["held_out"], 24);
    assert_eq!(trained["trained_on"], 96);
    let status = ci.cli(&["status", "--mode", "ci"]).ok().json();
    assert_eq!(status["server"]["dictionary_version"], 1);

    let after = ci
        .as_member()
        .open(repo, Mode::Ci, &work.path().join("s2"))
        .await
        .expect("open");
    let tuned = sets::make_text(&work.path().join("w2"), "tuned", 40, 8_000);
    let uploaded = store(&after, &tuned).await;
    assert!(
        uploaded * 10 < baseline * 8,
        "the dictionary must pay off: {uploaded} bytes against {baseline}"
    );

    let reader = dev
        .as_member()
        .open(repo, Mode::Developer, &work.path().join("s3"))
        .await
        .expect("open");
    let all: Vec<_> = plain.into_iter().chain(tuned).collect();
    let stage = work.path().join("stage");
    assert_installed(&fetch(&reader, &all, &stage).await, &all, &stage);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_dictionary_that_gains_too_little_or_comes_from_a_non_ci_member_is_not_published() {
    let owned = Owned::start().await;
    let ci = owned.member("ci", "ci", &["read", "write:trusted"]).await;
    let dev = owned
        .member("dev", "developer", &["read", "write:personal"])
        .await;

    let noise = TestDir::new("noise").expect("noise");
    sets::write_outputs(noise.path(), "noise", 60, 8_000, false);
    let noise_arg = noise.path().display().to_string();
    let refused = ci
        .cli(&[
            "dict",
            "train",
            "--outputs",
            &noise_arg,
            "--publish",
            "--mode",
            "ci",
        ])
        .exits(1);
    assert!(refused.stderr.contains("below 1.3x"), "{}", refused.stderr);

    let outputs = TestDir::new("outputs").expect("outputs");
    sets::write_outputs(outputs.path(), "train", 120, 8_000, true);
    let outputs_arg = outputs.path().display().to_string();
    let not_ci = dev
        .cli(&[
            "dict",
            "train",
            "--outputs",
            &outputs_arg,
            "--publish",
            "--mode",
            "ci",
        ])
        .exits(1);
    assert!(not_ci.stderr.contains("ci member"), "{}", not_ci.stderr);

    let status = ci.cli(&["status", "--mode", "ci"]).ok().json();
    assert_eq!(
        status["server"]["dictionary_version"], 0,
        "nothing was published"
    );

    let few = TestDir::new("few").expect("few");
    sets::write_outputs(few.path(), "few", 3, 1_000, true);
    ci.cli(&[
        "dict",
        "train",
        "--outputs",
        &few.path().display().to_string(),
    ])
    .exits(1);
}
