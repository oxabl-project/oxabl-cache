use std::sync::Arc;

use crate::cache::{Inner, RosterView, roster};

/// Fetches the roster again and, when it is newer than the one held, adopts it. `None` when
/// nothing newer verified or the identity is no longer a member.
pub(crate) async fn refresh(inner: Arc<Inner>) -> Option<Arc<RosterView>> {
    let loaded = roster::load(&inner.api, &inner.pins, &inner.high_water)
        .await
        .ok()?;
    let held = inner.view();
    if loaded.version() <= held.roster.version() {
        return None;
    }
    let view = Arc::new(RosterView::build(loaded, &inner.identity).ok()?);
    let mut slot = inner.view.write().expect("roster view lock");
    if view.roster.version() > slot.roster.version() {
        *slot = view.clone();
    }
    Some(view)
}
