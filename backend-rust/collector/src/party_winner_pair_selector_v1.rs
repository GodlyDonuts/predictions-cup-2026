// live-paper/party-winner-pair-selector-v1.mjs — read REQUEST proposal only.
// The caller owns persistence, source plan and collector.
use baal_paper::fixed::{big_amount, instant};
use baal_service_runtime::contract::ms_to_iso;
use baal_service_runtime::json_text::{js_int, js_stringify, js_truthy};
use num_bigint::BigInt;
use serde_json::{json, Value as V};

// execution/ledger.mjs payloadHash: sorted-key canonical JSON sha256.
fn payload_hash(v: &V) -> Result<String, String> {
    baal_service_runtime::ledger::payload_hash(v).map_err(|e| e.message)
}

/// PARTY_PAIR_SELECTOR_POLICY (frozen object).
pub fn party_pair_selector_policy() -> V {
    json!({"negativeCooldownMs":300000,"requestLifetimeMs":900000,
        "maximumPlanLifetimeMs":1800000,"quoteMaximumAgeMs":20000,
        "maximumSkewMs":10000,"accountMaximumAgeMs":20000,
        "maximumProtectiveCost":"0.990000","protectedBookSlots":4,
        "maximumAuxiliarySlots":2,"maximumRetainedAttempts":512})
}
fn need(ok: bool, c: &str) -> Result<(), String> {
    if !ok { return Err(format!("PARTY_PAIR_SELECTOR_{}", c)); } Ok(())
}
fn is_hex(s: &V) -> bool {
    s.as_str().is_some_and(|x| x.len() == 64
        && x.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)))
}
// /^[1-9]\d*$/ and Number(s) stays a safe integer.
fn is_id(s: &V) -> bool {
    s.as_str().is_some_and(|x| !x.is_empty()
        && x.bytes().all(|b| b.is_ascii_digit()) && x.as_bytes()[0] != b'0'
        && x.parse::<f64>().ok().is_some_and(|n| n.is_finite()
            && n.fract() == 0.0 && n.abs() <= 9_007_199_254_740_991.0))
}
// /^[a-z][a-z0-9-]{1,63}$/
fn is_name(s: &V) -> bool {
    s.as_str().is_some_and(|x| {
        let b = x.as_bytes();
        b.len() >= 2 && b.len() <= 64 && b[0].is_ascii_lowercase()
            && b[1..].iter().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'-')
    })
}
fn terminal(phase: &V) -> bool {
    ["complete", "failed", "interrupted"].contains(&phase.as_str().unwrap_or(""))
}
// JS `===` for JSON values: numbers compare by value, other kinds by shape.
fn js_eq(a: &V, b: &V) -> bool {
    match (a, b) {
        (V::Number(_), V::Number(_)) => a.as_f64().unwrap_or(f64::NAN) == b.as_f64().unwrap_or(f64::NAN),
        _ => a == b,
    }
}

// pairKey(ids): exactly two distinct numeric-id strings, sorted by Number.
fn pair_key(ids: &V) -> Result<String, String> {
    let arr = ids.as_array().cloned().unwrap_or_default();
    need(arr.len() == 2
        && arr[0] != arr[1]
        && arr.iter().all(is_id), "PAIR")?;
    let mut v: Vec<f64> = arr.iter().map(|x| x.as_str().unwrap().parse::<f64>().unwrap_or(f64::NAN)).collect();
    v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    Ok(format!("{}:{}", v[0] as i64, v[1] as i64))
}

