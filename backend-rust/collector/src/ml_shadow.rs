// live-paper/ml-shadow.mjs — verified-source helpers (subset used downstream).
use baal_service_runtime::json_text::digest;
use baal_service_runtime::contract::parse_iso_ms;
use serde_json::{json, Value as V};
use sha2::{Digest, Sha256};

pub struct VerifiedSource { pub records: Vec<V>, pub prefix_hashes: Vec<String> }
/// verifySource(raw) — null when raw fails the whole-string shape, throw on
/// integrity/clock failures (JS semantics preserved: null vs Err).
pub fn verify_source(raw: &str) -> Result<Option<VerifiedSource>, String> {
    if raw.is_empty() || !raw.ends_with('\n') { return Ok(None); }
    let mut previous = String::new();
    let mut clock = f64::NEG_INFINITY;
    let mut records = Vec::new();
    let mut prefix_hashes = Vec::new();
    let mut prefix = Sha256::new();
    for (i, line) in raw[..raw.len() - 1].split('\n').enumerate() {
        let r: V = serde_json::from_str(line).map_err(|_| "Source JSON invalid".to_string())?;
        let hash = r["hash"].as_str().unwrap_or("").to_string();
        let mut body = r.clone();
        body.as_object_mut().unwrap().remove("hash");
        if r["sequence"].as_i64() != Some(i as i64 + 1)
            || r["previous"].as_str().unwrap_or("") != previous
            || hash != digest(&body) {
            return Err("Source integrity failure".into());
        }
        let next = r["event"]["clockAt"].as_str().or(r["at"].as_str()).and_then(parse_iso_ms)
            .ok_or("Source clock rollback")?;
        if next < clock { return Err("Source clock rollback".into()); }
        clock = next;
        prefix.update(format!("{}\n", line).as_bytes());
        prefix_hashes.push(format!("{:x}", prefix.clone().finalize()));
        previous = hash;
        records.push(r);
    }
    Ok(Some(VerifiedSource { records, prefix_hashes }))
}

// --- advanceShadow / shadowReport / combineReports / metadata ---

use baal_paper::fixed::{big_fee, TICK};
use num_bigint::BigInt;
use std::collections::BTreeSet;

