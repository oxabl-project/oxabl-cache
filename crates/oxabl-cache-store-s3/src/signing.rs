//! Presigning windows.

use jiff::Timestamp;
use rusty_s3::S3Action;
use std::time::Duration;
use url::Url;

const MAX_SECONDS: i64 = 7 * 24 * 60 * 60;
const OWN_CALL_SECONDS: i64 = 60;

/// The moment a URL is signed and how long it lives.
pub(crate) struct Window {
    time: Timestamp,
    lifetime: Duration,
    /// Unix seconds at which the URL stops working.
    pub(crate) expires_at: i64,
}

impl Window {
    /// A window that ends at `expires_at`, clamped to what S3 accepts.
    pub(crate) fn until(expires_at: i64) -> Self {
        let time = Timestamp::now();
        let now = time.as_second();
        let seconds = expires_at.saturating_sub(now).clamp(1, MAX_SECONDS);
        Self {
            time,
            lifetime: Duration::from_secs(seconds.unsigned_abs()),
            expires_at: now + seconds,
        }
    }

    /// A window for the adapter's own request.
    pub(crate) fn own_call() -> Self {
        Self::until(Timestamp::now().as_second() + OWN_CALL_SECONDS)
    }

    /// Signs `action` inside this window.
    pub(crate) fn sign<'a>(&self, action: &impl S3Action<'a>) -> Url {
        action.sign_with_time(self.lifetime, &self.time)
    }
}
