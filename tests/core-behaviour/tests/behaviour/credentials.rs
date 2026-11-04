//! Tokens and sessions: only hashes are stored, and every refusal is `Unauthenticated`.

use crate::support::Env;
use oxabl_cache_core::Error;
use oxabl_cache_core::domain::scope::{Scope, ScopeSet};
use oxabl_cache_core::domain::tenant::TenantStatus;
use oxabl_cache_core::usecases::TokenSpec;

fn spec(env: &Env, scopes: &[Scope]) -> TokenSpec {
    TokenSpec {
        name: "ci token".into(),
        principal: None,
        scopes: scopes.iter().copied().collect(),
        caches: vec![env.cache],
        expires_in_secs: None,
    }
}

#[tokio::test]
async fn a_token_authenticates_with_exactly_the_scopes_it_was_created_with() {
    let env = Env::new().await;
    let created = env
        .engine
        .create_token(&env.admin, spec(&env, &[Scope::Read, Scope::WriteTrusted]))
        .await
        .expect("create");
    assert!(created.secret.starts_with("oxc_"));
    let caller = env
        .engine
        .authenticate(&created.secret)
        .await
        .expect("authenticate");
    assert_eq!(caller.tenant, env.tenant);
    assert_eq!(caller.principal, created.principal);
    assert!(caller.can(&env.cache, Scope::WriteTrusted));
    assert!(caller.can(&env.cache, Scope::Read));
    assert!(!caller.can(&env.cache, Scope::WritePersonal));
    assert!(!caller.is_tenant_admin());
}

#[tokio::test]
async fn a_wrong_secret_an_unknown_id_and_a_malformed_token_are_all_refused() {
    let env = Env::new().await;
    let created = env
        .engine
        .create_token(&env.admin, spec(&env, &[Scope::Read]))
        .await
        .expect("create");
    let (head, secret) = created.secret.rsplit_once('_').expect("shape");
    let flipped = format!(
        "{head}_{}{}",
        if secret.starts_with('0') { '1' } else { '0' },
        &secret[1..]
    );
    for bad in [
        flipped,
        format!("oxc_{}_{}", "0".repeat(16), secret),
        created.secret.to_uppercase(),
        created.secret[..created.secret.len() - 1].to_owned(),
        created.secret.replacen("oxc_", "oxs_", 1),
        String::new(),
        "oxc_".into(),
    ] {
        assert_eq!(
            env.engine.authenticate(&bad).await,
            Err(Error::Unauthenticated),
            "{bad}"
        );
    }
}

#[tokio::test]
async fn a_revoked_token_stops_working_at_once() {
    let env = Env::new().await;
    let created = env
        .engine
        .create_token(&env.admin, spec(&env, &[Scope::Read]))
        .await
        .expect("create");
    env.engine
        .authenticate(&created.secret)
        .await
        .expect("works before");
    env.engine
        .revoke_token(&env.admin, created.id)
        .await
        .expect("revoke");
    assert_eq!(
        env.engine.authenticate(&created.secret).await,
        Err(Error::Unauthenticated)
    );
}

#[tokio::test]
async fn tokens_expire_and_cannot_outlive_a_year() {
    let env = Env::new().await;
    let created = env
        .engine
        .create_token(&env.admin, spec(&env, &[Scope::Read]))
        .await
        .expect("create");
    env.clock.advance(89 * 24 * 3600);
    env.engine
        .authenticate(&created.secret)
        .await
        .expect("still valid");
    env.clock.advance(2 * 24 * 3600);
    assert_eq!(
        env.engine.authenticate(&created.secret).await,
        Err(Error::Unauthenticated)
    );

    let mut too_long = spec(&env, &[Scope::Read]);
    too_long.expires_in_secs = Some(366 * 24 * 3600);
    assert!(matches!(
        env.engine.create_token(&env.admin, too_long).await,
        Err(Error::BadRequest(_))
    ));
}

#[tokio::test]
async fn creating_tokens_is_a_tenant_admin_action_and_stays_inside_the_tenant() {
    let env = Env::new().await;
    assert!(matches!(
        env.engine
            .create_token(&env.dev, spec(&env, &[Scope::Read]))
            .await,
        Err(Error::Forbidden(_))
    ));
    let mut foreign = spec(&env, &[Scope::Read]);
    foreign.caches = vec![oxabl_cache_core::ids::CacheId::random()];
    assert_eq!(
        env.engine.create_token(&env.admin, foreign).await.err(),
        Some(Error::NotFound)
    );
    let mut admin_token = spec(&env, &[Scope::TenantAdmin]);
    admin_token.caches = vec![];
    let created = env
        .engine
        .create_token(&env.admin, admin_token)
        .await
        .expect("admin token");
    let caller = env
        .engine
        .authenticate(&created.secret)
        .await
        .expect("authenticate");
    assert!(caller.is_tenant_admin());
    assert!(
        !caller.can(&env.cache, Scope::Read),
        "tenant:admin carries no data scopes"
    );
    let empty = TokenSpec {
        scopes: ScopeSet::EMPTY,
        ..spec(&env, &[Scope::Read])
    };
    assert!(matches!(
        env.engine.create_token(&env.admin, empty).await,
        Err(Error::BadRequest(_))
    ));
}

