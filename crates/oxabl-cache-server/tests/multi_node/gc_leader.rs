//! One node collects garbage; when it dies, another takes over.

use crate::nodes::{Node, has_collected, kill_primary, leads};
use crate::support::fixture::{World, wait_until};
use crate::support::frames;
use crate::support::server::{Client, Options};
use std::time::Duration;

const WAIT: Duration = Duration::from_secs(30);

fn swept_after_the_lease(world: &World, client: &Client) -> bool {
    let pack = frames::pack(world.cache_bytes, &[4_000]);
    let slot = world.reserve_bytes(client, "trusted", &pack.bytes).json()["packs"][0].clone();
    assert_eq!(world.put_slot(&slot, &pack.bytes).status, 200);
    let pack_id = slot["pack_id"].as_str().expect("id").to_owned();
    assert!(world.object_exists(&pack_id));
    wait_until(WAIT, || !world.object_exists(&pack_id))
}

#[test]
fn only_one_node_collects_garbage_and_another_takes_over_when_it_dies() {
    let world = World::start("gc-leader", Options::default());
    let mut node_b = Node::start(&world, "b");
    let (a_metrics, b_metrics) = (world.server.metrics.clone(), node_b.metrics.clone());
    assert!(
        wait_until(WAIT, || leads(&a_metrics) || leads(&b_metrics)),
        "a leader is elected"
    );
    assert!(
        !(leads(&a_metrics) && leads(&b_metrics)),
        "never two leaders"
    );
    let a_leads = leads(&a_metrics);
    let (leader, follower) = if a_leads {
        (&a_metrics, &b_metrics)
    } else {
        (&b_metrics, &a_metrics)
    };
    let through_follower = if a_leads {
        node_b.client(&world.ci)
    } else {
        world.ci.clone()
    };
    assert!(
        swept_after_the_lease(&world, &through_follower),
        "the leader sweeps an expired lease for a pack reserved on the follower"
    );
    assert!(has_collected(leader), "the leader ran garbage collection");
    assert!(
        !has_collected(follower),
        "the follower never ran garbage collection"
    );

    let (survivor, through_survivor) = if a_leads {
        kill_primary(&world);
        (&b_metrics, node_b.client(&world.ci))
    } else {
        node_b.kill();
        (&a_metrics, world.ci.clone())
    };
    assert!(
        wait_until(WAIT, || leads(survivor)),
        "the surviving node becomes the leader"
    );
    assert!(
        swept_after_the_lease(&world, &through_survivor),
        "the new leader sweeps expired leases"
    );
    assert!(has_collected(survivor));
}
