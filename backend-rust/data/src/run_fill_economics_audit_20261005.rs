// data/run-fill-economics-audit-20261005.mjs — attributed fill economics audit.
use baal_collector::passive_opportunity_audit::audit_passive_opportunities;
use baal_paper::fixed::{big_amount as amount, money};
use baal_service_runtime::json_text::{js_stringify, sha256};
use baal_service_runtime::journal::{journal_lines, JournalReadOptions};
use baal_service_runtime::ledger::replay_journal;
use num_bigint::BigInt;
use serde_json::{json, Value as V};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

fn sha(b: &[u8]) -> String { sha256(b) }
fn read(f: &Path) -> Result<V, String> {
    serde_json::from_slice(&std::fs::read(f).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}
fn rt() -> PathBuf {
    Path::new(&std::env::var("LOCALAPPDATA").unwrap_or_default()).join("SigTradingCup").join("paper-runtime")
}
fn now_iso() -> String {
    baal_service_runtime::contract::ms_to_iso(std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as f64).unwrap_or(0.0))
}
fn parse_ms(s: &V) -> f64 {
    s.as_str().and_then(baal_service_runtime::contract::parse_iso_ms).unwrap_or(f64::NAN)
}
fn js_str(v: &V) -> String {
    match v {
        V::Null => "undefined".to_string(),
        V::String(s) => s.clone(),
        V::Bool(b) => b.to_string(),
        V::Number(n) => js_stringify(&V::Number(n.clone())),
        other => js_stringify(other),
    }
}
fn eq(a: &V, b: &V) -> Result<(), String> {
    if a == b { Ok(()) } else { Err(format!("assert.equal: {} !== {}", js_stringify(a), js_stringify(b))) }
}
fn a_true(ok: bool) -> Result<(), String> {
    if ok { Ok(()) } else { Err("assertion failed".to_string()) }
}
fn write_pretty_excl(f: &Path, v: &V) -> Result<(), String> {
    use std::io::Write;
    let mut fd = std::fs::OpenOptions::new().write(true).create_new(true).open(f).map_err(|e| e.to_string())?;
    fd.write_all(format!("{}\n", serde_json::to_string_pretty(v).map_err(|e| e.to_string())?).as_bytes())
        .map_err(|e| e.to_string())
}
struct Verified { rows: Vec<V>, last: V, proof: V }
fn verified(file: &Path, through: Option<i64>) -> Result<Verified, String> {
    let mut seq = 0i64;
    let mut previous = String::new();
    let mut bytes = 0u64;
    let mut last = V::Null;
    let mut h = Sha256::new();
    let mut rows = Vec::new();
    for line in journal_lines(file, &JournalReadOptions::default())? {
        let r: V = serde_json::from_str(&line).map_err(|e| e.to_string())?;
        let mut body = r.as_object().cloned().unwrap_or_default();
        let hash = body.shift_remove("hash").unwrap_or(V::Null);
        seq += 1;
        eq(&r["sequence"], &json!(seq))?;
        eq(&r["previous"], &json!(previous))?;
        eq(&hash, &json!(sha(js_stringify(&V::Object(body)).as_bytes())))?;
        previous = hash.as_str().unwrap_or("").to_string();
        h.update(format!("{}\n", line).as_bytes());
        bytes += line.len() as u64 + 1;
        last = r.clone();
        rows.push(r);
        if Some(seq) == through { break; }
    }
    if through.is_some() { eq(&json!(seq), &json!(through.unwrap()))?; }
    Ok(Verified { rows, last, proof: json!({"bytes": bytes as i64,
        "sha256": hex_encode(&h.finalize()), "sequence": seq, "tailHash": previous}) })
}
fn hex_encode(d: &[u8]) -> String { d.iter().map(|b| format!("{b:02x}")).collect() }
fn amount_v(v: &V) -> BigInt { amount(v.as_str().unwrap_or("")).unwrap_or_else(|_| BigInt::from(0)) }

