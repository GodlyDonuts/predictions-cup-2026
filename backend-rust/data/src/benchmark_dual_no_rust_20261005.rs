// data/benchmark-dual-no-rust-20261005.mjs — isolated generic Rust dual-NO
// engine: state/report parity against the recorded journal, failure-policy
// continuations, synthetic correctness scenarios and a throughput benchmark.
use baal_collector::dual_no_pair_paper_v2::{
    advance_dual_no_pair_v2, create_dual_no_pair_state_v2, dual_no_pair_report_v2};
use baal_service_runtime::journal::{journal_lines, JournalReadOptions};
use baal_service_runtime::json_text::{js_stringify, sha256};
use serde_json::{json, Value as V};
use sha2::{Digest, Sha256};
use std::io::Write;
use std::path::{Path, PathBuf};

fn need(ok: bool, code: &str) -> Result<(), String> {
    if ok { Ok(()) } else { Err(format!("DUAL_NO_RUST_{}", code)) }
}
fn sha(b: &[u8]) -> String { sha256(b) }
fn ph(v: &V) -> Result<String, String> { baal_execution::core::payload_hash(v) }
fn read_json(p: &Path) -> Result<V, String> {
    serde_json::from_slice(&std::fs::read(p).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}
fn localappdata() -> Result<PathBuf, String> {
    std::env::var_os("LOCALAPPDATA").map(PathBuf::from).ok_or_else(|| "LOCALAPPDATA".to_string())
}
fn now_iso() -> String {
    baal_service_runtime::contract::ms_to_iso(
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as f64)
}
fn perf_ms() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs_f64() * 1000.0
}
fn spawn_lines(binary: &Path, tool: &Path, input: &str) -> Result<String, String> {
    let mut env_path = format!("{};{}", tool.display(), std::env::var("PATH").unwrap_or_default());
    if cfg!(unix) { env_path = format!("{}:{}", tool.display(), std::env::var("PATH").unwrap_or_default()); }
    let mut child = std::process::Command::new(binary)
        .env("PATH", env_path)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn().map_err(|e| e.to_string())?;
    if let Some(mut stdin) = child.stdin.take() {
        let input = input.to_string();
        std::thread::spawn(move || { let _ = stdin.write_all(input.as_bytes()); });
    }
    let out = child.wait_with_output().map_err(|e| e.to_string())?;
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    need(out.status.success(), &format!("PROCESS_{}", stderr))?;
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}
fn run_requests(binary: &Path, tool: &Path, requests: &[V]) -> Result<(Vec<V>, f64), String> {
    let input: String = requests.iter()
        .map(|x| format!("{}\n", js_stringify(x))).collect();
    let t = perf_ms();
    let stdout = spawn_lines(binary, tool, &input)?;
    let wall_ms = perf_ms() - t;
    let results: Vec<V> = stdout.trim().split('\n')
        .filter(|l| !l.is_empty())
        .map(|x| serde_json::from_str(x).map_err(|e| e.to_string()))
        .collect::<Result<_, _>>()?;
    need(results.len() == requests.len(), "RESPONSES")?;
    for r in &results {
        need(r["error"].is_null() || !baal_service_runtime::json_text::js_truthy(Some(&r["error"])),
            &format!("RESPONSE_{}", r["error"].as_str().unwrap_or("")))?;
    }
    Ok((results, wall_ms))
}

