mod capture;
mod cache;
mod engine { include!("engine.rs"); include!("intent_observation.rs"); }
mod evaluation;
mod money;
mod publication;
mod replay;
mod sweep;
mod trace;
mod types;
use sha2::{Digest, Sha256};
use std::{collections::HashMap, fs, path::Path};
fn run() -> Result<(), String> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let command = args.first().ok_or("COMMAND_REQUIRED")?;
    let allowed = match command.as_str() {
        "capture" => vec!["--config", "--journal", "--end-byte", "--expected-sha"],
        "publication" => vec!["--scope", "--journal", "--end-byte", "--expected-sha", "--out"],
        "replay" => vec![
            "--bank",
            "--journal",
            "--end-byte",
            "--expected-sha",
            "--workers",
            "--out",
        ],
        "sweep" => vec![
            "--config",
            "--journal",
            "--end-byte",
            "--expected-sha",
            "--workers",
            "--out",
        ],
        "evaluate" => vec![
            "--declaration",
            "--journal",
            "--end-byte",
            "--expected-sha",
            "--workers",
            "--out",
        ],
        "prepare-cache" => vec!["--declaration","--journal","--end-byte","--expected-sha","--out"],
        "evaluate-cached" => vec!["--declaration","--journal","--end-byte","--expected-sha","--workers","--out","--cache","--cache-sha","--cache-source-sha","--cache-end-byte","--prior-end-byte","--prior-sha"],
        "evaluate-observed" => vec!["--declaration","--journal","--end-byte","--expected-sha","--workers","--out"],
        "evaluate-observed-cached" => vec!["--declaration","--journal","--end-byte","--expected-sha","--workers","--out","--cache","--cache-sha","--cache-source-sha","--cache-end-byte","--prior-end-byte","--prior-sha"],
        "trace" => vec!["--fixture"],
        _ => return Err("UNKNOWN_COMMAND".into()),
    };
    let mut options = HashMap::new();
    for pair in args[1..].chunks(2) {
        if pair.len() != 2
            || !allowed.contains(&pair[0].as_str())
            || pair[1].starts_with("--")
            || options.insert(pair[0].as_str(), pair[1].as_str()).is_some()
        {
            return Err("INVALID_ARGUMENTS".into());
        }
    }
    let get = |key: &str| {
        options
            .get(key)
            .copied()
            .ok_or_else(|| "MISSING_ARGUMENT".to_owned())
    };
    if command == "trace" {
        println!("{}", trace::run(Path::new(get("--fixture")?))?);
        return Ok(());
    }
    if command == "publication" {
        if Path::new(get("--out")?).exists() { return Err("OUTPUT_EXISTS_OR_UNAVAILABLE".into()); }
        let scope_bytes = fs::read(get("--scope")?).map_err(|_| "SCOPE_OPEN_FAILED")?;
        let scope_hash = format!("{:x}",Sha256::digest(&scope_bytes));
        let scope: serde_json::Value = serde_json::from_slice(&scope_bytes).map_err(|_| "SCOPE_JSON_INVALID")?;
        let end = get("--end-byte")?.parse().map_err(|_| "INVALID_SOURCE_PREFIX")?;
        let mut result = publication::read_prefix(Path::new(get("--journal")?),end,get("--expected-sha")?,&scope)?;
        result["scopeFileHash"] = serde_json::json!(scope_hash);
        let mut file = std::fs::OpenOptions::new().write(true).create_new(true).open(get("--out")?).map_err(|_| "OUTPUT_EXISTS_OR_UNAVAILABLE")?;
        std::io::Write::write_all(&mut file,&serde_json::to_vec(&result).map_err(|_| "OUTPUT_INVALID")?).map_err(|_| "OUTPUT_WRITE_FAILED")?;
        file.sync_all().map_err(|_| "OUTPUT_SYNC_FAILED")?;
        println!("{}",serde_json::json!({"sourceVerified":true,"sourceRecords":result["sourceRecords"],"sourceBytes":end,"verifyMs":result["verifyMs"],"competitionOrders":0}));
        return Ok(());
    }
    if matches!(command.as_str(), "evaluate" | "evaluate-cached" | "prepare-cache" | "evaluate-observed" | "evaluate-observed-cached") {
        if Path::new(get("--out")?).exists() {
            return Err("OUTPUT_EXISTS_OR_UNAVAILABLE".into());
        }
        let bytes = fs::read(get("--declaration")?).map_err(|_| "DECLARATION_OPEN_FAILED")?;
        let declaration_hash = format!("{:x}", Sha256::digest(&bytes));
        let d: evaluation::Declaration =
            serde_json::from_slice(&bytes).map_err(|_| "DECLARATION_JSON_INVALID")?;
        let end = get("--end-byte")?
            .parse()
            .map_err(|_| "INVALID_SOURCE_PREFIX")?;
        let cached=matches!(command.as_str(), "evaluate-cached" | "evaluate-observed-cached");
        let cache_end=if cached {get("--cache-end-byte")?.parse().map_err(|_|"CACHE_BINDING_INVALID")?}else{0};
        let seed=if cached {Some(cache::read(Path::new(get("--cache")?),get("--cache-sha")?,get("--cache-source-sha")?,cache_end,&d.config,true)?)}else{None};
        let expected=if cached {options.get("--expected-sha").copied()}else{Some(get("--expected-sha")?)};
        let source=capture::read_capture_seeded(Path::new(get("--journal")?),end,&d.config,expected,true,seed)?;
        let prior_end:u64=if cached {get("--prior-end-byte")?.parse().map_err(|_|"CACHE_PRIOR_BINDING_INVALID")?}else{0};
        if cached {
            // One native pass establishes current source SHA, unchanged prior
            // source SHA, and canonical append continuity. The caller supplies
            // the prior digest from its immutable verified result, not a new
            // unchecked hash computed from potentially changed current bytes.
            let i=source.prefix_bytes.iter().position(|n|*n==prior_end).ok_or("CACHE_PRIOR_BOUNDARY_MISSING")?;
            if source.prefix_hashes[i]!=get("--prior-sha")?{return Err("CACHE_PRIOR_PREFIX_CHANGED".into());}
        }
        if command=="prepare-cache" {
            println!("{}",cache::write(Path::new(get("--out")?),&source,&d.config,true)?);
            return Ok(());
        }
        let workers = options
            .get("--workers")
            .unwrap_or(&"8")
            .parse()
            .map_err(|_| "INVALID_REPLAY_WORKERS")?;
        let result = if command.starts_with("evaluate-observed") {
            evaluation::run_observed(&source, &d, workers)?
        } else { evaluation::run(&source, &d, workers)? };
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(get("--out")?)
            .map_err(|_| "OUTPUT_EXISTS_OR_UNAVAILABLE")?;
        let mut output=serde_json::json!({"capture":source.meta,"evaluation":result,"declarationFileHash":declaration_hash});
        if cached {output["cacheProof"]=serde_json::json!({"schemaVersion":1,"mode":"pinned_original_prefix_cache_and_verified_append_v1","cacheHash":get("--cache-sha")?,"sourceHash":get("--cache-source-sha")?,"sourceBytes":cache_end,"priorSourceHash":get("--prior-sha")?,"priorSourceBytes":prior_end,"fullOriginalPrefixRehashed":true,"appendedBytes":end-cache_end});}
        std::io::Write::write_all(&mut file,&serde_json::to_vec(&output).map_err(|_|"OUTPUT_INVALID")?).map_err(|_|"OUTPUT_WRITE_FAILED")?;
        file.sync_all().map_err(|_| "OUTPUT_SYNC_FAILED")?;
        println!(
            "{}",
            serde_json::json!({"sourceVerified":true,"candidates":result.candidates,"subsequentRecords":result.subsequent_records,"captureMs":source.meta.verify_ms,"replayMs":result.replay_ms,"competitionOrders":0,"profitabilityEstablished":false})
        );
        return Ok(());
    }
    if command == "sweep" {
        if Path::new(get("--out")?).exists() {
            return Err("OUTPUT_EXISTS_OR_UNAVAILABLE".into());
        }
        let config_bytes = fs::read(get("--config")?).map_err(|_| "CONFIG_OPEN_FAILED")?;
        let config_file_hash = format!("{:x}", Sha256::digest(&config_bytes));
        let c: types::Config =
            serde_json::from_slice(&config_bytes).map_err(|_| "CONFIG_JSON_INVALID")?;
        let end = get("--end-byte")?
            .parse()
            .map_err(|_| "INVALID_SOURCE_PREFIX")?;
        let source = capture::read_capture(
            Path::new(get("--journal")?),
            end,
            &c,
            options.get("--expected-sha").copied(),
            true,
        )?;
        let workers = options
            .get("--workers")
            .unwrap_or(&"8")
            .parse()
            .map_err(|_| "INVALID_REPLAY_WORKERS")?;
        let result = sweep::run(&source, &c, workers)?;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(get("--out")?)
            .map_err(|_| "OUTPUT_EXISTS_OR_UNAVAILABLE")?;
        std::io::Write::write_all(
            &mut file,
            &serde_json::to_vec(&serde_json::json!({"capture":source.meta,"sweep":result,"configFileHash":config_file_hash}))
                .map_err(|_| "OUTPUT_INVALID")?,
        )
        .map_err(|_| "OUTPUT_WRITE_FAILED")?;
        file.sync_all().map_err(|_| "OUTPUT_SYNC_FAILED")?;
        println!(
            "{}",
            serde_json::json!({"sourceVerified":true,"captureDays":result.capture_days,"candidates":result.candidates,"captureMs":source.meta.verify_ms,"replayMs":result.replay_ms,"competitionOrders":0,"profitabilityEstablished":false})
        );
        return Ok(());
    }
    if command == "capture" {
        let c: types::Config =
            serde_json::from_slice(&fs::read(get("--config")?).map_err(|_| "CONFIG_OPEN_FAILED")?)
                .map_err(|_| "CONFIG_JSON_INVALID")?;
        let end = get("--end-byte")?
            .parse()
            .map_err(|_| "INVALID_SOURCE_PREFIX")?;
        let source = capture::read_capture(
            Path::new(get("--journal")?),
            end,
            &c,
            options.get("--expected-sha").copied(),
            true,
        )?;
        println!(
            "{}",
            serde_json::to_string(&source.meta).map_err(|_| "OUTPUT_INVALID")?
        );
        return Ok(());
    }
    if command == "replay" {
        if Path::new(get("--out")?).exists() {
            return Err("OUTPUT_EXISTS_OR_UNAVAILABLE".into());
        }
        let bank_bytes = fs::read(get("--bank")?).map_err(|_| "BANK_OPEN_FAILED")?;
        let bank_file_hash = format!("{:x}", Sha256::digest(&bank_bytes));
        let bank: serde_json::Value =
            serde_json::from_slice(&bank_bytes).map_err(|_| "BANK_JSON_INVALID")?;
        let c: types::Config =
            serde_json::from_value(bank["config"].clone()).map_err(|_| "CONFIG_JSON_INVALID")?;
        let end = get("--end-byte")?
            .parse()
            .map_err(|_| "INVALID_SOURCE_PREFIX")?;
        let receipt = match bank["snapshotOrderPolicy"].as_str() {
            None | Some("core_then_supplemental_v1") => false,
            Some("actual_receipt_order_v1") => true,
            _ => return Err("UNKNOWN_SNAPSHOT_POLICY".into()),
        };
        let source = capture::read_capture(
            Path::new(get("--journal")?),
            end,
            &c,
            options.get("--expected-sha").copied(),
            receipt,
        )?;
        let workers = options
            .get("--workers")
            .unwrap_or(&"8")
            .parse()
            .map_err(|_| "INVALID_REPLAY_WORKERS")?;
        let result = replay::replay_validation(&source, &bank, &c, workers)?;
        let output =
            serde_json::to_vec(&serde_json::json!({"capture":source.meta,"replay":result,"bankFileHash":bank_file_hash}))
                .map_err(|_| "OUTPUT_INVALID")?;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(get("--out")?)
            .map_err(|_| "OUTPUT_EXISTS_OR_UNAVAILABLE")?;
        std::io::Write::write_all(&mut file, &output).map_err(|_| "OUTPUT_WRITE_FAILED")?;
        file.sync_all().map_err(|_| "OUTPUT_SYNC_FAILED")?;
        println!(
            "{}",
            serde_json::json!({"sourceVerified":true,"candidates":result.candidates,"captureMs":source.meta.verify_ms,"prepareMs":result.prepare_ms,"replayMs":result.replay_ms,"workers":workers,"competitionOrders":0})
        );
        return Ok(());
    }
    Err("UNKNOWN_COMMAND".into())
}
fn main() {
    if let Err(error) = run() {
        eprintln!("{}", serde_json::json!({"error":error}));
        std::process::exit(1);
    }
}
