// live-paper/basket-watch.mjs — durable prospective dual-NO basket observer.
use crate::basket_observer::{basket_report, create_basket_observer, invalidate_basket, observe_basket};
use crate::ml_shadow::{verify_source, VerifiedSource};
use crate::model::{digest, normalize_book};
use baal_service_runtime::atomic::atomic_json;
use baal_service_runtime::contract::{ms_to_iso, parse_iso_ms};
use baal_service_runtime::json_text::{js_int, js_truthy};
use serde_json::{json, Map, Value as V};
use std::collections::BTreeSet;
use std::io::Write;
use std::path::{Path, PathBuf};

fn canonical(x: &V) -> bool {
    x.as_str().and_then(parse_iso_ms).is_some_and(|ms| ms_to_iso(ms) == x.as_str().unwrap())
}
/// JS `===` for JSON values: scalars by value, objects/arrays never equal.
fn strict_eq(a: &V, b: &V) -> bool {
    match (a, b) {
        (V::Null, V::Null) => true,
        (V::Bool(x), V::Bool(y)) => x == y,
        (V::Number(x), V::Number(y)) => x.as_f64() == y.as_f64(),
        (V::String(x), V::String(y)) => x == y,
        _ => false,
    }
}
/// a === b + 1 under JS number semantics.
fn plus1_eq(a: &V, b: &V) -> bool {
    a.is_number() && b.is_number() && a.as_f64() == b.as_f64().map(|x| x + 1.0)
}
fn js_str(v: &V) -> String {
    match v {
        V::String(s) => s.clone(),
        V::Number(n) => {
            let mut b = ryu_js::Buffer::new();
            b.format_finite(n.as_f64().unwrap_or(f64::NAN)).to_string()
        }
        V::Bool(b) => if *b { "true".into() } else { "false".into() },
        V::Null => "null".into(),
        _ => "[object Object]".into(),
    }
}
fn record_clock<'a>(r: &'a V) -> &'a V {
    // e.clockAt || record.at — e is already required non-null by callers.
    if js_truthy(r["event"].get("clockAt")) { &r["event"]["clockAt"] } else { &r["at"] }
}
/// records[i]?.hash !== expected — distinguishes a missing row/hash (undefined)
/// from a stored null.
fn row_hash_eq(records: &VerifiedSource, index: Option<usize>, expected: &V, expected_key_present: bool) -> bool {
    match index.and_then(|i| records.records.get(i)) {
        Some(r) => strict_eq(&r["hash"], expected),
        None => !expected_key_present,
    }
}
fn json_index_usize(v: &V) -> Option<usize> {
    // arr[i] hits only for canonical integer indices.
    v.as_f64().filter(|x| x.fract() == 0.0 && *x >= 0.0 && *x <= usize::MAX as f64).map(|x| x as usize)
}

/// basketCodeIdentity() — digests of the frozen JS sources, read relative to
/// the live-paper directory supplied by the caller (JS resolves its own dir).
pub fn basket_code_identity(live_paper_dir: &Path) -> Result<V, String> {
    let mut map = Map::new();
    for p in [
        "basket-watch.mjs", "basket-observer.mjs", "basket-inputs.mjs", "model.mjs",
        "../paper/fixed.mjs", "../feed/contract.mjs",
    ] {
        let text = std::fs::read_to_string(live_paper_dir.join(p)).map_err(|e| e.to_string())?;
        map.insert(p.to_string(), json!(digest(&V::from(text))));
    }
    Ok(V::Object(map))
}

