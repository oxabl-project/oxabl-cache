//! Streaming a span of a stored object, counting the bytes actually sent.

use crate::app::{App, Backend};
use bytes::Bytes;
use futures_util::stream::{self, Stream};
use oxabl_cache_core::ids::CacheId;
use oxabl_cache_core::ports::{ObjectKey, ObjectStore};
use std::io;
use std::sync::Arc;

const CHUNK: u64 = 256 * 1024;

/// Counts what was sent when the response ends, completed or cut short.
struct Counter<B: Backend> {
    app: Arc<App<B>>,
    cache: CacheId,
    sent: u64,
}

impl<B: Backend> Drop for Counter<B> {
    fn drop(&mut self) {
        let (app, cache, sent) = (Arc::clone(&self.app), self.cache, self.sent);
        if sent > 0 {
            tokio::spawn(async move {
                let _ = app.engine.count_served_bytes(cache, sent).await;
            });
        }
    }
}

struct Cursor<B: Backend> {
    key: ObjectKey,
    etag: String,
    next: u64,
    end: u64,
    counter: Counter<B>,
}

/// The bytes `start..=end` of the object, read in chunks pinned to `etag`.
pub(crate) fn span_stream<B: Backend>(
    app: Arc<App<B>>,
    key: ObjectKey,
    etag: String,
    start: u64,
    end: u64,
) -> impl Stream<Item = Result<Bytes, io::Error>> {
    let counter = Counter {
        app,
        cache: key.cache,
        sent: 0,
    };
    let cursor = Cursor {
        key,
        etag,
        next: start,
        end,
        counter,
    };
    stream::unfold(Some(cursor), |state| async move {
        let mut cursor = state?;
        if cursor.next > cursor.end {
            return None;
        }
        let length = (cursor.end - cursor.next + 1).min(CHUNK);
        let read = cursor
            .counter
            .app
            .engine
            .object_store()
            .read_range(&cursor.key, cursor.next, length, Some(&cursor.etag))
            .await;
        match read {
            Ok(bytes) => {
                cursor.next += length;
                cursor.counter.sent += length;
                Some((Ok(Bytes::from(bytes)), Some(cursor)))
            }
            Err(error) => Some((Err(io::Error::other(error.to_string())), None)),
        }
    })
}
