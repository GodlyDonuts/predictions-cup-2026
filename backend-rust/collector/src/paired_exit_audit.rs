// live-paper/paired-exit-audit.mjs — offline paired-exit measurement over the
// durable source + journal buffers.
use crate::book_publication_v2::{apply_book_publication, create_book_publication_state};
use crate::paired_package_paper_v3::{
    advance_paired_paper_v3, create_paired_paper_v3, paired_paper_hash_v3, paired_paper_report_v3,
};
use crate::paired_package_watch_v4::{paired_package_code_identity_v4, paired_package_event_v4};
use baal_paper::fixed::{big_amount, big_price, instant, money, SCALE, TICK};
use baal_service_runtime::json_text::{js_int, js_truthy, sha256};
use indexmap::IndexMap;
use num_bigint::BigInt;
use serde_json::{json, Map, Value as V};
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

const LEGS: [&str; 2] = ["381", "382"];
fn need(ok: bool, code: &str) -> Result<(), String> { if !ok { return Err(code.into()); } Ok(()) }

/// measurePairedExit(state,bank,clock)
pub fn measure_paired_exit(state: &V, bank: &V, clock: &str) -> Result<V, String> {
    let now = instant(clock)?;
    let books: Vec<V> = LEGS.iter().map(|id| state["books"][id].clone()).collect();
    let mut reason = "ready";
    if books.iter().any(|x| x.is_null()) {
        reason = "missing_book";
    } else {
        for x in &books {
            if !x["bids"].is_array() || !x["asks"].is_array() {
                // JS: x.bids.length / x.asks.length on undefined — TypeError.
                return Err("Cannot read properties of undefined (reading 'length')".into());
            }
        }
        if books.iter().any(|x| {
            x["bids"].as_array().map(|a| a.is_empty()).unwrap_or(false)
                || x["asks"].as_array().map(|a| a.is_empty()).unwrap_or(false)
        }) {
            reason = "missing_side";
        } else if {
            let mut hit = false;
            for x in &books { if instant(x["receivedAt"].as_str().unwrap_or(""))? > now { hit = true; break; } }
            hit
        } {
            reason = "future_quote";
        } else if {
            let mut hit = false;
            for x in &books {
                if now - instant(x["receivedAt"].as_str().unwrap_or(""))?
                    > bank["policy"]["maxQuoteAgeMs"].as_i64().unwrap_or(0) { hit = true; break; }
            }
            hit
        } {
            reason = "stale_quote";
        } else if (instant(books[0]["receivedAt"].as_str().unwrap_or(""))?
            - instant(books[1]["receivedAt"].as_str().unwrap_or(""))?).abs()
            > bank["policy"]["maxPairSkewMs"].as_i64().unwrap_or(0) {
            reason = "quote_skew";
        }
    }
    let mut lots: Vec<V> = state["packages"].as_array().cloned().unwrap_or_default();
    if js_truthy(state.get("active"))
        && state["active"]["fills"][LEGS[0]] == state["active"]["fills"][LEGS[1]]
        && state["active"]["fills"][LEGS[0]].as_f64().unwrap_or(0.0) > 0.0 {
        lots.push(state["active"].clone());
    }
    let mut results = Vec::new();
    for p in &lots {
        let q = LEGS.iter()
            .map(|id| js_int(&p["fills"][id]).unwrap_or(0)).min().unwrap_or(0);
        let lot_cost = LEGS.iter().fold(BigInt::from(0), |n, id| {
            n + big_amount(p["costs"][id].as_str().unwrap_or("")).unwrap_or_default()
        });
        let mut basic = Map::new();
        basic.insert("packageId".into(), p["id"].clone());
        basic.insert("quantity".into(), json!(q));
        basic.insert("lotCost".into(), json!(money(&lot_cost)));
        basic.insert("openedAt".into(),
            if js_truthy(p.get("completedAt")) { p["completedAt"].clone() } else { p["createdAt"].clone() });
        basic.insert("reason".into(), json!(reason));
        if reason != "ready" {
            results.push(V::Object(basic));
            continue;
        }
        let n = books.iter().fold(q, |n, x| {
            n.min((x["asks"][0]["quantity"].as_i64().unwrap_or(0) as i128
                * bank["policy"]["depthFractionBps"].as_i64().unwrap_or(0) as i128 / 10000) as i64)
        });
        if n == 0 {
            basic["reason"] = json!("insufficient_top_depth");
            results.push(V::Object(basic));
            continue;
        }
        let mut raw_units = Vec::new();
        for x in &books {
            raw_units.push(BigInt::from(SCALE)
                - big_price(x["asks"][0]["price"].as_str().unwrap_or(""))?);
        }
        let units: Vec<BigInt> = raw_units.iter().map(|x| x - BigInt::from(TICK)).collect();
        if units.iter().any(|x| *x < BigInt::from(TICK) || *x > BigInt::from(SCALE - TICK)) {
            basic["reason"] = json!("invalid_sale_limit");
            results.push(V::Object(basic));
            continue;
        }
        let basis_ceiling = (&lot_cost * BigInt::from(n) + BigInt::from(q) - BigInt::from(1)) / BigInt::from(q);
        let raw_margin = BigInt::from(n) * (&raw_units[0] + &raw_units[1]) - &basis_ceiling;
        let padded_margin = &raw_margin - BigInt::from(2) * BigInt::from(n) * BigInt::from(TICK);
        let target = BigInt::from(n) * big_amount(bank["policy"]["pairedTakeProfitPerShare"].as_str().unwrap_or(""))?;
        let mut r = basic;
        r["reason"] = json!("ready");
        r.insert("saleQuantity".into(), json!(n));
        r.insert("rawMargin".into(), json!(money(&raw_margin)));
        r.insert("paddedMargin".into(), json!(money(&padded_margin)));
        r.insert("target".into(), json!(money(&target)));
        r.insert("targetSurplus".into(), json!(money(&(&padded_margin - &target))));
        r.insert("targetEligible".into(), json!(padded_margin >= target));
        r.insert("versions".into(), json!(books.iter().map(|x| x["versionKey"].clone()).collect::<Vec<_>>()));
        results.push(V::Object(r));
    }
    Ok(json!({"reason": reason, "results": results}))
}
/// rows(buffer,{partial}) — in-memory variant of the journal row reader.
fn buf_rows(buffer: &[u8], partial: bool) -> Result<Vec<(V, u64)>, String> {
    let complete = buffer.iter().rposition(|b| *b == 10).map(|i| i + 1).unwrap_or(0);
    need(partial || complete == buffer.len(), "EXIT_AUDIT_PARTIAL_JOURNAL")?;
    let mut out = Vec::new();
    let mut start = 0usize;
    while start < complete {
        let end = buffer[start..complete].iter().position(|b| *b == 10).map(|i| start + i + 1).unwrap_or(0);
        need(end - start > 1 && end - start <= MAX_RECORD, "EXIT_AUDIT_RECORD_SIZE")?;
        out.push((serde_json::from_slice(&buffer[start..end]).map_err(|e| e.to_string())?, end as u64));
        start = end;
    }
    Ok(out)
}
const MAX_RECORD: usize = 8 * 1024 * 1024;