/// createBasketCapture(records, definitions, cutoff, codeIdentity)
/// `records` is the verified source stream (ml_shadow::verify_source output).
pub fn create_basket_capture(records: &VerifiedSource, definitions: &V, cutoff: &str, code_identity: &V) -> Result<V, String> {
    let cutoff_ms = parse_iso_ms(cutoff);
    let last_clock_ms = match records.records.last() {
        Some(r) => {
            if r["event"].is_null() { return Err("Cannot read properties of undefined (reading 'clockAt')".into()); }
            record_clock(r).as_str().and_then(parse_iso_ms)
        }
        None => None,
    };
    let valid = !records.records.is_empty()
        && !records.prefix_hashes.is_empty()
        && canonical(&json!(cutoff))
        && !(cutoff_ms.unwrap_or(f64::NAN) < last_clock_ms.unwrap_or(f64::NAN))
        && definitions.as_array().is_some_and(|ds| {
            !ds.is_empty()
                && ds.iter().all(|d| {
                    d["id"].as_str().is_some_and(|s| {
                        s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
                    })
                })
                && {
                    let mut seen = Vec::new();
                    !ds.iter().any(|d| {
                        let dup = seen.iter().any(|x| strict_eq(x, &d["id"]));
                        seen.push(d["id"].clone());
                        dup
                    })
                }
        });
    if !valid { return Err("Invalid prospective basket source".into()); }
    let source_hash = records.prefix_hashes.last().unwrap().clone();
    let mut pairs = Vec::new();
    for d in definitions.as_array().unwrap() {
        let obs = create_basket_observer(
            d["tournamentId"].as_str().unwrap_or(""), &d["legs"], &source_hash, cutoff, &V::Null, &json!({}),
        )?;
        pairs.push(json!({"id": d["id"], "bank": obs["bank"], "state": obs["state"]}));
    }
    let bank = json!({
        "schemaVersion": 1, "mode": "dual_no_basket_measurement", "cutoff": cutoff,
        "sourceRecords": records.records.len() as i64, "sourceHash": source_hash,
        "codeIdentity": code_identity, "definitions": definitions,
        "pairs": pairs.iter().map(|p| json!({"id": p["id"], "bank": p["bank"], "initialState": p["state"]})).collect::<Vec<_>>(),
    });
    let state = initial_capture(&bank, records)?;
    Ok(json!({"bank": bank, "state": state}))
}
fn bound(bank: &V, records: &VerifiedSource) -> Result<(), String> {
    let n = js_int(&bank["sourceRecords"]);
    if js_int(&bank["schemaVersion"]) != Some(1)
        || bank["mode"].as_str() != Some("dual_no_basket_measurement")
        || n.map_or(true, |x| x < 1)
        || n.and_then(|x| records.prefix_hashes.get(x as usize - 1)).map(String::as_str)
            != bank["sourceHash"].as_str()
    {
        return Err("Basket source prefix changed".into());
    }
    Ok(())
}
fn initial_capture(bank: &V, records: &VerifiedSource) -> Result<V, String> {
    bound(bank, records)?;
    let n = js_int(&bank["sourceRecords"]).unwrap() as usize;
    let last_hash = records.records.get(n - 1).map(|r| r["hash"].clone())
        .ok_or_else(|| "Cannot read properties of undefined (reading 'hash')".to_string())?;
    let mut pairs = Map::new();
    for p in bank["pairs"].as_array().cloned().unwrap_or_default() {
        pairs.insert(js_str(&p["id"]), p["initialState"].clone());
    }
    Ok(json!({
        "bankHash": digest(bank),
        "sourceSequence": bank["sourceRecords"],
        "lastRecordHash": last_hash,
        "pairs": V::Object(pairs),
    }))
}