/// createPartyPairSelectorState(universeHash)
pub fn create_party_pair_selector_state(universe_hash: &V) -> Result<V, String> {
    need(is_hex(universe_hash), "UNIVERSE_HASH")?;
    Ok(json!({"schemaVersion":1,"universeHash":universe_hash,
        "policyHash":payload_hash(&party_pair_selector_policy())?,
        "nextSequence":0,"attempts":{},"handledTerminals":{},"cooldowns":{},
        "handledSources":{}}))
}
fn validate_state(s: &V, u: &V) -> Result<V, String> {
    let ph = payload_hash(&party_pair_selector_policy())?;
    need(js_int(&s["schemaVersion"]) == Some(1)
        && &s["universeHash"] == u
        && s["policyHash"].as_str() == Some(ph.as_str())
        && js_int(&s["nextSequence"]).is_some_and(|n| n >= 0)
        && ["attempts", "handledTerminals", "cooldowns", "handledSources"].iter()
            .all(|k| s[*k].is_object()),
        "STATE")?;
    Ok(s.clone())
}
fn plan_current(p: &V, universe: &V, now: i64) -> bool {
    (|| -> Result<bool, String> {
        need(js_int(&p["schemaVersion"]) == Some(1)
            && p["owner"].as_str() == Some("Codex")
            && is_name(&p["id"])
            && p["mode"].as_str() == Some("allowed_universe_pair_read_candidates")
            && js_eq(&p["universeHash"], &universe["hash"])
            && js_int(&p["protectedBookSlots"]) == Some(4)
            && js_int(&p["maximumAuxiliarySlots"]) == Some(2),
            "SOURCE_PLAN")?;
        let start = instant(p["createdAt"].as_str().unwrap_or(""))?;
        let end = instant(p["expiresAt"].as_str().unwrap_or(""))?;
        let life = party_pair_selector_policy()["maximumPlanLifetimeMs"].as_i64().unwrap_or(0);
        need(start <= now && now < end && end - start > 0 && end - start <= life,
            "SOURCE_PLAN_WINDOW")?;
        Ok(true)
    })().unwrap_or(false)
}
fn current_account(a: &V, tid: &V, now: i64) -> bool {
    (|| -> Result<bool, String> {
        need(a["complete"].as_bool() == Some(true)
            && js_eq(&a["tournamentId"], tid)
            && a["unknownAcceptance"].as_bool() == Some(false)
            && a["excludedExchangeIds"].as_array()
                .is_some_and(|x| x.iter().all(is_id))
            && is_hex(&a["sourceHash"]),
            "ACCOUNT")?;
        let known = instant(a["availableAt"].as_str().unwrap_or(""))?;
        Ok(known <= now
            && [a["positionsAt"].clone(), a["ordersAt"].clone()].iter()
                .all(|t| instant(t.as_str().unwrap_or(""))
                    .map(|x| x <= known && now - x <= 20000).unwrap_or(false)))
    })().unwrap_or(false)
}
fn fresh_prices(p: &V, now: i64) -> bool {
    (|| -> Result<bool, String> {
        need(js_truthy(p.get("indicativeMathValid"))
            && p["quoteClocks"].as_array().is_some_and(|a| a.len() == 2),
            "QUOTES")?;
        let clocks = p["quoteClocks"].as_array().unwrap();
        let starts: Vec<i64> = clocks.iter()
            .map(|q| instant(q["requestStartedAt"].as_str().unwrap_or("")))
            .collect::<Result<_, _>>()?;
        let ends: Vec<i64> = clocks.iter()
            .map(|q| instant(q["receivedAt"].as_str().unwrap_or("")))
            .collect::<Result<_, _>>()?;
        for (i, q) in clocks.iter().enumerate() {
            let known = instant(q["knownAt"].as_str().unwrap_or(""))?;
            if !(starts[i] <= ends[i] && ends[i] <= known && known <= now
                && now - starts[i] <= 20000) {
                return Ok(false);
            }
        }
        Ok(ends.iter().max().copied().unwrap_or(i64::MIN)
            - starts.iter().min().copied().unwrap_or(i64::MAX) <= 10000)
    })().unwrap_or(false)
}
// A field feeding payloadHash that is absent (JS undefined) makes
// canonicalJson throw the same message.
fn req(v: Option<&V>) -> Result<V, String> {
    v.cloned().ok_or_else(|| "Canonical JSON requires finite JSON values".to_string())
}
// String(v) coercion for the values this module stringifies.
fn js_string(v: &V) -> String {
    match v {
        V::String(s) => s.clone(),
        V::Number(_) | V::Bool(_) => js_stringify(v),
        V::Null => "null".into(),
        V::Array(a) => a.iter()
            .map(|x| match x { V::Null => String::new(), _ => js_string(x) })
            .collect::<Vec<_>>().join(","),
        V::Object(_) => "[object Object]".into(),
    }
}
// s.cooldowns[key]||0 then numeric comparison.
fn num_or_zero(v: &V) -> f64 {
    if !js_truthy(Some(v)) { return 0.0; }
    match v {
        V::Number(_) => v.as_f64().unwrap_or(f64::NAN),
        V::String(s) => s.trim().parse::<f64>().unwrap_or(f64::NAN),
        V::Bool(b) => if *b { 1.0 } else { 0.0 },
        _ => f64::NAN,
    }
}