/// requireSourceMetadata(bank)
pub fn require_source_metadata(bank: &V) -> Result<(), String> {
    crate::candidate_plan::validate_candidate_plan(bank)?;
    crate::snapshot_order::validate_snapshot_order(bank["snapshotOrderPolicy"].as_str().unwrap_or(""))?;
    if !bank.get("forecastLifecycle").is_none()
        && bank["forecastLifecycle"].as_str() != Some("revoke-on-invalidation-v2") {
        return Err("Unknown frozen forecast lifecycle".into());
    }
    crate::roundtrip_target::target_policy(bank["targetPolicy"].as_str().unwrap_or(""))?;
    crate::ml_bank::validate_training_scope(bank["trainingScope"].as_str().unwrap_or(""),
        bank["requireInstrumentCoverage"].as_bool().unwrap_or(false),
        bank["targetPolicy"].as_str().unwrap_or(""))?;
    if bank["sourceRecords"].as_i64().unwrap_or(0) < 1
        || bank["sourceHash"].as_str().is_none_or(|s| !(s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())))
        || parse_iso_ms(bank["trainingCaptureThrough"].as_str().unwrap_or("")).is_none() {
        return Err("Missing or invalid frozen training source metadata".into());
    }
    Ok(())
}
/// bindTrainingSource(records, bank, {allowUnboundFixture})
/// records is the verified stream (with prefix_hashes).
pub fn bind_training_source(source: &VerifiedSource, bank: &V, allow_unbound_fixture: bool) -> Result<Option<V>, String> {
    if allow_unbound_fixture && bank["sourceHash"].is_null() && bank["sourceRecords"].is_null() { return Ok(None); }
    require_source_metadata(bank)?;
    let n = bank["sourceRecords"].as_i64().unwrap_or(0);
    if n < 1 || n as usize > source.records.len()
        || bank["sourceHash"].as_str().is_none_or(|s| !(s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()))) {
        return Err("Invalid frozen training source".into());
    }
    let n = n as usize;
    let hash = source.prefix_hashes.get(n - 1).cloned().unwrap_or_else(|| {
        let joined = source.records[..n].iter().map(|r| serde_json::to_string(r).unwrap()).collect::<Vec<_>>().join("\n") + "\n";
        baal_service_runtime::json_text::sha256(joined.as_bytes())
    });
    if hash != bank["sourceHash"].as_str().unwrap_or("")
        || parse_iso_ms(source.records[n - 1]["event"]["clockAt"].as_str().or(source.records[n - 1]["at"].as_str()).unwrap_or(""))
            != parse_iso_ms(bank["trainingCaptureThrough"].as_str().unwrap_or("")) {
        return Err("Frozen training source mismatch".into());
    }
    Ok(Some(json!({"sourceRecords": n, "sourceHash": hash,
        "lastRecordHash": source.records[n - 1]["hash"].clone()})))
}
/// advanceShadow(input, records, bank, {allowUnboundFixture})
pub fn advance_shadow(input: &V, source: &VerifiedSource, bank: &V, allow_unbound_fixture: bool) -> Result<V, String> {
    crate::candidate_plan::validate_candidate_plan(bank)?;
    crate::snapshot_order::validate_snapshot_order(bank["snapshotOrderPolicy"].as_str().unwrap_or(""))?;
    let mut state = input.clone();
    let c = &bank["config"];
    let policy = crate::history_policy::history_policy(bank["historyPolicy"].as_str().unwrap_or(""))?;
    crate::roundtrip_target::target_policy(bank["targetPolicy"].as_str().unwrap_or(""))?;
    crate::ml_bank::validate_training_scope(bank["trainingScope"].as_str().unwrap_or(""),
        bank["requireInstrumentCoverage"].as_bool().unwrap_or(false), bank["targetPolicy"].as_str().unwrap_or(""))?;
    if state["bankHash"] != json!(crate::model::digest(bank)) { return Err("Frozen model bank changed".into()); }
    let binding = bind_training_source(source, bank, allow_unbound_fixture)?;
    if !state["sourceBinding"].is_null()
        && crate::model::digest(&state["sourceBinding"]) != crate::model::digest(&binding.clone().unwrap_or(V::Null)) {
        return Err("Frozen training binding changed".into());
    }
    if let Some(b) = &binding { state["sourceBinding"] = b.clone(); }
    if state["sourceSequence"].as_i64().unwrap_or(0) > source.records.len() as i64
        || (state["sourceSequence"].as_i64().unwrap_or(0) > 0
            && source.records[state["sourceSequence"].as_u64().unwrap() as usize - 1]["hash"] != state["sourceHash"]) {
        return Err("Source prefix changed on resume".into());
    }
    let revoke = bank["forecastLifecycle"].as_str() == Some("revoke-on-invalidation-v2");
    for r in &source.records {
        if r["sequence"].as_i64().unwrap_or(0) <= state["sourceSequence"].as_i64().unwrap_or(0) { continue; }
        let e = &r["event"];
        state["sourceSequence"] = r["sequence"].clone();
        state["sourceHash"] = r["hash"].clone();
        let capture_through = e["clockAt"].as_str().or(r["at"].as_str()).unwrap_or("").to_string();
        state["captureThrough"] = json!(capture_through);
        if parse_iso_ms(&capture_through).unwrap_or(0.0)
            <= parse_iso_ms(bank["trainingCaptureThrough"].as_str().unwrap_or("")).unwrap_or(f64::MAX) { continue; }
        if e["type"].as_str() == Some("account_read_outage") {
            if let Some(wallets) = state["wallets"].as_object_mut() {
                for (_id, wallet) in wallets.iter_mut() {
                    crate::model::invalidate(wallet, None, &capture_through, "account_outage");
                    if revoke {
                        let pend: Vec<String> = wallet["strategies"]["mean_reversion"]["pending"].as_object()
                            .map(|p| p.iter().filter(|(_, p)| p["action"].as_str() == Some("buy")).map(|(k, _)| k.clone()).collect()).unwrap_or_default();
                        for id in pend {
                            wallet["strategies"]["mean_reversion"]["pending"].as_object_mut().unwrap().remove(&id);
                            let n = wallet["strategies"]["mean_reversion"]["expiredIntents"].as_i64().unwrap_or(0);
                            wallet["strategies"]["mean_reversion"]["expiredIntents"] = json!(n + 1);
                        }
                    }
                }
            }
            state["histories"] = json!({});
            continue;
        }
        if e["type"].as_str() != Some("cycle") { continue; }
        for snap in crate::snapshot_order::ordered_snapshots(e, c, bank["snapshotOrderPolicy"].as_str().unwrap_or(""), Some(&capture_through))? {
            if !snap["skipped"].is_null() {
                if let Some(wallets) = state["wallets"].as_object_mut() {
                    let mid = snap["marketId"].as_str().unwrap_or("").to_string();
                    for (_id, wallet) in wallets.iter_mut() {
                        let at = if !snap["at"].is_null() { snap["at"].as_str().unwrap_or(&capture_through).to_string() } else { capture_through.clone() };
                        crate::model::invalidate(wallet, Some(&mid), &at, snap["skipped"].as_str().unwrap_or(""));
                        if revoke {
                            let pend: Vec<String> = wallet["strategies"]["mean_reversion"]["pending"].as_object()
                                .map(|p| p.iter().filter(|(k, p)| p["action"].as_str() == Some("buy")
                                        && wallet["books"][k.as_str()]["marketId"].as_str() == Some(&mid))
                                    .map(|(k, _)| k.clone()).collect()).unwrap_or_default();
                            for id in pend {
                                wallet["strategies"]["mean_reversion"]["pending"].as_object_mut().unwrap().remove(&id);
                                let n = wallet["strategies"]["mean_reversion"]["expiredIntents"].as_i64().unwrap_or(0);
                                wallet["strategies"]["mean_reversion"]["expiredIntents"] = json!(n + 1);
                            }
                        }
                    }
                }
                crate::history_policy::reset_history(state["histories"].as_object_mut().map(|o| {
                    // reset_history takes &mut Map<String,V>
                    o
                }).unwrap_or(&mut serde_json::Map::new()), snap["marketId"].as_str(), policy)?;
                continue;
            }
            let endpoint = snap["endpoint"].as_str().unwrap_or("");
            let mid = endpoint.strip_prefix("/markets/").and_then(|x| x.split('/').next()).unwrap_or("").to_string();
            if mid.is_empty() || !endpoint.contains("/orderbook?") { continue; }
            if !c["marketIds"].as_array().map(|a| a.iter().any(|x| x.as_str() == Some(&mid))).unwrap_or(false) { continue; }
            if parse_iso_ms(snap["requestStartedAt"].as_str().unwrap_or("")).unwrap_or(0.0)
                <= parse_iso_ms(bank["trainingCaptureThrough"].as_str().unwrap_or("")).unwrap_or(f64::MAX) { continue; }
            let excluded: BTreeSet<String> = e["accountExclusions"].as_array()
                .map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default();
            let b = match crate::model::normalize_book(&snap["data"], &mid, c["tournamentId"].as_str().unwrap_or(""),
                snap["receivedAt"].as_str().unwrap_or(""), snap["requestStartedAt"].as_str().unwrap_or(""),
                c["maxDataAgeMs"].as_i64().unwrap_or(0), &excluded, "authoritative_receipt_v1") {
                Ok(b) => b,
                Err(_) => {
                    crate::history_policy::reset_history(state["histories"].as_object_mut().unwrap_or(&mut serde_json::Map::new()), Some(&mid), policy)?;
                    if let Some(wallets) = state["wallets"].as_object_mut() {
                        for (_id, w) in wallets.iter_mut() {
                            crate::model::invalidate(w, Some(&mid), snap["receivedAt"].as_str().unwrap_or(""), "invalid_book");
                        }
                    }
                    continue;
                }
            };
            let inst = b["instrument"].as_str().unwrap_or("").to_string();
            let mut history = state["histories"][&inst].as_array().cloned().unwrap_or_default();
            if !history.is_empty() && parse_at(&b["receivedAt"]) - parse_at(&history.last().unwrap()["at"]) > 120000.0 { history = Vec::new(); }
            let base = crate::ml_bank::features(&b, &history);
            let x = if bank["breadthContext"].as_bool().unwrap_or(false) {
                let mut v = base.clone();
                v.extend(crate::breadth_context::context_features(&state["breadth"], &b, c["tournamentId"].as_str().unwrap_or("")).unwrap_or([0.0; 7]));
                v
            } else { base };
            let mid_price = x[11];
            let mut advanced = false;
            for model in bank["selected"].as_array().cloned().unwrap_or_default() {
                for margin in crate::candidate_plan::model_margins(bank, model["id"].as_str().unwrap_or("")) {
                    let id = format!("{}-edge{}", model["id"].as_str().unwrap_or(""), margin);
                    if bank["requireInstrumentCoverage"].as_bool().unwrap_or(false) {
                        let support = model["coverage"].as_array().and_then(|cv| cv.iter().find(|x| x["marketId"] == b["marketId"]));
                        if support.is_none_or(|s| s["eligible"] != V::Bool(true)
                            || !s["trainingInstruments"].as_array().map(|t| t.iter().any(|i| *i == b["instrument"])).unwrap_or(false)) {
                            let key = format!("{}:{}", id, inst);
                            state["unsupportedInstruments"][&key] = json!({"modelId": model["id"], "marketId": b["marketId"],
                                "instrument": b["instrument"], "at": b["receivedAt"], "reason": "not_in_frozen_training_support"});
                            if !state["wallets"][&id].is_null() {
                                crate::model::invalidate(&mut state["wallets"][&id], b["marketId"].as_str(),
                                    b["receivedAt"].as_str().unwrap_or(""), "unsupported_instrument");
                                state["wallets"][&id]["strategies"]["mean_reversion"]["pending"].as_object_mut()
                                    .map(|p| p.remove(&inst));
                            }
                            continue;
                        }
                    }
                    let w = if state["wallets"][&id].is_null() {
                        crate::model::initial_state(c, &id, bank["trainingCaptureThrough"].as_str().unwrap_or(""))?
                    } else { state["wallets"][&id].clone() };
                    let same = w["books"][&inst]["versionKey"] == b["versionKey"];
                    let ask = baal_paper::fixed::big_amount(b["asks"][0]["price"].as_str().unwrap_or(""))?
                        + BigInt::from(TICK) * BigInt::from(c["slippageTicks"].as_i64().unwrap_or(0));
                    let costs = json!({"feeBps": c["feeBps"], "feePerShare": "0.000000"});
                    let entry_cost = (ask.clone() + big_fee(&ask, &BigInt::from(1), &costs)?).to_string().parse::<f64>().unwrap_or(0.0) / 1e6;
                    let roundtrip = crate::roundtrip_target::target_policy(bank["targetPolicy"].as_str().unwrap_or(""))
                        .ok() == Some(crate::roundtrip_target::ROUND_TRIP);
                    let forecast = if history.len() >= 6 {
                        crate::ml_bank::predict(&model, &x).ok().filter(|v| v.is_finite())
                    } else { None };
                    let exit_value = if roundtrip || forecast.is_none() { None } else { Some(mid_price + forecast.unwrap()) };
                    let net_edge = if roundtrip { forecast } else { exit_value.map(|e| e - entry_cost) };
                    if state["signalMetrics"].is_null() { state["signalMetrics"] = json!({}); }
                    if state["signalMetrics"][&id].is_null() {
                        state["signalMetrics"][&id] = json!({"observations":0,"warmup":0,"invalidForecast":0,"edgeBelowMargin":0,"edgePass":0,"zeroTopAskCapacity":0});
                    }
                    if !same {
                        let m = &mut state["signalMetrics"][&id];
                        m["observations"] = json!(m["observations"].as_i64().unwrap_or(0) + 1);
                        if history.len() < 6 { m["warmup"] = json!(m["warmup"].as_i64().unwrap_or(0) + 1); }
                        else if if roundtrip { net_edge.is_none() } else { exit_value.is_none_or(|v| !(0.0..=1.0).contains(&v)) } {
                            m["invalidForecast"] = json!(m["invalidForecast"].as_i64().unwrap_or(0) + 1);
                        } else if net_edge.unwrap_or(0.0) < margin { m["edgeBelowMargin"] = json!(m["edgeBelowMargin"].as_i64().unwrap_or(0) + 1); }
                        else { m["edgePass"] = json!(m["edgePass"].as_i64().unwrap_or(0) + 1); }
                        if (b["asks"][0]["quantity"].as_i64().unwrap_or(0) * c["depthFractionBps"].as_i64().unwrap_or(0) / 10000) < 1 {
                            m["zeroTopAskCapacity"] = json!(m["zeroTopAskCapacity"].as_i64().unwrap_or(0) + 1);
                        }
                    }
                    state["wallets"][&id] = crate::model::step_forecast(&w, &b, c, exit_value,
                        if roundtrip { net_edge } else { None }, model["horizon"].as_i64().unwrap_or(0) * 1000, margin)?;
                    state["predictions"][&id] = json!({"at": b["receivedAt"], "exitValue": exit_value, "netEdge": net_edge,
                        "entryCost": entry_cost, "modelRmse": model["rmse"], "margin": margin,
                        "instrument": b["instrument"], "warmupRemaining": (6i64 - history.len() as i64).max(0)});
                    if !same { advanced = true; }
                }
            }
            if advanced {
                history.push(json!({"at": b["receivedAt"], "mid": mid_price, "marketId": b["marketId"]}));
                if history.len() > 30 { history.remove(0); }
                state["histories"][&inst] = json!(history);
            }
        }
        if bank["breadthContext"].as_bool().unwrap_or(false) {
            state["breadth"] = crate::breadth_context::update_context(
                if state["breadth"].is_null() { None } else { Some(&state["breadth"]) }, Some(&e["bulkPrices"]), Some(&capture_through))?;
        }
    }
    Ok(state)
}
fn parse_at(v: &V) -> f64 { parse_iso_ms(v.as_str().unwrap_or("")).unwrap_or(f64::NAN) }
/// shadowReport(s, bank, now)
pub fn shadow_report(s: &V, bank: &V, now: &str) -> Result<V, String> {
    let mut candidates = Vec::new();
    for (id, w) in s["wallets"].as_object().cloned().unwrap_or_default() {
        crate::model::assert_state(&w, &bank["config"])?;
        let r = crate::model::report(&w, &bank["config"], now);
        let r = &r["strategies"]["mean_reversion"];
        candidates.push(json!({"id": id, "cash": r["cash"], "realizedPnl": r["realizedPnl"],
            "referencePnl": r["referencePnl"], "liquidationPnl": r["liquidationPnl"],
            "intents": r["intents"], "fills": r["fills"], "closedTrades": r["closedTrades"], "fees": r["fees"],
            "positions": r["positions"].as_array().unwrap_or(&vec![]).iter().map(|p| json!({"instrument": p["instrument"],
                "quantity": p["quantity"], "unsoldShares": p["unsoldShares"], "cost": p["cost"],
                "enteredAt": p["enteredAt"], "observationAgeMs": p["observationAgeMs"]})).collect::<Vec<_>>(),
            "prediction": s["predictions"][&id], "signalMetrics": s["signalMetrics"].get(&id).cloned().unwrap_or(V::Null)}));
    }
    Ok(json!({"mode":"prospective_ml_paper","candidatePlan": bank.get("candidatePlan").cloned().unwrap_or(V::Null),
        "portfolioSelection": if bank["portfolioSelection"].is_null() { V::Null } else {
            json!({"version": bank["portfolioSelection"]["version"], "decision": bank["portfolioSelection"]["decision"],
                "holdout": bank["portfolioSelection"]["holdout"]}) },
        "feeBps": bank["config"]["feeBps"],
        "feeScenario": if bank["config"]["feeBps"].as_i64() == Some(0) { "zero_additional_explicit_fee_baseline" } else { "historical_fee_stress" },
        "updatedAt": now, "bankHash": s["bankHash"],
        "sourceBinding": if s["sourceBinding"].is_null() { V::Null } else { s["sourceBinding"].clone() },
        "unsupportedInstruments": s["unsupportedInstruments"].as_object().map(|o| o.values().cloned().collect::<Vec<_>>()).unwrap_or_default(),
        "targetPolicy": bank["targetPolicy"].as_str().unwrap_or("legacy_decision_time_bid_forecast_not_executable_roundtrip_target"),
        "trainingCaptureThrough": bank["trainingCaptureThrough"], "captureThrough": s["captureThrough"],
        "modelsAttempted": bank["candidatesAttempted"], "modelsTrained": bank["modelsTrained"],
        "modelRejections": bank["rejected"].as_array().map(|a| a.len()).unwrap_or(0),
        "candidateWallets": candidates.len(), "sourceSequence": s["sourceSequence"], "candidates": candidates,
        "competitionOrders": 0, "networkRequests": 0, "profitabilityEstablished": false,
        "limitations": bank["limitations"]}))
}
/// combineReports(base, cohorts: &[(name, report)])
pub fn combine_reports(base: &V, cohorts: &[(String, V)]) -> V {
    let mut reports: Vec<(String, &V)> = vec![("ridge-original".into(), base)];
    for (n, r) in cohorts { reports.push((n.clone(), r)); }
    let mut out = base.clone();
    out["status"] = json!(if base["status"].as_str() == Some("failed") { "failed" }
        else if reports.iter().any(|(_, r)| r["status"].as_str() == Some("failed")) { "degraded" } else { base["status"].as_str().unwrap_or("") });
    out["modelsAttempted"] = json!(reports.iter().map(|(_, r)| r["modelsAttempted"].as_i64().unwrap_or(0)).sum::<i64>());
    out["modelsTrained"] = json!(reports.iter().map(|(_, r)| r["modelsTrained"].as_i64().unwrap_or(0)).sum::<i64>());
    out["modelRejections"] = json!(reports.iter().map(|(_, r)| r["modelRejections"].as_i64().unwrap_or(0)).sum::<i64>());
    out["candidateWallets"] = json!(reports.iter().map(|(_, r)| r["candidateWallets"].as_i64().unwrap_or(0)).sum::<i64>());
    out["cohorts"] = json!(reports.iter().map(|(name, r)| json!({"name": name, "status": r["status"], "feeBps": r["feeBps"],
        "feeScenario": r["feeScenario"], "bankHash": r["bankHash"], "sourceBinding": r["sourceBinding"],
        "targetPolicy": r["targetPolicy"], "trainingCaptureThrough": r["trainingCaptureThrough"],
        "modelsTrained": r["modelsTrained"], "candidateWallets": r["candidateWallets"]})).collect::<Vec<_>>());
    out["candidates"] = json!(reports.iter().flat_map(|(name, r)| {
        r["candidates"].as_array().cloned().unwrap_or_default().into_iter().map(|c| {
            let mut c = c;
            c["id"] = json!(format!("{}/{}", name, c["id"].as_str().unwrap_or("")));
            c["cohort"] = json!(name);
            c["cohortStatus"] = r["status"].clone();
            c["feeBps"] = r["feeBps"].clone();
            c["feeScenario"] = r["feeScenario"].clone();
            c
        }).collect::<Vec<_>>()
    }).collect::<Vec<_>>());
    out
}
