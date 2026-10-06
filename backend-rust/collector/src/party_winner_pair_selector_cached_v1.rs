// live-paper/party-winner-pair-selector-cached-v1.mjs — one-shot filesystem
// INPUT adapter only. No writes, client, daemon or API.
use baal_service_runtime::contract::ms_to_iso;
use baal_service_runtime::json_text::{js_int, js_stringify, js_truthy, sha256};
use serde_json::{json, Map, Value as V};
use std::fs;
use std::path::{Path, PathBuf};
use crate::breadth::validate_universe;
use crate::party_winner_no_screen_v1::{party_winner_quote_hash, screen_party_winner_no_pairs};
use crate::party_winner_pair_selector_v1::select_party_winner_pair_capture;
use crate::transport_runtime::Journal;

type R<T> = Result<T, String>;
// execution/ledger.mjs payloadHash.
fn payload_hash(v: &V) -> R<String> {
    baal_service_runtime::ledger::payload_hash(v).map_err(|e| e.message)
}
fn js_eq(a: &V, b: &V) -> bool {
    if let (Some(x), Some(y)) = (a.as_f64(), b.as_f64()) { return x == y; }
    a == b
}
// /^[1-9]\d*$/ and Number(s) stays a safe integer.
fn is_id(s: &V) -> bool {
    s.as_str().is_some_and(|x| !x.is_empty()
        && x.bytes().all(|b| b.is_ascii_digit()) && x.as_bytes()[0] != b'0'
        && x.parse::<f64>().ok().is_some_and(|n| n.is_finite()
            && n.fract() == 0.0 && n.abs() <= 9_007_199_254_740_991.0))
}
fn terminal(phase: &V) -> bool {
    ["complete", "failed", "interrupted"].contains(&phase.as_str().unwrap_or(""))
}
// String(v) coercion: 'history:'+v and 'current:'+v concatenate it.
fn js_str(v: &V) -> String {
    match v {
        V::String(s) => s.clone(),
        V::Number(_) | V::Bool(_) => js_stringify(v),
        V::Null => "null".into(),
        V::Array(a) => a.iter()
            .map(|x| match x { V::Null => String::new(), _ => js_str(x) })
            .collect::<Vec<_>>().join(","),
        V::Object(_) => "[object Object]".into(),
    }
}
fn prop<'a>(v: &'a V, k: &str) -> R<Option<&'a V>> {
    match v {
        V::Object(m) => Ok(m.get(k)),
        V::Null => Err(format!("Cannot read properties of null (reading '{}')", k)),
        _ => Ok(None),
    }
}
// `{...v}` own-enumerable spread into a fresh object.
fn spread_map(v: &V) -> Map<String, V> {
    let mut out = Map::new();
    match v {
        V::Object(m) => out.extend(m.iter().map(|(k, x)| (k.clone(), x.clone()))),
        V::Array(a) => out.extend(a.iter().enumerate().map(|(i, x)| (i.to_string(), x.clone()))),
        V::String(s) => out.extend(s.encode_utf16().enumerate()
            .map(|(i, c)| (i.to_string(), json!(String::from_utf16_lossy(&[c]))))),
        _ => {}
    }
    out
}
// Object.values(v) over JSON shapes; the JS TypeError on null/undefined.
fn values_or_err(v: Option<&V>) -> R<Vec<V>> {
    match v {
        None | Some(V::Null) => Err("Cannot convert undefined or null to object".into()),
        Some(V::Object(m)) => Ok(m.values().cloned().collect()),
        Some(V::Array(a)) => Ok(a.clone()),
        Some(V::String(s)) => Ok(s.encode_utf16()
            .map(|c| json!(String::from_utf16_lossy(&[c]))).collect()),
        Some(_) => Ok(Vec::new()),
    }
}
struct ReadVal { data: V, sha256: String }
// read(file): {data:JSON.parse(bytes),sha256:sha(bytes)} — errors propagate.
fn read_file(file: &Path) -> R<ReadVal> {
    let bytes = fs::read(file).map_err(|e| e.to_string())?;
    let data: V = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    Ok(ReadVal { data, sha256: sha256(&bytes) })
}
// optional(file): existsSync?read:null.
fn optional(file: &Path) -> R<Option<ReadVal>> {
    Ok(if file.exists() { Some(read_file(file)?) } else { None })
}
fn is_abs_str(v: &V) -> R<String> {
    match v.as_str() {
        Some(s) => Ok(s.to_string()),
        None => Err(format!("The \"path\" argument must be of type string. Received {}",
            if v.is_null() { "null" } else { "undefined or a non-string value" })),
    }
}

