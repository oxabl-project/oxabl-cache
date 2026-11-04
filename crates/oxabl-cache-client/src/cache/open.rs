use std::sync::{Arc, Mutex, RwLock};

use oxabl_cache_format::Limits;
use tokio::runtime::Handle;

use super::{Cache, Dictionaries, Gates, Inner, open_error, roster, view};
use crate::api::{Api, http_client};
use crate::auth::Authenticator;
use crate::config::{Config, ConfigError, check_server};
use crate::error::OpenError;
use crate::progress::Progress;
use crate::state::StateDirs;
use crate::stats::Counters;

pub(super) async fn open(config: Config) -> Result<Cache, OpenError> {
    check_server(&config.server)?;
    let limits = Limits::new(config.max_plain_bytes)
        .map_err(|_| ConfigError::Invalid("the plaintext cap is above 1 GiB"))?;
    let (source, provider) = (config.identity.clone(), config.passphrase.clone());
    let identity =
        tokio::task::spawn_blocking(move || super::identity::resolve(&source, provider.as_ref()))
            .await
            .map_err(|_| OpenError::Identity("identity loading failed"))??;

    let roots = crate::config::roots::load(config.ca_file.as_deref())?;
    let http = http_client(config.connections, &roots).map_err(|_| OpenError::Unreachable)?;
    let state = StateDirs::new(config.state_dir.as_deref());
    let server = config.server.as_str().trim_end_matches('/').to_owned();
    let auth = Authenticator::establish(
        &config.credentials,
        &http,
        &server,
        config.pins.tenant,
        &state,
    )
    .await?;
    let counters = Arc::new(Counters::default());
    let api = Api::new(
        http,
        &server,
        &config.pins.cache.to_text(),
        Arc::new(auth),
        counters.clone(),
    );
    let info = api.info(&Progress::new()).await.map_err(open_error)?;

    let high_water = state.high_water(&config.pins.cache.to_text());
    let verified = roster::load(&api, &config.pins, &high_water).await?;
    let view = view::RosterView::build(verified, &identity)?;
    let namespaces = view::read_namespaces(config.mode, view.role(&identity));
    Ok(Cache {
        inner: Arc::new(Inner {
            reader_key: identity.signing_public(),
            api,
            pins: config.pins,
            identity,
            mode: config.mode,
            namespaces,
            view: RwLock::new(Arc::new(view)),
            high_water,
            dictionaries: Dictionaries::default(),
            limits,
            zstd_level: config.zstd_level,
            gates: Gates::new(config.connections),
            handle: Handle::current(),
            checksum_sha256: info.checksum_sha256(),
            counters,
            reported: Mutex::new(Default::default()),
        }),
    })
}
