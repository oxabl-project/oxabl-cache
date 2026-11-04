use std::io;
use std::path::Path;

use oxabl_cache_crypto::{DEFAULT_DICTIONARY_BYTES, DEFAULT_ZSTD_LEVEL};

use super::sampling::{head, regular_files, shuffle};

/// The held-out gain a dictionary must reach before it is worth publishing.
pub const MIN_PUBLISH_GAIN: f64 = 1.3;
const TRAIN_HEAD_BYTES: u64 = 128 * 1024;
const MEASURE_HEAD_BYTES: u64 = 1024 * 1024;
const HELD_OUT_PERCENT: usize = 20;
const MIN_SAMPLES: usize = 20;

/// How a dictionary is trained and measured.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TrainingOptions {
    /// Size of the dictionary, 112 KiB by default.
    pub dictionary_bytes: usize,
    /// Files sampled at most, 2,000 by default.
    pub max_samples: usize,
    pub zstd_level: i32,
}

impl Default for TrainingOptions {
    fn default() -> Self {
        Self {
            dictionary_bytes: DEFAULT_DICTIONARY_BYTES,
            max_samples: 2_000,
            zstd_level: DEFAULT_ZSTD_LEVEL,
        }
    }
}

/// Why no dictionary was trained.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum TrainError {
    Io(io::ErrorKind),
    /// The directory holds fewer files than training and a held-out check need.
    TooFewSamples(usize),
    Training,
}

impl std::fmt::Display for TrainError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(kind) => write!(f, "cannot read the outputs: {kind}"),
            Self::TooFewSamples(n) => write!(
                f,
                "{n} sample files are too few to train and measure a dictionary (need {MIN_SAMPLES})"
            ),
            Self::Training => f.write_str("the dictionary could not be trained from these samples"),
        }
    }
}

impl std::error::Error for TrainError {}

/// A trained dictionary and what it gains on files it was not trained on.
#[derive(Debug, Clone, PartialEq)]
pub struct Trained {
    pub dictionary: Vec<u8>,
    pub trained_on: usize,
    pub held_out: usize,
    /// Bytes of the held-out files before compression.
    pub held_out_plain_bytes: u64,
    /// Their size under zstd at the configured level without a dictionary.
    pub baseline_bytes: u64,
    /// Their size with the dictionary.
    pub dictionary_bytes: u64,
}

impl Trained {
    /// How much smaller the held-out files are with the dictionary: baseline over dictionary.
    pub fn gain(&self) -> f64 {
        self.baseline_bytes as f64 / self.dictionary_bytes.max(1) as f64
    }

    /// Whether the gain reaches [`MIN_PUBLISH_GAIN`].
    pub fn worth_publishing(&self) -> bool {
        self.gain() >= MIN_PUBLISH_GAIN
    }
}

fn compressed_size(
    samples: &[Vec<u8>],
    mut compressor: zstd::bulk::Compressor<'_>,
) -> Result<u64, TrainError> {
    let mut total = 0u64;
    for sample in samples {
        total += compressor
            .compress(sample)
            .map_err(|_| TrainError::Training)?
            .len() as u64;
    }
    Ok(total)
}

/// Samples the files under `outputs`, trains on 80% and measures on the other 20%.
pub fn train_from_outputs(
    outputs: &Path,
    options: &TrainingOptions,
) -> Result<Trained, TrainError> {
    let mut files = regular_files(outputs).map_err(|e| TrainError::Io(e.kind()))?;
    shuffle(&mut files).map_err(|e| TrainError::Io(e.kind()))?;
    files.truncate(options.max_samples);
    if files.len() < MIN_SAMPLES {
        return Err(TrainError::TooFewSamples(files.len()));
    }
    let held = (files.len() * HELD_OUT_PERCENT / 100).max(1);
    let (held_files, train_files) = files.split_at(held);
    let read = |paths: &[std::path::PathBuf], limit| {
        paths
            .iter()
            .map(|p| head(p, limit))
            .collect::<io::Result<Vec<Vec<u8>>>>()
            .map_err(|e| TrainError::Io(e.kind()))
    };
    let training = read(train_files, TRAIN_HEAD_BYTES)?;
    let held_out = read(held_files, MEASURE_HEAD_BYTES)?;
    let dictionary = oxabl_cache_crypto::train(&training, options.dictionary_bytes)
        .map_err(|_| TrainError::Training)?;
    let plain =
        zstd::bulk::Compressor::new(options.zstd_level).map_err(|_| TrainError::Training)?;
    let tuned = zstd::bulk::Compressor::with_dictionary(options.zstd_level, &dictionary)
        .map_err(|_| TrainError::Training)?;
    Ok(Trained {
        held_out_plain_bytes: held_out.iter().map(|s| s.len() as u64).sum(),
        baseline_bytes: compressed_size(&held_out, plain)?,
        dictionary_bytes: compressed_size(&held_out, tuned)?,
        trained_on: training.len(),
        held_out: held_out.len(),
        dictionary,
    })
}