fn snapshot_market_id(r: &V) -> Option<V> {
    // r.marketId || /^\/markets\/([1-9]\d*)\/orderbook\?/.exec(r.endpoint || '')?.[1]
    if js_truthy(r.get("marketId")) { return Some(r["marketId"].clone()); }
    let ep = r["endpoint"].as_str().unwrap_or("");
    let rest = ep.strip_prefix("/markets/")?;
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() || digits.starts_with('0') { return None; }
    if !rest[digits.len()..].starts_with("/orderbook?") { return None; }
    Some(V::from(digits))
}
fn spread(v: &V) -> Result<Vec<V>, String> {
    if !js_truthy(Some(v)) { return Ok(vec![]); }
    match v {
        V::Array(a) => Ok(a.clone()),
        V::String(s) => Ok(s.chars().map(|c| V::from(c.to_string())).collect()),
        _ => Err("spread argument is not iterable".into()),
    }
}
fn pairs_with_leg<'a>(bank: &'a V, market_id: &V) -> Vec<&'a V> {
    bank["pairs"].as_array().map(|ps| {
        ps.iter()
            .filter(|p| p["bank"]["legs"].as_array().is_some_and(|ls| ls.iter().any(|l| strict_eq(&l["marketId"], market_id))))
            .collect()
    }).unwrap_or_default()
}
fn invalidate(state: &mut V, bank: &V, market_id: Option<&V>, clock: &str) -> Result<(), String> {
    for p in bank["pairs"].as_array().cloned().unwrap_or_default() {
        let hit = market_id.is_none()
            || p["bank"]["legs"].as_array().is_some_and(|ls| ls.iter().any(|l| strict_eq(&l["marketId"], market_id.unwrap())));
        if !hit { continue; }
        for l in p["bank"]["legs"].as_array().cloned().unwrap_or_default() {
            if market_id.is_none() || strict_eq(&l["marketId"], market_id.unwrap()) {
                let pid = js_str(&p["id"]);
                let next = invalidate_basket(&state["pairs"][&pid], l["marketId"].as_str(), clock, &p["bank"])?;
                state["pairs"][&pid] = next;
            }
        }
    }
    Ok(())
}