/// signedWholeAccountReadProjection(cache,tournamentId,sourceHash,competition)
/// — the whole body runs inside try/catch returning null on any throw.
pub fn signed_whole_account_read_projection(cache: &V, tournament_id: &V,
    source_hash: &V, competition: &V) -> V {
    projection(cache, tournament_id, source_hash, competition).unwrap_or(V::Null)
}
fn projection(cache: &V, tournament_id: &V, source_hash: &V, competition: &V) -> R<V> {
    // cache.errors?.positions || cache.errors?.orders
    let err_side = |k: &str| -> bool {
        cache.get("errors").and_then(|e| e.get(k)).map(js_truthy_some).unwrap_or(false)
    };
    let pos_rows = cache.get("positions").and_then(|p| p.get("rows")).and_then(|r| r.as_array());
    let ord_rows = cache.get("orders").and_then(|p| p.get("rows")).and_then(|r| r.as_array());
    if err_side("positions") || err_side("orders") || pos_rows.is_none() || ord_rows.is_none() {
        return Ok(V::Null);
    }
    let mut excluded: Vec<String> = Vec::new();
    for p in pos_rows.unwrap() {
        if !(is_id(&p["exchangeId"]) && js_int(&p["quantity"]).is_some()) {
            return Ok(V::Null);
        }
        if js_int(&p["quantity"]) != Some(0) {
            let k = p["exchangeId"].as_str().unwrap_or("").to_string();
            if !excluded.contains(&k) { excluded.push(k); }
        }
    }
    for o in ord_rows.unwrap() {
        if !(is_id(&o["exchangeId"]) && o["open"] == V::Bool(true)
            && js_int(&o["quantity"]).is_some_and(|q| q >= 1))
        {
            return Ok(V::Null);
        }
        let k = o["exchangeId"].as_str().unwrap_or("").to_string();
        if !excluded.contains(&k) { excluded.push(k); }
    }
    let positions_at = cache["positions"]["at"].clone();
    let orders_at = cache["orders"]["at"].clone();
    let known = parse_ms(&positions_at).max(parse_ms(&orders_at));
    if !known.is_finite() { return Ok(V::Null); }
    excluded.sort_by(|a, b| a.parse::<f64>().unwrap_or(f64::NAN)
        .partial_cmp(&b.parse::<f64>().unwrap_or(f64::NAN)).unwrap_or(std::cmp::Ordering::Equal));
    Ok(json!({"complete":true,"tournamentId":tournament_id,"sourceHash":source_hash,
        "excludedExchangeIds":excluded,
        "unknownAcceptance":js_truthy(competition.get("unconfirmedIntents"))
            || js_truthy(competition.get("openIntent")),
        "availableAt":ms_to_iso(known),"positionsAt":positions_at,"ordersAt":orders_at}))
}
fn js_truthy_some(v: &V) -> bool { js_truthy(Some(v)) }
fn js_str_opt(v: Option<&V>) -> String {
    v.map(js_str).unwrap_or_else(|| "undefined".to_string())
}
fn parse_ms(v: &V) -> f64 {
    v.as_str().and_then(baal_service_runtime::contract::parse_iso_ms).unwrap_or(f64::NAN)
}

/// terminalRecord(sourceId,updatedAt,r)
fn terminal_record(source_id: V, updated_at: V, r: Option<&V>) -> R<Option<V>> {
    let r = match r {
        None => return Err("Cannot read properties of undefined (reading 'legs')".into()),
        Some(x) if x.is_null() => {
            return Err("Cannot read properties of null (reading 'legs')".into())
        }
        Some(x) => x,
    };
    // r.legs?.map(l=>l.marketId)||r.marketIds
    let market_ids = match r.get("legs") {
        None | Some(V::Null) => r.get("marketIds").cloned().unwrap_or(V::Null),
        Some(V::Array(a)) => V::Array(a.iter().map(|l| l["marketId"].clone()).collect()),
        Some(_) => return Err("r.legs?.map is not a function".into()),
    };
    let len2 = match &market_ids {
        V::Array(a) => Some(a.len()),
        V::String(s) => Some(s.encode_utf16().count()),
        _ => None,
    };
    if !(terminal(&r["phase"]) && js_truthy(r.get("name"))
        && js_truthy(Some(&market_ids)) && len2 == Some(2))
    {
        return Ok(None);
    }
    Ok(Some(json!({"sourceId":source_id,"updatedAt":updated_at,"name":r["name"],
        "marketIds":market_ids,"phase":r["phase"],
        "positiveConditionalScreen":r["screen"]["positiveConditionalScreen"] == V::Bool(true)})))
}

