use oxabl_cache_crypto::VerifiedRoster;

use super::open_error;
use crate::api::Api;
use crate::config::Pins;
use crate::error::OpenError;
use crate::progress::Progress;
use crate::state::HighWater;

/// Fetches the roster, verifies it against the pins and the high-water mark, and raises the
/// mark to its version.
pub(crate) async fn load(
    api: &Api,
    pins: &Pins,
    high_water: &HighWater,
) -> Result<VerifiedRoster, OpenError> {
    let bytes = api
        .roster(&Progress::new())
        .await
        .map_err(open_error)?
        .ok_or(OpenError::NoRoster)?;
    let mark = high_water.read().map_err(|e| OpenError::State(e.kind()))?;
    let roster = VerifiedRoster::verify(
        &bytes,
        &oxabl_cache_crypto::RosterPins {
            cache_id: pins.cache,
            owner_keys: pins.owner_keys.clone(),
            min_version: pins.min_roster_version,
        },
        mark,
    )
    .map_err(OpenError::Roster)?;
    if let Err(error) = high_water.advance(roster.version()) {
        tracing::warn!(%error, "the roster high-water mark was not saved");
    }
    Ok(roster)
}
