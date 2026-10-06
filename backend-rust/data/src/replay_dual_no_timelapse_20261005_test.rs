// data/replay-dual-no-timelapse-20261005.test.mjs — node:test suite ported as
// plain functions returning Result<(), String> (assert.equal -> equality checks).
use crate::replay_dual_no_timelapse_20261005::{
    adapt_paper_module, causal_account, read_eligibility, replay_publication_lines,
    sealed_end, verify_live_journal, PaperNamespace};
use baal_collector::book_publication_v2::BookPublicationWriter;
use baal_collector::canonical_no_capacity::canonical_no_policy;
use baal_collector::dual_no_pair_paper_v1::{create_dual_no_pair_state, dual_no_pair_policy};
use baal_service_runtime::contract::ms_to_iso;
use baal_service_runtime::json_text::js_stringify;
use baal_service_runtime::ledger::payload_hash as ledger_payload_hash;
fn payload_hash(v: &V) -> Result<String, String> {
    ledger_payload_hash(v).map_err(|e| e.to_string())
}
use serde_json::{json, Value as V};
use sha2::{Digest, Sha256};
use std::path::PathBuf;

const BASE: f64 = 1759698000000.0; // Date.parse('2026-10-05T21:00:00Z')
fn at(n: f64) -> String { ms_to_iso(BASE + n) }
fn sha(b: &[u8]) -> String { Sha256::digest(b).iter().map(|x| format!("{:02x}", x)).collect() }
fn assert_eq(a: &V, b: &V) -> Result<(), String> {
    if a == b { Ok(()) } else { Err(format!("assert.equal {} vs {}", js_stringify(a), js_stringify(b))) }
}
fn scope() -> V {
    json!({"tournamentId": "bda92870-621e-47b0-bc3c-3602c5c26f55",
        "exchanges": [{"marketId": "305", "exchangeId": "994", "option": "YES"},
            {"marketId": "306", "exchangeId": "995", "option": "YES"}]})
}
fn review() -> Result<V, String> {
    let scope = scope();
    let mut r = json!({"schemaVersion": 1, "kind": "conditional_no_pair_review",
        "id": "correctness-only", "tournamentId": scope["tournamentId"],
        "evidenceHash": "a".repeat(64), "recordedAt": at(-1000.0),
        "expiresAt": at(3600000.0), "normalResolutionOnly": true,
        "administrativeRefundGapAcknowledged": true,
        "assumptions": ["Synthetic correctness fixture, never empirical profit"],
        "scenarios": [{"id": "d", "category": "ordinary", "payouts": [0, 1]},
            {"id": "r", "category": "ordinary", "payouts": [1, 0]},
            {"id": "o", "category": "ordinary", "payouts": [1, 1]},
            {"id": "x", "category": "exception", "payouts": [0, 0]}]});
    let legs: Vec<V> = scope["exchanges"].as_array().cloned().unwrap_or_default().iter()
        .enumerate().map(|(i, l)| {
            let node = format!("{}", 306 + i);
            let root = json!({"node_id": node, "contract_id": node,
                "node_type": "contract", "contract_type": "Election Outcome",
                "settled_with": null, "settlement_date": "2026-11-04T17:00:00.000Z",
                "contract_details": {"raceId": "76735", "stageId": "118938",
                    "usState": "MI", "raceName": "Governor", "raceStage": "General",
                    "winnerName": if i == 1 { "Republican Party" } else { "Democratic Party" },
                    "officeLevel": "State", "contractType": "Election Outcome",
                    "electionDate": "2026-11-03", "resolutionType": "Party Winner"}});
            let mut leg = l.clone();
            leg["captureMarketId"] = l["marketId"].clone();
            leg["outcome"] = json!("NO");
            leg["root"] = root.clone();
            leg["rulesHash"] = json!(payload_hash(&root).unwrap_or_default());
            leg
        }).collect();
    r["legs"] = V::Array(legs);
    Ok(r)
}

