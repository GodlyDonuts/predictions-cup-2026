// data/prepare-production-bank-20261004.mjs — stage the settlement production
// bank (GET-only) with a native settlement-value diagnostic.
use crate::publication_pack::{now_ms, read_json, runtime_dir, sha};
use baal_collector::model::digest;
use baal_collector::native_publication::native_publication_state;
use baal_collector::probability_receipt_v2::probability_account_eligibility;
use baal_collector::settlement_value_native::run_settlement_value;
use baal_execution::core::payload_hash;
use baal_service_runtime::contract::{ms_to_iso, parse_iso_ms};
use serde_json::{json, Value as V};
use std::io::Write;
use std::path::{Path, PathBuf};

fn assert_eq(a: &V, b: &V) -> Result<(), String> {
    if a == b { Ok(()) } else { Err("assert".into()) }
}
pub fn run() -> Result<V, String> {
    let rt = runtime_dir();
    let read = |f: &str| read_json(Path::new(f));
    let active = read(&format!("{rt}/probability-receipt-active.json"))?;
    let prior_dir = format!("{rt}/probability-receipt-cohorts/{}", active["cohort"].as_str().unwrap_or(""));
    let prior_bank_bytes = std::fs::read(format!("{prior_dir}/bank.json")).map_err(|e| e.to_string())?;
    let prior: V = serde_json::from_slice(&prior_bank_bytes).map_err(|e| e.to_string())?;
    let stats = read(&format!("{prior_dir}/stats.json"))?;
    assert_eq(&stats["bankHash"], &json!(digest(&prior)))?;
    assert_eq(&stats["failed"], &json!(false))?;
    let native_backend = read("data/settlement-value-milestone-20261004.json")?["nativeBackend"].clone();
    let mut contracts = serde_json::Map::<String, V>::new();
    let mut candidates = Vec::new();
    for (market_id, state) in [("297", "WV"), ("363", "TN"), ("357", "KS")] {
        let receipt = read(&format!("{rt}/production-metadata/baal-production-20261004-v1/{market_id}.json"))?;
        let root = receipt["root"].clone();
        let d = root["contract_details"].clone();
        let spec = prior["universe"]["exchanges"].as_array().unwrap()
            .iter().find(|x| x["marketId"] == json!(market_id)).cloned().ok_or("assert")?;
        assert_eq(&receipt["rootHash"], &json!(payload_hash(&root)?))?;
        assert_eq(&root["settled_with"], &V::Null)?;
        assert_eq(&d["usState"], &json!(state))?;
        assert_eq(&d["raceStage"], &json!("General"))?;
        assert_eq(&d["winnerName"], &json!("Republican Party"))?;
        assert_eq(&d["resolutionType"], &json!("Party Winner"))?;
        assert_eq(&d["electionDate"], &json!("2026-11-03"))?;
        let old = prior["research"].as_array().unwrap().iter().find(|r| r["marketId"] == json!(market_id)).cloned();
        let predicate_hash = match &old {
            Some(o) if o["predicateHash"].is_string() => o["predicateHash"].clone(),
            _ => V::String(digest(&json!({
                "marketId": market_id, "exchangeId": spec["exchangeId"],
                "rootHash": receipt["rootHash"],
                "visibleRulesHash": sha(&std::fs::read("data/kansas-current-contract-20261004.json").map_err(|e| e.to_string())?),
            }))),
        };
        let research = old.map(|o| json!({
            "id": o["id"], "predicateHash": predicate_hash, "side": "YES",
            "lower": o["lower"], "upper": o["upper"],
            "recordedAtMs": parse_iso_ms(o["recordedAt"].as_str().unwrap_or("")).unwrap_or(0.0),
            "sourceAtMs": parse_iso_ms(o["sourceAt"].as_str().unwrap_or("")).unwrap_or(0.0),
            "expiresAtMs": parse_iso_ms(o["expiresAt"].as_str().unwrap_or("")).unwrap_or(0.0),
            "method": o["method"], "settlementReviewed": o["settlementReviewed"],
            "flags": o["flags"], "sources": o["sources"],
        }));
        contracts.insert(market_id.to_string(), json!({
            "exchangeId": spec["exchangeId"], "predicateHash": predicate_hash,
            "rootHash": receipt["rootHash"], "receivedAt": receipt["receivedAt"],
            "sourceHash": sha(&std::fs::read(format!("{rt}/production-metadata/baal-production-20261004-v1/{market_id}.json")).map_err(|e| e.to_string())?),
        }));
        candidates.push(json!({
            "id": format!("{market_id}:YES"), "marketId": market_id,
            "exchangeId": spec["exchangeId"], "side": "YES",
            "predicateHash": predicate_hash,
            "riskGroups": ["national_election", format!("race_{}", d["raceId"].as_str().unwrap_or(""))],
            "settlesAtMs": parse_iso_ms(root["settlement_date"].as_str().unwrap_or("")).unwrap_or(0.0),
            "research": research,
        }));
    }
    let group_caps = {
        let mut m = serde_json::Map::new();
        m.insert("national_election".into(), json!("5.000000"));
        for c in &candidates {
            m.insert(c["riskGroups"][1].as_str().unwrap().to_string(), json!("2.000000"));
        }
        m
    };
    let policy = json!({
        "minimumEdge": "0.020000", "maximumSpread": "0.050000",
        "maximumBookAgeMs": 20000, "maximumResearchAgeMs": 21600000,
        "fractionalKellyBps": 2500, "slippageTicks": 1, "depthFractionBps": 100,
        "maxOrderCost": "1.000000", "maxExposure": "10.000000",
        "cashReserve": "90.000000", "cumulativeBuyBudget": "25.000000",
        "groupCaps": V::Object(group_caps),
    });
    let bank = json!({
        "schemaVersion": 1, "mode": "baal_settlement_production_v1",
        "cutoff": ms_to_iso(now_ms() as f64), "nativeBackend": native_backend,
        "policy": policy, "contracts": V::Object(contracts.clone()), "candidates": candidates,
        "limitations": [
            "Original assessment clocks are preserved. TN/WV are expired; Kansas has no admissible forecast.",
            "Group caps contain additional bot risk; inherited whole-account holdings are not adopted or fully modeled.",
            "This bank is staged and grants no execution authority.",
        ],
    });
    let bank_hash = digest(&bank);
    let bank_file = format!("{rt}/production-banks/{bank_hash}.json");
    std::fs::create_dir_all(format!("{rt}/production-banks")).map_err(|e| e.to_string())?;
    std::fs::OpenOptions::new().write(true).create_new(true).open(&bank_file)
        .and_then(|mut f| f.write_all(format!("{}\n", serde_json::to_string_pretty(&bank).unwrap()).as_bytes()))
        .map_err(|e| e.to_string())?;
    let dir = format!("{rt}/production-preflight/{}", now_ms());
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let file = format!("{rt}/public-books.jsonl");
    let source_len = stats["sourceBytes"].as_u64().unwrap_or(0) as usize;
    let prefix = {
        let mut b = vec![0u8; source_len];
        use std::io::Read;
        std::fs::File::open(&file).map_err(|e| e.to_string())?
            .read_exact(&mut b).map_err(|e| e.to_string())?;
        b
    };
    let publication = native_publication_state(
        Path::new(&rt), &native_backend, &prior["universe"], Path::new(&file),
        prefix.len() as u64, &sha(&prefix), Path::new(&format!("{dir}/source")),
    )?;
    assert_eq(&publication["state"]["sequence"], &stats["sourceSequence"])?;
    assert_eq(&publication["state"]["hash"], &stats["sourceHash"])?;
    let account = probability_account_eligibility(&read(&format!("{rt}/official-account.json"))?)
        .unwrap_or(V::Null);
    let as_of_ms = parse_iso_ms(stats["updatedAt"].as_str().unwrap_or("")).unwrap_or(0.0);
    let mapped: Vec<V> = candidates.iter().map(|c| {
        let mut c = c.clone();
        let key = format!("{}:YES", c["exchangeId"].as_str().unwrap_or(""));
        let p = &publication["state"]["books"][&key];
        let b = if p["book"].is_object() { p["book"].clone() } else { p.clone() };
        if b.is_object() && b["bids"].is_array() {
            c.as_object_mut().unwrap().insert("book".into(), json!({
                "requestStartedAtMs": parse_iso_ms(b["requestStartedAt"].as_str().unwrap_or("")).unwrap_or(0.0),
                "receivedAtMs": parse_iso_ms(b["receivedAt"].as_str().unwrap_or("")).unwrap_or(0.0),
                "availableAtMs": parse_iso_ms(if p["availableAt"].is_string() { p["availableAt"].as_str().unwrap() } else { p["publishedAt"].as_str().unwrap_or("") }).unwrap_or(0.0),
                "sequence": b["sequence"],
                "sourceHash": if p["publicationHash"].is_string() { p["publicationHash"].clone() } else if p["hash"].is_string() { p["hash"].clone() } else { publication["state"]["hash"].clone() },
                "representation": "direct_yes", "bids": b["bids"], "asks": b["asks"],
            }));
        } else {
            c.as_object_mut().unwrap().insert("book".into(), V::Null);
        }
        let ex_id = c["exchangeId"].clone();
        c.as_object_mut().unwrap().insert("account".into(), json!({
            "positionsAtMs": parse_iso_ms(account["positionsAt"].as_str().unwrap_or("")).unwrap_or(0.0),
            "ordersAtMs": parse_iso_ms(account["ordersAt"].as_str().unwrap_or("")).unwrap_or(0.0),
            "knownAtMs": parse_iso_ms(account["positionsAt"].as_str().unwrap_or("")).unwrap_or(0.0)
                .max(parse_iso_ms(account["ordersAt"].as_str().unwrap_or("")).unwrap_or(0.0)),
            "complete": true,
            "positionExcluded": account["positionExchangeIds"].as_array().map(|a| a.contains(&ex_id)).unwrap_or(false),
            "openOrderExcluded": account["openOrderExchangeIds"].as_array().map(|a| a.contains(&ex_id)).unwrap_or(false),
        }));
        c
    }).collect();
    let input = json!({
        "schemaVersion": 1, "mode": "settlement_value_policy_v1", "asOfMs": as_of_ms,
        "policy": policy,
        "wallet": {"capital": "100.000000", "cash": "100.000000",
            "cumulativeBuys": "0.000000", "positions": [], "reservations": []},
        "candidates": mapped,
    });
    let checked = run_settlement_value(&input, Path::new(&format!("{dir}/decision")),
        Path::new(&format!("{rt}/native-builds/{}/baal-backend.exe", native_backend["binaryHash"].as_str().unwrap_or(""))))?;
    assert_eq(&checked["result"]["proposedMaximumCost"], &json!("0.000000"))?;
    assert_eq(
        &json!(sha(&std::fs::read(format!("{prior_dir}/bank.json")).map_err(|e| e.to_string())?)),
        &json!(sha(&prior_bank_bytes)),
    )?;
    let evidence = json!({
        "schemaVersion": 1, "at": ms_to_iso(now_ms() as f64), "claimComment": 5984073274i64,
        "status": "production_code_deployed_get_only_bank_staged", "bankHash": bank_hash,
        "bankFileHash": sha(&std::fs::read(&bank_file).map_err(|e| e.to_string())?),
        "nativeBackend": native_backend,
        "metadata": contracts.iter().map(|(market_id, c)| json!({
            "marketId": market_id, "exchangeId": c["exchangeId"],
            "rootHash": c["rootHash"], "receivedAt": c["receivedAt"],
            "sourceHash": c["sourceHash"],
        })).collect::<Vec<V>>(),
        "source": {"bytes": prefix.len(), "hash": sha(&prefix),
            "sequence": stats["sourceSequence"], "asOf": stats["updatedAt"]},
        "diagnostic": {
            "walletKind": "isolated_counterfactual100_not_official_cash",
            "decisionMs": checked["wallMs"], "inputHash": checked["inputHash"],
            "outputHash": checked["outputHash"],
            "decisions": checked["result"]["decisions"].as_array().map(|a| a.iter().map(|d| json!({
                "candidateId": d["candidateId"], "action": d["action"], "reason": d["reason"],
            })).collect::<Vec<V>>()).unwrap_or_default(),
            "proposedMaximumCost": checked["result"]["proposedMaximumCost"], "fills": 0,
        },
        "activationEnabled": false, "ownershipHandoffComplete": false,
        "pendingReleaseComment": 5984085086i64, "qualifiedCurrentResearch": 0,
        "officialCashFreshlyReconciled": false, "fees": 0, "competitionOrders": 0,
        "walletsReset": false, "additionalPersistentProcesses": 0,
        "profitabilityEstablished": false,
    });
    std::fs::OpenOptions::new().write(true).create_new(true).open("data/production-bank-stage-20261004.json")
        .and_then(|mut f| f.write_all(format!("{}\n", serde_json::to_string_pretty(&evidence).unwrap()).as_bytes()))
        .map_err(|e| e.to_string())?;
    let _ = PathBuf::new();
    Ok(json!({
        "file": "data/production-bank-stage-20261004.json", "bankHash": bank_hash,
        "metadataMarkets": evidence["metadata"].as_array().map(|a| a.len()).unwrap_or(0),
        "decisions": evidence["diagnostic"]["decisions"],
        "activationEnabled": false, "qualifiedCurrentResearch": 0,
    }))
}
