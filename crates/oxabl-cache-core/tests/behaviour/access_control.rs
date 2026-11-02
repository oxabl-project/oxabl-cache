//! Scopes, namespaces and tenant isolation.

use crate::support::{Env, entry, lookup_id};
use oxabl_cache_core::domain::entry::EntryStatus::Committed;
use oxabl_cache_core::domain::namespace::NamespaceKind::{Personal, Trusted};
use oxabl_cache_core::domain::scope::{Grant, Scope, ScopeSet};
use oxabl_cache_core::ids::CacheId;
use oxabl_cache_core::usecases::{CacheSpec, LookupRequest, NamespaceSet, TenantSpec};
use oxabl_cache_core::{Error, domain::principal::Caller};

#[tokio::test]
async fn a_read_scope_cannot_reserve_or_commit() {
    let env = Env::new().await;
    let built = env.artifacts(&[1], 2048);
    let reserve = env.reserve(&env.reader, Trusted, &built).await;
    assert!(
        matches!(reserve, Err(Error::Forbidden(_))),
        "reader reserving trusted"
    );
    let reserve = env.reserve(&env.reader, Personal, &built).await;
    assert!(
        matches!(reserve, Err(Error::Forbidden(_))),
        "reader reserving personal"
    );

    let pack = env.upload(&env.ci, Trusted, &built).await;
    let commit = env
        .commit(&env.reader, pack, Trusted, &built, vec![entry(1, 0)])
        .await;
    assert!(
        matches!(commit, Err(Error::Forbidden(_))),
        "reader committing"
    );
    assert_eq!(
        env.hits(&env.reader, &[1], false).await,
        Vec::<usize>::new()
    );
}

#[tokio::test]
async fn a_developer_cannot_write_the_trusted_namespace() {
    let env = Env::new().await;
    let built = env.artifacts(&[1], 2048);
    let reserve = env.reserve(&env.dev, Trusted, &built).await;
    assert!(
        matches!(reserve, Err(Error::Forbidden(_))),
        "developer reserving trusted"
    );

    let pack = env.upload(&env.dev, Personal, &built).await;
    let commit = env
        .commit(&env.dev, pack, Trusted, &built, vec![entry(1, 0)])
        .await;
    assert!(
        matches!(commit, Err(Error::Forbidden(_))),
        "a trusted frame on a personal pack"
    );
    let commit = env
        .commit(&env.dev, pack, Personal, &built, vec![entry(1, 0)])
        .await;
    assert_eq!(
        commit,
        Ok(vec![Committed]),
        "the same pack commits into the developer's own namespace"
    );
}

#[tokio::test]
async fn ci_cannot_write_a_personal_namespace_and_a_developer_cannot_commit_a_peers_pack() {
    let env = Env::new().await;
    let built = env.artifacts(&[1], 2048);
    assert!(matches!(
        env.reserve(&env.ci, Personal, &built).await,
        Err(Error::Forbidden(_))
    ));

    let pack = env.upload(&env.dev, Personal, &built).await;
    let stolen = env
        .commit(&env.dev2, pack, Personal, &built, vec![entry(1, 0)])
        .await;
    assert!(
        matches!(stolen, Err(Error::Forbidden(_))),
        "only the uploader may commit"
    );
    let still_open = env
        .commit(&env.dev, pack, Personal, &built, vec![entry(1, 0)])
        .await;
    assert_eq!(still_open, Ok(vec![Committed]));
}

#[tokio::test]
async fn another_users_personal_namespace_is_invisible_but_trusted_is_shared() {
    let env = Env::new().await;
    let mine = env.artifacts(&[1], 2048);
    env.publish(&env.dev, Personal, &mine, vec![entry(1, 0)])
        .await;
    let shared = env.artifacts(&[2], 2048);
    env.publish(&env.ci, Trusted, &shared, vec![entry(2, 0)])
        .await;

    assert_eq!(
        env.hits(&env.dev, &[1, 2], true).await,
        vec![0, 1],
        "the owner sees both"
    );
    assert_eq!(
        env.hits(&env.dev2, &[1, 2], true).await,
        vec![1],
        "a peer sees only trusted"
    );
    assert_eq!(env.hits(&env.reader, &[1, 2], true).await, vec![1]);

    let asked_for_peer = env
        .engine
        .lookup(
            &env.dev2,
            LookupRequest {
                cache: env.cache,
                namespaces: NamespaceSet {
                    trusted: false,
                    personal: true,
                },
                ids: vec![lookup_id(1)],
                records: true,
                urls: false,
            },
        )
        .await
        .expect("lookup");
    assert!(
        asked_for_peer.hits.is_empty(),
        "asking for personal returns the caller's own namespace only"
    );
}

#[tokio::test]
async fn a_cache_of_another_tenant_is_not_found() {
    let env = Env::new().await;
    let other_tenant = env
        .engine
        .create_tenant(TenantSpec {
            name: "other".into(),
            hard_bytes: None,
            egress_bytes: None,
            rate: 50,
            burst: 200,
        })
        .await
        .expect("tenant");
    let other_admin = Caller {
        tenant: other_tenant,
        principal: oxabl_cache_core::ids::PrincipalId::random(),
        grants: vec![Grant {
            cache: None,
            scopes: ScopeSet::EMPTY.with(Scope::TenantAdmin),
        }],
    };
    let other_cache = env
        .engine
        .create_cache(
            &other_admin,
            CacheSpec {
                display_name: "theirs".into(),
                retention_days: None,
                hard_bytes: None,
                owner_keys: vec![env.keys.owner_public()],
            },
        )
        .await
        .expect("cache");

    assert_eq!(
        env.engine.cache_info(&env.reader, other_cache).await.err(),
        Some(Error::NotFound)
    );
    assert_eq!(
        env.engine.cache_info(&other_admin, env.cache).await.err(),
        Some(Error::NotFound)
    );
    let built = env.artifacts(&[1], 2048);
    let forged = Caller {
        grants: vec![Grant {
            cache: Some(other_cache),
            scopes: ScopeSet::EMPTY.with(Scope::WriteTrusted),
        }],
        ..env.ci.clone()
    };
    let reserve = env
        .engine
        .reserve_uploads(
            &forged,
            other_cache,
            Trusted,
            vec![oxabl_cache_core::usecases::PackRequest {
                client_ref: oxabl_cache_core::ids::ClientRef::random(),
                size: built.bytes.len() as u64,
                sha256: None,
            }],
        )
        .await;
    assert_eq!(
        reserve.err(),
        Some(Error::NotFound),
        "the tenant scope applies before the scope check"
    );
    let unknown = env.engine.cache_info(&env.reader, CacheId::random()).await;
    assert_eq!(unknown.err(), Some(Error::NotFound));
}

#[tokio::test]
async fn administration_needs_its_own_scopes() {
    let env = Env::new().await;
    let roster = env.keys.owner_signed_roster(1, 1);
    let denied = env
        .engine
        .put_roster(&env.dev, env.cache, 0, roster.clone())
        .await;
    assert!(matches!(denied, Err(Error::Forbidden(_))));
    assert!(matches!(
        env.engine
            .purge(
                &env.dev,
                env.cache,
                oxabl_cache_core::usecases::PurgeScope::BelowEpoch(1)
            )
            .await,
        Err(Error::Forbidden(_))
    ));
    assert!(matches!(
        env.engine
            .create_user(&env.dev, "i".into(), "s".into(), "l".into())
            .await,
        Err(Error::Forbidden(_))
    ));
    env.engine
        .put_roster(&env.cache_admin, env.cache, 0, roster)
        .await
        .expect("cache admin publishes");
}
