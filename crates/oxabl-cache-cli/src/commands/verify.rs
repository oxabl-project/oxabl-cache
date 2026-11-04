use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use oxabl_cache_client::{
    Cache, CompileKey, FetchOptions, FetchOutcome, FetchRequest, Tier, default_state_dir,
};
use serde_json::json;

use super::fetch::miss_name;
use super::keys;
use crate::cli::VerifyArgs;
use crate::context::Context;
use crate::failure::Failure;
use crate::output::{print_fields, print_json};

const MAX_KEY_FILE_BYTES: u64 = 64 * 1024 * 1024;
const MAX_SERVER_SAMPLE: usize = oxabl_cache_client::MAX_SAMPLE;

/// A scratch directory that is removed when it goes out of scope.
struct Scratch(PathBuf);

impl Scratch {
    fn create(state: Option<&Path>) -> Result<Self, Failure> {
        let base = state
            .map(Path::to_path_buf)
            .or_else(default_state_dir)
            .ok_or_else(|| Failure::usage("no state directory: pass --state-dir"))?
            .join("scratch");
        let mut random = [0u8; 8];
        getrandom::fill(&mut random)
            .map_err(|_| Failure::refused("the random generator failed"))?;
        let name: String = random.iter().map(|b| format!("{b:02x}")).collect();
        let path = base.join(name);
        fs::create_dir_all(&path)
            .map_err(|e| Failure::refused(format!("{}: {e}", path.display())))?;
        Ok(Self(path))
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn read_keys(path: &Path) -> Result<Vec<CompileKey>, Failure> {
    let meta =
        fs::metadata(path).map_err(|e| Failure::usage(format!("{}: {e}", path.display())))?;
    if !meta.is_file() || meta.len() > MAX_KEY_FILE_BYTES {
        return Err(Failure::usage(format!(
            "{}: not a regular key file",
            path.display()
        )));
    }
    let text =
        fs::read_to_string(path).map_err(|e| Failure::usage(format!("{}: {e}", path.display())))?;
    let mut keys = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(keys::parse)
        .collect::<Result<Vec<_>, _>>()?;
    keys.sort();
    keys.dedup();
    Ok(keys)
}

fn sample(keys: &mut Vec<CompileKey>, count: usize) -> Result<(), Failure> {
    for i in 0..count.min(keys.len()) {
        let mut word = [0u8; 8];
        getrandom::fill(&mut word).map_err(|_| Failure::refused("the random generator failed"))?;
        let j = i + (u64::from_le_bytes(word) % (keys.len() - i) as u64) as usize;
        keys.swap(i, j);
    }
    keys.truncate(count);
    Ok(())
}

/// Fetches a random sample of trusted entries into a scratch directory with every verification
/// the client does, and fails unless at least one was sampled and every one installs from the
/// trusted namespace. The sample is drawn from `--keys` when given, else by the server.
pub async fn run(context: &Context, args: VerifyArgs) -> Result<(), Failure> {
    if args.sample == 0 {
        return Err(Failure::usage("--sample must be at least 1"));
    }
    if args.keys.is_none() && args.sample > MAX_SERVER_SAMPLE {
        return Err(Failure::usage(format!(
            "--sample is at most {MAX_SERVER_SAMPLE} without --keys"
        )));
    }
    let pool = match &args.keys {
        Some(path) => {
            let mut pool = read_keys(path)?;
            if pool.is_empty() {
                return Err(Failure::usage(format!("{}: no keys", path.display())));
            }
            sample(&mut pool, args.sample)?;
            Some(pool)
        }
        None => None,
    };
    let scratch = Scratch::create(context.state_dir())?;
    let cache = Cache::open(context.config()?).await?;
    let options = FetchOptions::new(scratch.0.join("stage"));
    let mut fetch = match pool {
        Some(pool) => cache.fetch(
            pool.iter()
                .map(|key| FetchRequest {
                    key: *key,
                    expect: None,
                })
                .collect(),
            options,
        ),
        None => cache
            .fetch_sample(args.sample, options)
            .await
            .map_err(|e| Failure::refused(format!("sample: {e}")))?,
    };
    let (mut sampled, mut trusted, mut personal) = (0usize, 0usize, 0usize);
    let mut misses: BTreeMap<String, usize> = BTreeMap::new();
    while let Some(outcome) = fetch.next().await {
        sampled += 1;
        match outcome {
            FetchOutcome::Installed { provenance, .. } => match provenance.tier {
                Tier::Trusted => trusted += 1,
                Tier::Personal => personal += 1,
            },
            FetchOutcome::Miss { reason, .. } => *misses.entry(miss_name(reason)).or_default() += 1,
        }
    }
    if context.json {
        print_json(&json!({
            "sampled": sampled,
            "verified": trusted,
            "not_trusted": personal,
            "misses": misses,
        }));
    } else {
        let mut fields = vec![
            ("sampled", sampled.to_string()),
            ("verified", trusted.to_string()),
            ("not trusted", personal.to_string()),
        ];
        for (reason, count) in &misses {
            fields.push(("miss", format!("{reason}: {count}")));
        }
        print_fields(&fields);
    }
    if sampled == 0 {
        return Err(Failure::refused(
            "the cache holds no trusted entries: nothing was verified",
        ));
    }
    if trusted != sampled {
        return Err(Failure::refused(format!(
            "{} of {sampled} sampled entries did not verify as trusted",
            sampled - trusted
        )));
    }
    Ok(())
}