/// advanceBasketRecord(input, record, bank) — {state, observations}
pub fn advance_basket_record(input: &V, record: &V, bank: &V) -> Result<V, String> {
    if !strict_eq(&input["bankHash"], &V::from(digest(bank)))
        || !plus1_eq(&record["sequence"], &input["sourceSequence"])
        || !strict_eq(&record["previous"], &input["lastRecordHash"])
    {
        return Err("Basket source continuity failure".into());
    }
    if record["event"].is_null() { return Err("Cannot read properties of undefined (reading 'clockAt')".into()); }
    let mut state = input.clone();
    let mut observations: Vec<V> = Vec::new();
    let e = &record["event"];
    let at_v = record_clock(record);
    if !canonical(at_v) { return Err("Invalid basket source clock".into()); }
    let at = at_v.as_str().unwrap();
    let at_ms = parse_iso_ms(at).unwrap();
    let cutoff_ms = parse_iso_ms(bank["cutoff"].as_str().unwrap_or("")).unwrap_or(f64::NAN);
    if at_ms > cutoff_ms {
        if e["type"].as_str() == Some("account_read_outage") {
            invalidate(&mut state, bank, None, at)?;
        }
        if e["type"].as_str() == Some("cycle") {
            let excl_at_ms = parse_iso_ms(e["exclusionObservedAt"].as_str().unwrap_or(""));
            let stale_legs = match &state["pairs"] {
                V::Object(m) => m.values().any(|s| {
                    js_truthy(s.get("lastAt"))
                        && s["lastAt"].as_str().and_then(parse_iso_ms)
                            .zip(excl_at_ms)
                            .is_some_and(|(l, x)| l > x)
                }),
                V::Null => return Err("Cannot convert undefined or null to object".into()),
                _ => false, // Object.values(primitive) is []
            };
            let fresh_exclusions = e["accountExclusions"].as_array().is_some_and(|a| a.iter().all(|x| x.is_string()))
                && canonical(&e["exclusionObservedAt"])
                && excl_at_ms.unwrap() <= at_ms
                && !stale_legs;
            if !fresh_exclusions {
                invalidate(&mut state, bank, None, at)?;
            } else {
                let excluded: BTreeSet<String> = e["accountExclusions"].as_array().unwrap().iter()
                    .filter_map(|x| x.as_str().map(String::from)).collect();
                let mut deferred: Vec<V> = Vec::new();
                let mut snaps = Vec::new();
                for (index, r) in spread(&e["sourceSnapshots"])?.into_iter()
                    .chain(spread(&e["supplementalDepth"])?.into_iter())
                    .enumerate()
                {
                    let clock_v = if js_truthy(r.get("receivedAt")) { r["receivedAt"].clone() }
                        else if js_truthy(r.get("at")) { r["at"].clone() }
                        else { at_v.clone() };
                    let clock_ms = if canonical(&clock_v) { parse_iso_ms(clock_v.as_str().unwrap()).unwrap() } else { f64::INFINITY };
                    snaps.push((index, clock_ms, r, clock_v));
                }
                snaps.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal).then(a.0.cmp(&b.0)));
                for p in bank["pairs"].as_array().cloned().unwrap_or_default() {
                    for leg in p["bank"]["legs"].as_array().cloned().unwrap_or_default() {
                        if leg["exchangeId"].as_str().is_some_and(|s| excluded.contains(s)) {
                            let pid = js_str(&p["id"]);
                            let next = invalidate_basket(&state["pairs"][&pid], leg["marketId"].as_str(), e["exclusionObservedAt"].as_str().unwrap_or(""), &p["bank"])?;
                            state["pairs"][&pid] = next;
                        }
                    }
                }
                for (_, clock_ms, r, clock_v) in snaps {
                    let Some(market_id) = snapshot_market_id(&r) else { continue; };
                    let pairs = pairs_with_leg(bank, &market_id);
                    if pairs.is_empty() { continue; }
                    if clock_ms == f64::INFINITY || clock_ms > at_ms || clock_ms < excl_at_ms.unwrap() {
                        if !deferred.iter().any(|x| strict_eq(x, &market_id)) { deferred.push(market_id.clone()); }
                        continue;
                    }
                    let clock = clock_v.as_str().unwrap();
                    if js_truthy(r.get("skipped")) {
                        invalidate(&mut state, bank, Some(&market_id), clock)?;
                        continue;
                    }
                    if clock_ms <= cutoff_ms { continue; }
                    for p in pairs {
                        let leg = p["bank"]["legs"].as_array().cloned().unwrap_or_default().into_iter()
                            .find(|l| strict_eq(&l["marketId"], &market_id)).unwrap();
                        let pid = js_str(&p["id"]);
                        if leg["exchangeId"].as_str().is_some_and(|s| excluded.contains(s)) {
                            let next = invalidate_basket(&state["pairs"][&pid], market_id.as_str(), clock, &p["bank"])?;
                            state["pairs"][&pid] = next;
                            continue;
                        }
                        let market_str = js_str(&market_id);
                        let book = (|| -> Result<V, String> {
                            let expected = format!("/markets/{}/orderbook?tournamentId={}&depth=20",
                                market_str, js_str(&p["bank"]["tournamentId"]));
                            if r["endpoint"].as_str() != Some(expected.as_str()) { return Err("Book endpoint mismatch".into()); }
                            let mut b = normalize_book(&r["data"], &market_str, p["bank"]["tournamentId"].as_str().unwrap_or(""),
                                r["receivedAt"].as_str().unwrap_or(""), r["requestStartedAt"].as_str().unwrap_or(""),
                                20000, &excluded, "authoritative_receipt_v1")?;
                            b["tournamentId"] = p["bank"]["tournamentId"].clone();
                            Ok(b)
                        })();
                        match book {
                            Err(_) => {
                                let next = invalidate_basket(&state["pairs"][&pid], market_id.as_str(), clock, &p["bank"])?;
                                state["pairs"][&pid] = next;
                            }
                            Ok(book) => match observe_basket(&state["pairs"][&pid], &book, &p["bank"]) {
                                Ok(result) => {
                                    let next_state = result["state"].clone();
                                    if js_truthy(result.get("observation")) {
                                        let mut o = Map::new();
                                        o.insert("pairId".into(), p["id"].clone());
                                        for (k, v) in result["observation"].as_object().unwrap() { o.insert(k.clone(), v.clone()); }
                                        observations.push(V::Object(o));
                                    }
                                    state["pairs"][&pid] = next_state;
                                }
                                Err(_) => {
                                    let next = invalidate_basket(&state["pairs"][&pid], market_id.as_str(), clock, &p["bank"])?;
                                    state["pairs"][&pid] = next;
                                }
                            },
                        }
                    }
                }
                for market_id in &deferred {
                    invalidate(&mut state, bank, Some(market_id), at)?;
                }
                // Excluded books must not survive just because this cycle fetched neither leg.
                for p in bank["pairs"].as_array().cloned().unwrap_or_default() {
                    for leg in p["bank"]["legs"].as_array().cloned().unwrap_or_default() {
                        if leg["exchangeId"].as_str().is_some_and(|s| excluded.contains(s)) {
                            let pid = js_str(&p["id"]);
                            let next = invalidate_basket(&state["pairs"][&pid], leg["marketId"].as_str(), at, &p["bank"])?;
                            state["pairs"][&pid] = next;
                        }
                    }
                }
            }
        }
    }
    state["sourceSequence"] = record["sequence"].clone();
    state["lastRecordHash"] = record["hash"].clone();
    Ok(json!({"state": state, "observations": observations}))
}