#[tokio::test]
async fn sessions_last_one_hour_for_workloads_and_twelve_for_users_and_can_be_revoked() {
    let env = Env::new().await;
    let workload = env
        .engine
        .open_workload_session(
            env.tenant,
            oxabl_cache_core::ids::PrincipalId::random(),
            vec![oxabl_cache_core::domain::scope::Grant {
                cache: Some(env.cache),
                scopes: ScopeSet::EMPTY.with(Scope::Read),
            }],
        )
        .await
        .expect("workload session");
    assert!(workload.secret.starts_with("oxs_"));
    let now = oxabl_cache_core::ports::Clock::now(env.engine.clock());
    assert_eq!(workload.expires_at, now + 3600);
    env.clock.advance(3601);
    assert_eq!(
        env.engine.authenticate(&workload.secret).await,
        Err(Error::Unauthenticated)
    );

    let user = env
        .engine
        .create_user(
            &env.admin,
            "https://idp.test".into(),
            "human".into(),
            "human".into(),
        )
        .await
        .expect("user");
    let session = env
        .engine
        .open_user_session(env.tenant, user)
        .await
        .expect("session");
    assert_eq!(session.expires_at, now + 3601 + 12 * 3600);
    env.engine
        .authenticate(&session.secret)
        .await
        .expect("valid");
    env.engine
        .revoke_session(&session.secret)
        .await
        .expect("revoke");
    assert_eq!(
        env.engine.authenticate(&session.secret).await,
        Err(Error::Unauthenticated)
    );
}

#[tokio::test]
async fn a_workload_never_receives_an_administration_scope() {
    let env = Env::new().await;
    for scope in [Scope::CacheAdmin, Scope::TenantAdmin] {
        let refused = env
            .engine
            .open_workload_session(
                env.tenant,
                oxabl_cache_core::ids::PrincipalId::random(),
                vec![oxabl_cache_core::domain::scope::Grant {
                    cache: Some(env.cache),
                    scopes: ScopeSet::EMPTY.with(scope),
                }],
            )
            .await;
        assert!(matches!(refused, Err(Error::Forbidden(_))), "{scope:?}");
    }
}

#[tokio::test]
async fn disabling_a_user_revokes_their_tokens_and_sessions() {
    let env = Env::new().await;
    let user = env
        .engine
        .create_user(
            &env.admin,
            "https://idp.test".into(),
            "leaver".into(),
            "leaver".into(),
        )
        .await
        .expect("user");
    let session = env
        .engine
        .open_user_session(env.tenant, user)
        .await
        .expect("session");
    let token = env
        .engine
        .create_token(
            &env.admin,
            TokenSpec {
                principal: Some(user),
                ..spec(&env, &[Scope::Read])
            },
        )
        .await
        .expect("token");
    env.engine
        .authenticate(&session.secret)
        .await
        .expect("session works");
    env.engine
        .authenticate(&token.secret)
        .await
        .expect("token works");
    env.engine
        .disable_user(&env.admin, user)
        .await
        .expect("disable");
    assert_eq!(
        env.engine.authenticate(&session.secret).await,
        Err(Error::Unauthenticated)
    );
    assert_eq!(
        env.engine.authenticate(&token.secret).await,
        Err(Error::Unauthenticated)
    );
    assert_eq!(
        env.engine.open_user_session(env.tenant, user).await.err(),
        Some(Error::NotFound)
    );
}

#[tokio::test]
async fn a_suspended_tenant_is_refused_until_it_is_resumed() {
    let env = Env::new().await;
    let created = env
        .engine
        .create_token(&env.admin, spec(&env, &[Scope::Read]))
        .await
        .expect("create");
    env.engine
        .set_tenant_status(env.tenant, TenantStatus::Suspended)
        .await
        .expect("suspend");
    assert_eq!(
        env.engine.authenticate(&created.secret).await,
        Err(Error::TenantSuspended)
    );
    env.engine
        .set_tenant_status(env.tenant, TenantStatus::Active)
        .await
        .expect("resume");
    env.engine
        .authenticate(&created.secret)
        .await
        .expect("works again");
}
