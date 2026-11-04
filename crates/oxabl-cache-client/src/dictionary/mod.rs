//! A cache's compression dictionary: training one from outputs and publishing it as `ci`.

mod publish;
mod sampling;
mod training;

pub use publish::{DictionaryError, DictionaryPublished};
pub use training::{MIN_PUBLISH_GAIN, TrainError, Trained, TrainingOptions, train_from_outputs};
