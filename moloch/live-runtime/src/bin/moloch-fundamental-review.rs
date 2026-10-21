//! Offline typed review inspection. No worker, transport, credentials or ledger.
use moloch_live_runtime::fundamental_policy::{review_sha256, ReviewAuthority, ReviewInput, Reviewed, Settings, SourceBytes};
use serde_json::{json, Value};
use std::{collections::BTreeSet, env, fs, path::Path};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = env::args().collect();
    if args.len() == 3 && args[1] == "hash-review" {
        let review: ReviewInput = serde_json::from_slice(&fs::read(&args[2])?)?;
        println!("{}", review_sha256(&review)?);
        return Ok(());
    }
    if args.len() != 4 || args[1] != "validate-control" {
        return Err("usage: hash-review PATH | validate-control PATH NOW_MS".into());
    }
    let now: u64 = args[3].parse()?;
    let control: Value = serde_json::from_slice(&fs::read(&args[2])?)?;
    let settings: Settings = serde_json::from_value(control["fundamentalPolicy"].clone())?;
    let loaded = settings.load()?;
    let authority = ReviewAuthority::load(&settings.authority_path, &settings.authority_sha256)?;
    let mut result = Vec::new();
    for file in &settings.reviews {
        let review: ReviewInput = serde_json::from_slice(&fs::read(&file.path)?)?;
        let bytes: Vec<_> = file.source_files.iter().map(|s| fs::read(Path::new(&s.path))).collect::<Result<_, _>>()?;
        let originals: Vec<_> = file.source_files.iter().zip(&bytes).map(|(s, b)| SourceBytes { sha256: &s.sha256, bytes: b }).collect();
        let trusted: BTreeSet<String> = authority.trusted().clone();
        let sha = review_sha256(&review)?;
        let current = Reviewed::verify(review.clone(), &originals, &trusted, now).is_ok();
        result.push(json!({"exchangeId": review.exchange_id, "side": review.side, "reviewSha256": sha,
            "current": current, "expiresAtMs": review.expires_at_ms,
            "ordinaryProbability": review.ordinary_probability, "passiveValueExit": review.passive_value_exit}));
    }
    println!("{}", json!({"settingsSha256": loaded.settings_sha(), "reviews": result,
        "accountRequests": 0, "productionModified": false, "executionAuthority": false}));
    Ok(())
}
