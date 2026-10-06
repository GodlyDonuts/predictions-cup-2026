// live-paper/passive-capture.mjs — one immutable capture session; observedAt is
// local availability, receivedAt remains source metadata.
use num_bigint::BigInt;
use serde_json::{json, Value as V};

fn js_string(v: &V) -> String {
    match v {
        V::Null => "undefined".to_string(),
        V::String(s) => s.clone(),
        V::Number(n) => n.to_string(),
        V::Bool(b) => b.to_string(),
        other => baal_service_runtime::json_text::js_stringify(other),
    }
}
fn to_book(raw: &V, market: &V, available_at: &str, request_started_at: &str) -> Result<V, String> {
    let source_at = baal_service_runtime::contract::timestamp_str(
        raw["asOf"]["at"].as_str().unwrap_or("")).map_err(|e| e.to_string())?;
    let sequence = js_string(&raw["asOf"]["sequence"]);
    let map_levels = |levels: &V| -> Result<Vec<V>, String> {
        levels.as_array().cloned().unwrap_or_default().iter().map(|l| {
            Ok(json!({"price": baal_paper::fixed::money(
                &baal_paper::fixed::big_price(&js_string(&l["price"]))?),
                "quantity": l["quantity"]}))
        }).collect()
    };
    Ok(json!({"marketId": market["bank"]["marketId"], "exchangeId": raw["exchangeId"],
        "instrument": format!("{}:YES", raw["exchangeId"].as_str().unwrap_or("")),
        "receivedAt": available_at, "requestStartedAt": request_started_at,
        "sourceAt": source_at, "sequence": sequence.clone(),
        "versionKey": format!("{}:{}", sequence, source_at),
        "bids": map_levels(&raw["bids"])?, "asks": map_levels(&raw["asks"])?}))
}
/// capturedEvent(row, scope) — restore fixed placeholders to reuse the public
/// shape/scope validator; never pretend placeholders are original wire values.
fn captured_event(row: &V, scope: &baal_service_runtime::contract::Scope) -> Result<V, String> {
    let mut payload = row["payload"].clone();
    if row["event"].as_str() == Some("market_batch") {
        payload["delivery"]["correlationId"] = json!("");
        let settled: Vec<V> = payload["marketSettled"].as_array().cloned().unwrap_or_default()
            .into_iter().map(|mut x| {
                x["settledWith"] = json!(if x["refund"] == V::Bool(true) { "REFUND" } else { "SETTLED" });
                x
            }).collect();
        payload["marketSettled"] = json!(settled);
    } else if row["event"].as_str() == Some("market_settled") {
        payload["settledWith"] = json!(if payload["refund"] == V::Bool(true) { "REFUND" } else { "SETTLED" });
    }
    baal_service_runtime::contract::parse_event(row["event"].as_str().unwrap_or(""), &payload, scope)
        .map_err(|e| e.to_string())
}
fn round_micros(x: f64) -> BigInt {
    BigInt::from((x * 1_000_000.0).round() as i64)
}
/// createPassiveCapture({scope, sourceHash, cutoff, sessionId})
pub fn create_passive_capture(scope: &V, source_hash: &str, cutoff: &str,
    session_id: &str) -> Result<V, String> {
    if session_id.is_empty() || session_id.len() > 60
        || !session_id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        || !scope["markets"].is_array() || scope["markets"].as_array().unwrap().is_empty() {
        return Err("Invalid passive capture scope".into());
    }
    let mut seen = std::collections::BTreeSet::new();
    let mut exchanges = std::collections::BTreeSet::new();
    let mut contexts = serde_json::Map::new();
    for market in scope["markets"].as_array().unwrap() {
        let checked = baal_service_runtime::contract::config(&json!({
            "tournamentId": scope["tournamentId"],
            "marketId": market["marketId"],
            "exchangeIds": [market["exchangeId"]],
        })).map_err(|e| e.to_string())?;
        if seen.contains(&checked.market_id) || exchanges.contains(&checked.exchange_ids[0]) {
            return Err("Duplicate passive capture market".into());
        }
        seen.insert(checked.market_id.clone());
        exchanges.insert(checked.exchange_ids[0].clone());
        let observer = crate::passive_observer::create_passive_observer(
            &checked.market_id, &checked.exchange_ids[0],
            scope["tournamentId"].as_str().unwrap_or(""), source_hash, cutoff, None)?;
        contexts.insert(checked.market_id.clone(), json!({
            "epoch": 0, "revision": V::Null, "ready": false, "snapshot": V::Null,
            "recovery": V::Null, "latestBook": V::Null, "knownExpiry": V::Null,
            "recoveredAt": V::Null, "staleMs": 20000, "sourceEnded": false,
            "observer": observer}));
    }
    Ok(json!({"schemaVersion":1,"sessionId":session_id,"cutoff":cutoff,"captureIndex":-1,
        "lastObservedAt":V::Null,"lastObservedNs":V::Null,"sourceEnded":false,"contexts":contexts}))
}
/// consumePassiveCapture(input, row)
pub fn consume_passive_capture(input: &V, row: &V) -> Result<V, String> {
    if row["format"].as_str() != Some("sig-feed-capture")
        || row["schemaVersion"].as_i64() != Some(1)
        || row["captureIndex"].as_i64().is_none()
        || row["captureIndex"].as_i64() != Some(input["captureIndex"].as_i64().unwrap_or(0) + 1) {
        return Err("Missing or reordered realtime capture".into());
    }
    let observed_ns = baal_service_runtime::contract::instant_ns(
        row["observedAt"].as_str().unwrap_or("")).map_err(|e| e.to_string())?;
    let received_ns = if row["receivedAt"].is_null() { observed_ns } else {
        baal_service_runtime::contract::instant_ns(row["receivedAt"].as_str().unwrap_or(""))
            .map_err(|e| e.to_string())?
    };
    let observed = (observed_ns / 1_000_000) as f64;
    let received = (received_ns / 1_000_000) as f64;
    if !input["lastObservedNs"].is_null()
        && observed_ns < input["lastObservedNs"].as_str().unwrap_or("0").parse::<i128>().unwrap_or(0) {
        return Err("Capture observation clock rollback".into());
    }
    if received_ns > observed_ns + 1_000_000_000 {
        return Err("Future capture receipt".into());
    }
    let mut state = input.clone();
    state["captureIndex"] = row["captureIndex"].clone();
    state["lastObservedAt"] = json!(baal_service_runtime::contract::ms_to_iso(observed));
    state["lastObservedNs"] = json!(observed_ns.to_string());
    let re = regex_lite(row["topic"].as_str().unwrap_or(""));
    let id = re.unwrap_or_default();
    if id.is_empty() || state["contexts"][&id].is_null() {
        return Err("Unknown capture market".into());
    }
    let bank = state["contexts"][&id]["observer"]["bank"].clone();
    let scope = baal_service_runtime::contract::config(&json!({
        "tournamentId": bank["tournamentId"], "marketId": id, "exchangeIds": [bank["exchangeId"]],
    })).map_err(|e| e.to_string())?;
    if row["topic"].as_str() != Some(scope.topic.as_str())
        || row["epoch"].as_i64().is_none()
        || row["epoch"].as_i64().unwrap_or(0) < state["contexts"][&id]["epoch"].as_i64().unwrap_or(0) {
        return Err("Capture scope or epoch rollback".into());
    }
    let at = baal_service_runtime::contract::ms_to_iso(observed.max(received));
    let mut records: Vec<V> = Vec::new();
    let mut events: Vec<V> = Vec::new();
    macro_rules! ctx_get {
        ($state:expr, $key:expr) => { $state["contexts"][&id][$key].clone() };
    }
    macro_rules! emit {
        ($state:expr, $kind:expr, $extra:expr, $recovered:expr, $prev_rev:expr) => {{
            let prev_revision = ($prev_rev as i64).min(
                $state["contexts"][&id]["observer"]["state"]["revision"].as_i64().unwrap_or(0));
            let ctx = &$state["contexts"][&id];
            let mut e = json!({"index": ctx["observer"]["state"]["index"].as_i64().unwrap_or(0) + 1,
                "kind": $kind, "receivedAt": at,
                "continuity": {"epoch": format!("{}-{}", $state["sessionId"].as_str().unwrap_or(""),
                        ctx["epoch"].as_i64().unwrap_or(0)),
                    "revision": ctx["revision"].as_i64().unwrap_or(0),
                    "previousRevision": prev_revision,
                    "ready": ctx["ready"], "continuous": ctx["ready"], "recovered": $recovered}});
            for (k, v) in ($extra).as_object().unwrap() { e[k] = v.clone(); }
            let result = crate::passive_observer::observe_passive(
                &$state["contexts"][&id]["observer"]["state"], &e,
                &$state["contexts"][&id]["observer"]["bank"])?;
            $state["contexts"][&id]["observer"]["state"] = result["state"].clone();
            if !result["record"].is_null() {
                let mut rec = json!({"marketId": id,
                    "sourceCaptureIndex": row["captureIndex"],
                    "sourceReceivedAt": row["receivedAt"]});
                for (k, v) in result["record"].as_object().unwrap() { rec[k] = v.clone(); }
                records.push(rec);
            }
            for ev in result["events"].as_array().cloned().unwrap_or_default() {
                let mut out = json!({"marketId": id});
                for (k, v) in ev.as_object().unwrap() { out[k] = v.clone(); }
                events.push(out);
            }
            Ok::<(), String>(())
        }};
    }
    macro_rules! invalidate {
        ($state:expr, $reason:expr, $clear:expr) => {{
            $state["contexts"][&id]["ready"] = json!(false);
            $state["contexts"][&id]["recoveredAt"] = V::Null;
            if $clear {
                $state["contexts"][&id]["snapshot"] = V::Null;
                $state["contexts"][&id]["recovery"] = V::Null;
            }
            emit!($state, "invalidate", json!({"reason": $reason}), false,
                ctx_get!($state, "revision").as_i64().unwrap_or(0))?;
        }};
    }
    macro_rules! remember {
        ($state:expr, $raw:expr, $origin:expr) => {{
            let raw = $raw;
            let mut stored = false;
            if raw["asOf"].is_null() {
                false
            } else {
                let old = ctx_get!($state, "latestBook");
                let cmp = if old.is_null() { 1 } else {
                    baal_service_runtime::contract::compare_version(&raw["asOf"], &old["book"]["asOf"])
                };
                if cmp < 0 { false } else {
                    if cmp == 0
                        && baal_service_runtime::json_text::js_stringify(
                            &json!([raw["bids"], raw["asks"]]))
                            != baal_service_runtime::json_text::js_stringify(
                                &json!([old["book"]["bids"], old["book"]["asks"]])) {
                        invalidate!($state, "schema", true);
                        false
                    } else {
                        if raw["expiryKnown"] == V::Bool(true)
                            && (ctx_get!($state, "knownExpiry").is_null()
                                || baal_service_runtime::contract::compare_version(&raw["asOf"],
                                    &ctx_get!($state, "knownExpiry")["version"]) >= 0) {
                            $state["contexts"][&id]["knownExpiry"] = json!({"at": raw["nextExpiryAt"],
                                "version": raw["asOf"]});
                        }
                        if cmp == 0 && $origin == "stream" { false } else {
                            $state["contexts"][&id]["latestBook"] = json!({"book": raw,
                                "receivedAt": if row["receivedAt"].is_null() { row["observedAt"].clone() }
                                    else { row["receivedAt"].clone() },
                                "availableAt": at, "epoch": row["epoch"],
                                "captureIndex": row["captureIndex"], "origin": $origin});
                            stored = true;
                            stored
                        }
                    }
                }
            }
        }};
    }
    let expired = |state: &V| -> bool {
        let expiry = &state["contexts"][&id]["knownExpiry"]["at"];
        !expiry.is_null()
            && baal_service_runtime::contract::parse_iso_ms(expiry.as_str().unwrap_or(""))
                .unwrap_or(f64::NAN)
                <= baal_service_runtime::contract::parse_iso_ms(&at).unwrap_or(f64::NAN)
    };
    if row["epoch"].as_i64().unwrap_or(0) != state["contexts"][&id]["epoch"].as_i64().unwrap_or(0) {
        state["contexts"][&id]["epoch"] = row["epoch"].clone();
        state["contexts"][&id]["revision"] = V::Null;
        invalidate!(state, "disconnect", true);
    }
    match row["kind"].as_str().unwrap_or("") {
        "config" => {
            if row["scope"]["topic"].as_str() != Some(scope.topic.as_str())
                || row["scope"]["exchangeIds"].as_array().map(|a| a.len()) != Some(1)
                || row["scope"]["exchangeIds"][0] != bank["exchangeId"]
                || row["staleMs"].as_i64().map(|v| v <= 0 || v > 60000).unwrap_or(true) {
                return Err("Capture config mismatch".into());
            }
            let v = row["staleMs"].as_i64().unwrap_or(0)
                .min(bank["config"]["maxDataAgeMs"].as_i64().unwrap_or(0));
            state["contexts"][&id]["staleMs"] = json!(v);
        }
        k @ ("invalidation" | "disconnected" | "stopped" | "rejected" | "subscribed") => {
            let reason = match row["reason"].as_str().unwrap_or("") {
                "SETTLEMENT" => "settlement",
                "GAP" => "gap",
                "STALE" | "EXPIRY" => "stale",
                "BOOK_DIRTY" => "book_dirty",
                _ => if k == "disconnected" || k == "subscribed" { "disconnect" } else { "resync" },
            };
            invalidate!(state, reason, true);
            if k == "stopped" { state["contexts"][&id]["sourceEnded"] = json!(true); }
        }
        "recovery_started" => {
            if row["generation"].as_i64().is_none() || row["generation"].as_i64().unwrap_or(-1) < 0 {
                return Err("Invalid recovery generation".into());
            }
            invalidate!(state, "resync", true);
            state["contexts"][&id]["recovery"] = json!({"generation": row["generation"],
                "captureIndex": row["captureIndex"], "startedAt": at,
                "startedNs": observed_ns.max(received_ns).to_string()});
        }
        "snapshot" => {
            if state["contexts"][&id]["recovery"].is_null()
                || row["receivedAt"].is_null() || row["requestStartedAt"].is_null()
                || !row["books"].is_array() || row["books"].as_array().map(|b| b.len()) != Some(1) {
                invalidate!(state, "schema", true);
            } else {
                let request_ns = baal_service_runtime::contract::instant_ns(
                    row["requestStartedAt"].as_str().unwrap_or("")).map_err(|e| e.to_string())?;
                let started_ns: i128 = state["contexts"][&id]["recovery"]["startedNs"]
                    .as_str().unwrap_or("0").parse().unwrap_or(0);
                if request_ns < started_ns || request_ns > received_ns
                    || received_ns - request_ns
                        > i128::from(bank["config"]["maxDataAgeMs"].as_i64().unwrap_or(0)) * 1_000_000 {
                    invalidate!(state, "schema", true);
                } else {
                    let raw = baal_service_runtime::contract::book(&row["books"][0], &scope, true)
                        .map_err(|e| e.to_string())?;
                    state["contexts"][&id]["snapshot"] = json!({"book": raw,
                        "receivedAt": row["receivedAt"],
                        "requestStartedAt": baal_service_runtime::contract::ms_to_iso(
                            (request_ns / 1_000_000) as f64),
                        "originalRequestStartedAt": row["requestStartedAt"],
                        "captureIndex": row["captureIndex"]});
                }
            }
        }
        "recovery_finished" => {
            let snapshot = ctx_get!(state, "snapshot");
            let recovery = ctx_get!(state, "recovery");
            if row["booksReady"] != V::Bool(true) || recovery.is_null() || snapshot.is_null()
                || row["generation"] != recovery["generation"] {
                invalidate!(state, "resync", false);
            } else {
                let mut raw = snapshot["book"].clone();
                let mut request = snapshot["requestStartedAt"].clone();
                let mut receipt = snapshot["receivedAt"].clone();
                let pushed = ctx_get!(state, "latestBook");
                if !raw["asOf"].is_null()
                    && pushed["epoch"] == state["contexts"][&id]["epoch"]
                    && pushed["origin"].as_str() == Some("stream")
                    && pushed["captureIndex"].as_i64().unwrap_or(0)
                        > recovery["captureIndex"].as_i64().unwrap_or(0)
                    && baal_service_runtime::contract::parse_iso_ms(
                        pushed["receivedAt"].as_str().unwrap_or("")).unwrap_or(f64::NAN)
                        >= baal_service_runtime::contract::parse_iso_ms(
                            recovery["startedAt"].as_str().unwrap_or("")).unwrap_or(f64::NAN)
                    && baal_service_runtime::contract::compare_version(&pushed["book"]["asOf"],
                        &raw["asOf"]) > 0 {
                    raw = pushed["book"].clone();
                    request = json!(pushed["receivedAt"].as_str().unwrap_or(""));
                    receipt = pushed["receivedAt"].clone();
                }
                let stale_ms = state["contexts"][&id]["staleMs"].as_i64().unwrap_or(0) as f64;
                let latest = ctx_get!(state, "latestBook");
                if raw["asOf"].is_null()
                    || raw["bids"].as_array().map(|a| a.is_empty()).unwrap_or(true)
                    || raw["asks"].as_array().map(|a| a.is_empty()).unwrap_or(true)
                    || baal_service_runtime::contract::parse_iso_ms(&at).unwrap_or(f64::NAN)
                        - baal_service_runtime::contract::parse_iso_ms(receipt.as_str().unwrap_or(""))
                            .unwrap_or(f64::NAN) >= stale_ms
                    || baal_service_runtime::contract::parse_iso_ms(&at).unwrap_or(f64::NAN)
                        - baal_service_runtime::contract::parse_iso_ms(request.as_str().unwrap_or(""))
                            .unwrap_or(f64::NAN)
                        > bank["config"]["maxDataAgeMs"].as_i64().unwrap_or(0) as f64
                    || expired(&state)
                    || (!latest.is_null()
                        && baal_service_runtime::contract::compare_version(&raw["asOf"],
                            &latest["book"]["asOf"]) < 0) {
                    invalidate!(state, "stale", true);
                } else {
                    state["contexts"][&id]["ready"] = json!(true);
                    state["contexts"][&id]["recoveredAt"] = json!(at);
                    let book = to_book(&raw, &state["contexts"][&id]["observer"], &at,
                        request.as_str().unwrap_or(""))?;
                    emit!(state, "book", json!({"book": book}), true,
                        state["contexts"][&id]["revision"].as_i64().unwrap_or(0))?;
                    state["contexts"][&id]["latestBook"] = json!({"book": raw,
                        "receivedAt": receipt, "availableAt": at,
                        "epoch": state["contexts"][&id]["epoch"],
                        "captureIndex": row["captureIndex"], "origin": "recovery"});
                }
            }
            state["contexts"][&id]["snapshot"] = V::Null;
            state["contexts"][&id]["recovery"] = V::Null;
        }
        "event" => {
            let parsed = captured_event(row, &scope)?;
            if row["event"].as_str() != Some("market_batch") {
                invalidate!(state,
                    if row["event"].as_str() == Some("market_settled") { "settlement" } else { "book_dirty" },
                    true);
            } else {
                let p = &parsed["payload"];
                let d = &p["delivery"];
                let previous = state["contexts"][&id]["revision"].clone();
                let recovered_at = &state["contexts"][&id]["recoveredAt"];
                let recovery = &state["contexts"][&id]["recovery"];
                if (state["contexts"][&id]["ready"] == V::Bool(true) && !recovered_at.is_null()
                    && received < baal_service_runtime::contract::parse_iso_ms(
                        recovered_at.as_str().unwrap_or("")).unwrap_or(f64::NAN))
                    || (!recovery.is_null()
                        && received < baal_service_runtime::contract::parse_iso_ms(
                            recovery["startedAt"].as_str().unwrap_or("")).unwrap_or(f64::NAN)) {
                    events.push(json!({"marketId": id, "kind": "ignored_pre_recovery_receipt", "at": at}));
                    return Ok(json!({"state": state, "records": records, "events": events}));
                }
                let missing_named_book = p["trades"].as_array().cloned().unwrap_or_default().iter()
                    .chain(p["bookDirty"].as_array().cloned().unwrap_or_default().iter())
                    .any(|t| !p["books"].as_array().cloned().unwrap_or_default().iter()
                        .any(|b| b["exchangeId"] == t["exchangeId"]));
                let stale_ms = state["contexts"][&id]["staleMs"].as_i64().unwrap_or(0) as f64;
                let latest_received = state["contexts"][&id]["latestBook"]["receivedAt"].clone();
                if state["contexts"][&id]["ready"] == V::Bool(true)
                    && (baal_service_runtime::contract::parse_iso_ms(&at).unwrap_or(f64::NAN)
                        - baal_service_runtime::contract::parse_iso_ms(
                            latest_received.as_str().unwrap_or("")).unwrap_or(f64::NAN) >= stale_ms
                        || expired(&state)) {
                    invalidate!(state, "stale", true);
                }
                if p["resyncRequired"] == V::Bool(true)
                    || p["marketSettled"].as_array().map(|a| !a.is_empty()).unwrap_or(false)
                    || p["trades"].as_array().cloned().unwrap_or_default().iter()
                        .any(|t| t["sequence"].is_null())
                    || missing_named_book
                    || (!previous.is_null()
                        && d["revision"].as_i64().unwrap_or(0) > previous.as_i64().unwrap_or(0)
                        && d["previousRevision"].as_i64().unwrap_or(0) > previous.as_i64().unwrap_or(0)) {
                    invalidate!(state, "gap", true);
                }
                let new_rev = previous.as_i64().unwrap_or(0).max(d["revision"].as_i64().unwrap_or(0));
                state["contexts"][&id]["revision"] = json!(new_rev);
                let boundary = if previous.is_null() { 0 } else {
                    d["previousRevision"].as_i64().unwrap_or(0)
                        .min(state["contexts"][&id]["revision"].as_i64().unwrap_or(0))
                };
                for raw in p["books"].as_array().cloned().unwrap_or_default() {
                    if raw["bids"].as_array().map(|a| a.is_empty()).unwrap_or(true)
                        || raw["asks"].as_array().map(|a| a.is_empty()).unwrap_or(true) {
                        remember!(state, &raw, "stream");
                        invalidate!(state, "stale", true);
                        continue;
                    }
                    let was_ready = state["contexts"][&id]["ready"] == V::Bool(true);
                    if remember!(state, &raw, "stream") && was_ready {
                        if expired(&state) {
                            invalidate!(state, "stale", true);
                        } else {
                            let book = to_book(&raw, &state["contexts"][&id]["observer"], &at,
                                &baal_service_runtime::contract::ms_to_iso(received))?;
                            emit!(state, "book", json!({"book": book}), false, boundary)?;
                        }
                    }
                }
                if (previous.is_null() || d["revision"].as_i64().unwrap_or(0) > previous.as_i64().unwrap_or(0))
                    && state["contexts"][&id]["ready"] == V::Bool(true) {
                    for t in p["trades"].as_array().cloned().unwrap_or_default() {
                        if !t["id"].is_null() && !t["sequence"].is_null() {
                            let mut trade = t.clone();
                            trade["sourceAt"] = t["executedAt"].clone();
                            trade["sequence"] = json!(js_string(&t["sequence"]));
                            trade["price"] = json!(baal_paper::fixed::money(&round_micros(
                                t["price"].as_f64().unwrap_or(f64::NAN))));
                            emit!(state, "trade", json!({"trade": trade}), false, boundary)?;
                        }
                    }
                }
            }
        }
        kind => {
            if !["trade_observed", "summary"].contains(&kind) {
                return Err("Unknown normalized capture kind".into());
            }
        }
    }
    state["sourceEnded"] = json!(state["contexts"].as_object().map(|m| m.values()
        .all(|c| c["sourceEnded"] == V::Bool(true))).unwrap_or(false));
    Ok(json!({"state": state, "records": records, "events": events}))
}
fn regex_lite(topic: &str) -> Option<String> {
    // /^tournament:[a-f0-9-]{36}:market:([1-9]\d*)$/
    let rest = topic.strip_prefix("tournament:")?;
    let (tid, mid) = rest.split_once(":market:")?;
    if tid.len() != 36 || !tid.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase() || b == b'-') {
        return None;
    }
    if mid.is_empty() || mid.starts_with('0') || !mid.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    Some(mid.to_string())
}
/// passiveCaptureReport(state)
pub fn passive_capture_report(state: &V) -> Result<V, String> {
    let mut markets = Vec::new();
    for c in state["contexts"].as_object().cloned().unwrap_or_default().values() {
        markets.push(crate::passive_observer::passive_report(&c["observer"]["state"],
            &c["observer"]["bank"])?);
    }
    Ok(json!({"schemaVersion":1,"mode":"passive_opportunity_measurement",
        "sessionId":state["sessionId"],"cutoff":state["cutoff"],
        "captureIndex":state["captureIndex"],"sourceEnded":state["sourceEnded"],
        "markets":markets,"ordersSubmitted":0,"simulatedFills":V::Null,"pnl":V::Null}))
}