pub fn run(root: &Path, argv: &[String]) -> Result<V, String> {
    let verify_existing = argv.iter().any(|a| a == "--verify-existing");
    let existing = if verify_existing {
        Some(read(&root.join("data/fill-economics-audit-20261005.json"))?)
    } else { None };
    let runtime = rt();
    let passive_through = existing.as_ref()
        .map(|e| e["sourceProof"]["passive"]["sequence"].as_i64()).flatten();
    let passive = verified(&runtime.join("stable-maker-cohorts")
        .join("baal-stable-spread-20261005-v2-live").join("journal.jsonl"), passive_through)?;
    let state = &passive.last["state"];
    let tape = verified(&runtime.join("anonymous-trades.jsonl"), state["tapeSequence"].as_i64())?;
    eq(&tape.last["hash"], &state["tapeHash"])?;
    let quotes: Vec<V> = state["paper"]["wallets"].as_object().cloned().unwrap_or_default()
        .values().flat_map(|w| w["quotes"].as_object().cloned().unwrap_or_default()
            .values().cloned().collect::<Vec<V>>()).collect();
    let events: Vec<V> = tape.rows.iter().map(|r| r["event"].clone()).collect();
    let opportunities = audit_passive_opportunities(&quotes, &events)?;
    let core = verified(&runtime.join("competition").join("core-journal.jsonl"),
        existing.as_ref().map(|e| e["sourceProof"]["officialCore"]["sequence"].as_i64()).flatten())?;
    let mut intents: Vec<V> = core.last["state"]["intents"].as_object().cloned().unwrap_or_default()
        .values().cloned().collect();
    intents.sort_by(|a, b| parse_ms(&a["preparedAt"]).partial_cmp(&parse_ms(&b["preparedAt"]))
        .unwrap_or(std::cmp::Ordering::Equal));
    // marketId -> {buyShares,sellShares} as numbers + (buyCost,sellProceeds,realized) BigInts.
    #[derive(Default, Clone)]
    struct MarketAgg { market_id: V, buy_shares: i64, sell_shares: i64,
        buy_cost: BigInt, sell_proceeds: BigInt, realized: BigInt }
    let mut inventory: indexmap::IndexMap<String, Vec<(i64, BigInt)>> = indexmap::IndexMap::new();
    let mut markets: indexmap::IndexMap<String, MarketAgg> = indexmap::IndexMap::new();
    let mut actual_fills = 0i64;
    let mut at_limit = 0i64;
    let mut price_improved = 0i64;
    let ledger_bytes = std::fs::read(runtime.join("competition").join("execution-journal.jsonl"))
        .map_err(|e| e.to_string())?;
    let ledger = replay_journal(&String::from_utf8_lossy(&ledger_bytes), None)
        .map_err(|e| e.to_string())?;
    for i in &intents {
        let attributed = &ledger["intents"][i["intentId"].as_str().unwrap_or("")];
        eq(&attributed["attribution"]["ownerId"], &json!("Codex"))?;
        eq(&json!(js_str(&attributed["orderId"])), &json!(js_str(&i["orderId"])))?;
        eq(&attributed["attribution"]["marketId"], &i["marketId"])?;
        eq(&i["acceptanceUncertain"], &json!(false))?;
        eq(&i["status"], &json!("closed"))?;
        let mut filled = 0i64;
        for f in i["reconciledFills"].as_array().cloned().unwrap_or_default() {
            actual_fills += 1;
            filled += f["quantity"].as_i64().unwrap_or(0);
            a_true(f["quantity"].as_i64().unwrap_or(0) > 0)?;
            let px = amount_v(&f["price"]);
            let limit = amount_v(&i["price"]);
            a_true(if i["action"].as_str() == Some("buy") { px <= limit } else { px >= limit })?;
            if px == limit { at_limit += 1; } else { price_improved += 1; }
            let mid = i["marketId"].as_str().unwrap_or("").to_string();
            markets.entry(mid.clone()).or_insert_with(|| MarketAgg {
                market_id: i["marketId"].clone(), ..Default::default() });
            let fifo = inventory.entry(i["exchangeId"].as_str().unwrap_or("").to_string())
                .or_insert_with(Vec::new);
            if i["action"].as_str() == Some("buy") {
                fifo.push((f["quantity"].as_i64().unwrap_or(0), px.clone()));
                let m = markets.get_mut(&mid).unwrap();
                m.buy_shares += f["quantity"].as_i64().unwrap_or(0);
                m.buy_cost += BigInt::from(f["quantity"].as_i64().unwrap_or(0)) * &px;
            } else {
                let m = markets.get_mut(&mid).unwrap();
                m.sell_shares += f["quantity"].as_i64().unwrap_or(0);
                m.sell_proceeds += BigInt::from(f["quantity"].as_i64().unwrap_or(0)) * &px;
                let mut remaining = f["quantity"].as_i64().unwrap_or(0);
                let mut realized_add = BigInt::from(0);
                while remaining > 0 {
                    if fifo.is_empty() {
                        return Err("Unattributed sell cannot be adopted".into());
                    }
                    let (lot_q, lot_p) = fifo[0].clone();
                    let n = remaining.min(lot_q);
                    realized_add += BigInt::from(n) * (&px - &lot_p);
                    remaining -= n;
                    if lot_q - n > 0 {
                        fifo[0] = (lot_q - n, lot_p);
                    } else {
                        fifo.remove(0);
                    }
                }
                markets.get_mut(&mid).unwrap().realized += realized_add;
            }
        }
        eq(&json!(filled), &i["filledQuantity"])?;
        eq(&json!(filled), &i["quantity"])?;
    }
    a_true(inventory.values().all(|lots| lots.is_empty()))?;
    let decisions = verified(&runtime.join("competition").join("decisions.jsonl"),
        existing.as_ref().map(|e| e["sourceProof"]["officialDecisions"]["sequence"].as_i64()).flatten())?;
    let matched: Vec<V> = decisions.rows.iter()
        .map(|r| r["event"]["decision"].clone())
        .filter(|d| d["code"].as_str() == Some("INTENT_READY"))
        .flat_map(|d| intents.iter()
            .filter(|i| i["action"].as_str() == Some("buy")
                && i["marketId"] == d["marketId"]
                && (parse_ms(&i["preparedAt"]) - parse_ms(&d["clockAt"])).abs() < 1000.0)
            .map(|i| {
                let mut seen = std::collections::HashSet::new();
                let mut prices = Vec::new();
                for f in i["reconciledFills"].as_array().cloned().unwrap_or_default() {
                    let k = js_stringify(&f["price"]);
                    if seen.insert(k) { prices.push(f["price"].clone()); }
                }
                json!({"marketId": d["marketId"], "decisionAt": d["clockAt"], "sourceBid": d["bid"],
                    "sourceAsk": d["ask"], "submittedLimit": i["price"], "actualFillPrices": prices})
            }).collect::<Vec<V>>()).collect();
    let mechanism = read(&root.join("data/native-mechanisms-future-1791205489070.json"))?;
    let full = std::fs::read(runtime.join("research-cohorts")
        .join(mechanism["protocol"]["cohort"].as_str().unwrap_or(""))
        .join(mechanism["fullOutputName"].as_str().unwrap_or(""))).map_err(|e| e.to_string())?;
    eq(&json!(sha(&full)), &mechanism["fullOutputHash"])?;
    let native: V = serde_json::from_slice(&full).map_err(|e| e.to_string())?;
    let mut sensitivity: Vec<V> = Vec::new();
    for (side, s) in native["markouts"]["sides"].as_object().cloned().unwrap_or_default() {
        for c in s["strict"]["cards"].as_array().cloned().unwrap_or_default() {
            let observed: Vec<V> = c["episodes"].as_array().cloned().unwrap_or_default().into_iter()
                .filter(|e| e["status"].as_str() == Some("observed")).collect();
            let ideal: Vec<BigInt> = observed.iter().map(|e| {
                amount_v(&e["quotedNetMarkoutMicros"]) + amount_v(&e["slippageCostMicros"])
            }).collect();
            let mean_ideal = if ideal.is_empty() { V::Null } else {
                let total: BigInt = ideal.iter().sum();
                json!(total.to_string().parse::<f64>().unwrap_or(f64::NAN) / ideal.len() as f64 / 1e6)
            };
            sensitivity.push(json!({"side": side, "strategy": c["strategy"],
                "scenario": c["scenario"]["id"], "observed": ideal.len() as i64,
                "declaredAdverseTicks": c["scenario"]["slippageTicks"],
                "meanDeclaredStress": c["summary"]["quotedNetMarkout"]["mean"],
                "meanWithoutAdditionalAdverseTicks": mean_ideal,
                "positiveWithoutAdditionalAdverseTicks":
                    ideal.iter().filter(|x| **x > BigInt::from(0)).count() as i64}));
        }
    }
    let total_realized: BigInt = markets.values().map(|m| m.realized.clone()).sum();
    let markets_report: Vec<V> = markets.values().map(|m| {
        json!({"marketId": m.market_id, "buyShares": m.buy_shares, "sellShares": m.sell_shares,
            "buyCost": money(&m.buy_cost), "sellProceeds": money(&m.sell_proceeds),
            "recordedPriceGrossRealized": money(&m.realized)})
    }).collect();
    let report = json!({"schemaVersion": 1, "at": now_iso(), "claimComment": 5996357048i64,
        "mode": "attributed_fill_cost_and_counterfactual_opportunity_audit",
        "sourceProof": {"passive": passive.proof, "tape": tape.proof, "officialCore": core.proof,
            "officialIntentLedger": {"bytes": ledger_bytes.len() as i64, "sha256": sha(&ledger_bytes),
                "sequence": ledger["sequence"], "tailHash": ledger["lastHash"]},
            "officialDecisions": decisions.proof,
            "mechanismOutputHash": mechanism["fullOutputHash"]},
        "official": {"intents": intents.len() as i64, "filledOrders": intents.len() as i64,
            "fillRecords": actual_fills, "fillsAtSubmittedLimit": at_limit,
            "priceImprovedFills": price_improved, "nonLimitViolations": 0,
            "matchedEntryBookObservations": matched, "openAttributedShares": 0,
            "recordedPriceGrossRealized": money(&total_realized), "markets": markets_report, "addedFees": 0,
            "otherAccountHoldingsAdopted": false},
        "passive": {"cohort": "baal-stable-spread-20261005-v2-live", "bankHash": state["paper"]["bankHash"],
            "quotes": opportunities.as_array().map(|a| a.len() as i64).unwrap_or(0),
            "quotesWithTouches": opportunities.as_array().cloned().unwrap_or_default().iter()
                .filter(|x| x["touchEvents"].as_i64().unwrap_or(0) > 0).count() as i64,
            "quotesWithStrictThrough": opportunities.as_array().cloned().unwrap_or_default().iter()
                .filter(|x| x["strictThroughEvents"].as_i64().unwrap_or(0) > 0).count() as i64,
            "unusablePrices": opportunities.as_array().cloned().unwrap_or_default().iter()
                .map(|x| x["unusablePrices"].as_i64().unwrap_or(0)).sum::<i64>(),
            "opportunities": opportunities, "queuePositionProven": false, "executionProven": false},
        "quoteCostSensitivity": {"cases": sensitivity.len() as i64,
            "positiveMeanWithoutExtraTicks": sensitivity.iter()
                .filter(|x| x["meanWithoutAdditionalAdverseTicks"].as_f64().unwrap_or(0.0) > 0.0).count() as i64,
            "sensitivity": sensitivity},
        "limitations": ["Actual fills at the submitted limit do not prove zero slippage relative to an earlier book. Only source-linked entry observations can support that comparison.",
            "Recorded-price gross realization excludes unverified non-price deductions; zero added fees is the bot policy.",
            "Removing the declared adverse-tick stress is an optimistic quote sensitivity, not an execution model or demonstrated profit.",
            "Touches and anonymous trade sizes do not establish aggressor, priority or a hypothetical fill.",
            "Cases, scenarios and YES/NO opportunities are correlated; reused data is not new out-of-sample evidence."],
        "networkRequests": 0, "competitionOrders": 0, "persistentServicesAdded": 0,
        "profitabilityEstablished": false});
    if verify_existing {
        let existing = existing.unwrap();
        for key in ["passive", "tape", "officialCore", "officialDecisions"] {
            a_true(report["sourceProof"][key] == existing["sourceProof"][key])?;
        }
        for key in ["official", "passive", "quoteCostSensitivity"] {
            a_true(report[key] == existing[key])?;
        }
        let rep = json!({"schemaVersion": 1, "at": now_iso(),
            "reportHash": sha(&std::fs::read(root.join("data/fill-economics-audit-20261005.json")).map_err(|e| e.to_string())?),
            "sourcePrefixProofsExact": true, "economicResultsExact": true,
            "officialIntentLedger": report["sourceProof"]["officialIntentLedger"],
            "networkRequests": 0, "competitionOrders": 0, "profitabilityEstablished": false});
        write_pretty_excl(&root.join("data/fill-economics-reproduction-20261005.json"), &rep)?;
        println!("{}", js_stringify(&json!({"sourcePrefixProofsExact": true, "economicResultsExact": true,
            "officialGrossRealized": report["official"]["recordedPriceGrossRealized"],
            "networkRequests": 0})));
    } else {
        write_pretty_excl(&root.join("data/fill-economics-audit-20261005.json"), &report)?;
        println!("{}", js_stringify(&json!({"official": report["official"],
            "passive": {"quotes": report["passive"]["quotes"],
                "touchQuotes": report["passive"]["quotesWithTouches"],
                "throughQuotes": report["passive"]["quotesWithStrictThrough"],
                "unusable": report["passive"]["unusablePrices"]},
            "sensitivity": {"cases": sensitivity.len() as i64,
                "positiveIdealCases": report["quoteCostSensitivity"]["positiveMeanWithoutExtraTicks"]}})));
    }
    Ok(report)
}
