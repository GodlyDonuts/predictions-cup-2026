// live-paper/stream-book-adapter.mjs — verified stream rows -> decision events.
// Accepted public market evidence only. This module never invents a REST
// request for a push, nor turns a quote/trade into an execution claim.
use crate::passive_capture::{consume_passive_capture, create_passive_capture};
use baal_paper::fixed::instant;
use baal_service_runtime::contract::instant_ns;
use baal_service_runtime::json_text::{digest, js_int, js_truthy};
/// streamDigest — sha256 over JSON.stringify (insertion order).
pub fn stream_digest(v: &V) -> String { digest(v) }
/// instantNs(v)
fn ns(v: &V) -> Result<i128, String> {
    instant_ns(v.as_str().unwrap_or("")).map_err(|e| e.to_string())
}
use serde_json::{json, Map, Value as V};

fn need(ok: bool, code: &str) -> Result<(), String> {
    if !ok { return Err(code.into()); }
    Ok(())
}
fn ms(v: &V) -> Result<f64, String> {
    Ok((instant_ns(v.as_str().unwrap_or("")).map_err(|e| e.to_string())? / 1_000_000) as f64)
}
fn truthy(v: &V) -> bool { js_truthy(Some(v)) }

/// createStreamBookAdapter(bank)
pub fn create_stream_book_adapter(bank: &V) -> Result<V, String> {
    need(
        bank["schemaVersion"].as_i64() == Some(1)
            && bank["mode"].as_str() == Some("verified_stream_book_input")
            && js_int(&bank["sourcePrefixRecords"]).is_some_and(|n| n >= 0),
        "INVALID_STREAM_BANK",
    )?;
    instant(bank["cutoff"].as_str().unwrap_or(""))?;
    need(
        bank["sourceHash"].as_str().is_some_and(|s| {
            s.len() == 64 && s.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        }),
        "INVALID_STREAM_SOURCE",
    )?;
    let capture = create_passive_capture(&json!({
        "scope":bank["scope"],"sourceHash":bank["sourceHash"],"cutoff":bank["cutoff"],
        "sessionId":bank["sessionId"]}))?;
    Ok(json!({"schemaVersion":1,"bankHash":stream_digest(bank),"capture":capture,
        "lastAvailableAt":V::Null,"latest":{},
        "counters":{"sourceRows":0,"prefixRows":0,"acceptedBooks":0,"streamBooks":0,
            "restBooks":0,"bufferedRecoveryBooks":0,"invalidations":0,"staleAtConsumption":0},
        "lastEventId":0}))
}
/// advanceStreamBookAdapter(input,row,bank,{availableAt}) -> {state,events}
pub fn advance_stream_book_adapter(input: &V, row: &V, bank: &V, available_at: &str) -> Result<V, String> {
    need(
        input["bankHash"].as_str() == Some(stream_digest(bank).as_str()),
        "STREAM_BANK_CHANGED",
    )?;
    let host = instant(available_at)?;
    let observed = ms(&row["observedAt"])?;
    need(
        observed <= host as f64
            && (input["lastAvailableAt"].is_null()
                || (host as f64) >= instant(input["lastAvailableAt"].as_str().unwrap_or(""))? as f64),
        "STREAM_HOST_CLOCK_INVALID",
    )?;
    let topic = row["topic"].as_str().unwrap_or("");
    let market_id = topic
        .rsplit(":market:")
        .next()
        .filter(|_| topic.contains(":market:"))
        .and_then(|m| {
            if !m.is_empty() && m.as_bytes()[0] != b'0' && m.bytes().all(|b| b.is_ascii_digit()) {
                Some(m.to_string())
            } else {
                None
            }
        });
    // before is read from the PRE-consume capture state.
    let before = input["capture"]["contexts"][market_id.as_deref().unwrap_or("")].clone();
    let out = consume_passive_capture(&input["capture"], row)?;
    let mut state = input.clone();
    state["capture"] = out["state"].clone();
    state["lastAvailableAt"] = json!(available_at);
    state["latest"] = input["latest"].clone();
    state["counters"] = input["counters"].clone();
    let mut events: Vec<V> = Vec::new();
    let prefix = js_int(&row["captureIndex"]).unwrap_or(i64::MAX)
        < js_int(&bank["sourcePrefixRecords"]).unwrap_or(i64::MIN);
    need(
        !prefix || observed <= instant(bank["cutoff"].as_str().unwrap_or(""))? as f64,
        "STREAM_PREFIX_AFTER_CUTOFF",
    )?;
    let sr = state["counters"]["sourceRows"].as_i64().unwrap_or(0) + 1;
    state["counters"]["sourceRows"] = json!(sr);
    if prefix {
        let pr = state["counters"]["prefixRows"].as_i64().unwrap_or(0) + 1;
        state["counters"]["prefixRows"] = json!(pr);
    }
    // publish(e): prefix rows never publish.
    let mut publish = |state: &mut V, events: &mut Vec<V>, e: &V| {
        if prefix {
            return;
        }
        let mut m = e.as_object().cloned().unwrap_or_default();
        let next = state["lastEventId"].as_i64().unwrap_or(0) + 1;
        state["lastEventId"] = json!(next);
        m.insert("eventId".into(), json!(next));
        m.insert("sourceCaptureIndex".into(), row["captureIndex"].clone());
        m.insert("sourceRowHash".into(), json!(stream_digest(row)));
        m.insert("captureObservedAt".into(), row["observedAt"].clone());
        m.insert("availableAt".into(), json!(available_at));
        events.push(V::Object(m));
    };
    for record in out["records"].as_array().cloned().unwrap_or_default() {
        let e = record["event"].clone();
        let ctx = out["state"]["contexts"][record["marketId"].as_str().unwrap_or("")].clone();
        let common = json!({"marketId":record["marketId"],
            "exchangeId":ctx["observer"]["bank"]["exchangeId"],
            "tournamentId":bank["scope"]["tournamentId"],"confirmedAt":e["receivedAt"],
            "continuity":e["continuity"]});
        if e["kind"].as_str() == Some("invalidate") {
            if let V::Object(l) = &mut state["latest"] {
                l.remove(record["marketId"].as_str().unwrap_or(""));
            }
            if !prefix {
                let n = state["counters"]["invalidations"].as_i64().unwrap_or(0) + 1;
                state["counters"]["invalidations"] = json!(n);
            }
            let mut ev = common.as_object().cloned().unwrap_or_default();
            ev.insert("kind".into(), json!("invalidate"));
            ev.insert("reason".into(), e["reason"].clone());
            publish(&mut state, &mut events, &V::Object(ev));
            continue;
        }
        if e["kind"].as_str() != Some("book") {
            continue;
        }
        let (origin, quote_received_at, request_started_at): (String, V, V);
        if row["kind"].as_str() == Some("event") {
            origin = "stream".to_string();
            quote_received_at = if row.get("receivedAt").is_some() && !row["receivedAt"].is_null() {
                row["receivedAt"].clone()
            } else {
                row["observedAt"].clone()
            };
            request_started_at = V::Null;
        } else {
            need(
                row["kind"].as_str() == Some("recovery_finished") && before["snapshot"].is_object(),
                "RECOVERY_EVIDENCE_MISSING",
            )?;
            let snap = &before["snapshot"]["book"];
            if !snap.is_object() {
                // JS: snap.asOf on undefined book — TypeError.
                return Err("Cannot read properties of undefined (reading 'asOf')".into());
            }
            if snap["asOf"].is_null() {
                // JS: snap.asOf.sequence on undefined asOf — TypeError.
                return Err("Cannot read properties of undefined (reading 'sequence')".into());
            }
            let from_snapshot = snap["asOf"]["sequence"].as_i64()
                .map(|s| s.to_string()) == e["book"]["sequence"].as_str().map(|s| s.to_string())
                && instant_ns(snap["asOf"]["at"].as_str().unwrap_or("")).map_err(|x| x.to_string())?
                    == ns(&e["book"]["sourceAt"])?;
            if from_snapshot {
                origin = "rest_recovery".to_string();
                quote_received_at = before["snapshot"]["receivedAt"].clone();
                request_started_at = before["snapshot"]["originalRequestStartedAt"].clone();
            } else {
                let lb = &before["latestBook"];
                need(
                    lb["origin"].as_str() == Some("stream")
                        && lb["book"]["asOf"]["sequence"].as_i64().map(|s| s.to_string())
                            == e["book"]["sequence"].as_str().map(|s| s.to_string())
                        && instant_ns(lb["book"]["asOf"]["at"].as_str().unwrap_or("")).map_err(|x| x.to_string())?
                            == ns(&e["book"]["sourceAt"])?,
                    "BUFFERED_RECOVERY_EVIDENCE_MISSING",
                )?;
                origin = "buffered_stream_recovery".to_string();
                quote_received_at = lb["receivedAt"].clone();
                request_started_at = V::Null;
            }
        }
        let receipt = ms(&quote_received_at)?;
        let confirmed = instant(e["receivedAt"].as_str().unwrap_or(""))? as f64;
        let expiry = if ctx["knownExpiry"].is_object() {
            ctx["knownExpiry"]["at"].clone()
        } else {
            V::Null
        };
        need(
            receipt <= host as f64 + 1000.0
                && confirmed <= host as f64
                && (!truthy(&request_started_at)
                    || ns(&request_started_at)? <= ns(&quote_received_at)?),
            "STREAM_RECEIPT_CLOCK_INVALID",
        )?;
        // A recovery confirmation can be later than the book's original receipt.
        // Never refresh quote age to the confirmation or the local polling time.
        let age = host as f64 - receipt;
        let stale_ms = ctx["staleMs"].as_f64().unwrap_or(0.0);
        let eligible = !prefix
            && observed > instant(bank["cutoff"].as_str().unwrap_or(""))? as f64
            && truthy(&ctx["ready"]) && truthy(&e["continuity"]["ready"])
            && truthy(&e["continuity"]["continuous"])
            && age >= 0.0 && age < 20000.0_f64.min(stale_ms)
            && (!truthy(&expiry) || ms(&expiry)? > host as f64);
        let mut book = e["book"].as_object().cloned().unwrap_or_default();
        book.remove("requestStartedAt");
        book.remove("receivedAt");
        let mut ev = common.as_object().cloned().unwrap_or_default();
        ev.insert("kind".into(), json!("book"));
        ev.insert("origin".into(), json!(origin));
        ev.insert("quoteReceivedAt".into(), quote_received_at.clone());
        ev.insert("requestStartedAt".into(), request_started_at);
        ev.insert("nextExpiryAt".into(), expiry);
        ev.insert("decisionEligible".into(), json!(eligible));
        ev.insert("book".into(), V::Object(book));
        ev.insert("evidence".into(), json!("accepted_public_book_not_order_fill"));
        let mut latest_entry = ev.clone();
        latest_entry.insert("availableAt".into(), json!(available_at));
        latest_entry.insert("captureObservedAt".into(), row["observedAt"].clone());
        state["latest"][record["marketId"].as_str().unwrap_or("")] = V::Object(latest_entry);
        if !prefix {
            let ab = state["counters"]["acceptedBooks"].as_i64().unwrap_or(0) + 1;
            state["counters"]["acceptedBooks"] = json!(ab);
            let key = match ev["origin"].as_str() {
                Some("stream") => "streamBooks",
                Some("rest_recovery") => "restBooks",
                _ => "bufferedRecoveryBooks",
            };
            let n = state["counters"][key].as_i64().unwrap_or(0) + 1;
            state["counters"][key] = json!(n);
            if !eligible {
                let n = state["counters"]["staleAtConsumption"].as_i64().unwrap_or(0) + 1;
                state["counters"]["staleAtConsumption"] = json!(n);
            }
        }
        publish(&mut state, &mut events, &V::Object(ev));
    }
    Ok(json!({"state":state,"events":events}))
}
/// streamBookReport(state,bank,at)
pub fn stream_book_report(state: &V, bank: &V, at: &str) -> Result<V, String> {
    let now = instant(at)?;
    need(
        state["bankHash"].as_str() == Some(stream_digest(bank).as_str())
            && (state["lastAvailableAt"].is_null()
                || now >= instant(state["lastAvailableAt"].as_str().unwrap_or(""))?),
        "STREAM_REPORT_CLOCK_INVALID",
    )?;
    let mut markets = Vec::new();
    for m in bank["scope"]["markets"].as_array().cloned().unwrap_or_default() {
        let market_id = m["marketId"].clone();
        let exchange_id = m["exchangeId"].clone();
        let b = &state["latest"][market_id.as_str().unwrap_or("")];
        // age = now - ms(b.quoteReceivedAt): whole-ms i64 in JS semantics.
        let age: V = if truthy(b) {
            json!(now - ms(&b["quoteReceivedAt"])? as i64)
        } else {
            V::Null
        };
        let ready = truthy(b)
            && truthy(&b["decisionEligible"])
            && truthy(&state["capture"]["contexts"][market_id.as_str().unwrap_or("")]["ready"])
            && age.as_f64().unwrap_or(f64::NEG_INFINITY) >= 0.0
            && age.as_f64().unwrap_or(f64::INFINITY) < 20000.0
            && (!truthy(&b["nextExpiryAt"]) || ms(&b["nextExpiryAt"])? > now as f64);
        markets.push(json!({"marketId":market_id,"exchangeId":exchange_id,
            "origin":if b.is_object() { b["origin"].clone() } else { V::Null },
            "quoteReceivedAt":if b.is_object() { b["quoteReceivedAt"].clone() } else { V::Null },
            "confirmedAt":if b.is_object() { b["confirmedAt"].clone() } else { V::Null },
            "availableAt":if b.is_object() { b["availableAt"].clone() } else { V::Null },
            "ageMs":age,"ready":ready}));
    }
    Ok(json!({"schemaVersion":1,"mode":bank["mode"],"bankHash":state["bankHash"],
        "cutoff":bank["cutoff"],"updatedAt":at,
        "sourceCaptureIndex":state["capture"]["captureIndex"],
        "lastAvailableAt":state["lastAvailableAt"],"counters":state["counters"],
        "sourceEnded":state["capture"]["sourceEnded"],"markets":markets,
        "networkRequests":0,"ordersSubmitted":0,"simulatedFills":V::Null,"pnl":V::Null}))
}