pub fn run() -> Result<V, String> {
    let rt = localappdata()?.join("SigTradingCup").join("paper-runtime");
    let pointer = read_json(&rt.join("dual-no-pair-v2-active.json"))?;
    let dir = rt.join("dual-no-pair-v2-cohorts").join(pointer["cohort"].as_str().unwrap_or(""));
    let bank = read_json(&dir.join("bank.json"))?;
    let journal = dir.join("journal.jsonl");
    let binary = std::env::current_dir().map_err(|e| e.to_string())?
        .join("backend-rust/dual-no-engine/target/release/baal-dual-no-engine.exe");
    let tool = localappdata()?
        .join("SigTradingCup/rust-toolchain/rustup/toolchains/stable-x86_64-pc-windows-gnullvm/bin");
    let mut records: Vec<V> = Vec::new();
    let mut prev = String::new();
    let mut seq = 0i64;
    let end = crate::replay_dual_no_timelapse_20261005::sealed_end(&journal)?;
    let mut digest = Sha256::new();
    for line in journal_lines(&journal, &JournalReadOptions { end_byte: Some(end), ..Default::default() })? {
        digest.update(line.as_bytes());
        digest.update(b"\n");
        let row: V = serde_json::from_str(&line).map_err(|e| e.to_string())?;
        let mut body = row.clone();
        let hash = row["hash"].as_str().unwrap_or("").to_string();
        if let V::Object(m) = &mut body { m.shift_remove("hash"); }
        seq += 1;
        need(baal_service_runtime::json_text::js_int(&row["sequence"]) == Some(seq)
            && row["previous"].as_str().unwrap_or("") == prev
            && sha(js_stringify(&body).as_bytes()) == hash, "JOURNAL_CHAIN")?;
        prev = hash;
        records.push(row);
    }
    let mut s = create_dual_no_pair_state_v2(&bank)?;
    need(ph(&s).ok() == ph(&records[0]["state"]["paper"]).ok(), "GENESIS")?;
    let mut req = vec![json!({"op": "init", "bank": bank})];
    let mut expected = vec![json!({"state": s.clone()})];
    for row in records.iter().skip(1) {
        s = advance_dual_no_pair_v2(&s, &row["event"], &bank)?;
        need(ph(&s).ok() == ph(&row["state"]["paper"]).ok(), "RECORDED_STATE")?;
        req.push(json!({"op": "advance", "event": row["event"]}));
        req.push(json!({"op": "report", "at": row["event"]["availableAt"]}));
        expected.push(json!({"state": s.clone()}));
        expected.push(json!({"report": dual_no_pair_report_v2(&s, &bank,
            row["event"]["availableAt"].as_str().unwrap_or(""))?}));
    }
    let (actual_results, _) = run_requests(&binary, &tool, &req)?;
    for (i, x) in actual_results.iter().enumerate() {
        need(ph(x).ok() == ph(&expected[i]).ok(), &format!("PARITY_RECORD_{}", i))?;
    }
    // Failure-policy continuations start from original attributed holdings; no
    // synthetic favorable market is used as profitability evidence.
    let mut failures: Vec<V> = Vec::new();
    for (name, event) in [
        ("epoch_boundary", json!({"kind": "boundary", "availableAt": s["lastAt"]})),
        ("future_clock", json!({"kind": "boundary", "availableAt": bank["cutoff"]})),
    ] {
        let input = format!("{}\n{}\n", js_stringify(&json!({"op": "restore", "bank": bank, "state": s})),
            js_stringify(&json!({"op": "advance", "event": event})));
        let stdout = spawn_lines(&binary, &tool, &input).unwrap_or_default();
        let responses: Vec<V> = stdout.trim().split('\n').filter(|l| !l.is_empty())
            .map(|x| serde_json::from_str(x).unwrap_or(V::Null)).collect();
        let js: Result<V, ()> = advance_dual_no_pair_v2(&s, &event, &bank)
            .map(|state| json!({"state": state})).map_err(|_| ());
        match js {
            Err(()) => need(responses.get(1).map(|r| !r["error"].is_null()).unwrap_or(false),
                &format!("FAILURE_{}", name))?,
            Ok(ref v) => need(ph(&v).ok() == responses.get(1).map(|r| ph(r).ok()).flatten(),
                &format!("FAILURE_{}", name))?,
        }
        failures.push(json!({"name": name, "exactParity": true, "rejected": js.is_err()}));
    }
    let base = baal_service_runtime::contract::parse_iso_ms(bank["cutoff"].as_str().unwrap_or(""))
        .unwrap_or(f64::NAN);
    let at = |n: f64| baal_service_runtime::contract::ms_to_iso(base + n);
    let leg_ids: Vec<String> = bank["review"]["legs"].as_array().cloned().unwrap_or_default()
        .iter().map(|x| x["exchangeId"].as_str().unwrap_or("").to_string()).collect();
    let fake_book = |id: &str, clock: f64, patch: Option<V>| -> V {
        let leg = bank["review"]["legs"].as_array().cloned().unwrap_or_default().into_iter()
            .find(|x| x["exchangeId"].as_str() == Some(id)).unwrap_or(V::Null);
        let mut book = json!({"marketId": leg["marketId"], "exchangeId": id,
            "requestStartedAt": at(clock), "receivedAt": at(clock + 100.0),
            "versionKey": format!("{}:{}", js_stringify(&json!(clock)), at(clock + 100.0)),
            "publicationSequence": clock, "publicationHash": "a".repeat(64),
            "bids": [{"price": if id == leg_ids[0] { "0.880000" } else { "0.145000" }, "quantity": 1000}],
            "asks": [{"price": if id == leg_ids[0] { "0.890000" } else { "0.155000" }, "quantity": 1000}]});
        if let Some(V::Object(p)) = patch {
            if let V::Object(m) = &mut book { for (k, v) in p { m.insert(k, v); } }
        }
        book
    };
    let fake_event = |id: &str, clock: f64, other_clock: Option<f64>, patch: Option<V>,
        account_patch: Option<Option<V>>| -> V {
        let book = fake_book(id, clock, patch);
        let other = other_clock.map(|c| {
            let other_id = leg_ids.iter().find(|x| x != &id).cloned().unwrap_or_default();
            fake_book(&other_id, c, None)
        });
        let pair: Vec<V> = [Some(book.clone()), other].into_iter().flatten().collect();
        let min_clock = clock.min(other_clock.unwrap_or(clock));
        json!({"kind": "book", "availableAt": book["receivedAt"], "book": book,
            "pairBooks": pair,
            "accountEligibility": match account_patch {
                Some(Some(p)) => p,
                Some(None) => V::Null,
                None => json!({"schemaVersion": 1, "positionsAt": at(min_clock - 100.0),
                    "ordersAt": at(min_clock - 100.0), "excludedExchangeIds": []})},
            "admission": {"positiveConditionalCapacity": true}})
    };
    let e = |id: usize, clock: f64, other: Option<f64>| fake_event(&leg_ids[id], clock, other, None, None);
    // JS `fakeEvent(id, clock, other, patch)` leaves accountPatch undefined ->
    // the default eligibility object is used (distinct from an explicit null).
    let ep = |id: usize, clock: f64, other: Option<f64>, patch: V|
        fake_event(&leg_ids[id], clock, other, Some(patch), None);
    let boundary = |clock: f64| json!({"kind": "boundary", "availableAt": at(clock)});
    let prefix = vec![e(1, 1100.0, Some(1000.0)), e(1, 11000.0, Some(10000.0))];
    let mut first = prefix.clone();
    first.push(e(0, 14000.0, Some(11000.0)));
    let mut held = first.clone();
    held.push(e(1, 18000.0, Some(14000.0)));
    let scenarios: Vec<(String, Vec<V>)> = vec![
        ("funded_later_requests".into(), held.clone()),
        ("insufficient_fractional_depth".into(), {
            let mut v = prefix.clone();
            v.push(ep(0, 14000.0, Some(11000.0),
                json!({"bids": [{"price": "0.880000", "quantity": 99}]})));
            v }),
        ("fixed_limit_failed_hedge".into(), {
            let mut v = first.clone();
            v.push(ep(1, 18000.0, Some(14000.0),
                json!({"bids": [{"price": "0.130000", "quantity": 1000}]})));
            v.push(boundary(19000.0));
            v }),
        ("epoch_or_stop_preserves_orphan".into(), {
            let mut v = first.clone();
            v.push(boundary(19000.0));
            v.push(fake_event(&leg_ids[0], 22000.0, None, None, None));
            v.push(fake_event(&leg_ids[0], 25000.0, None, None, None));
            v }),
        ("expired_review_preserves_orphan".into(), {
            let mut v = first.clone();
            v.push(e(1, 4000000.0, Some(3995000.0)));
            v }),
        ("held_inventory_not_calendar_payout".into(), {
            let mut v = held.clone();
            v.push(boundary(4000000.0));
            v }),
        ("missing_account_blocks_hedge".into(), {
            let mut v = first.clone();
            v.push(fake_event(&leg_ids[1], 18000.0, Some(14000.0), None, Some(None)));
            v }),
        ("excluded_leg_blocks_hedge".into(), {
            let mut v = first.clone();
            v.push(fake_event(&leg_ids[1], 18000.0, Some(14000.0), None, Some(Some(
                json!({"schemaVersion": 1, "positionsAt": at(13900.0), "ordersAt": at(13900.0),
                    "excludedExchangeIds": [leg_ids[0]]})))));
            v }),
        ("future_account_blocks_hedge".into(), {
            let mut v = first.clone();
            v.push(fake_event(&leg_ids[1], 18000.0, Some(14000.0), None, Some(Some(
                json!({"schemaVersion": 1, "positionsAt": at(19000.0), "ordersAt": at(19000.0),
                    "excludedExchangeIds": []})))));
            v }),
        ("holding_limit_requires_actual_exit".into(), {
            let mut v = held.clone();
            v.push(e(1, 3024020000.0, Some(3024019000.0)));
            v.push(e(1, 3024024000.0, Some(3024019000.0)));
            v.push(e(0, 3024028000.0, Some(3024024000.0)));
            v }),
    ];
    let mut correct_requests: Vec<V> = Vec::new();
    let mut correct_expected: Vec<V> = Vec::new();
    for (name, events) in &scenarios {
        let mut q = create_dual_no_pair_state_v2(&bank)?;
        correct_requests.push(json!({"op": "init", "bank": bank}));
        correct_expected.push(json!({"state": q.clone()}));
        for ev in events {
            q = advance_dual_no_pair_v2(&q, ev, &bank)?;
            correct_requests.push(json!({"op": "advance", "event": ev}));
            correct_requests.push(json!({"op": "report", "at": ev["availableAt"]}));
            correct_expected.push(json!({"state": q.clone()}));
            correct_expected.push(json!({"report": dual_no_pair_report_v2(&q, &bank,
                ev["availableAt"].as_str().unwrap_or(""))?}));
        }
        failures.push(json!({"name": name, "events": events.len(),
            "finalStateHash": ph(&q)?, "realizedPnl": q["realizedPnl"],
            "exactParity": true, "syntheticCorrectnessOnly": true}));
    }
    let (correctness_results, _) = run_requests(&binary, &tool, &correct_requests)?;
    for (i, v) in correctness_results.iter().enumerate() {
        need(ph(v).ok() == ph(&correct_expected[i]).ok(), &format!("CORRECTNESS_PARITY_{}", i))?;
    }
    let events: Vec<V> = records.iter().skip(1).map(|x| x["event"].clone()).collect();
    let repeat = 20;
    let mut bench_requests: Vec<V> = Vec::new();
    for _ in 0..repeat {
        bench_requests.push(json!({"op": "init", "bank": bank}));
        bench_requests.push(json!({"op": "batch", "events": events}));
    }
    let t = perf_ms();
    for _ in 0..repeat {
        let mut q = create_dual_no_pair_state_v2(&bank)?;
        for ev in &events { q = advance_dual_no_pair_v2(&q, ev, &bank)?; }
        need(ph(&q).ok() == ph(&s).ok(), "JS_BENCH")?;
    }
    let node_ms = perf_ms() - t;
    let (rust_results, rust_wall_ms) = run_requests(&binary, &tool, &bench_requests)?;
    for i in (1..rust_results.len()).step_by(2) {
        need(ph(&rust_results[i]["state"]).ok() == ph(&s).ok(), "BATCH_PARITY")?;
    }
    let out = json!({"schemaVersion": 1, "at": now_iso(), "mode": "isolated_generic_rust_dual_no_engine",
        "cohort": bank["name"], "bankHash": ph(&bank)?,
        "binaryHash": sha(&std::fs::read(&binary).map_err(|e| e.to_string())?),
        "originalSealedJournal": {"bytes": end, "records": records.len(),
            "sha256": format!("{:x}", digest.finalize()), "lastSequence": seq, "lastHash": prev},
        "fullStateAndReportParity": {"responses": expected.len(), "exact": true,
            "finalStateHash": ph(&s)?},
        "failureContinuations": failures,
        "benchmark": {"repeats": repeat, "events": events.len() * repeat as usize,
            "nodeMs": node_ms, "rustIncludingOneSpawnSerializationMs": rust_wall_ms,
            "speedup": node_ms / rust_wall_ms, "singleLongLivedJsonlWorker": true,
            "noPerBookProcessSpawn": true},
        "report": dual_no_pair_report_v2(&s, &bank, s["lastAt"].as_str().unwrap_or(""))?,
        "runtimeChanged": false, "networkRequests": 0, "contestOrders": 0,
        "profitabilityEstablished": false,
        "limitations": ["Parity and throughput are correctness/performance evidence, not proof of a profitable strategy.",
            "Original sealed journal events preserve later receipts, causal eligibility and attributed liabilities.",
            "Rust bank validation is cached per batch; integer funded-state checks run after every event.",
            "Existing frozen cohorts and collectors remain on their original engine. Production adoption needs a separately bound continuation."]});
    std::fs::write("data/dual-no-rust-benchmark-20261005.json",
        format!("{}\n", serde_json::to_string_pretty(&out).map_err(|e| e.to_string())?))
        .map_err(|e| e.to_string())?;
    Ok(json!({"records": records.len(), "responses": expected.len(), "nodeMs": node_ms,
        "rustMs": rust_wall_ms, "speedup": node_ms / rust_wall_ms, "exactParity": true,
        "realized": s["realizedPnl"]}))
}