struct Fixture { lines: Vec<String>, bank: V, eligibility: V, dir: PathBuf }
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = self.dir.parent().map(|p| p.to_string_lossy().to_string())
            == Some(std::env::temp_dir().to_string_lossy().to_string());
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn fixture() -> Result<Fixture, String> {
    let dir = std::env::temp_dir().join(format!("baal-timelapse-{}", std::process::id())
        + &format!("-{}", uuid::Uuid::new_v4().simple()));
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let file = dir.join("books.jsonl");
    let mut n = 0f64;
    let mut version = 0i64;
    let mut writer = {
        let now = move || (BASE + n) as i64;
        // JS now() reads the shared n at call time — capture by thread-local copy is
        // fine here because the writer's internal clock reads n at publish time;
        // we rebuild the closure each publish below instead.
        let _ = now;
        BookPublicationWriter::new(&file, &scope(), "timelapse-correctness-only",
            move || {
                // closure captures `n` at creation; JS shares the variable. We emulate
                // by writing through a mutex cell.
                N_CELL.with(|c| *c.borrow()) as i64
            }, || true).map_err(|e| e.to_string())?
    };
    let prefix = std::fs::read(&file).map_err(|e| e.to_string())?;
    let first: V = serde_json::from_slice(&prefix).map_err(|e| e.to_string())?;
    let mut capacity = canonical_no_policy();
    capacity["maxQuantity"] = json!(1);
    capacity["maxCost"] = json!("1.000000");
    capacity["maxUnmatchedCost"] = json!("1.000000");
    let bank = json!({"mode": "dual_no_sequential_paper_v1", "name": "correctness-only",
        "cutoff": at(500.0), "review": review()?, "policy": dual_no_pair_policy(),
        "capacityPolicy": capacity,
        "sourcePrefixBytes": prefix.len() as i64, "sourcePrefixHash": sha(&prefix),
        "sourcePrefixSequence": 1, "sourcePrefixTailHash": first["hash"]});
    let mut snapshots: Vec<V> = Vec::new();
    for (id, start, favorable) in [
        ("994", 1000f64, false), ("995", 2100.0, false), ("994", 5000.0, false),
        ("995", 6100.0, false), ("994", 9000.0, false), ("995", 12000.0, false),
        ("994", 15000.0, true), ("995", 16100.0, true), ("995", 20000.0, true),
        ("994", 23000.0, true)] {
        n = start + 100.0;
        N_CELL.with(|c| *c.borrow_mut() = n);
        let mid = if id == "994" { "305" } else { "306" };
        let bid = if favorable { if id == "994" { 0.85 } else { 0.12 } }
            else { if id == "994" { 0.88 } else { 0.145 } };
        let ask = bid + if favorable { 0.005 } else { 0.010 };
        version += 1;
        let response = json!({"endpoint": format!("/markets/{}/orderbook?tournamentId={}&depth=20",
                mid, scope()["tournamentId"].as_str().unwrap_or("")),
            "requestStartedAt": at(start), "receivedAt": at(start + 50.0),
            "data": {"marketId": mid, "contexts": [{"type": "tournament",
                "tournament": {"id": scope()["tournamentId"]},
                "orderbook": {"exchanges": [{"exchangeId": id, "option": "YES",
                    "asOf": {"sequence": version, "at": at(start + 50.0)},
                    "bids": [{"price": bid, "quantity": 1000}],
                    "asks": [{"price": ask, "quantity": 1000}]}]}}]}});
        let ok = writer.publish_book(mid, &response)?;
        assert_eq(&json!(ok), &json!(true))?;
    }
    for known in [800f64, 4800.0, 8800.0, 14800.0, 19800.0] {
        snapshots.push(json!({"knownAt": BASE + known,
            "snapshot": {"tournamentId": scope()["tournamentId"],
                "positionsAt": at(known - 100.0), "ordersAt": at(known),
                "excludedMarketIds": []}}));
    }
    writer.close("STOP")?;
    let text = std::fs::read_to_string(&file).map_err(|e| e.to_string())?;
    let lines: Vec<String> = text.trim_end_matches('\n').split('\n').map(String::from).collect();
    Ok(Fixture { lines, bank, eligibility: json!({"snapshots": snapshots}), dir })
}

thread_local! { static N_CELL: std::cell::RefCell<f64> = std::cell::RefCell::new(0.0); }

