//! A published dictionary shrinks what writers upload and readers still install byte for byte.

use oxabl_cache_client::{FetchOutcome, Mode, StoreOutcome};
use oxabl_cache_format::{CacheId, Namespace};

use crate::support::fixture::{self, vocabulary_text};
use crate::support::forge::Forgery;
use crate::support::world::{World, assert_installed, identity_of};

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn writers_use_a_verified_dictionary_and_readers_fetch_it_with_the_entries() {
    let world = World::start().await;
    let before = world.machine(&world.ci, Mode::Ci).await;
    let plain = fixture::text_sets(&before.workspace(), 40, "plain", 8_000);
    let baseline = before.store(&plain).await.bytes_uploaded;

    let samples: Vec<Vec<u8>> = (0..300)
        .map(|n| vocabulary_text(&format!("training-{n}"), 8_000))
        .collect();
    let dictionary = oxabl_cache_crypto::train(&samples, 16 * 1024).expect("train");
    let ci = identity_of(&world.ci);
    Forgery {
        world: &world,
        holder: &ci,
        signer: &ci,
        namespace: Namespace::Trusted,
        token: &world.ci.token,
        target: &world.cache,
        record_cache: CacheId::from_text(&world.cache).expect("id"),
        paths: None,
    }
    .publish_dictionary(&dictionary)
    .await;

    let writer = world.machine(&world.ci, Mode::Ci).await;
    let tuned = fixture::text_sets(&writer.workspace(), 40, "tuned", 8_000);
    let report = writer.store(&tuned).await;
    assert!(
        report
            .outcomes
            .iter()
            .all(|(_, o)| *o == StoreOutcome::Committed)
    );
    assert!(
        report.bytes_uploaded * 10 < baseline * 8,
        "the dictionary must pay off: {} bytes against {baseline}",
        report.bytes_uploaded
    );

    let reader = world.machine(&world.dev, Mode::Developer).await;
    let all: Vec<_> = plain.iter().chain(&tuned).cloned().collect();
    let outcomes = reader.fetch(&all).await;
    assert!(
        outcomes
            .iter()
            .all(|o| matches!(o, FetchOutcome::Installed { .. })),
        "{outcomes:?}"
    );
    assert_installed(&reader.stage(), &all);
}