// add(label,c,sourceHash)
fn add(universe: &V, tid: &str, roots: &mut Vec<V>, source_ids: &mut Vec<V>,
    label: String, c: &V, source_hash: &str) -> R<()> {
    if c.is_null() {
        return Err("Cannot read properties of null (reading 'marketId')".into());
    }
    // c.marketId||c.response?.market_id
    let market_id = if js_truthy(c.get("marketId")) {
        c["marketId"].clone()
    } else {
        match prop(c, "response")? {
            None | Some(V::Null) => V::Null,
            Some(resp) => resp.get("market_id").cloned().unwrap_or(V::Null),
        }
    };
    // c.root||c.response?.root
    let root = if js_truthy(c.get("root")) {
        c["root"].clone()
    } else {
        match prop(c, "response")? {
            None | Some(V::Null) => V::Null,
            Some(resp) => resp.get("root").cloned().unwrap_or(V::Null),
        }
    };
    let m = universe["exchanges"].as_array().cloned().unwrap_or_default().into_iter()
        .find(|e| js_eq(&e["marketId"], &market_id) && e["option"].as_str() == Some("YES"));
    if let Some(m) = m {
        if js_truthy(Some(&root)) && js_truthy(c.get("requestStartedAt"))
            && js_truthy(c.get("receivedAt"))
        {
            roots.push(json!({"marketId":market_id,"exchangeId":m["exchangeId"],
                "tournamentId":tid,"requestStartedAt":c["requestStartedAt"],
                "receivedAt":c["receivedAt"],"knownAt":c["receivedAt"],
                "root":root,"rootHash":payload_hash(&root)?,"sourceHash":source_hash}));
            source_ids.push(json!({"id":label,"sha256":source_hash,
                "marketId":market_id,"receivedAt":c["receivedAt"]}));
        }
    }
    Ok(())
}

// /^\/markets\/(\d+)\/nodes\?tournamentId=([a-f0-9-]+)$/
fn endpoint_match(ep: &str) -> Option<(&str, &str)> {
    let rest = ep.strip_prefix("/markets/")?;
    let (digits, tail) = rest.split_once("/nodes?tournamentId=")?;
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) { return None; }
    if tail.is_empty()
        || !tail.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b) || b == b'-')
    {
        return None;
    }
    Some((digits, tail))
}

