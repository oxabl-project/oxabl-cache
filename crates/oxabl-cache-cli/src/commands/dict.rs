use oxabl_cache_client::{Cache, MIN_PUBLISH_GAIN, Mode, TrainingOptions, train_from_outputs};
use serde_json::json;

use crate::cli::DictCommand;
use crate::context::Context;
use crate::failure::Failure;
use crate::output::{print_fields, print_json};

pub async fn run(context: &Context, command: DictCommand) -> Result<(), Failure> {
    let DictCommand::Train { outputs, publish } = command;
    let trained = tokio::task::spawn_blocking(move || {
        train_from_outputs(&outputs, &TrainingOptions::default())
    })
    .await
    .map_err(|_| Failure::refused("training failed"))?
    .map_err(Failure::refused)?;
    let gain = trained.gain();
    let mut published = None;
    let mut refusal = None;
    if publish {
        if !trained.worth_publishing() {
            refusal = Some(Failure::refused(format!(
                "not published: the gain {gain:.2}x is below {MIN_PUBLISH_GAIN}x"
            )));
        } else {
            let cache = Cache::open(context.config()?.with_mode(Mode::Ci)).await?;
            match cache.publish_dictionary(&trained.dictionary).await {
                Ok(done) => published = Some(done),
                Err(error) => refusal = Some(Failure::refused(error)),
            }
        }
    }
    if context.json {
        print_json(&json!({
            "dictionary_bytes": trained.dictionary.len(),
            "trained_on": trained.trained_on,
            "held_out": trained.held_out,
            "held_out_plain_bytes": trained.held_out_plain_bytes,
            "baseline_bytes": trained.baseline_bytes,
            "with_dictionary_bytes": trained.dictionary_bytes,
            "gain": gain,
            "published": published.as_ref().map(|p| json!({
                "segment_id": p.segment_id, "pointer_version": p.pointer_version,
            })),
        }));
    } else {
        let mut fields = vec![
            ("dictionary", format!("{} bytes", trained.dictionary.len())),
            ("trained on", format!("{} files", trained.trained_on)),
            (
                "held out",
                format!(
                    "{} files, {} bytes",
                    trained.held_out, trained.held_out_plain_bytes
                ),
            ),
            ("without", format!("{} bytes", trained.baseline_bytes)),
            ("with", format!("{} bytes", trained.dictionary_bytes)),
            ("gain", format!("{gain:.2}x")),
        ];
        if let Some(done) = &published {
            fields.push((
                "published",
                format!(
                    "pointer v{}, segment {}",
                    done.pointer_version, done.segment_id
                ),
            ));
        }
        print_fields(&fields);
    }
    refusal.map_or(Ok(()), Err)
}