fn append_journal(file: &Path, row: &V) -> Result<(), String> {
    let mut fd = std::fs::OpenOptions::new().create(true).append(true).open(file).map_err(|e| e.to_string())?;
    let bytes = format!("{}\n", baal_service_runtime::json_text::js_stringify(row));
    fd.write_all(bytes.as_bytes()).map_err(|_| "Basket journal short write".to_string())?;
    fd.sync_all().map_err(|e| e.to_string())?;
    Ok(())
}

/// openBasketWatch(runtime, {initialize, cutoff, definitions, codeIdentity})
/// `cutoff` must already be the caller's instant (JS defaults to now).
pub struct BasketWatch {
    runtime: PathBuf,
    journal_file: PathBuf,
    bank: V,
    state: V,
    sequence: i64,
    previous: String,
}
impl BasketWatch {
    pub fn bank(&self) -> &V { &self.bank }
    pub fn state(&self) -> &V { &self.state }
    /// consume() — None while the source journal has an incomplete tail.
    pub fn consume(&mut self) -> Result<Option<i64>, String> {
        let raw = std::fs::read_to_string(self.runtime.join("journal.jsonl")).map_err(|e| e.to_string())?;
        let Some(records) = verify_source(&raw)? else { return Ok(None); };
        bound(&self.bank, &records)?;
        let src_seq = self.state["sourceSequence"].as_f64().unwrap_or(f64::NAN);
        let prior_ok = row_hash_eq(&records,
            json_index_usize(&self.state["sourceSequence"]).map(|i| i.wrapping_sub(1)).filter(|_| src_seq.is_finite()),
            &self.state["lastRecordHash"],
            self.state.as_object().map(|m| m.contains_key("lastRecordHash")).unwrap_or(false));
        if (records.records.len() as f64) < src_seq || !prior_ok {
            return Err("Basket source changed".into());
        }
        let start = if src_seq.is_finite() && src_seq >= 0.0 { (src_seq as usize).min(records.records.len()) } else { 0 };
        let mut consumed = 0i64;
        for record in &records.records[start..] {
            let output = advance_basket_record(&self.state, record, &self.bank)?;
            let mut body = json!({
                "sequence": self.sequence + 1, "previous": self.previous,
                "sourceSequence": record["sequence"], "sourceHash": record["hash"],
                "stateHash": digest(&output["state"]), "observationsHash": digest(&output["observations"]),
                "observations": output["observations"],
            });
            let hash = digest(&body);
            body.as_object_mut().unwrap().insert("hash".into(), json!(hash));
            append_journal(&self.journal_file, &body)?;
            self.state = output["state"].clone();
            self.sequence += 1;
            self.previous = hash;
            consumed += 1;
        }
        atomic_json(&self.runtime.join("basket-state.json"), &self.state)?;
        Ok(Some(consumed))
    }
    pub fn status(&self) -> Result<V, String> {
        let mut pairs = Vec::new();
        for p in self.bank["pairs"].as_array().cloned().unwrap_or_default() {
            let pid = js_str(&p["id"]);
            let report = basket_report(&self.state["pairs"][&pid], &p["bank"])?;
            let mut o = Map::new();
            o.insert("pairId".into(), p["id"].clone());
            if let Some(obj) = report.as_object() {
                for (k, v) in obj { o.insert(k.clone(), v.clone()); }
            }
            pairs.push(V::Object(o));
        }
        Ok(json!({
            "schemaVersion": 1, "mode": "dual_no_basket_measurement", "bankHash": digest(&self.bank),
            "cutoff": self.bank["cutoff"], "sourceSequence": self.state["sourceSequence"],
            "sourceHash": self.state["lastRecordHash"], "journalSequence": self.sequence,
            "journalHash": self.previous, "pairs": pairs,
            "networkRequests": 0, "ordersSubmitted": 0, "simulatedFills": null, "pnl": null,
            "eligibleForExecution": false,
        }))
    }
}
pub fn open_basket_watch(runtime: &Path, initialize: bool, cutoff: &str, definitions: &V, code_identity: &V) -> Result<BasketWatch, String> {
    let bank_file = runtime.join("basket-bank.json");
    let journal_file = runtime.join("basket-journal.jsonl");
    let raw_source = std::fs::read_to_string(runtime.join("journal.jsonl")).map_err(|e| e.to_string())?;
    let Some(records) = verify_source(&raw_source)? else { return Err("Basket source temporarily incomplete".into()); };
    let bank: V;
    if initialize {
        if bank_file.exists() || journal_file.exists() { return Err("Refusing to replace existing basket cohort".into()); }
        bank = create_basket_capture(&records, definitions, cutoff, code_identity)?["bank"].clone();
        let pretty = serde_json::to_string_pretty(&bank).map_err(|e| e.to_string())?;
        let mut fd = std::fs::OpenOptions::new();
        fd.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            fd.mode(0o600);
        }
        fd.open(&bank_file).and_then(|mut f| f.write_all(pretty.as_bytes())).map_err(|e| e.to_string())?;
        let mut jf = std::fs::OpenOptions::new();
        jf.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            jf.mode(0o600);
        }
        jf.open(&journal_file).map_err(|e| e.to_string())?;
    } else {
        bank = serde_json::from_str(&std::fs::read_to_string(&bank_file).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    }
    if digest(&bank["codeIdentity"]) != digest(code_identity) {
        return Err("Frozen basket code changed; new cohort required".into());
    }
    let mut state = initial_capture(&bank, &records)?;
    let mut sequence = 0i64;
    let mut previous = digest(&bank);
    let raw = std::fs::read_to_string(&journal_file).map_err(|e| e.to_string())?;
    if !raw.is_empty() && !raw.ends_with('\n') { return Err("Incomplete basket journal".into()); }
    for line in raw.split('\n').filter(|l| !l.is_empty()) {
        let row: V = serde_json::from_str(line).map_err(|e| e.to_string())?;
        let hash = row["hash"].as_str().unwrap_or("").to_string();
        let mut body = row.clone();
        if let Some(m) = body.as_object_mut() { m.shift_remove("hash"); }
        sequence += 1;
        let src_idx = json_index_usize(&body["sourceSequence"]).map(|i| i.wrapping_sub(1));
        let expected_src = plus1_eq(&body["sourceSequence"], &state["sourceSequence"]);
        if hash != digest(&body)
            || js_int(&body["sequence"]) != Some(sequence)
            || body["previous"].as_str() != Some(previous.as_str())
            || !expected_src
            || !row_hash_eq(&records, src_idx, &body["sourceHash"], body.as_object().map(|m| m.contains_key("sourceHash")).unwrap_or(false))
        {
            return Err("Basket journal integrity failure".into());
        }
        let rec = src_idx.and_then(|i| records.records.get(i))
            .ok_or_else(|| "Cannot read properties of undefined (reading 'sequence')".to_string())?;
        let output = advance_basket_record(&state, rec, &bank)?;
        if digest(&output["state"]) != body["stateHash"].as_str().unwrap_or("")
            || digest(&output["observations"]) != body["observationsHash"].as_str().unwrap_or("")
            || digest(&body["observations"]) != body["observationsHash"].as_str().unwrap_or("")
        {
            return Err("Basket replay mismatch".into());
        }
        state = output["state"].clone();
        previous = hash;
    }
    Ok(BasketWatch { runtime: runtime.to_path_buf(), journal_file, bank, state, sequence, previous })
}