/// selectPartyWinnerPairCapture({universe,screen,account,state,sourcePlan,
/// currentRequest,capture,terminalRecords,boundPlanHash,collectorCanRead,now})
pub fn select_party_winner_pair_capture(input: &V) -> Result<V, String> {
    let universe = &input["universe"];
    let screen = &input["screen"];
    let account = &input["account"];
    let source_plan = input.get("sourcePlan").cloned().unwrap_or(V::Null);
    let current_request = input.get("currentRequest").cloned().unwrap_or(V::Null);
    let capture = input.get("capture").cloned().unwrap_or(V::Null);
    // terminalRecords=[] applies only when undefined; an explicit null is not
    // iterable in JS and throws.
    let terminal_records: Vec<V> = match input.get("terminalRecords") {
        None => Vec::new(),
        Some(V::Array(a)) => a.clone(),
        // for..of iterates strings by UTF-16 code unit; null/other shapes throw.
        Some(V::String(x)) => x.encode_utf16()
            .map(|c| json!(String::from_utf16_lossy(&[c]))).collect(),
        Some(_) => return Err("terminalRecords is not iterable".into()),
    };
    let bound_plan_hash = input.get("boundPlanHash").cloned().unwrap_or(V::Null);
    // collectorCanRead=true only fills in for undefined; explicit false/null stay falsy.
    let collector_can_read = match input.get("collectorCanRead") {
        None => true,
        Some(v) => js_truthy(Some(v)),
    };
    let now = &input["now"];

    let at = instant(now.as_str().unwrap_or(""))?;
    let mut s = if js_truthy(input.get("state")) {
        input["state"].clone()
    } else {
        create_party_pair_selector_state(&universe["hash"])?
    };
    s = validate_state(&s, &universe["hash"])?;

    // need(...&&instant(screen.evaluatedAt)<=at,'SCREEN'): instant throws raw
    // only once the structural prefix passes.
    let screen_prefix = js_int(&screen["schemaVersion"]) == Some(1)
        && screen["mode"].as_str() == Some("source_bound_party_winner_conditional_no_read_ranking")
        && screen["pairs"].is_array();
    let screen_at = if screen_prefix {
        instant(screen["evaluatedAt"].as_str().unwrap_or(""))?
    } else { i64::MAX };
    need(screen_prefix && screen_at <= at, "SCREEN")?;

    // A terminal source ID can be edited; compare digest as well as update time.
    for t in &terminal_records {
        let t_prefix = t["sourceId"].as_str().is_some_and(|x| {
            let n = x.encode_utf16().count();
            !x.is_empty() && n <= 200
        }) && is_name(&t["name"])
            && terminal(&t["phase"]);
        let upd = if t_prefix {
            instant(t["updatedAt"].as_str().unwrap_or(""))?
        } else { i64::MAX };
        need(t_prefix && upd <= at, "TERMINAL_SOURCE")?;
        let key = pair_key(&t["marketIds"])?;
        let digest = payload_hash(t)?;
        let terminal_key = payload_hash(&json!({"sourceId":t["sourceId"]}))?;
        if s["handledTerminals"][terminal_key.as_str()].as_str() == Some(digest.as_str()) {
            continue;
        }
        s["handledTerminals"][terminal_key.as_str()] = json!(digest);
        let negative = t["phase"].as_str() != Some("complete")
            || t["positiveConditionalScreen"].as_bool() != Some(true);
        if negative {
            let prev = num_or_zero(&s["cooldowns"][key.as_str()]);
            let nv = prev.max(upd as f64 + 300000.0);
            s["cooldowns"][&key] = V::Number(
                serde_json::Number::from_f64(nv).unwrap_or_else(|| serde_json::Number::from(0)));
        }
        let name = t["name"].as_str().unwrap_or("").to_string();
        if s["attempts"].as_object().is_some_and(|m| m.contains_key(&name)) {
            // Object.assign(s.attempts[t.name],{...}) — a primitive target is
            // wrapped in JS and the assign is a silent no-op.
            if let Some(a) = s["attempts"][name.as_str()].as_object_mut() {
                a.insert("phase".into(), t["phase"].clone());
                a.insert("terminalSourceId".into(), t["sourceId"].clone());
                a.insert("terminalDigest".into(), json!(digest));
                a.insert("updatedAt".into(), t["updatedAt"].clone());
            }
        }
    }

    let source_plan_hash = || -> Result<V, String> {
        Ok(if js_truthy(Some(&source_plan)) {
            json!(payload_hash(&source_plan)?)
        } else { V::Null })
    };
    let result = |reason: &str, request: V, selected: V, s: &V| -> Result<V, String> {
        Ok(json!({"schemaVersion":1,"mode":"party_pair_read_request_proposal_only",
            "reason":reason,"request":request,"selected":selected,"nextState":s,
            "sourcePlanHash":source_plan_hash()?,
            "protectedBookSlots":4,"maximumAuxiliarySlots":2,
            "competitionOrders":0,"networkRequests":0,"executionAuthority":false}))
    };

    if !bound_plan_hash.is_null() {
        need(is_hex(&bound_plan_hash), "BOUND_PLAN_HASH")?;
        return result("rest_lifecycle_plan_bound", V::Null, V::Null, &s);
    }
    if js_truthy(Some(&capture))
        && capture["phase"].as_str() != Some("idle")
        && !terminal(&capture["phase"])
    {
        return result("capture_active_no_flip", V::Null, V::Null, &s);
    }
    if js_truthy(Some(&current_request))
        && instant(current_request["expiresAt"].as_str().unwrap_or(""))? > at
        && !(js_truthy(Some(&capture)) && js_eq(&capture["name"], &current_request["name"]))
    {
        return result("pending_request_no_flip", V::Null, V::Null, &s);
    }
    {
        let mut pending_nomination = false;
        if let Some(attempts) = s["attempts"].as_object() {
            for (name, a) in attempts.iter() {
                if a["phase"].as_str() == Some("proposed") {
                    let exp = instant(a["expiresAt"].as_str().unwrap_or(""))?;
                    if exp > at
                        && !(capture["name"].as_str() == Some(name.as_str())
                            && terminal(&capture["phase"]))
                    {
                        pending_nomination = true;
                        break;
                    }
                }
            }
        }
        if pending_nomination {
            return result("persisted_nomination_pending_no_flip", V::Null, V::Null, &s);
        }
    }
    if capture["phase"].as_str() == Some("complete")
        && capture["screen"]["positiveConditionalScreen"].as_bool() == Some(true)
        && js_truthy(capture.get("review"))
        && instant(capture["review"]["expiresAt"].as_str().unwrap_or(""))? > at
    {
        return result("positive_capture_awaiting_owner", V::Null, V::Null, &s);
    }
    if !collector_can_read {
        return result("collector_unavailable_or_backoff", V::Null, V::Null, &s);
    }
    if !current_account(account, &universe["tournamentId"], at) {
        return result("current_account_unavailable", V::Null, V::Null, &s);
    }

    let excluded: std::collections::HashSet<String> = account["excludedExchangeIds"]
        .as_array().cloned().unwrap_or_default().iter()
        .filter_map(|x| x.as_str().map(|s| s.to_string())).collect();
    // universe.exchanges.map(...): non-array throws TypeError in JS.
    let exchanges = match universe["exchanges"].as_array() {
        Some(a) => a.clone(),
        None if universe["exchanges"].is_null() => {
            return Err(format!("Cannot read properties of {} (reading 'map')",
                if universe.get("exchanges").is_some() { "null" } else { "undefined" }));
        }
        None => return Err("universe.exchanges.map is not a function".into()),
    };
    let mut scope: indexmap::IndexMap<String, V> = indexmap::IndexMap::new();
    for e in &exchanges {
        if let Some(mid) = e["marketId"].as_str() {
            scope.insert(mid.to_string(), e["exchangeId"].clone());
        }
    }

    let mut candidates: Vec<(BigInt, V)> = Vec::new();
    for p in screen["pairs"].as_array().cloned().unwrap_or_default() {
        // p.marketIds.length===2 — JS .length: arrays and strings only.
        let market_ids_len = match &p["marketIds"] {
            V::Array(a) => Some(a.len()),
            V::String(s) => Some(s.encode_utf16().count()),
            _ => None,
        };
        if !(p["ordinaryResolutionOnly"].as_bool() == Some(true)
            && p["orderEligible"].as_bool() == Some(false)
            && p["executionAuthority"].as_bool() == Some(false)
            && p["depthKnown"].as_bool() == Some(false)
            && market_ids_len == Some(2))
        {
            continue;
        }
        // p.exchangeIds.every(...): non-array throws TypeError in JS.
        let exchange_ids = match p["exchangeIds"].as_array() {
            Some(a) => a.clone(),
            None if p["exchangeIds"].is_null() => {
                return Err(format!("Cannot read properties of {} (reading 'every')",
                    if p.get("exchangeIds").is_some() { "null" } else { "undefined" }));
            }
            None => return Err("p.exchangeIds.every is not a function".into()),
        };
        // p.exchangeIds.every((x,i)=>scope.get(p.marketIds[i])===x&&!excluded.has(x))
        // p.marketIds[i]: array element, string char, or undefined elsewhere.
        let js_index = |v: &V, i: usize| -> V {
            match v {
                V::Array(a) => a.get(i).cloned().unwrap_or(V::Null),
                V::String(s) => s.encode_utf16().nth(i)
                    .map(|c| json!(String::from_utf16_lossy(&[c]))).unwrap_or(V::Null),
                _ => V::Null,
            }
        };
        let scoped = exchange_ids.iter().enumerate().all(|(i, x)| {
            let mv = js_index(&p["marketIds"], i);
            let m = scope.get(mv.as_str().unwrap_or(""));
            m.map(|mx| js_eq(mx, x)).unwrap_or(false)
                && !(x.as_str().is_some_and(|s| excluded.contains(s)))
        });
        if !scoped { continue; }
        if !fresh_prices(&p, at) { continue; }
        // p.protectiveTickCost!==null: a missing key (undefined) also passes !==null.
        if !(p.get("protectiveTickCost").map(|v| !v.is_null()).unwrap_or(true)) { continue; }
        let cost = big_amount(p["protectiveTickCost"].as_str().unwrap_or(""))?;
        if !(cost > BigInt::from(0) && cost <= BigInt::from(990_000)) { continue; }
        candidates.push((cost, p));
    }
    // a.id.localeCompare(b.id): non-string a.id throws TypeError in JS.
    for (_, p) in &candidates {
        if !p["id"].is_string() {
            return Err("a.id.localeCompare is not a function".into());
        }
    }
    // Stable sort by amount then localeCompare — UTF-16 code-unit order is the
    // exact order localeCompare gives on ASCII ids (non-ASCII could differ).
    candidates.sort_by(|(ca, a), (cb, b)| {
        ca.cmp(cb).then_with(|| {
            // b.id is coerced by localeCompare's String() on the argument.
            let bkey = if b.get("id").is_none() {
                "undefined".to_string()
            } else { js_string(&b["id"]) };
            a["id"].as_str().unwrap_or("").encode_utf16()
                .cmp(bkey.encode_utf16())
        })
    });

    for (_cost, p) in &candidates {
        let key = pair_key(&p["marketIds"])?;
        if num_or_zero(&s["cooldowns"][&key]) > at as f64 { continue; }
        // {eventKey:p.eventKey,...}: a missing field hashes undefined inside
        // canonicalJson in JS — it throws 'Canonical JSON requires finite JSON
        // values' rather than producing a hash.
        let fingerprint = payload_hash(&json!({
            "eventKey":req(p.get("eventKey"))?,
            "rootHashes":req(p.get("rootHashes"))?,
            "quoteClocks":p["quoteClocks"].as_array().cloned().unwrap_or_default()
                .iter().map(|q| Ok(json!({"requestStartedAt":req(q.get("requestStartedAt"))?,
                    "receivedAt":req(q.get("receivedAt"))?}))).collect::<Result<Vec<V>, String>>()?,
            "rawNoAsks":req(p.get("rawNoAsks"))?,
            "protectiveTickCost":req(p.get("protectiveTickCost"))?}))?;
        if js_truthy(s["handledSources"].get(fingerprint.as_str())) { continue; }
        let selected = json!({
            "pairId":p["id"],"marketIds":p["marketIds"],"exchangeIds":p["exchangeIds"],
            "protectiveTickCost":p["protectiveTickCost"],
            "conditionalOrdinarySurplus":p["conditionalProtectiveSurplus"],
            "rootsCurrent":p["rootsCurrent"],
            "metadataRefreshRequired":!js_truthy(p.get("rootsCurrent")),
            "depthUnknown":true,
            "sourceFingerprint":fingerprint,
            "accountSourceHash":account["sourceHash"],
            "accountAfterCachedBulkAllowedForReadHintOnly":true,
            "accountBeforeFutureBookPacketRequired":true,
            "orderEligible":false});
        if !plan_current(&source_plan, universe, at) {
            return result("bounded_source_plan_required_or_expired", V::Null, selected, &s);
        }
        let sequence = js_int(&s["nextSequence"]).unwrap_or(0) + 1;
        s["nextSequence"] = json!(sequence);
        let sph = payload_hash(&source_plan)?;
        let request_name = format!("paired-screen-{}",
            &payload_hash(&json!({"sourcePlanHash":sph,"fingerprint":fingerprint,
                "sequence":sequence}))?[..40]);
        let expires = (at + 900_000).min(instant(source_plan["expiresAt"].as_str().unwrap_or(""))?);
        let request = json!({"schemaVersion":1,"owner":"Codex","name":request_name,
            "marketIds":p["marketIds"].clone(),"createdAt":now,
            "expiresAt":ms_to_iso(expires as f64)});
        let expires_at_str = request["expiresAt"].as_str().unwrap_or("").to_string();
        let mut attempts = s["attempts"].as_object().cloned().unwrap_or_default();
        attempts.insert(request_name.clone(), json!({"sequence":sequence,
            "pairKey":key,"sourceFingerprint":fingerprint,"sourcePlanHash":sph,
            "createdAt":now,"expiresAt":expires_at_str,"phase":"proposed"}));
        s["attempts"] = V::Object(attempts);
        s["handledSources"][fingerprint.as_str()] = json!(request_name);
        let mut names: Vec<String> = s["attempts"].as_object().unwrap()
            .keys().cloned().collect();
        names.sort_by(|a, b| {
            js_int(&s["attempts"][a.as_str()]["sequence"]).unwrap_or(0)
                .cmp(&js_int(&s["attempts"][b.as_str()]["sequence"]).unwrap_or(0))
        });
        let drop_count = names.len().saturating_sub(512);
        for old in names.iter().take(drop_count) {
            // delete s.handledSources[s.attempts[old].sourceFingerprint]
            let fp = s["attempts"][old.as_str()]["sourceFingerprint"]
                .as_str().map(|x| x.to_string());
            if let Some(fp) = fp {
                if let Some(hs) = s["handledSources"].as_object_mut() {
                    hs.shift_remove(&fp);
                }
            }
            if let Some(att) = s["attempts"].as_object_mut() {
                att.shift_remove(old);
            }
        }
        return result("propose_one_metadata_then_depth_capture", request, selected, &s);
    }
    result(if candidates.is_empty() {
        "no_current_unowned_positive_pair"
    } else {
        "positive_candidates_cooling_down_or_deduplicated"
    }, V::Null, V::Null, &s)
}
