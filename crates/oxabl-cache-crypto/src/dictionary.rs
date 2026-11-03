//! Trained zstd dictionaries.

use oxabl_cache_format::SegmentId;
use zeroize::Zeroizing;
use zstd::dict::{DecoderDictionary, EncoderDictionary};

use crate::error::{Error, Result};

/// Size of a dictionary that `train` produces unless told otherwise.
pub const DEFAULT_DICTIONARY_BYTES: usize = 112 * 1024;
/// Largest dictionary a reader accepts.
pub const MAX_DICTIONARY_BYTES: usize = 8 * 1024 * 1024;

/// A dictionary ready for compression and decompression, named by the id of the sealed
/// dictionary segment that carries it.
pub struct Dictionary {
    id: SegmentId,
    bytes: Zeroizing<Vec<u8>>,
    encoder: EncoderDictionary<'static>,
    decoder: DecoderDictionary<'static>,
}

impl Dictionary {
    /// Prepares `bytes` for compression at `level` and for decompression.
    pub fn new(id: SegmentId, bytes: Vec<u8>, level: i32) -> Result<Self> {
        let bytes = Zeroizing::new(bytes);
        if bytes.is_empty() || bytes.len() > MAX_DICTIONARY_BYTES {
            return Err(Error::Limit("dictionary size"));
        }
        Ok(Self {
            id,
            encoder: EncoderDictionary::copy(&bytes, level),
            decoder: DecoderDictionary::copy(&bytes),
            bytes,
        })
    }

    /// Id of the sealed segment that carries this dictionary.
    pub fn id(&self) -> SegmentId {
        self.id
    }

    /// The raw dictionary.
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub(crate) fn encoder(&self) -> &EncoderDictionary<'static> {
        &self.encoder
    }

    pub(crate) fn decoder(&self) -> &DecoderDictionary<'static> {
        &self.decoder
    }
}

impl std::fmt::Debug for Dictionary {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Dictionary({})", self.id)
    }
}

/// Trains a dictionary of at most `max_size` bytes from sample artifact sets.
pub fn train<S: AsRef<[u8]>>(samples: &[S], max_size: usize) -> Result<Vec<u8>> {
    if max_size == 0 || max_size > MAX_DICTIONARY_BYTES {
        return Err(Error::Limit("dictionary size"));
    }
    zstd::dict::from_samples(samples, max_size)
        .map_err(|_| Error::Compression("dictionary training failed"))
}