/// readOnlyPartyPairSelection({workspaceDir,runtimeDir,sourcePlan,state,
/// boundPlanHash,collectorCanRead,now})
pub fn read_only_party_pair_selection(input: &V) -> R<V> {
    let workspace_dir = is_abs_str(&input["workspaceDir"])?;
    let runtime_dir = is_abs_str(&input["runtimeDir"])?;
    if !(Path::new(&workspace_dir).is_absolute() && Path::new(&runtime_dir).is_absolute()) {
        return Err("PAIR_SELECTOR_ABSOLUTE_ROOTS".into());
    }
    let source_plan = input.get("sourcePlan").cloned().unwrap_or(V::Null);
    let state_in = input.get("state").cloned().unwrap_or(V::Null);
    let bound_plan_hash = input.get("boundPlanHash").cloned().unwrap_or(V::Null);
    let collector_can_read = input.get("collectorCanRead").cloned().unwrap_or(V::Bool(true));
    let now = match input.get("now") {
        None => {
            let ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as f64).unwrap_or(0.0);
            json!(ms_to_iso(ms))
        }
        Some(v) => v.clone(),
    };
    let tid = "bda92870-621e-47b0-bc3c-3602c5c26f55";
    let ws = PathBuf::from(&workspace_dir);
    let rt = PathBuf::from(&runtime_dir);
    let u = read_file(&ws.join("live-paper").join("price-universe.json"))?;
    let universe = validate_universe(&u.data, tid)?;
    let mut roots: Vec<V> = Vec::new();
    let mut source_ids: Vec<V> = Vec::new();

    // data/sources captures
    let sources = ws.join("data").join("sources");
    for entry in fs::read_dir(&sources).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let name = entry.file_name().to_string_lossy().to_string();
        if !name.ends_with(".json") { continue; }
        let r = read_file(&sources.join(&name))?;
        if r.data.is_null() {
            return Err("Cannot read properties of null (reading 'endpoint')".into());
        }
        let ep = if js_truthy(r.data.get("endpoint")) { js_str(&r.data["endpoint"]) } else { String::new() };
        if let Some((digits, mtid)) = endpoint_match(&ep) {
            if mtid == tid {
                // r.data.response?.market_id===match[1]
                let response = prop(&r.data, "response")?;
                let market_id = match response {
                    None | Some(V::Null) => None,
                    Some(x) => x.get("market_id"),
                };
                if market_id.map(|v| js_eq(v, &json!(digits))).unwrap_or(false) {
                    add(&universe, tid, &mut roots, &mut source_ids,
                        format!("captured-nodes/{}", name), &r.data, &r.sha256)?;
                }
            }
        }
    }

    // production-metadata cohorts
    let metadata = rt.join("production-metadata");
    if metadata.exists() {
        for entry in fs::read_dir(&metadata).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            let cohort = entry.file_name().to_string_lossy().to_string();
            let dir = metadata.join(&cohort);
            if !fs::metadata(&dir).map_err(|e| e.to_string())?.is_dir() { continue; }
            for f in fs::read_dir(&dir).map_err(|e| e.to_string())? {
                let f = f.map_err(|e| e.to_string())?;
                let name = f.file_name().to_string_lossy().to_string();
                let stem = name.strip_suffix(".json");
                let digits_ok = stem.map(|s| !s.is_empty()
                    && s.bytes().all(|b| b.is_ascii_digit())).unwrap_or(false);
                if !digits_ok { continue; }
                let r = read_file(&dir.join(&name))?;
                if r.data.is_null() {
                    return Err("Cannot read properties of null (reading 'marketId')".into());
                }
                if js_eq(&r.data["marketId"], &json!(stem.unwrap())) {
                    add(&universe, tid, &mut roots, &mut source_ids,
                        format!("production-metadata/{}/{}", cohort, name), &r.data, &r.sha256)?;
                }
            }
        }
    }

    let captured = optional(&rt.join("paired-candidate-status.json"))?;
    // captured?.data.roots||[] — for..of accepts arrays and iterates strings.
    let mut capture_roots: Vec<V> = Vec::new();
    if let Some(cv) = &captured {
        if cv.data.is_null() {
            return Err("Cannot read properties of null (reading 'roots')".into());
        }
        match cv.data.get("roots") {
            Some(V::Array(a)) => capture_roots = a.clone(),
            Some(V::String(s)) => {
                capture_roots = s.encode_utf16()
                    .map(|c| json!(String::from_utf16_lossy(&[c]))).collect()
            }
            Some(v) if js_truthy(Some(v)) => {
                return Err("captured?.data.roots is not iterable".into())
            }
            _ => {}
        }
        for c in &capture_roots {
            if js_eq(&c["tournamentId"], &json!(tid)) {
                add(&universe, tid, &mut roots, &mut source_ids,
                    "current-paired-capture".to_string(), c, &cv.sha256)?;
            }
        }
    }

    let coverage = read_file(&rt.join("price-coverage.json"))?;
    if coverage.data.is_null() {
        return Err("Cannot read properties of null (reading 'tournamentId')".into());
    }
    if !js_eq(&coverage.data["tournamentId"], &json!(tid))
        || !js_eq(&coverage.data["universeHash"], &universe["hash"])
    {
        return Err("PAIR_SELECTOR_COVERAGE_SCOPE".into());
    }
    let mut quotes: Vec<V> = Vec::new();
    for q in values_or_err(coverage.data.get("quotes"))? {
        let mut v = spread_map(&q);
        v.insert("tournamentId".into(), json!(tid));
        v.insert("sourceHash".into(), json!(coverage.sha256));
        v.insert("knownAt".into(), now.clone());
        let mut v = V::Object(v);
        v["quoteHash"] = json!(party_winner_quote_hash(&v)?);
        quotes.push(v);
    }

    let c = read_file(&rt.join("official-account.json"))?;
    let competition = optional(&rt.join("competition-stats.json"))?;
    let competition_data = competition.as_ref()
        .map(|x| x.data.clone()).filter(|d| js_truthy(Some(d))).unwrap_or(json!({}));
    let account = signed_whole_account_read_projection(&c.data, &json!(tid),
        &json!(c.sha256), &competition_data);

    let current_request = optional(&rt.join("paired-candidate-request.json"))?
        .map(|x| x.data).filter(|d| js_truthy(Some(d))).unwrap_or(V::Null);
    let mut records: Vec<V> = Vec::new();
    let history = rt.join("paired-candidate-history.jsonl");
    if history.exists() {
        Journal::open(&history)?; // `new Journal(history)` verifies the chain.
        for line in fs::read_to_string(&history).map_err(|e| e.to_string())?
            .trim().split('\n').filter(|l| !l.is_empty())
        {
            let r: V = serde_json::from_str(line).map_err(|e| e.to_string())?;
            let ev = match r.get("event") {
                None => {
                    return Err("Cannot read properties of undefined (reading 'result')".into())
                }
                Some(V::Null) => {
                    return Err("Cannot read properties of null (reading 'result')".into())
                }
                Some(x) => x.get("result"),
            };
            if let Some(t) = terminal_record(
                json!(format!("history:{}", js_str_opt(r.get("sequence")))),
                r["at"].clone(), ev)?
            {
                records.push(t);
            }
        }
    }
    if let Some(cv) = &captured {
        if cv.data.is_null() {
            return Err("Cannot read properties of null (reading 'name')".into());
        }
        let at = if js_truthy(cv.data.get("at")) { cv.data["at"].clone() }
            else { cv.data.get("finishedAt").cloned().unwrap_or(V::Null) };
        if let Some(t) = terminal_record(
            json!(format!("current:{}", js_str_opt(cv.data.get("name")))), at, Some(&cv.data))?
        {
            records.push(t);
        }
    }
    let saved = optional(&rt.join("party-winner-pair-selector-state.json"))?;
    let screen = screen_party_winner_no_pairs(&json!({
        "universe":universe,"rootCaptures":roots,"quoteCaptures":quotes,
        "account":account,"now":now}))?;
    let state_arg = if js_truthy(Some(&state_in)) { state_in.clone() }
        else { saved.as_ref().map(|x| x.data.clone()).unwrap_or(V::Null) };
    let selection = select_party_winner_pair_capture(&json!({
        "universe":universe,"screen":screen,"account":account,
        "state":state_arg,"sourcePlan":source_plan,"currentRequest":current_request,
        "capture":captured.as_ref().map(|x| x.data.clone()).filter(|d| js_truthy(Some(d)))
            .unwrap_or(V::Null),
        "terminalRecords":records,"boundPlanHash":bound_plan_hash,
        "collectorCanRead":collector_can_read,"now":now}))?;
    // account?.excludedExchangeIds.length??null — the ?. applies only to
    // account; .length on an undefined member is a TypeError in JS.
    let account_excluded = match &account {
        V::Null => V::Null,
        a => match a.get("excludedExchangeIds") {
            None => {
                return Err("Cannot read properties of undefined (reading 'length')".into())
            }
            Some(V::Array(x)) => json!(x.len()),
            Some(V::String(s)) => json!(s.encode_utf16().count()),
            Some(V::Object(m)) => m.get("length").cloned().unwrap_or(V::Null),
            Some(_) => V::Null,
        },
    };
    // sourceSummary — keys are assigned in this order; a member whose value is
    // undefined (missing) is dropped by JSON.stringify and is omitted here.
    let mut ss = Map::new();
    ss.insert("universeHash".into(), universe["hash"].clone());
    ss.insert("universeSourceSha256".into(), json!(u.sha256));
    ss.insert("rootSources".into(), json!(source_ids));
    ss.insert("quoteCoverageSha256".into(), json!(coverage.sha256));
    if let Some(x) = prop(&coverage.data, "updatedAt")? {
        ss.insert("quoteCoverageUpdatedAt".into(), x.clone());
    }
    ss.insert("accountProjectionSourceHash".into(), json!(c.sha256));
    ss.insert("wholeAccountExcludedExchangeCount".into(), account_excluded);
    ss.insert("currentRequestHash".into(), if js_truthy(Some(&current_request)) {
        json!(payload_hash(&current_request)?)
    } else { V::Null });
    ss.insert("captureSourceHash".into(), captured.as_ref()
        .map(|x| json!(x.sha256.clone()))
        .filter(|s| js_truthy(Some(s))).unwrap_or(V::Null));
    ss.insert("stateSourceHash".into(), saved.as_ref()
        .map(|x| json!(x.sha256.clone()))
        .filter(|s| js_truthy(Some(s))).unwrap_or(V::Null));
    let source_summary = V::Object(ss);
    Ok(json!({"selection":selection,"screen":screen,"sourceSummary":source_summary,
        "networkRequests":0,"competitionOrders":0,"runtimeFilesWritten":0,
        "executionAuthority":false}))
}