fn default_capacity(review: &V, books: &[V], account: Option<&V>, now: &str, policy: &V)
    -> Result<V, String> {
    baal_collector::canonical_no_capacity::evaluate_canonical_no_capacity(
        review, books, account, now, policy)
}
fn v1_module() -> crate::replay_dual_no_timelapse_20261005::AdaptedPaper {
    crate::replay_dual_no_timelapse_20261005::AdaptedPaper { v2: false }
}

/// test 1: virtual chronology preserves exact sequential latency, cash reservations
/// and later executable exit accounting.
pub fn test_virtual_chronology() -> Result<(), String> {
    let f = fixture()?;
    let before = payload_hash(&f.bank)?;
    let result = replay_publication_lines(&f.lines, &f.eligibility, &f.bank,
        &v1_module(), false, default_capacity)?;
    assert_eq(&json!(payload_hash(&f.bank)?), &json!(before))?;
    assert_eq(&result["strict"]["report"]["fills"], &json!(4))?;
    assert_eq(&result["strict"]["report"]["realizedPnl"], &json!("0.025000"))?;
    assert_eq(&json!(result["strict"]["report"]["positions"].as_array()
            .map(|a| a.len()).unwrap_or(0)), &json!(0))?;
    assert_eq(&result["strict"]["report"]["closedPackages"], &json!(1))?;
    assert_eq(&result["strict"]["nearTermProfitabilityEstablished"], &json!(false))?;
    assert_eq(&result["development"], &V::Null)?;
    assert_eq(&result["coverage"]["missingDataInterpolated"], &json!(false))?;
    assert_eq(&result["source"]["frozenPrefixVerified"], &json!(true))?;
    Ok(())
}

/// test 2: future/current account data is never backfilled into past books and
/// missing-account coverage produces no fills.
pub fn test_no_account_backfill() -> Result<(), String> {
    let mut f = fixture()?;
    let shifted: Vec<V> = f.eligibility["snapshots"].as_array().cloned().unwrap_or_default()
        .iter().map(|s| {
            let mut x = s.clone();
            x["knownAt"] = json!(s["knownAt"].as_f64().unwrap_or(0.0) + 86400000.0);
            x
        }).collect();
    f.eligibility["snapshots"] = V::Array(shifted);
    let r = replay_publication_lines(&f.lines, &f.eligibility, &f.bank,
        &v1_module(), false, default_capacity)?;
    assert_eq(&r["strict"]["report"]["fills"], &json!(0))?;
    assert_eq(&r["strict"]["report"]["realizedPnl"], &json!("0.000000"))?;
    assert_eq(&r["strict"]["noEligibleAccountBooks"], &r["strict"]["scopeBooks"])?;
    assert_eq(&causal_account(f.eligibility["snapshots"].as_array().cloned()
            .unwrap_or_default().as_slice(), BASE, &scope())?, &V::Null)?;
    Ok(())
}

/// test 3: live-journal identity parity rejects rehashed economic tampering and
/// historical transport remains explicitly separate.
pub fn test_live_journal_parity() -> Result<(), String> {
    let f = fixture()?;
    let initial = json!({"paper": create_dual_no_pair_state(&f.bank)?,
        "sourceSequence": f.bank["sourcePrefixSequence"],
        "sourceHash": f.bank["sourcePrefixTailHash"]});
    let body = json!({"sequence": 1, "previous": "", "at": at(500.0),
        "event": {"kind": "genesis", "cutoff": f.bank["cutoff"],
            "bankHash": payload_hash(&f.bank)?}, "state": initial});
    let mut row = body.clone();
    row["hash"] = json!(sha(js_stringify(&body).as_bytes()));
    let line = js_stringify(&row);
    assert_eq(&verify_live_journal(&[line], &f.bank, &v1_module())?
            ["exactAsRecordedIdentityParity"], &json!(true))?;
    let mut tampered = body.clone();
    tampered["state"]["paper"]["cash"] = json!("999.000000");
    let mut trow = tampered.clone();
    trow["hash"] = json!(sha(js_stringify(&tampered).as_bytes()));
    let err = verify_live_journal(&[js_stringify(&trow)], &f.bank, &v1_module());
    match err {
        Err(e) if e.contains("LIVE_STATE_PARITY") => {}
        _ => return Err("expected LIVE_STATE_PARITY".into()),
    }
    let result = replay_publication_lines(&f.lines, &f.eligibility, &f.bank,
        &v1_module(), true, default_capacity)?;
    assert_eq(&result["development"]["ruleTransport"]["historicalAsOfRulesKnown"],
        &json!(false))?;
    assert_eq(&result["development"]["nearTermProfitabilityEstablished"], &json!(false))?;
    Ok(())
}

