// live-paper/passive-capture.mjs — normalized realtime capture -> observer events.
// One immutable capture session. observedAt is local availability; receivedAt
// remains source metadata. A buffered earlier receipt never backdates a decision.
use crate::passive_observer::{create_passive_observer, observe_passive, passive_report};
use baal_paper::fixed::{big_price, money};
use baal_service_runtime::contract::{
    book as feed_book, compare_version, config as feed_config, instant_ns, ms_to_iso,
    parse_event, timestamp, Scope,
};
use baal_service_runtime::json_text::{js_int, js_stringify, js_truthy};
use serde_json::{json, Map, Value as V};

fn ns(v: &V) -> Result<i128, String> {
    instant_ns(v.as_str().unwrap_or("")).map_err(|e| e.to_string())
}
fn date_parse(s: &str) -> f64 {
    baal_service_runtime::contract::parse_iso_ms(s).unwrap_or(f64::NAN)
}
/// JS String(number) — shortest f64 spelling.
fn num_str(n: f64) -> String { ryu_js::Buffer::new().format_finite(n).to_string() }
fn truthy(v: &V) -> bool { js_truthy(Some(v)) }

/// createPassiveCapture({scope,sourceHash,cutoff,sessionId})
pub fn create_passive_capture(args: &V) -> Result<V, String> {
    let (scope, source_hash, cutoff, session_id) =
        (&args["scope"], &args["sourceHash"], &args["cutoff"], &args["sessionId"]);
    let session_ok = session_id.as_str().is_some_and(|s| {
        !s.is_empty() && s.len() <= 60
            && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    });
    let markets = scope["markets"].as_array();
    if !session_ok || markets.is_none_or(|m| m.is_empty()) {
        return Err("Invalid passive capture scope".into());
    }
    let mut seen = std::collections::BTreeSet::new();
    let mut exchanges = std::collections::BTreeSet::new();
    let mut contexts = Map::new();
    for market in markets.unwrap() {
        let mut cfg_args = market.clone();
        cfg_args["tournamentId"] = scope["tournamentId"].clone();
        cfg_args["exchangeIds"] = json!([market["exchangeId"]]);
        let checked = feed_config(&cfg_args).map_err(|e| e.to_string())?;
        if seen.contains(&checked.market_id) || exchanges.contains(&checked.exchange_ids[0]) {
            return Err("Duplicate passive capture market".into());
        }
        seen.insert(checked.market_id.clone());
        exchanges.insert(checked.exchange_ids[0].clone());
        let mut obs_args = market.clone();
        obs_args["tournamentId"] = scope["tournamentId"].clone();
        obs_args["sourceHash"] = source_hash.clone();
        obs_args["cutoff"] = cutoff.clone();
        let observer = create_passive_observer(&obs_args)?;
        contexts.insert(checked.market_id.clone(), json!({
            "epoch":0,"revision":V::Null,"ready":false,"snapshot":V::Null,"recovery":V::Null,
            "latestBook":V::Null,"knownExpiry":V::Null,"recoveredAt":V::Null,"staleMs":20000,
            "sourceEnded":false,"observer":observer}));
    }
    Ok(json!({"schemaVersion":1,"sessionId":session_id,"cutoff":cutoff,
        "captureIndex":-1,"lastObservedAt":V::Null,"lastObservedNs":V::Null,
        "sourceEnded":false,"contexts":contexts}))
}
fn to_book(raw: &V, market: &V, available_at: &str, request_started_at: &V) -> Result<V, String> {
    let source_at = timestamp(&raw["asOf"]["at"]).map_err(|e| e.to_string())?;
    let sequence = raw["asOf"]["sequence"].as_i64().unwrap_or(0).to_string();
    // JS String(l.price): the level price is a number -> shortest f64 spelling.
    let level = |l: &V| -> Result<V, String> {
        let pstr = match &l["price"] {
            V::Number(n) => num_str(n.as_f64().unwrap_or(0.0)),
            V::String(s) => s.clone(),
            _ => String::new(),
        };
        Ok(json!({"price":money(&big_price(&pstr)?),"quantity":l["quantity"]}))
    };
    let bids = raw["bids"].as_array().cloned().unwrap_or_default().iter()
        .map(level).collect::<Result<Vec<_>, _>>()?;
    let asks = raw["asks"].as_array().cloned().unwrap_or_default().iter()
        .map(level).collect::<Result<Vec<_>, _>>()?;
    Ok(json!({"marketId":market["bank"]["marketId"],"exchangeId":raw["exchangeId"],
        "instrument":format!("{}:YES",raw["exchangeId"].as_str().unwrap_or("")),
        "receivedAt":available_at,"requestStartedAt":request_started_at,
        "sourceAt":source_at,"sequence":sequence,
        "versionKey":format!("{}:{}",sequence,source_at),
        "bids":bids,"asks":asks}))
}
fn captured_event(row: &V, scope: &Scope) -> Result<V, String> {
    // Capture already removed diagnostic free text and settlement labels. Restore
    // fixed placeholders solely to reuse the public shape/scope validator; never
    // pretend that these placeholders are original wire values.
    let mut payload = row["payload"].clone();
    if row["event"].as_str() == Some("market_batch") {
        let mut d = payload["delivery"].as_object().cloned().unwrap_or_default();
        d.insert("correlationId".into(), json!(""));
        payload["delivery"] = V::Object(d);
        let mut settled = Vec::new();
        if let Some(a) = payload["marketSettled"].as_array() {
            for x in a {
                let mut m = x.as_object().cloned().unwrap_or_default();
                m.insert("settledWith".into(),
                    json!(if x["refund"] == json!(true) { "REFUND" } else { "SETTLED" }));
                settled.push(V::Object(m));
            }
        }
        payload["marketSettled"] = V::Array(settled);
    } else if row["event"].as_str() == Some("market_settled") {
        payload["settledWith"] =
            json!(if payload["refund"] == json!(true) { "REFUND" } else { "SETTLED" });
    }
    parse_event(row["event"].as_str().unwrap_or(""), &payload, scope).map_err(|e| e.to_string())
}
#[allow(clippy::too_many_arguments)]
fn emit(session_id: &str, ctx: &mut V, id: &str, bank: &V, row: &V, at: &str,
    records: &mut Vec<V>, events: &mut Vec<V>, kind: &str, extra: Map<String, V>,
    recovered: bool, previous_revision: i64) -> Result<(), String> {
    // Raw delivery gaps are checked below. Derived records may omit unchanged
    // books, so their revision boundary cannot imply a gap across those rows.
    let prev = previous_revision
        .min(js_int(&ctx["observer"]["state"]["revision"]).unwrap_or(0));
    let mut ev = Map::new();
    ev.insert("index".into(), json!(ctx["observer"]["state"]["index"].as_i64().unwrap_or(0) + 1));
    ev.insert("kind".into(), json!(kind));
    ev.insert("receivedAt".into(), json!(at));
    ev.insert("continuity".into(), json!({
        "epoch":format!("{}-{}",session_id,ctx["epoch"].as_i64().unwrap_or(0)),
        "revision":js_int(&ctx["revision"]).unwrap_or(0),
        "previousRevision":prev,
        "ready":ctx["ready"],"continuous":ctx["ready"],"recovered":recovered}));
    for (k, v) in extra { ev.insert(k, v); }
    let result = observe_passive(&ctx["observer"]["state"], &V::Object(ev), bank)?;
    ctx["observer"]["state"] = result["state"].clone();
    if truthy(&result["record"]) {
        let mut rec = Map::new();
        rec.insert("marketId".into(), json!(id));
        rec.insert("sourceCaptureIndex".into(), row["captureIndex"].clone());
        rec.insert("sourceReceivedAt".into(),
            if row.get("receivedAt").is_some() { row["receivedAt"].clone() } else { V::Null });
        if let V::Object(m) = &result["record"] {
            for (k, v) in m { rec.insert(k.clone(), v.clone()); }
        }
        records.push(V::Object(rec));
    }
    for e in result["events"].as_array().cloned().unwrap_or_default() {
        let mut m = Map::new();
        m.insert("marketId".into(), json!(id));
        if let V::Object(em) = e {
            for (k, v) in em { m.insert(k, v); }
        }
        events.push(V::Object(m));
    }
    Ok(())
}
fn invalidate(ctx: &mut V, session_id: &str, id: &str, bank: &V, row: &V, at: &str,
    records: &mut Vec<V>, events: &mut Vec<V>, reason: &str, clear_recovery: bool) -> Result<(), String> {
    ctx["ready"] = json!(false);
    ctx["recoveredAt"] = V::Null;
    if clear_recovery {
        ctx["snapshot"] = V::Null;
        ctx["recovery"] = V::Null;
    }
    let mut extra = Map::new();
    extra.insert("reason".into(), json!(reason));
    let prev_rev = js_int(&ctx["revision"]).unwrap_or(0);
    emit(session_id, ctx, id, bank, row, at, records, events, "invalidate", extra, false, prev_rev)
}
fn remember(ctx: &mut V, session_id: &str, id: &str, bank: &V, row: &V, at: &str,
    records: &mut Vec<V>, events: &mut Vec<V>, raw: &V, origin: &str) -> Result<bool, String> {
    if raw["asOf"].is_null() {
        return Ok(false);
    }
    let old = ctx["latestBook"].clone();
    let cmp = if old.is_object() {
        compare_version(&raw["asOf"], &old["book"]["asOf"])
    } else {
        1
    };
    if cmp < 0 {
        return Ok(false);
    }
    if cmp == 0
        && js_stringify(&json!([raw["bids"], raw["asks"]]))
            != js_stringify(&json!([old["book"]["bids"], old["book"]["asks"]])) {
        invalidate(ctx, session_id, id, bank, row, at, records, events, "schema", true)?;
        return Ok(false);
    }
    if truthy(&raw["expiryKnown"])
        && (!ctx["knownExpiry"].is_object()
            || compare_version(&raw["asOf"], &ctx["knownExpiry"]["version"]) >= 0) {
        ctx["knownExpiry"] = json!({"at":raw["nextExpiryAt"],"version":raw["asOf"]});
    }
    // Broadcast duplicates cannot refresh a quiet book's receipt age.
    if cmp == 0 && origin == "stream" {
        return Ok(false);
    }
    ctx["latestBook"] = json!({"book":raw,"receivedAt":if truthy(&row["receivedAt"]) { row["receivedAt"].clone() } else { row["observedAt"].clone() },
        "availableAt":at,"epoch":row["epoch"],"captureIndex":row["captureIndex"],"origin":origin});
    Ok(true)
}
fn expired(ctx: &V, at: &str) -> bool {
    !ctx["knownExpiry"]["at"].is_null()
        && date_parse(ctx["knownExpiry"]["at"].as_str().unwrap_or("")) <= date_parse(at)
}
/// consumePassiveCapture(input,row) -> {state,records,events}
pub fn consume_passive_capture(input: &V, row: &V) -> Result<V, String> {
    if row["format"].as_str() != Some("sig-feed-capture")
        || row["schemaVersion"].as_i64() != Some(1)
        || js_int(&row["captureIndex"]).is_none()
        || js_int(&row["captureIndex"]) != Some(js_int(&input["captureIndex"]).unwrap_or(0) + 1)
    {
        return Err("Missing or reordered realtime capture".into());
    }
    let observed_ns = ns(&row["observedAt"])?;
    let received_ns = match row.get("receivedAt") {
        None => observed_ns,
        Some(v) => ns(v)?,
    };
    let observed = (observed_ns / 1_000_000) as f64;
    let received = (received_ns / 1_000_000) as f64;
    if !input["lastObservedNs"].is_null()
        && observed_ns < input["lastObservedNs"].as_str().unwrap_or("0").parse::<i128>().unwrap_or(0)
    {
        return Err("Capture observation clock rollback".into());
    }
    if received_ns > observed_ns + 1_000_000_000i128 {
        return Err("Future capture receipt".into());
    }
    let mut state = input.clone();
    state["captureIndex"] = row["captureIndex"].clone();
    state["lastObservedAt"] = json!(ms_to_iso(observed));
    state["lastObservedNs"] = json!(observed_ns.to_string());
    let topic = row["topic"].as_str().unwrap_or("");
    let id = regex_market_id(topic);
    let ctx_lookup = id.as_deref().unwrap_or("");
    if !state["contexts"][ctx_lookup].is_object() {
        return Err("Unknown capture market".into());
    }
    let mut ctx = state["contexts"][ctx_lookup].clone();
    let bank = ctx["observer"]["bank"].clone();
    let scope = feed_config(&json!({"tournamentId":bank["tournamentId"],"marketId":id.as_deref().unwrap_or(""),
        "exchangeIds":[bank["exchangeId"]]})).map_err(|e| e.to_string())?;
    if row["topic"].as_str() != Some(scope.topic.as_str())
        || js_int(&row["epoch"]).is_none()
        || js_int(&row["epoch"]).unwrap_or(i64::MIN) < js_int(&ctx["epoch"]).unwrap_or(0)
    {
        return Err("Capture scope or epoch rollback".into());
    }
    let at = ms_to_iso(observed.max(received));
    let session_id = state["sessionId"].as_str().unwrap_or("").to_string();
    let mut records: Vec<V> = Vec::new();
    let mut events: Vec<V> = Vec::new();

    if js_int(&row["epoch"]) != js_int(&ctx["epoch"]) {
        ctx["epoch"] = row["epoch"].clone();
        ctx["revision"] = V::Null;
        invalidate(&mut ctx, &session_id, id.as_deref().unwrap_or(""), &bank, row, &at,
            &mut records, &mut events, "disconnect", true)?;
    }
    let id_s = id.clone().unwrap_or_default();
    match row["kind"].as_str().unwrap_or("") {
        "config" => {
            let scope_ok = row["scope"]["topic"].as_str() == Some(scope.topic.as_str())
                && row["scope"]["exchangeIds"].as_array().map(|a| a.len() == 1).unwrap_or(false)
                && row["scope"]["exchangeIds"][0] == bank["exchangeId"];
            if !scope_ok
                || js_int(&row["staleMs"]).is_none_or(|s| s <= 0 || s > 60000) {
                return Err("Capture config mismatch".into());
            }
            ctx["staleMs"] = json!(js_int(&row["staleMs"]).unwrap_or(0)
                .min(js_int(&bank["config"]["maxDataAgeMs"]).unwrap_or(0)));
        }
        k if ["invalidation", "disconnected", "stopped", "rejected", "subscribed"].contains(&k) => {
            let reason = match row["reason"].as_str() {
                Some("SETTLEMENT") => "settlement",
                Some("GAP") => "gap",
                Some("STALE") | Some("EXPIRY") => "stale",
                Some("BOOK_DIRTY") => "book_dirty",
                _ if k == "disconnected" || k == "subscribed" => "disconnect",
                _ => "resync",
            };
            invalidate(&mut ctx, &session_id, &id_s, &bank, row, &at,
                &mut records, &mut events, reason, true)?;
            if k == "stopped" {
                ctx["sourceEnded"] = json!(true);
            }
        }
        "recovery_started" => {
            if js_int(&row["generation"]).is_none_or(|g| g < 0) {
                return Err("Invalid recovery generation".into());
            }
            invalidate(&mut ctx, &session_id, &id_s, &bank, row, &at,
                &mut records, &mut events, "resync", true)?;
            ctx["recovery"] = json!({"generation":row["generation"],
                "captureIndex":row["captureIndex"],"startedAt":at,
                "startedNs":observed_ns.max(received_ns).to_string()});
        }
        "snapshot" => {
            let valid_shape = ctx["recovery"].is_object()
                && truthy(&row["receivedAt"]) && truthy(&row["requestStartedAt"])
                && row["books"].as_array().map(|a| a.len() == 1).unwrap_or(false);
            if !valid_shape {
                invalidate(&mut ctx, &session_id, &id_s, &bank, row, &at,
                    &mut records, &mut events, "schema", true)?;
            } else {
                let request_ns = ns(&row["requestStartedAt"])?;
                let started_ns = ctx["recovery"]["startedNs"].as_str()
                    .unwrap_or("0").parse::<i128>().unwrap_or(0);
                if request_ns < started_ns || request_ns > received_ns
                    || received_ns - request_ns
                        > js_int(&bank["config"]["maxDataAgeMs"]).unwrap_or(0) as i128 * 1_000_000 {
                    invalidate(&mut ctx, &session_id, &id_s, &bank, row, &at,
                        &mut records, &mut events, "schema", true)?;
                } else {
                    let raw = feed_book(&row["books"][0], &scope, true)
                        .map_err(|e| e.to_string())?;
                    ctx["snapshot"] = json!({"book":raw,"receivedAt":row["receivedAt"],
                        "requestStartedAt":ms_to_iso((request_ns / 1_000_000) as f64),
                        "originalRequestStartedAt":row["requestStartedAt"],
                        "captureIndex":row["captureIndex"]});
                }
            }
        }
        "recovery_finished" => {
            let bad = row["booksReady"] != json!(true)
                || !ctx["recovery"].is_object() || !ctx["snapshot"].is_object()
                || row["generation"] != ctx["recovery"]["generation"];
            if bad {
                invalidate(&mut ctx, &session_id, &id_s, &bank, row, &at,
                    &mut records, &mut events, "resync", true)?;
            } else {
                let mut raw = ctx["snapshot"]["book"].clone();
                let mut request = ctx["snapshot"]["requestStartedAt"].clone();
                let mut receipt = ctx["snapshot"]["receivedAt"].clone();
                let pushed = ctx["latestBook"].clone();
                if raw["asOf"].is_object()
                    && pushed["epoch"] == ctx["epoch"]
                    && pushed["origin"].as_str() == Some("stream")
                    && js_int(&pushed["captureIndex"]).unwrap_or(i64::MIN)
                        > js_int(&ctx["recovery"]["captureIndex"]).unwrap_or(i64::MIN)
                    && date_parse(pushed["receivedAt"].as_str().unwrap_or(""))
                        >= date_parse(ctx["recovery"]["startedAt"].as_str().unwrap_or(""))
                    && compare_version(&pushed["book"]["asOf"], &raw["asOf"]) > 0 {
                    raw = pushed["book"].clone();
                    request = json!(ms_to_iso(date_parse(pushed["receivedAt"].as_str().unwrap_or(""))));
                    receipt = pushed["receivedAt"].clone();
                }
                let stale = raw["asOf"].is_null()
                    || raw["bids"].as_array().map(|a| a.is_empty()).unwrap_or(true)
                    || raw["asks"].as_array().map(|a| a.is_empty()).unwrap_or(true)
                    || date_parse(&at) - date_parse(receipt.as_str().unwrap_or(""))
                        >= ctx["staleMs"].as_f64().unwrap_or(0.0)
                    || date_parse(&at) - date_parse(request.as_str().unwrap_or(""))
                        > js_int(&bank["config"]["maxDataAgeMs"]).unwrap_or(0) as f64
                    || expired(&ctx, &at)
                    || (ctx["latestBook"].is_object()
                        && compare_version(&raw["asOf"], &ctx["latestBook"]["book"]["asOf"]) < 0);
                if stale {
                    invalidate(&mut ctx, &session_id, &id_s, &bank, row, &at,
                        &mut records, &mut events, "stale", true)?;
                } else {
                    ctx["ready"] = json!(true);
                    ctx["recoveredAt"] = json!(at);
                    let mut extra = Map::new();
                    extra.insert("book".into(), to_book(&raw, &ctx["observer"], &at, &request)?);
                    let rev_now = js_int(&ctx["revision"]).unwrap_or(0);
                    emit(&session_id, &mut ctx, &id_s, &bank, row, &at,
                        &mut records, &mut events, "book", extra, true, rev_now)?;
                    ctx["latestBook"] = json!({"book":raw,"receivedAt":receipt,
                        "availableAt":at,"epoch":ctx["epoch"],"captureIndex":row["captureIndex"],
                        "origin":"recovery"});
                }
            }
            ctx["snapshot"] = V::Null;
            ctx["recovery"] = V::Null;
        }
        "event" => {
            let parsed = captured_event(row, &scope)?;
            if row["event"].as_str() != Some("market_batch") {
                let reason = if row["event"].as_str() == Some("market_settled") {
                    "settlement"
                } else {
                    "book_dirty"
                };
                invalidate(&mut ctx, &session_id, &id_s, &bank, row, &at,
                    &mut records, &mut events, reason, true)?;
            } else {
                let p = parsed["payload"].clone();
                let d = p["delivery"].clone();
                let previous = js_int(&ctx["revision"]);
                // A callback from before accepted recovery must not become a new
                // causal observation just because its buffered capture row was
                // drained later.
                let pre_recovery = (truthy(&ctx["ready"]) && truthy(&ctx["recoveredAt"])
                    && received < date_parse(ctx["recoveredAt"].as_str().unwrap_or("")))
                    || (ctx["recovery"].is_object()
                        && received < date_parse(ctx["recovery"]["startedAt"].as_str().unwrap_or("")));
                if pre_recovery {
                    events.push(json!({"marketId":id_s,"kind":"ignored_pre_recovery_receipt","at":at}));
                    // JS early return skips the state.sourceEnded update below.
                    state["contexts"][ctx_lookup] = ctx;
                    return Ok(json!({"state":state,"records":records,"events":events}));
                }
                let missing_named_book = {
                    let mut v: Vec<V> = p["trades"].as_array().cloned().unwrap_or_default();
                    v.extend(p["bookDirty"].as_array().cloned().unwrap_or_default());
                    v.iter().any(|t| {
                        !p["books"].as_array().cloned().unwrap_or_default().iter()
                            .any(|b| b["exchangeId"] == t["exchangeId"])
                    })
                };
                let latest_age = date_parse(&at)
                    - date_parse(ctx["latestBook"]["receivedAt"].as_str().unwrap_or(""));
                if truthy(&ctx["ready"])
                    && (latest_age >= ctx["staleMs"].as_f64().unwrap_or(0.0) || expired(&ctx, &at)) {
                    invalidate(&mut ctx, &session_id, &id_s, &bank, row, &at,
                        &mut records, &mut events, "stale", true)?;
                }
                if truthy(&p["resyncRequired"])
                    || !p["marketSettled"].as_array().map(|a| a.is_empty()).unwrap_or(true)
                    || p["trades"].as_array().map(|a| a.iter().any(|t| t["sequence"].is_null())).unwrap_or(false)
                    || missing_named_book
                    || (previous.is_some()
                        && js_int(&d["revision"]).unwrap_or(i64::MIN) > previous.unwrap()
                        && js_int(&d["previousRevision"]).unwrap_or(i64::MIN) > previous.unwrap()) {
                    invalidate(&mut ctx, &session_id, &id_s, &bank, row, &at,
                        &mut records, &mut events, "gap", true)?;
                }
                let new_rev = js_int(&d["revision"]).unwrap_or(0).max(previous.unwrap_or(0));
                ctx["revision"] = json!(new_rev);
                // The first delivery after REST establishes the topic revision
                // baseline; its historical previousRevision says nothing about
                // pre-subscription tape.
                let boundary = match previous {
                    None => 0,
                    Some(prev) => js_int(&d["previousRevision"]).unwrap_or(0)
                        .min(js_int(&ctx["revision"]).unwrap_or(0)),
                };
                for raw in p["books"].as_array().cloned().unwrap_or_default() {
                    if raw["bids"].as_array().map(|a| a.is_empty()).unwrap_or(true)
                        || raw["asks"].as_array().map(|a| a.is_empty()).unwrap_or(true) {
                        remember(&mut ctx, &session_id, &id_s, &bank, row, &at,
                            &mut records, &mut events, &raw, "stream")?;
                        invalidate(&mut ctx, &session_id, &id_s, &bank, row, &at,
                            &mut records, &mut events, "stale", true)?;
                        continue;
                    }
                    if remember(&mut ctx, &session_id, &id_s, &bank, row, &at,
                        &mut records, &mut events, &raw, "stream")?
                        && truthy(&ctx["ready"]) {
                        if expired(&ctx, &at) {
                            invalidate(&mut ctx, &session_id, &id_s, &bank, row, &at,
                                &mut records, &mut events, "stale", true)?;
                        } else {
                            let mut extra = Map::new();
                            extra.insert("book".into(), to_book(&raw, &ctx["observer"], &at,
                                &json!(ms_to_iso(received)))?);
                            emit(&session_id, &mut ctx, &id_s, &bank, row, &at,
                                &mut records, &mut events, "book", extra, false, boundary)?;
                        }
                    }
                }
                if (previous.is_none() || js_int(&d["revision"]).unwrap_or(0) > previous.unwrap_or(0))
                    && truthy(&ctx["ready"]) {
                    for t in p["trades"].as_array().cloned().unwrap_or_default() {
                        if !t["id"].is_null() && !t["sequence"].is_null() {
                            let mut trade = t.as_object().cloned().unwrap_or_default();
                            trade.insert("sourceAt".into(), t["executedAt"].clone());
                            trade.insert("sequence".into(),
                                json!(js_int(&t["sequence"]).unwrap_or(0).to_string()));
                            let px = t["price"].as_f64().unwrap_or(0.0);
                            let micros = (px * 1_000_000.0).round() as i128;
                            trade.insert("price".into(),
                                json!(money(&num_bigint::BigInt::from(micros))));
                            let mut extra = Map::new();
                            extra.insert("trade".into(), V::Object(trade));
                            emit(&session_id, &mut ctx, &id_s, &bank, row, &at,
                                &mut records, &mut events, "trade", extra, false, boundary)?;
                        }
                    }
                }
            }
        }
        "trade_observed" | "summary" => {}
        _ => return Err("Unknown normalized capture kind".into()),
    }
    state["contexts"][ctx_lookup] = ctx;
    let ended = state["contexts"].as_object()
        .map(|m| m.values().all(|c| truthy(&c["sourceEnded"])))
        .unwrap_or(false);
    state["sourceEnded"] = json!(ended);
    Ok(json!({"state":state,"records":records,"events":events}))
}
fn regex_market_id(topic: &str) -> Option<String> {
    // /^tournament:[a-f0-9-]{36}:market:([1-9]\d*)$/
    let rest = topic.strip_prefix("tournament:")?;
    let (tid, mid) = rest.split_once(":market:")?;
    if tid.len() == 36
        && tid.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b) || b == b'-')
        && !mid.is_empty()
        && mid.as_bytes()[0] != b'0'
        && mid.bytes().all(|b| b.is_ascii_digit())
    {
        Some(mid.to_string())
    } else {
        None
    }
}
/// passiveCaptureReport(state)
pub fn passive_capture_report(state: &V) -> Result<V, String> {
    let mut markets = Vec::new();
    if let Some(m) = state["contexts"].as_object() {
        for (_k, c) in m {
            markets.push(passive_report(&c["observer"]["state"], &c["observer"]["bank"])?);
        }
    }
    Ok(json!({"schemaVersion":1,"mode":"passive_opportunity_measurement",
        "sessionId":state["sessionId"],"cutoff":state["cutoff"],
        "captureIndex":state["captureIndex"],"sourceEnded":state["sourceEnded"],
        "markets":markets,"ordersSubmitted":0,"simulatedFills":V::Null,"pnl":V::Null}))
}
