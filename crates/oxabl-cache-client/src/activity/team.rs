use std::collections::HashSet;

use oxabl_cache_crypto::{open_telemetry, telemetry_epoch};

use super::Activity;
use crate::cache::Inner;
use crate::error::CacheError;

/// Pages read at most: the server keeps 4,096 reports per cache, 1,000 per page.
const PAGES: usize = 8;

/// Every report the cache's members shared in the current epoch, each once, oldest first.
/// Reports of other epochs, and any that fail to open, are left out.
pub(crate) async fn team_activity(inner: &Inner) -> Result<Vec<Activity>, CacheError> {
    let view = inner.view();
    let mut seen = HashSet::new();
    let mut activities = Vec::new();
    let mut after = 0;
    for _ in 0..PAGES {
        let (frames, next) = inner
            .api
            .telemetry_page(after)
            .await
            .map_err(|_| CacheError::Unavailable)?;
        if frames.is_empty() {
            break;
        }
        for frame in frames {
            if telemetry_epoch(&frame).ok() != Some(view.epoch()) {
                continue;
            }
            let Ok(report) = open_telemetry(&view.root, &inner.pins.cache, view.epoch(), &frame)
            else {
                continue;
            };
            if seen.insert(report.id) {
                activities.push(Activity::from_report(&report));
            }
        }
        after = next;
    }
    Ok(activities)
}
