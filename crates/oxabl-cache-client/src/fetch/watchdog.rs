use std::time::Duration;

use crate::progress::Progress;

/// Completes once nothing has progressed for `idle`.
pub(crate) async fn idle(progress: &Progress, idle: Duration) {
    loop {
        let quiet = progress.idle_for();
        if quiet >= idle {
            return;
        }
        tokio::time::sleep(idle - quiet).await;
    }
}
