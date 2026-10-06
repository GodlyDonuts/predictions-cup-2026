// data/audit-baal-maker-roundtrip-20261004.mjs — chain-verify the no-maker
// journal, bind every recorded source to the sealed publication capture, and
// audit the single buy->sell roundtrip back to the trade tape.
use baal_collector::book_publication_v2::{apply_book_publication, create_book_publication_state};
use baal_service_runtime::json_text::{js_stringify, sha256};
use num_bigint::BigInt;
use serde_json::{json, Value as V};
use std::path::{Path, PathBuf};

fn need(ok: bool, code: &str) -> Result<(), String> {
    if ok { Ok(()) } else { Err(format!("BAAL_ROUNDTRIP_{}", code)) }
}
fn sha(b: &[u8]) -> String { sha256(b) }
fn ph(v: &V) -> Result<String, String> { baal_execution::core::payload_hash(v) }
fn read_json(p: &Path) -> Result<V, String> {
    serde_json::from_slice(&std::fs::read(p).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}
fn localappdata() -> Result<PathBuf, String> {
    std::env::var_os("LOCALAPPDATA").map(PathBuf::from).ok_or_else(|| "LOCALAPPDATA".to_string())
}
// complete(f) — bytes up to and including the last newline (or empty).
fn complete(p: &Path) -> Result<Vec<u8>, String> {
    let b = std::fs::read(p).map_err(|e| e.to_string())?;
    match b.iter().rposition(|c| *c == b'\n') {
        Some(n) => Ok(b[..n + 1].to_vec()),
        None => Ok(Vec::new()),
    }
}
fn rows(b: &[u8]) -> Result<Vec<V>, String> {
    let s = String::from_utf8_lossy(b);
    let t = s.trim_end();
    if t.is_empty() { return Ok(Vec::new()); }
    t.split('\n').map(|l| serde_json::from_str(l).map_err(|e| e.to_string())).collect()
}
fn row_body(row: &V) -> V {
    let mut body = row.clone();
    if let V::Object(m) = &mut body { m.shift_remove("hash"); }
    body
}
fn amount(v: &V) -> Result<BigInt, String> {
    baal_paper::fixed::big_amount(v.as_str().unwrap_or(""))
}
fn now_iso() -> String {
    baal_service_runtime::contract::ms_to_iso(
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as f64)
}
fn parse_ms(v: &V) -> f64 {
    baal_service_runtime::contract::parse_iso_ms(v.as_str().unwrap_or("")).unwrap_or(f64::NAN)
}

pub fn run() -> Result<V, String> {
    let r = localappdata()?.join("SigTradingCup").join("paper-runtime");
    let active = read_json(&r.join("no-maker-active.json"))?;
    need(active["cohort"].as_str() == Some("no-maker-tape-20261004-v3"), "COHORT")?;
    let dir = r.join("no-maker-cohorts").join(active["cohort"].as_str().unwrap());
    let bank = read_json(&dir.join("bank.json"))?;
    need(ph(&bank).ok().as_deref() == active["bankHash"].as_str(), "BANK")?;
    let bytes = complete(&dir.join("journal.jsonl"))?;
    let journal = rows(&bytes)?;
    let mut previous = String::new();
    let mut paper = V::Null;
    for (i, row) in journal.iter().enumerate() {
        let hash = row["hash"].as_str().unwrap_or("").to_string();
        need(row["sequence"].as_i64() == Some(i as i64 + 1)
            && row["previous"].as_str().unwrap_or("") == previous
            && sha(js_stringify(&row_body(row)).as_bytes()) == hash, "JOURNAL_CHAIN")?;
        previous = hash;
        if i == 0 {
            need(row["event"]["kind"].as_str() == Some("genesis"), "GENESIS")?;
            paper = row["state"]["paper"].clone();
            baal_collector::no_maker_paper_v2::assert_no_maker(&paper, &bank)?;
            continue;
        }
        let e = &row["event"];
        if e["kind"].as_str() == Some("trade_page") {
            paper = baal_collector::no_maker_paper_v2::advance_no_maker(&paper,
                &json!({"kind": "trades", "availableAt": e["availableAt"], "page": e["event"]}), &bank)?;
        } else if ["book", "paired_book"].contains(&e["kind"].as_str().unwrap_or("")) {
            for b in e["books"].as_array().cloned().unwrap_or_default() {
                paper = baal_collector::no_maker_paper_v2::advance_no_maker(&paper,
                    &json!({"kind": "book", "availableAt": e["availableAt"], "book": b,
                        "accountEligibility": e["accountEligibility"], "entryAllowed": e["entryAllowed"]}), &bank)?;
            }
        } else if e["kind"].as_str() == Some("boundary") {
            paper = baal_collector::no_maker_paper_v2::advance_no_maker(&paper,
                &json!({"kind": "boundary", "availableAt": e["availableAt"]}), &bank)?;
        } else {
            need(e["kind"].as_str() == Some("other_trade_page"), "EVENT")?;
        }
        need(ph(&paper).ok() == ph(&row["state"]["paper"]).ok(), "EXACT_STATE_REPLAY")?;
    }
    let tape_bytes = complete(&r.join("anonymous-trades.jsonl"))?;
    let pages = rows(&tape_bytes)?;
    let mut trades: indexmap::IndexMap<String, V> = indexmap::IndexMap::new();
    previous = String::new();
    for (i, row) in pages.iter().enumerate() {
        let hash = row["hash"].as_str().unwrap_or("").to_string();
        need(row["sequence"].as_i64() == Some(i as i64 + 1)
            && row["previous"].as_str().unwrap_or("") == previous
            && sha(js_stringify(&row_body(row)).as_bytes()) == hash, "TAPE_CHAIN")?;
        previous = hash.clone();
        for t in row["event"]["trades"].as_array().cloned().unwrap_or_default() {
            trades.insert(t["key"].as_str().unwrap_or("").to_string(),
                json!({"t": t, "page": row["event"], "sequence": row["sequence"], "hash": hash}));
        }
    }
    let book_bytes = complete(&r.join("public-books.jsonl"))?;
    let mut books = create_book_publication_state(&read_json(Path::new("live-paper/price-universe.json"))?)?;
    let wanted: std::collections::HashSet<i64> = journal.iter()
        .filter_map(|x| x["event"]["sourceSequence"].as_i64()).collect();
    let mut bindings: indexmap::IndexMap<i64, V> = indexmap::IndexMap::new();
    for row in rows(&book_bytes)? {
        books = apply_book_publication(&books, &row)?;
        if wanted.contains(&books["sequence"].as_i64().unwrap_or(-1)) {
            bindings.insert(books["sequence"].as_i64().unwrap_or(0), books.clone());
        }
    }
    for row in journal.iter().filter(|x| !x["event"]["sourceSequence"].is_null()) {
        let b = bindings.get(&row["event"]["sourceSequence"].as_i64().unwrap_or(-1));
        need(b.and_then(|b| b["hash"].as_str()) == row["event"]["sourceHash"].as_str(),
            "PUBLICATION_BINDING")?;
        for source in row["event"]["books"].as_array().cloned().unwrap_or_default() {
            let b = b.unwrap();
            need(ph(&b["books"][&source["instrument"].as_str().unwrap_or("")]).ok()
                == ph(&source).ok(), "BOOK_BINDING")?;
        }
    }
    let mut results: Vec<V> = Vec::new();
    if let V::Object(wallets) = &paper["wallets"] {
        for (family, w) in wallets {
            let fills = w["fills"].as_array().cloned().unwrap_or_default();
            for i in 0..fills.len() {
                let buy = &fills[i];
                if buy["action"].as_str() != Some("buy") { continue; }
                let sell = fills[i + 1..].iter().find(|f|
                    f["exchangeId"] == buy["exchangeId"] && f["action"].as_str() == Some("sell"));
                let Some(sell) = sell else { continue };
                let t = trades.get(buy["source"]["key"].as_str().unwrap_or("")).cloned();
                need(t.as_ref().is_some_and(|t| t["t"]["id"] == buy["source"]["id"]
                        && t["t"]["createdAt"] == buy["source"]["createdAt"]
                        && t["page"]["receivedAt"] == buy["source"]["firstAvailableAt"]
                        && baal_collector::canonical_entry_exit_study::tape_tick(&t["t"]).ok().flatten()
                            .map(|v| v["price"] == buy["source"]["normalizedYesPrice"]).unwrap_or(false)),
                    "ENTRY_SOURCE")?;
                let closed = journal.iter().find(|x| {
                    x["state"]["paper"]["wallets"][family]["fills"].as_array()
                        .map(|a| a.iter().any(|f| ph(f).ok() == ph(sell).ok())).unwrap_or(false)
                });
                need(closed.is_some_and(|c| ["paired_book", "book"]
                        .contains(&c["event"]["kind"].as_str().unwrap_or(""))), "EXIT_EVENT")?;
                let closed = closed.unwrap();
                let b = closed["event"]["books"].as_array().cloned().unwrap_or_default()
                    .into_iter().find(|b| b["exchangeId"] == sell["exchangeId"]);
                need(b.as_ref().is_some_and(|b|
                        b["publicationHash"] == sell["source"]["publicationHash"]
                        && b["publicationSequence"] == sell["source"]["publicationSequence"]
                        && b["versionKey"] == sell["source"]["version"]), "EXIT_SOURCE")?;
                let b = b.unwrap();
                let prior_row = journal.get((closed["sequence"].as_i64().unwrap_or(0) - 2) as usize)
                    .cloned().unwrap_or(V::Null);
                let prior = &prior_row["state"]["paper"]["wallets"][family];
                let risk = prior["risk"][sell["exchangeId"].as_str().unwrap_or("")].clone();
                let risk = &risk;
                need(!risk.is_null()
                        && parse_ms(&b["requestStartedAt"]) >= parse_ms(&risk["createdAt"])
                            + bank["policy"]["latencyMs"].as_f64().unwrap_or(0.0),
                    "INDEPENDENT_EXIT_REQUEST")?;
                need(BigInt::from(b["asks"][0]["quantity"].as_i64().unwrap_or(0)
                        * bank["policy"]["depthFractionBps"].as_i64().unwrap_or(0) / 10000)
                        >= BigInt::from(sell["quantity"].as_i64().unwrap_or(0))
                    && amount(&sell["price"])?
                        == BigInt::from(baal_paper::fixed::SCALE) - amount(&b["asks"][0]["price"])?
                            - BigInt::from(baal_paper::fixed::TICK), "EXIT_PRICE_DEPTH")?;
                results.push(json!({"family": family, "marketId": buy["marketId"],
                    "exchangeId": buy["exchangeId"], "quantity": buy["quantity"],
                    "entryPrice": buy["price"], "entrySourceAt": buy["source"]["createdAt"],
                    "entryKnownAt": buy["source"]["firstAvailableAt"], "entryBookedAt": buy["at"],
                    "entryTradeId": buy["source"]["id"], "entryTapeSequence": t.as_ref().unwrap()["sequence"],
                    "entryTapeHash": t.as_ref().unwrap()["hash"],
                    "normalizedTradeYesPrice": buy["source"]["normalizedYesPrice"],
                    "exitPrice": sell["price"], "exitBookedAt": sell["at"], "exitTrigger": risk["reason"],
                    "exitTriggerAt": risk["createdAt"], "exitRequestStartedAt": b["requestStartedAt"],
                    "exitReceiptAt": b["receivedAt"], "exitPublicationSequence": b["publicationSequence"],
                    "exitPublicationHash": b["publicationHash"],
                    "rawNoLiquidationBid": baal_paper::fixed::money(
                        &(BigInt::from(baal_paper::fixed::SCALE)
                            - amount(&b["asks"][0]["price"]).unwrap_or_default())),
                    "adverseExitTick": baal_paper::fixed::money(&BigInt::from(baal_paper::fixed::TICK)),
                    "eligibleExitDepth": b["asks"][0]["quantity"].as_i64().unwrap_or(0) / 100,
                    "realizedPnl": baal_paper::fixed::money(
                        &((amount(&sell["price"]).unwrap_or_default()
                            - amount(&buy["price"]).unwrap_or_default())
                            * BigInt::from(buy["quantity"].as_i64().unwrap_or(0)))),
                    "fees": "0.000000",
                    "heldFromSourceMs": parse_ms(&sell["at"]) - parse_ms(&buy["source"]["createdAt"]),
                    "actualCompetitionFill": false}));
            }
        }
    }
    need(results.len() == 1 && results[0]["realizedPnl"].as_str() == Some("-0.015000"), "FIRST_CLOSE")?;
    let wallets: serde_json::Map<String, V> = paper["wallets"].as_object().cloned().unwrap_or_default()
        .into_iter().map(|(k, w)| (k, json!({"cash": w["cash"], "realizedPnl": w["realized"],
            "fills": w["fills"].as_array().map(|a| a.len()).unwrap_or(0),
            "positions": w["positions"].as_object().map(|o| o.len()).unwrap_or(0)}))).collect();
    let out = json!({"schemaVersion": 1, "at": now_iso(), "cohort": active["cohort"],
        "bankHash": active["bankHash"], "exactStateReplay": true,
        "sources": {"makerJournal": {"bytes": bytes.len(), "sha256": sha(&bytes),
                "sequence": journal.len(), "tailHash": journal.last().map(|j| j["hash"].clone()).unwrap_or(V::Null)},
            "tradeTape": {"bytes": tape_bytes.len(), "sha256": sha(&tape_bytes), "sequence": pages.len()},
            "publicBooks": {"bytes": book_bytes.len(), "sha256": sha(&book_bytes),
                "sequence": books["sequence"]}},
        "roundtrips": results, "wallets": V::Object(wallets),
        "accountingContinuation": bank["accountingContinuation"]["cohort"],
        "independentRoundtrips": 1, "profitabilityEstablished": false, "competitionOrders": 0,
        "finding": "The paired capacity admission produced a standalone maker position. No partner fill completed a package. The holding-limit sale realized spread/adverse-tick cost; zero fees do not remove it.",
        "limitations": ["Strict price-through is a counterfactual fill, not proof of aggressor side or queue priority.",
            "This first close includes the inherited V2 purchase and must not be added to predecessor accounting.",
            "One correlated observation cannot establish profitability or select a strategy."]});
    std::fs::write("data/baal-maker-first-roundtrip-20261004.json",
        format!("{}\n", serde_json::to_string_pretty(&out).map_err(|e| e.to_string())?))
        .map_err(|e| e.to_string())?;
    Ok(json!({"roundtrips": out["roundtrips"].as_array().map(|a| a.len()).unwrap_or(0),
        "realizedPnl": out["roundtrips"][0]["realizedPnl"],
        "exitTrigger": out["roundtrips"][0]["exitTrigger"],
        "exactStateReplay": true, "sourceBindingsVerified": true}))
}
