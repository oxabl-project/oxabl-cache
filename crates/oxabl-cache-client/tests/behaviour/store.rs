//! Storing: lazy verified reads, what is never stored, and how sets are refused.

use std::fs;
use std::os::unix::fs::symlink;
use std::time::Duration;

use oxabl_cache_client::{
    ArtifactFileRef, ArtifactPath, FetchOutcome, MissReason, Mode, StoreFailure, StoreOutcome,
};

use crate::support::fixture::{self, key};
use crate::support::world::{World, assert_installed};

fn outcome_of(report: &oxabl_cache_client::StoreReport, n: usize) -> StoreOutcome {
    report
        .outcomes
        .iter()
        .find(|(k, _)| *k == key(n))
        .map(|(_, o)| *o)
        .unwrap_or_else(|| panic!("no outcome for key {n}"))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_file_edited_after_put_is_not_stored() {
    let world = World::start().await;
    world
        .proxy
        .delay_dictionary(Some(Duration::from_millis(800)));
    let ci = world.machine(&world.ci, Mode::Ci).await;
    let sets = fixture::sets(&ci.workspace(), 3, "edited");
    let uploader = ci.cache.uploader().expect("uploader");
    for set in &sets {
        uploader.put(set.key, set.refs()).await;
    }
    let same_size = &sets[0].files[0];
    let mut flipped = same_size.bytes.clone();
    flipped[100] ^= 0xff;
    fs::write(&same_size.disk, flipped).expect("rewrite");
    let other_size = &sets[1].files[0];
    fs::write(&other_size.disk, [&other_size.bytes[..], b"more"].concat()).expect("rewrite");
    let report = uploader.finish().await;

    assert_eq!(
        outcome_of(&report, 0),
        StoreOutcome::Changed,
        "same size, other bytes"
    );
    assert_eq!(outcome_of(&report, 1), StoreOutcome::Changed, "other size");
    assert_eq!(outcome_of(&report, 2), StoreOutcome::Committed);

    let reader = world.machine(&world.dev, Mode::Developer).await;
    let outcomes = reader.fetch(&sets).await;
    let found = |n: usize| {
        outcomes
            .iter()
            .find(|o| match o {
                FetchOutcome::Installed { key: k, .. } | FetchOutcome::Miss { key: k, .. } => {
                    *k == key(n)
                }
            })
            .cloned()
            .expect("outcome")
    };
    assert!(matches!(
        found(0),
        FetchOutcome::Miss {
            reason: MissReason::NotFound,
            ..
        }
    ));
    assert!(matches!(
        found(1),
        FetchOutcome::Miss {
            reason: MissReason::NotFound,
            ..
        }
    ));
    assert!(matches!(found(2), FetchOutcome::Installed { .. }));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_wrong_hash_from_the_start_is_changed_and_unreadable_files_are_reported() {
    let world = World::start().await;
    let ci = world.machine(&world.ci, Mode::Ci).await;
    let sets = fixture::sets(&ci.workspace(), 8, "unreadable");
    let uploader = ci.cache.uploader().expect("uploader");

    let mut wrong = sets[0].refs();
    wrong[0].blake3[0] ^= 1;
    uploader.put(sets[0].key, wrong).await;

    let mut missing = sets[1].refs();
    missing[0].disk_path = ci.workspace().join("never-written.r");
    uploader.put(sets[1].key, missing).await;

    let mut relative = sets[2].refs();
    relative[0].disk_path = "r/ar/unit2.r".into();
    uploader.put(sets[2].key, relative).await;

    let target = ci.workspace().join("elsewhere.r");
    fs::write(&target, &sets[3].files[0].bytes).expect("write");
    let link = ci.workspace().join("link.r");
    symlink(&target, &link).expect("link");
    let mut linked = sets[3].refs();
    linked[0].disk_path = link;
    uploader.put(sets[3].key, linked).await;

    let mut directory = sets[4].refs();
    directory[0].disk_path = ci.workspace().join("r/ar");
    uploader.put(sets[4].key, directory).await;

    let real = ci.workspace().join("real");
    fs::create_dir_all(&real).expect("directory");
    fs::write(real.join("file.r"), &sets[5].files[0].bytes).expect("write");
    let via = ci.workspace().join("via");
    symlink(&real, &via).expect("link");
    let mut through_a_link = sets[5].refs();
    through_a_link[0].disk_path = via.join("file.r");
    uploader.put(sets[5].key, through_a_link).await;
    uploader.put(sets[6].key, sets[6].refs()).await;
    let report = uploader.finish().await;

    assert_eq!(outcome_of(&report, 0), StoreOutcome::Changed);
    for n in 1..=4 {
        assert_eq!(
            outcome_of(&report, n),
            StoreOutcome::Failed(StoreFailure::Unreadable),
            "set {n}"
        );
    }
    assert_eq!(
        outcome_of(&report, 5),
        StoreOutcome::Committed,
        "a symlinked intermediate directory is harmless"
    );
    assert_eq!(outcome_of(&report, 6), StoreOutcome::Committed);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn sets_that_break_a_rule_are_rejected_without_failing_the_session() {
    let world = World::start().await;
    let ci = world.machine(&world.ci, Mode::Ci).await;
    let sets = fixture::sets(&ci.workspace(), 5, "rules");
    let uploader = ci.cache.uploader().expect("uploader");

    let mut duplicate = sets[0].refs();
    duplicate.push(duplicate[0].clone());
    uploader.put(sets[0].key, duplicate).await;

    let base = sets[1].refs()[0].clone();
    let many: Vec<ArtifactFileRef> = (0..17)
        .map(|n| ArtifactFileRef {
            path: ArtifactPath::new(&format!("r/many/{n}.r")).expect("path"),
            ..base.clone()
        })
        .collect();
    uploader.put(sets[1].key, many).await;

    let long: Vec<ArtifactFileRef> = (0..4)
        .map(|n| ArtifactFileRef {
            path: ArtifactPath::new(&format!("r/{}/{n}.r", "d".repeat(900))).expect("path"),
            ..base.clone()
        })
        .collect();
    uploader.put(sets[2].key, long).await;
    uploader.put(sets[3].key, Vec::new()).await;
    uploader.put(sets[4].key, sets[4].refs()).await;
    let report = uploader.finish().await;
    for n in 0..=3 {
        assert_eq!(
            outcome_of(&report, n),
            StoreOutcome::Failed(StoreFailure::Rejected),
            "set {n}"
        );
    }
    assert_eq!(outcome_of(&report, 4), StoreOutcome::Committed);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_set_over_the_plaintext_cap_is_too_large() {
    let world = World::start().await;
    let ci = world
        .open_tuned(&world.ci, Mode::Ci, |c| c.with_max_plain_bytes(10_000))
        .await;
    let sets = fixture::sets(&ci.workspace(), 3, "cap");
    let report = ci.store(&sets).await;
    let too_large = report
        .outcomes
        .iter()
        .filter(|(_, o)| *o == StoreOutcome::Failed(StoreFailure::TooLarge))
        .count();
    let committed = report
        .outcomes
        .iter()
        .filter(|(_, o)| *o == StoreOutcome::Committed)
        .count();
    assert!(
        too_large >= 1 && too_large + committed == 3,
        "{:?}",
        report.outcomes
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_non_default_output_layout_stores_and_installs_a_thousand_keys() {
    let world = World::start().await;
    let ci = world.machine(&world.ci, Mode::Ci).await;
    let sets = fixture::layout(
        &ci.workspace(),
        1_000,
        "layout",
        ("rcode", "reports/xref", "reports/dbg"),
    );
    assert!(sets[0].files[0].disk.to_string_lossy().contains("/rcode/"));
    assert!(sets[0].files[0].path.starts_with("r/"));
    let report = ci.store(&sets).await;
    assert!(
        report
            .outcomes
            .iter()
            .all(|(_, o)| *o == StoreOutcome::Committed),
        "{:?}",
        report
            .outcomes
            .iter()
            .find(|(_, o)| *o != StoreOutcome::Committed)
    );

    let reader = world.machine(&world.dev, Mode::Developer).await;
    let outcomes = reader.fetch(&sets).await;
    assert_eq!(outcomes.len(), 1_000);
    assert!(
        outcomes
            .iter()
            .all(|o| matches!(o, FetchOutcome::Installed { .. }))
    );
    assert_installed(&reader.stage(), &sets);
}
