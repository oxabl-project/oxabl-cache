use bytes::Bytes;

use super::pack_slot::PackSlot;
use super::ranges::MAX_READ;
use super::types::MissReason;
use crate::api::{Api, ApiError, RangeBody};
use crate::progress::Progress;

pub(crate) fn miss_of(error: &ApiError) -> MissReason {
    match error {
        ApiError::Status(e) if e.status == 404 => MissReason::NotFound,
        ApiError::Malformed => MissReason::Corrupt,
        _ => MissReason::Unavailable,
    }
}

/// Reads `start..end` of a pack as a stream of chunks, in requests of at most 8 MiB, renewing
/// an expired URL once per request.
pub(crate) struct RangeStream<'a> {
    api: &'a Api,
    slot: &'a PackSlot,
    progress: &'a Progress,
    next: u64,
    end: u64,
    body: Option<RangeBody>,
}

impl<'a> RangeStream<'a> {
    pub fn new(
        api: &'a Api,
        slot: &'a PackSlot,
        start: u64,
        end: u64,
        progress: &'a Progress,
    ) -> Self {
        Self {
            api,
            slot,
            progress,
            next: start,
            end,
            body: None,
        }
    }

    async fn open(&self, from: u64, last: u64) -> Result<RangeBody, ApiError> {
        let url = self.slot.url().await;
        match self.api.open_range(&url, from, last).await {
            Err(e) if e.status() == Some(403) => {
                let fresh = self
                    .slot
                    .renew(self.api, &url, self.progress)
                    .await
                    .ok_or(ApiError::Transport)?;
                self.api.open_range(&fresh, from, last).await
            }
            other => other,
        }
    }

    /// The next chunk, or `None` once every byte of the range has arrived.
    pub async fn next(&mut self) -> Result<Option<Bytes>, MissReason> {
        loop {
            if let Some(body) = &mut self.body {
                match body
                    .chunk(self.api, self.progress)
                    .await
                    .map_err(|e| miss_of(&e))?
                {
                    Some(chunk) => return Ok(Some(chunk)),
                    None => self.body = None,
                }
            }
            if self.next >= self.end {
                return Ok(None);
            }
            let to = (self.next + MAX_READ).min(self.end);
            let body = self
                .open(self.next, to - 1)
                .await
                .map_err(|e| miss_of(&e))?;
            self.body = Some(body);
            self.next = to;
        }
    }
}

/// Reads `start..end` of a pack whole.
pub(crate) async fn read_range(
    api: &Api,
    slot: &PackSlot,
    start: u64,
    end: u64,
    progress: &Progress,
) -> Result<Vec<u8>, MissReason> {
    let mut bytes = Vec::with_capacity((end - start) as usize);
    let mut stream = RangeStream::new(api, slot, start, end, progress);
    while let Some(chunk) = stream.next().await? {
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}