/// test 4: eligibility hash-chain and actual publication availability govern
/// exclusions.
pub fn test_eligibility_chain() -> Result<(), String> {
    let snapshot = json!({"tournamentId": scope()["tournamentId"],
        "universeHash": "a".repeat(64), "positionsAt": at(0.0), "ordersAt": at(100.0),
        "expiresAt": at(20000.0), "excludedMarketIds": ["305"], "complete": true});
    let body = json!({"sequence": 1, "previous": "", "at": at(200.0),
        "event": {"type": "market_account_eligibility", "snapshot": snapshot},
        "state": V::Null});
    let mut row = body.clone();
    row["hash"] = json!(sha(js_stringify(&body).as_bytes()));
    let line = js_stringify(&row);
    let e = read_eligibility(&[line.clone()])?;
    assert_eq(&causal_account(e["snapshots"].as_array().cloned().unwrap_or_default()
            .as_slice(), BASE + 150.0, &scope())?, &V::Null)?;
    assert_eq(&causal_account(e["snapshots"].as_array().cloned().unwrap_or_default()
            .as_slice(), BASE + 300.0, &scope())?["excludedExchangeIds"],
        &json!(["994"]))?;
    let bad = line.replace("\"305\"", "\"306\"");
    match read_eligibility(&[bad]) {
        Err(e) if e.contains("ACCOUNT_CHAIN") => {}
        _ => return Err("expected ACCOUNT_CHAIN".into()),
    }
    Ok(())
}

/// test 5: v2 adapter preserves its actual bank mode and future cutoff.
pub fn test_v2_adapter() -> Result<(), String> {
    let f = fixture()?;
    let mut bank = f.bank.clone();
    bank["mode"] = json!("dual_no_sequential_paper_v2_exact_receipt");
    bank["policy"] = baal_collector::dual_no_pair_paper_v2::dual_no_pair_policy_v2();
    let module = adapt_paper_module(PaperNamespace::V2,
        bank["mode"].as_str().unwrap_or(""))?;
    let before = payload_hash(&bank)?;
    let r = replay_publication_lines(&f.lines, &f.eligibility, &bank,
        &module, false, default_capacity)?;
    assert_eq(&json!(payload_hash(&bank)?), &json!(before))?;
    assert_eq(&r["strict"]["report"]["fills"], &json!(4))?;
    assert_eq(&r["strict"]["report"]["realizedPnl"], &json!("0.045000"))?;
    assert_eq(&r["strict"]["report"]["mode"], &bank["mode"])?;
    assert_eq(&r["strict"]["nearTermProfitabilityEstablished"], &json!(false))?;
    match adapt_paper_module(PaperNamespace::Empty, bank["mode"].as_str().unwrap_or("")) {
        Err(e) if e.contains("MODE_ADAPTER") => {}
        _ => return Err("expected MODE_ADAPTER".into()),
    }
    Ok(())
}

/// test 6: injected frozen capacity evaluator is actually used.
pub fn test_injected_capacity() -> Result<(), String> {
    let f = fixture()?;
    let called = std::cell::Cell::new(0i64);
    let r = replay_publication_lines(&f.lines, &f.eligibility, &f.bank, &v1_module(),
        false, |_review, _books, _account, _now, _policy| {
            called.set(called.get() + 1);
            Ok(json!({"positiveConditionalCapacity": false}))
        })?;
    if called.get() <= 0 { return Err("called > 0".into()); }
    assert_eq(&r["strict"]["report"]["fills"], &json!(0))?;
    assert_eq(&r["strict"]["report"]["realizedPnl"], &json!("0.000000"))?;
    Ok(())
}
