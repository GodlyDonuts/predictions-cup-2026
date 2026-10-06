// data/prepare-nh-paper-v5-20261004.mjs — deploy the no-maker v5 + baal-pair
// v3 cohorts carrying accounting continuations from their closed predecessors.
use crate::publication_pack::{now_ms, read_json, runtime_dir, sha};
use baal_collector::canonical_no_capacity_v2::validate_capacity_review;
use baal_collector::frozen_paper_graph::collect_frozen_paper_files;
use baal_collector::frozen_paper_runtime::frozen_paper_module;
use baal_collector::native_publication::native_publication_state;
use baal_collector::paper_book_continuation_frontier::paper_book_continuation_frontier;
use baal_collector::no_maker_paper_v3::no_maker_policy;
use baal_execution::core::payload_hash;
use baal_service_runtime::atomic::atomic_json;
use baal_service_runtime::contract::{ms_to_iso, parse_iso_ms};
use baal_service_runtime::journal::{journal_lines, JournalReadOptions};
use serde_json::{json, Value as V};
use sha2::{Digest, Sha256};
use std::io::Write;
use std::path::{Path, PathBuf};

fn need(v: bool, c: &str) -> Result<(), String> {
    if v { Ok(()) } else { Err(format!("NH_PAPER_DEPLOY_{c}")) }
}
struct VerifiedJournal {
    last: V, sequence: u64, hash: String, bytes: u64, file_hash: String,
}
fn verify_journal(file: &str) -> Result<VerifiedJournal, String> {
    let mut previous = String::new();
    let mut sequence = 0u64;
    let mut last = V::Null;
    let mut prefix = Sha256::new();
    let mut bytes = 0u64;
    for line in journal_lines(Path::new(file), &JournalReadOptions::default())? {
        let row: V = serde_json::from_str(&line).map_err(|e| e.to_string())?;
        let hash = row["hash"].as_str().unwrap_or("").to_string();
        let mut body = row.clone();
        body.as_object_mut().unwrap().remove("hash");
        sequence += 1;
        need(
            row["sequence"] == json!(sequence) && row["previous"] == json!(previous)
                && sha(serde_json::to_string(&body).unwrap().as_bytes()) == hash,
            "PRIOR_CHAIN",
        )?;
        previous = hash;
        last = row;
        prefix.update(format!("{line}\n").as_bytes());
        bytes += (line.len() + 1) as u64;
    }
    need(!last.is_null(), "EMPTY_PRIOR")?;
    Ok(VerifiedJournal {
        last, sequence, hash: previous, bytes,
        file_hash: format!("{:x}", prefix.finalize()),
    })
}
struct Prior {
    typ: &'static str, active: V, dir: String, bank: V, bank_bytes: Vec<u8>,
    stats: V, journal: VerifiedJournal, seed: V, seed_bytes: Vec<u8>,
}
pub fn run() -> Result<V, String> {
    let rt = runtime_dir();
    let read = |f: &str| read_json(Path::new(f));
    need(
        read(&format!("{rt}/COLLECTOR_STOP"))?["source"] == json!("nh-paper-v5-deployment-20261004"),
        "MARKER",
    )?;
    need(
        !Path::new(&format!("{rt}/../bot.lock")).exists()
            && !Path::new("STOP").exists()
            && !Path::new(&format!("{rt}/STOP")).exists(),
        "STOP_OR_LEASE",
    )?;
    need(read(&format!("{rt}/competition-activation.json"))?["enabled"] == json!(false), "ACTIVATION")?;
    let review = read("data/nh-conditional-refund-review-20261004.json")?;
    validate_capacity_review(&review)?;
    need(
        parse_iso_ms(review["recordedAt"].as_str().unwrap_or("")).unwrap_or(0.0) <= now_ms() as f64
            && parse_iso_ms(review["expiresAt"].as_str().unwrap_or("")).unwrap_or(0.0) > now_ms() as f64,
        "REVIEW_EXPIRED",
    )?;
    let prior = |typ: &'static str, expected: &str, revision: i64, module_path: &str,
        identity: &dyn Fn(&Path) -> Result<V, String>| -> Result<Prior, String> {
        let active = read(&format!("{rt}/{typ}-active.json"))?;
        need(
            active["cohort"] == json!(expected) && active["hostRevision"] == json!(revision),
            &format!("ACTIVE_{typ}"),
        )?;
        let dir = format!("{rt}/{typ}-cohorts/{}", active["cohort"].as_str().unwrap_or(""));
        let bank_bytes = std::fs::read(format!("{dir}/bank.json")).map_err(|e| e.to_string())?;
        let bank: V = serde_json::from_slice(&bank_bytes).map_err(|e| e.to_string())?;
        let stats = read(&format!("{dir}/stats.json"))?;
        let _preserved = frozen_paper_module(Path::new(&rt), &bank, module_path)?;
        let preserved_identity = identity(Path::new("live-paper"))?;
        need(
            payload_hash(&bank)? == active["bankHash"].as_str().unwrap_or("")
                && payload_hash(&bank["codeIdentity"])? == payload_hash(&preserved_identity)?,
            "FROZEN_PRIOR",
        )?;
        let journal = verify_journal(&format!("{dir}/journal.jsonl"))?;
        need(
            stats["failed"] != json!(true)
                && stats["journalSequence"] == json!(journal.sequence)
                && stats["journalHash"] == json!(journal.hash),
            "PUBLISHED_FRONTIER",
        )?;
        need(
            journal.last["event"]["kind"] == json!("boundary")
                && journal.last["event"]["reason"] == json!("close"),
            "PRIOR_NOT_CLOSED",
        )?;
        need(
            review["legs"].as_array().map(|a| a.len()).unwrap_or(0)
                == bank["review"]["legs"].as_array().map(|a| a.len()).unwrap_or(0)
                && review["legs"].as_array().unwrap().iter().zip(bank["review"]["legs"].as_array().unwrap())
                    .all(|(l, b)| ["marketId", "exchangeId", "outcome"].iter().all(|k| l[k] == b[k])),
            "SCOPE_CHANGED",
        )?;
        let seed = journal.last["state"]["paper"].clone();
        let seed_bytes = format!("{}\n", serde_json::to_string_pretty(&seed).unwrap()).into_bytes();
        need(seed["bankHash"] == active["bankHash"], "STATE_BANK")?;
        Ok(Prior { typ, active, dir, bank, bank_bytes, stats, journal, seed, seed_bytes })
    };
    let maker = prior("no-maker", "no-maker-tape-20261004-v3", 3,
        "live-paper/no-maker-host-v3.mjs",
        &|w: &Path| baal_collector::no_maker_host_v3::no_maker_code_identity_v3(w))?;
    let pair = prior("baal-pair", "baal-no-pair-20261004-v2", 2,
        "live-paper/baal-pair-host-v2.mjs",
        &|w: &Path| Ok(baal_collector::baal_pair_host_v2::baal_pair_code_identity(w)))?;
    let tape = verify_journal(&format!("{rt}/anonymous-trades.jsonl"))?;
    need(
        maker.journal.last["state"]["tapeSequence"] == json!(tape.sequence)
            && maker.journal.last["state"]["tapeHash"] == json!(tape.hash),
        "UNCONSUMED_TAPE",
    )?;
    let probability_active = read(&format!("{rt}/probability-receipt-active.json"))?;
    let probability_bank = read(&format!("{rt}/probability-receipt-cohorts/{}/bank.json",
        probability_active["cohort"].as_str().unwrap_or("")))?;
    let public_file = format!("{rt}/public-books.jsonl");
    let public_bytes = std::fs::read(&public_file).map_err(|e| e.to_string())?;
    let public_hash = sha(&public_bytes);
    let native = native_publication_state(
        Path::new(&rt), &probability_bank["nativeBackend"], &probability_bank["universe"],
        Path::new(&public_file), public_bytes.len() as u64, &public_hash,
        Path::new(&format!("{rt}/nh-paper-deployment-native-receipts")),
    )?;
    need(native["state"]["closed"] == json!(true), "SOURCE_NOT_CLOSED")?;
    let legs = pair.bank["review"]["legs"].as_array().cloned().unwrap_or_default();
    let frontier = paper_book_continuation_frontier(
        Path::new(&public_file), public_bytes.len() as u64, &pair.journal.last["state"], &legs)?;
    need(
        frontier["sealedSequence"] == native["state"]["sequence"]
            && frontier["sealedHash"] == native["state"]["hash"],
        "NATIVE_FRONTIER",
    )?;
    let cutoff = ms_to_iso(now_ms() as f64);
    need(
        parse_iso_ms(&cutoff).unwrap_or(0.0)
            < parse_iso_ms(review["expiresAt"].as_str().unwrap_or("")).unwrap_or(0.0),
        "REVIEW_EXPIRED_AFTER_VALIDATION",
    )?;
    let continuation = |p: &Prior, reason: &str| -> V {
        json!({
            "cohort": p.active["cohort"], "bankHash": p.active["bankHash"],
            "bankFileHash": sha(&p.bank_bytes), "journalPrefixBytes": p.journal.bytes,
            "journalPrefixHash": p.journal.file_hash,
            "openingStateFileHash": sha(&p.seed_bytes), "accountingCarried": true,
            "reason": reason,
        })
    };
    let mut maker_bank = maker.bank.clone();
    for (k, v) in [
        ("name", json!("no-maker-tape-20261004-v5")), ("hostRevision", json!(5)),
        ("cutoff", json!(cutoff)), ("policy", no_maker_policy()),
        ("codeIdentity", baal_collector::no_maker_host_v5::no_maker_code_identity_v5(Path::new("live-paper"))?),
        ("review", review.clone()), ("legs", review["legs"].clone()),
        ("reviewExpiresAt", review["expiresAt"].clone()),
        ("tapePrefixBytes", json!(tape.bytes)), ("tapePrefixHash", json!(tape.file_hash)),
        ("tapePrefixSequence", json!(tape.sequence)), ("tapePrefixTailHash", json!(tape.hash)),
        ("paperRevision", json!(3)),
        ("accountingContinuation", continuation(&maker, "fixed_quote_checkpoint_and_exact_consumed_frontier")),
        ("executionAuthority", json!(false)),
    ] { maker_bank.as_object_mut().unwrap().insert(k.into(), v); }
    let mut pair_bank = pair.bank.clone();
    let mut pair_cont = continuation(&pair, "fresh_source_refund_scenarios_unchanged_pair_economics");
    pair_cont.as_object_mut().unwrap().insert("sourceFrontier".into(), frontier.clone());
    for (k, v) in [
        ("name", json!("baal-no-pair-20261004-v3")), ("hostRevision", json!(3)),
        ("cutoff", json!(cutoff)), ("review", review.clone()),
        ("codeIdentity", baal_collector::baal_pair_host_v3::baal_pair_code_identity(Path::new("live-paper"))),
        ("sourcePrefixBytes", json!(public_bytes.len())),
        ("sourcePrefixHash", json!(public_hash)),
        ("sourcePrefixSequence", native["state"]["sequence"].clone()),
        ("sourcePrefixTailHash", native["state"]["hash"].clone()),
        ("accountingContinuation", pair_cont), ("executionAuthority", json!(false)),
    ] { pair_bank.as_object_mut().unwrap().insert(k.into(), v); }
    let plans: Vec<(&Prior, V)> = vec![(&maker, maker_bank.clone()), (&pair, pair_bank.clone())];
    for (p, b) in &plans {
        need(!Path::new(&format!("{}/{}-cohorts/{}", rt, p.typ, b["name"].as_str().unwrap_or(""))).exists(), "NO_OVERWRITE")?;
    }
    let files = collect_frozen_paper_files(Path::new("."), &[
        "live-paper/no-maker-host-v5.mjs".to_string(), "live-paper/baal-pair-host-v3.mjs".to_string()])?;
    let files_v = json!(files.iter().map(|(k, v)| (k.clone(), V::String(v.clone()))).collect::<serde_json::Map<String, V>>());
    let snapshot = payload_hash(&files_v)?;
    let root = format!("{rt}/frozen-code/{snapshot}");
    std::fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    for (file, hash) in &files {
        let target = PathBuf::from(format!("{root}/{file}"));
        let bytes = std::fs::read(file).map_err(|e| e.to_string())?;
        need(sha(&bytes) == *hash, "SOURCE_CHANGED")?;
        std::fs::create_dir_all(target.parent().unwrap()).map_err(|e| e.to_string())?;
        if target.exists() {
            need(sha(&std::fs::read(&target).map_err(|e| e.to_string())?) == *hash, "SNAPSHOT_CHANGED")?;
        } else {
            std::fs::OpenOptions::new().write(true).create_new(true).open(&target)
                .and_then(|mut f| f.write_all(&bytes)).map_err(|e| e.to_string())?;
        }
    }
    atomic_json(Path::new(&format!("{root}/manifest.json")), &json!({
        "schemaVersion": 1, "files": files_v,
    }))?;
    let index_file = format!("{rt}/frozen-code/index.json");
    let mut index = read(&index_file)?;
    for (p, b) in &plans {
        let dir = format!("{}/{}-cohorts/{}", rt, p.typ, b["name"].as_str().unwrap_or(""));
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        std::fs::OpenOptions::new().write(true).create_new(true).open(format!("{dir}/opening-state.json"))
            .and_then(|mut f| f.write_all(&p.seed_bytes)).map_err(|e| e.to_string())?;
        atomic_json(Path::new(&format!("{dir}/bank.json")), b)?;
        index["banks"][payload_hash(b)?] = json!({"snapshot": snapshot});
    }
    atomic_json(Path::new(&index_file), &index)?;
    for (p, b) in &plans {
        atomic_json(Path::new(&format!("{}/{}-active.json", rt, p.typ)), &json!({
            "schemaVersion": 1, "cohort": b["name"], "bankHash": payload_hash(b)?,
            "hostRevision": b["hostRevision"], "executionAuthority": false,
        }))?;
    }
    let _maker_frozen = frozen_paper_module(Path::new(&rt), &maker_bank,
        "live-paper/no-maker-host-v5.mjs")?;
    let _pair_frozen = frozen_paper_module(Path::new(&rt), &pair_bank,
        "live-paper/baal-pair-host-v3.mjs")?;
    let now_ms_fn = std::sync::Arc::new(|| crate::publication_pack::now_ms());
    let can_op = std::sync::Arc::new(|| true);
    let maker_host = baal_collector::no_maker_host_v5::open_no_maker_host_v5(
        Path::new(&rt), Path::new("live-paper"), now_ms_fn.clone(), can_op.clone())?.ok_or("NH_PAPER_DEPLOY_BOOT_PROBE")?;
    let pair_host = baal_collector::baal_pair_host_v3::BaalPairHost::open(
        Path::new(&rt), now_ms_fn, can_op)?.ok_or("NH_PAPER_DEPLOY_BOOT_PROBE")?;
    let maker_status = maker_host.status()?;
    let pair_status = pair_host.status();
    need(maker_status["failed"] != json!(true) && pair_status["failed"] != json!(true), "BOOT_PROBE")?;
    for (p, b) in &plans {
        need(
            sha(&std::fs::read(format!("{}/bank.json", p.dir)).map_err(|e| e.to_string())?) == sha(&p.bank_bytes)
                && verify_journal(&format!("{}/journal.jsonl", p.dir))?.file_hash == p.journal.file_hash,
            "PREDECESSOR_CHANGED",
        )?;
        atomic_json(Path::new(&format!("{}/STOP", p.dir)), &json!({
            "source": "Codex successor deployment", "at": cutoff,
            "successor": b["name"], "reason": b["accountingContinuation"]["reason"],
        }))?;
        atomic_json(Path::new(&format!("data/{}-bank-20261004-v{}.json", p.typ, b["hostRevision"].as_i64().unwrap_or(0))), b)?;
    }
    let maker_wallets = json!(maker_status["wallets"].as_object().map(|o| o.iter().map(|(k, w)| (
        k.clone(), json!({
            "cash": w["cash"], "reserved": w["reserved"], "fills": w["fills"],
            "realizedPnl": w["realizedPnl"],
            "positions": w["positions"].as_array().map(|a| a.len()).unwrap_or(0),
            "unresolvedQuotes": w["unresolvedQuotes"].as_array().map(|a| a.len()).unwrap_or(0),
        })
    )).collect::<serde_json::Map<String, V>>()).unwrap_or_default());
    let report = json!({
        "at": cutoff, "reviewHash": payload_hash(&review)?,
        "reviewExpiresAt": review["expiresAt"], "frozenSourceSnapshot": snapshot,
        "frozenFiles": files.len(),
        "maker": {"cohort": maker_bank["name"], "bankHash": payload_hash(&maker_bank)?,
            "accountingContinuation": maker_bank["accountingContinuation"],
            "openingWallets": maker_wallets},
        "pair": {"cohort": pair_bank["name"], "bankHash": payload_hash(&pair_bank)?,
            "accountingContinuation": pair_bank["accountingContinuation"],
            "openingWallet": {"cash": pair_status["cash"], "fills": pair_status["fills"],
                "realizedPnl": pair_status["realizedPnl"]}},
        "nativeSealedSource": {"bytes": public_bytes.len(), "hash": public_hash,
            "sequence": native["state"]["sequence"], "engineHash": native["engineHash"]},
        "accountingReset": false, "additionalReadSlots": 0,
        "additionalPersistentProcesses": 0, "competitionOrders": 0,
        "profitabilityEstablished": false,
    });
    atomic_json(Path::new("data/nh-paper-v5-preservation-20261004.json"), &report)?;
    Ok(report)
}