/// auditPairedExits({bank,sourceBytes,journalBytes})
/// workspace_dir is the repo root (live-paper/ directory holder).
pub fn audit_paired_exits(bank: &V, source_bytes: &[u8], journal_bytes: &[u8],
    workspace_dir: &Path) -> Result<V, String> {
    need(paired_paper_hash_v3(&bank["codeIdentity"])
        == paired_paper_hash_v3(&paired_package_code_identity_v4(workspace_dir)?),
        "EXIT_AUDIT_FROZEN_CODE")?;
    let source = buf_rows(source_bytes, true)?;
    let saved = buf_rows(journal_bytes, false)?;
    let mut publication = create_book_publication_state(&bank["universe"])?;
    let mut offset = 0u64;
    let mut index = 0usize;
    while index < source.len()
        && source[index].1 <= bank["sourcePrefixBytes"].as_u64().unwrap_or(0) {
        publication = apply_book_publication(&publication, &source[index].0)?;
        offset = source[index].1;
        index += 1;
    }
    need(offset == bank["sourcePrefixBytes"].as_u64().unwrap_or(0)
        && sha256(&source_bytes[..offset as usize]) == bank["sourcePrefixHash"].as_str().unwrap_or("")
        && publication["sequence"] == bank["sourcePrefixSequence"]
        && publication["hash"] == bank["sourcePrefixJournalHash"]
        && publication["scopeHash"] == bank["sourceScopeHash"]
        && publication["sessionId"] == bank["sourceSessionId"]
        && !js_truthy(publication.get("closed")),
        "EXIT_AUDIT_PREFIX")?;
    let host_hash = sha256(&fs::read(
        workspace_dir.join("live-paper").join("paired-package-watch-v4.mjs"))
        .map_err(|e| e.to_string())?);
    let mut state = create_paired_paper_v3(bank)?;
    let mut sequence = 0i64;
    let mut previous = paired_paper_hash_v3(bank);
    let mut ended = false;
    let mut checkpoints = Map::new();
    let mut lots: IndexMap<i64, V> = IndexMap::new();
    let mut pairs: BTreeSet<String> = BTreeSet::new();
    for saved_row in &saved {
        let mut body = saved_row.0.clone();
        let hash = body.as_object_mut().and_then(|m| m.shift_remove("hash"));
        sequence += 1;
        let body_hash = paired_paper_hash_v3(&body);
        need(js_int(&body["sequence"]) == Some(sequence)
            && body["previous"].as_str() == Some(previous.as_str())
            && hash.as_ref().and_then(|x| x.as_str()) == Some(body_hash.as_str())
            && body["bankHash"].as_str() == Some(paired_paper_hash_v3(bank).as_str()),
            "EXIT_AUDIT_JOURNAL_CHAIN")?;
        if body["kind"].as_str() == Some("genesis") {
            need(sequence == 1 && body["stateHash"].as_str() == Some(paired_paper_hash_v3(&state).as_str()),
                "EXIT_AUDIT_GENESIS")?;
        } else {
            need(body["kind"].as_str() == Some("batch")
                && js_int(&body["sourceSequence"]).unwrap_or(i64::MIN)
                    > js_int(&publication["sequence"]).unwrap_or(i64::MIN),
                "EXIT_AUDIT_CHECKPOINT")?;
            let continuity = body.get("hostVersion").is_some();
            need(!continuity
                || (body["hostVersion"].as_str() == Some("session-aware-paired-v4")
                    && body["hostHash"].as_str() == Some(host_hash.as_str())),
                "EXIT_AUDIT_HOST")?;
            let mut events = Vec::new();
            let mut event_id = state["lastEventId"].as_i64().unwrap_or(0);
            let available_at = body["availableAt"].as_str().unwrap_or("");
            while js_int(&publication["sequence"]).unwrap_or(i64::MIN)
                < js_int(&body["sourceSequence"]).unwrap_or(i64::MIN) {
                need(index < source.len(), "EXIT_AUDIT_SOURCE_TRUNCATED")?;
                let x = source[index].clone();
                index += 1;
                let r = &x.0;
                need(continuity
                    || (r["sessionId"] == bank["sourceSessionId"]
                        && r["kind"].as_str() != Some("session_start")),
                    "EXIT_AUDIT_SESSION")?;
                publication = apply_book_publication(&publication, r)?;
                offset = x.1;
                let kind = r["kind"].as_str().unwrap_or("");
                if kind == "book" {
                    event_id += 1;
                    events.push(paired_package_event_v4(r, event_id, available_at)?);
                } else if ["invalidation", "session_stop", "session_start"].contains(&kind) {
                    let id = if kind == "invalidation" { r["payload"]["marketId"].clone() } else { V::Null };
                    for m in bank["markets"].as_array().cloned().unwrap_or_default().iter()
                        .filter(|m| id.is_null() || m["marketId"] == id) {
                        event_id += 1;
                        let mut e = json!({"kind":"invalidate","eventId":event_id,
                            "tournamentId":bank["tournamentId"],"marketId":m["marketId"],
                            "exchangeId":m["exchangeId"],"confirmedAt":r["at"],
                            "availableAt":available_at});
                        if kind == "session_start" {
                            e["reason"] = json!("SESSION_START");
                        } else if let Some(reason) = r["payload"].get("reason") { e["reason"] = reason.clone(); }
                        events.push(e);
                    }
                }
                if kind == "session_stop" { ended = true; }
                if continuity && kind == "session_start" { ended = false; }
            }
            state = advance_paired_paper_v3(&state, &events, bank, available_at,
                js_truthy(body.get("allowDecisions")) && !ended)?;
            let expected = json!({"sourceBytes":offset,"sourceSequence":publication["sequence"],
                "sourceHash":publication["hash"],"sourceStateHash":paired_paper_hash_v3(&publication),
                "evidenceThrough":publication["lastAt"],"stateHash":paired_paper_hash_v3(&state)});
            for (k, v) in expected.as_object().unwrap() {
                need(paired_paper_hash_v3(&body.get(k).cloned().unwrap_or(V::Null))
                    == paired_paper_hash_v3(v), "EXIT_AUDIT_REPLAY")?;
            }
            let measured = measure_paired_exit(&state, bank, available_at)?;
            let results = measured["results"].as_array().cloned().unwrap_or_default();
            if !results.is_empty() {
                let key = measured["reason"].as_str().unwrap_or("");
                let n = js_int(&checkpoints[key]).unwrap_or(0);
                checkpoints.insert(key.to_string(), json!(n + 1));
            }
            for x in &results {
                let pkg = js_int(&x["packageId"]).unwrap_or(i64::MIN);
                if !lots.contains_key(&pkg) {
                    lots.insert(pkg, json!({"packageId":x["packageId"],"initialQuantity":x["quantity"],
                        "initialCost":x["lotCost"],"openedAt":x["openedAt"],
                        "lastObservedAt":body["availableAt"],
                        "validVersionPairs":[],"targetEligiblePairs":[],"reasons":{},
                        "bestRawMargin":V::Null,"bestPaddedMargin":V::Null,"bestTargetSurplus":V::Null,
                        "bestAt":V::Null}));
                }
                let mut l = lots.get(&pkg).cloned().unwrap_or(V::Null);
                l["lastObservedAt"] = body["availableAt"].clone();
                let reason = x["reason"].as_str().unwrap_or("");
                let rn = js_int(&l["reasons"][reason]).unwrap_or(0);
                l["reasons"][reason] = json!(rn + 1);
                if reason == "ready" {
                    let key = x["versions"].as_array().cloned().unwrap_or_default().iter()
                        .map(|v| v.as_str().map(String::from).unwrap_or_else(|| js_display(v)))
                        .collect::<Vec<_>>().join("|");
                    push_unique(&mut l["validVersionPairs"], &key);
                    pairs.insert(key.clone());
                    if js_truthy(x.get("targetEligible")) {
                        push_unique(&mut l["targetEligiblePairs"], &key);
                    }
                    let padded = big_amount(x["paddedMargin"].as_str().unwrap_or(""))?;
                    if l["bestPaddedMargin"].is_null()
                        || padded > big_amount(l["bestPaddedMargin"].as_str().unwrap_or(""))? {
                        l["bestPaddedMargin"] = x["paddedMargin"].clone();
                        l["bestAt"] = body["availableAt"].clone();
                        l["bestSaleQuantity"] = x["saleQuantity"].clone();
                        l["bestQuoteVersions"] = x["versions"].clone();
                    }
                    for (k, xv) in [("bestRawMargin", x["rawMargin"].clone()),
                        ("bestTargetSurplus", x["targetSurplus"].clone())] {
                        let candidate = big_amount(xv.as_str().unwrap_or(""))?;
                        if l[k].is_null() || candidate > big_amount(l[k].as_str().unwrap_or(""))? {
                            l[k] = xv;
                        }
                    }
                }
                lots.insert(pkg, l);
            }
        }
        previous = hash.and_then(|x| x.as_str().map(String::from)).unwrap_or_default();
    }
    need(sequence > 0, "EXIT_AUDIT_EMPTY")?;
    let report_at = if js_truthy(state.get("lastAvailableAt")) {
        state["lastAvailableAt"].as_str().unwrap_or("")
    } else {
        bank["cutoff"].as_str().unwrap_or("")
    };
    let report = paired_paper_report_v3(&state, bank, report_at)?;
    let mut out_lots = Vec::new();
    for (_, l) in lots.iter() {
        let closed = state["roundTrips"].as_array().cloned().unwrap_or_default().iter()
            .find(|p| p["packageId"] == l["packageId"]).cloned();
        let mut o = l.clone();
        let closed_at = closed.as_ref().map(|c| c["closedAt"].clone()).unwrap_or(V::Null);
        let hold_at = if js_truthy(closed.as_ref().map(|c| &c["closedAt"])) {
            closed.as_ref().unwrap()["closedAt"].as_str().unwrap_or("")
        } else {
            l["lastObservedAt"].as_str().unwrap_or("")
        };
        o["closedAt"] = if closed_at.is_null() { V::Null } else { closed_at };
        o["rightCensored"] = json!(closed.is_none());
        o["validVersionPairs"] = json!(l["validVersionPairs"].as_array().map(|a| a.len()).unwrap_or(0));
        o["targetEligiblePairs"] = json!(l["targetEligiblePairs"].as_array().map(|a| a.len()).unwrap_or(0));
        let minutes = (instant(hold_at)? - instant(l["openedAt"].as_str().unwrap_or(""))?) as f64 / 60000.0;
        o["observedHoldingMinutes"] = json!(minutes);
        out_lots.push(o);
    }
    Ok(json!({"schemaVersion":1,"bankHash":paired_paper_hash_v3(bank),"checkpointCount":sequence,
        "journalHash":previous,"stateHash":paired_paper_hash_v3(&state),
        "sourceSequence":publication["sequence"],"sourceHash":publication["hash"],
        "evidenceThrough":publication["lastAt"],
        "checkpointsWithHolding":V::Object(checkpoints),
        "distinctFreshVersionPairs":pairs.len() as i64,
        "lots":out_lots,
        "finalWallet":report["wallets"]["sequential_dual_no"],
        "networkRequests":0,"runtimeWrites":0,"profitabilityEstablished":false,
        "limitations":[
            "This verifies original consumer checkpoints, not unobserved continuous prices or actual queue fills.",
            "Distinct paired quote versions and repeated checkpoints are not independent trades.",
            "Raw margin removes only the two sale ticks; acquired basis retains original conservative entry ticks. Diagnostic alternatives are not a newly validated strategy.",
            "Holding durations are right-censored for open packages. Conditional settlement value is not realized profit."]}))
}
fn push_unique(arr: &mut V, key: &str) {
    if let Some(a) = arr.as_array_mut() {
        if !a.iter().any(|x| x.as_str() == Some(key)) { a.push(json!(key)); }
    }
}
fn js_display(v: &V) -> String {
    match v {
        V::String(s) => s.clone(),
        V::Number(n) => {
            let mut b = ryu_js::Buffer::new();
            b.format_finite(n.as_f64().unwrap_or(f64::NAN)).to_string()
        }
        V::Bool(x) => if *x { "true".into() } else { "false".into() },
        V::Null => String::new(),
        _ => String::new(),
    }
}
