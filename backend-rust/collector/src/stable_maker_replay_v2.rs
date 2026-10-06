// live-paper/stable-maker-replay-v2.mjs — sealed-prefix chained-journal replay sources.
use baal_service_runtime::contract::parse_iso_ms;
use baal_service_runtime::json_text::{digest, js_stringify, js_truthy, sha256};
use serde_json::{json, Value as V};
use std::fs;
use std::path::{Path, PathBuf};

fn need(ok: bool, code: &str) -> Result<(), String> {
    if !ok { return Err(format!("STABLE_MAKER_REPLAY_{}", code)); }
    Ok(())
}
pub(crate) fn sha(b: &[u8]) -> String { sha256(b) }
// JS Date.parse: lenient; unparseable -> NaN, and every NaN comparison is false.
pub(crate) fn dp(v: &V) -> f64 {
    v.as_str().and_then(parse_iso_ms).unwrap_or(f64::NAN)
}
// JS String(x) coercion used in `+` concatenation keys.
pub(crate) fn js_string(v: &V) -> String {
    match v {
        V::String(s) => s.clone(),
        V::Number(_) | V::Bool(_) | V::Null => js_stringify(v),
        V::Array(a) => a.iter().map(js_string).collect::<Vec<_>>().join(","),
        V::Object(_) => "[object Object]".into(),
    }
}
/// Strict === with an `undefined` notion: None (missing key) counts as undefined and
/// is === only to another missing value — never to null or anything else.
pub(crate) fn js_strict_eq_opt(a: Option<&V>, b: Option<&V>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(x), Some(y)) => js_strict_eq(x, y),
        _ => false,
    }
}
pub(crate) fn js_strict_eq(a: &V, b: &V) -> bool {
    match (a, b) {
        (V::Number(x), V::Number(y)) => x.as_f64() == y.as_f64(),
        _ => a == b,
    }
}
/// The TypeError JS raises for `x.prop` when x is null/undefined.
pub(crate) fn prop_read_err(base: Option<&V>, prop: &str) -> String {
    format!("Cannot read properties of {} (reading '{}')",
        match base { None => "undefined", _ => "null" }, prop)
}
/// Map/Set key for raw JS values: SameValueZero, so undefined (missing) is distinct
/// from null and from the string "undefined".
fn set_key(v: Option<&V>) -> String {
    match v {
        None => "\u{0}undefined".to_string(),
        Some(x) => js_stringify(x),
    }
}
/// Insert only when JS would keep the key (undefined-valued keys are dropped by
/// JSON.stringify and also change `in`/spread behavior downstream).
fn insert_defined(m: &mut serde_json::Map<String, V>, k: &str, v: Option<V>) {
    if let Some(v) = v { m.insert(k.to_string(), v); }
}
pub(crate) fn uuid_v4() -> String {
    let mut b = [0u8; 16];
    let mut seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64).unwrap_or(0) ^ std::process::id() as u64;
    for byte in &mut b { seed ^= seed << 13; seed ^= seed >> 7; seed ^= seed << 17; *byte = seed as u8; }
    b[6] = (b[6] & 0x0f) | 0x40; b[8] = (b[8] & 0x3f) | 0x80;
    format!("{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7], b[8], b[9], b[10], b[11], b[12], b[13], b[14], b[15])
}
pub(crate) fn now_iso() -> String {
    let ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as f64).unwrap_or(0.0);
    baal_service_runtime::contract::ms_to_iso(ms)
}
// fs.writeFileSync(out, data, {flag:'wx', mode:0o600})
pub(crate) fn write_private_new(path: &Path, body: &[u8]) -> Result<(), String> {
    use std::io::Write;
    let mut o = fs::OpenOptions::new();
    o.write(true).create_new(true);
    #[cfg(unix)] { use std::os::unix::fs::OpenOptionsExt; o.mode(0o600); }
    let mut f = o.open(path).map_err(|e| e.to_string())?;
    f.write_all(body).map_err(|e| e.to_string())
}
pub(crate) fn mkdir_private(path: &Path) -> Result<(), String> {
    fs::create_dir_all(path).map_err(|e| e.to_string())?;
    #[cfg(unix)] { use std::os::unix::fs::PermissionsExt; let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o700)); }
    Ok(())
}
// JS String.prototype.trimEnd — White_Space plus \u{FEFF}.
fn js_trim_end(s: &str) -> &str { s.trim_end_matches(|c: char| c.is_whitespace() || c == '\u{feff}') }
// v must be a JS array; else the .map call itself throws.
fn require_iterable<'a>(v: Option<&'a V>, name: &str) -> Result<&'a Vec<V>, String> {
    match v {
        None => Err(prop_read_err(None, "map")),
        Some(V::Null) => Err(prop_read_err(Some(&V::Null), "map")),
        Some(x) => x.as_array().ok_or_else(|| format!("{}.map is not a function", name)),
    }
}
fn require_for_of<'a>(v: Option<&'a V>, name: &str) -> Result<&'a Vec<V>, String> {
    match v.and_then(|x| x.as_array()) {
        Some(a) => Ok(a),
        None => Err(format!("{} is not iterable", name)),
    }
}
// JSON.stringify integer-index key promotion (numeric keys enumerate first).
fn index_key(k: &str) -> Option<u32> {
    let v = k.parse::<u32>().ok()?;
    if v < u32::MAX && v.to_string() == k { Some(v) } else { None }
}
/// JSON.stringify(v, null, 2): 2-space indent, JS object key order, and JS number
/// spelling via js_stringify for every scalar leaf.
pub(crate) fn js_pretty(v: &V) -> String {
    fn go(v: &V, indent: usize) -> String {
        match v {
            V::Object(m) => {
                if m.is_empty() { return "{}".into(); }
                let mut entries: Vec<(&String, &V)> = m.iter().collect();
                entries.sort_by(|(a, _), (b, _)| match (index_key(a), index_key(b)) {
                    (Some(a), Some(b)) => a.cmp(&b),
                    (Some(_), None) => std::cmp::Ordering::Less,
                    (None, Some(_)) => std::cmp::Ordering::Greater,
                    _ => std::cmp::Ordering::Equal,
                });
                let pad = "  ".repeat(indent + 1);
                let inner: Vec<String> = entries.iter()
                    .map(|(k, val)| format!("{}{}: {}", pad,
                        serde_json::to_string(k).unwrap_or_default(), go(val, indent + 1)))
                    .collect();
                format!("{{\n{}\n{}}}", inner.join(",\n"), "  ".repeat(indent))
            }
            V::Array(a) => {
                if a.is_empty() { return "[]".into(); }
                let pad = "  ".repeat(indent + 1);
                let inner: Vec<String> = a.iter()
                    .map(|x| format!("{}{}", pad, go(x, indent + 1))).collect();
                format!("[\n{}\n{}]", inner.join(",\n"), "  ".repeat(indent))
            }
            _ => js_stringify(v),
        }
    }
    go(v, 0)
}

/// verifyChainedRows(bytes) — hash-chained journal rows with sequence/clock checks.
pub fn verify_chained_rows(bytes: &[u8]) -> Result<Vec<V>, String> {
    need(bytes.is_empty() || bytes.last() == Some(&10u8), "PARTIAL_RECORD")?;
    let text = js_trim_end(std::str::from_utf8(bytes).unwrap_or(""));
    let mut previous = V::String(String::new());
    let mut sequence = 0i64;
    let mut at: Option<f64> = None;
    let mut rows = Vec::new();
    for l in text.split('\n').filter(|l| !l.is_empty()) {
        let row: V = serde_json::from_str(l).map_err(|e| e.to_string())?;
        let mut body = row.clone();
        let hash = body.as_object_mut()
            .and_then(|m| m.shift_remove("hash"))
            .unwrap_or(V::Null);
        sequence += 1;
        need(row.get("sequence").and_then(|s| s.as_f64()) == Some(sequence as f64)
            && js_strict_eq_opt(row.get("previous"), Some(&previous))
            && js_strict_eq_opt(row.get("hash"), Some(&V::String(digest(&body)))), "CHAIN")?;
        let parsed = row["at"].as_str().and_then(parse_iso_ms);
        need(parsed.is_some() && (at.is_none() || parsed.unwrap() >= at.unwrap()), "CLOCK")?;
        at = parsed;
        previous = hash;
        rows.push(row);
    }
    Ok(rows)
}

/// loadStableMakerSources({runtimeDir, workingDir, executable, binaryHash})
pub fn load_stable_maker_sources(runtime_dir: &Path, working_dir: &Path, executable: &Path,
    binary_hash: &str) -> Result<V, String> {
    mkdir_private(working_dir)?;
    let names = [("books", "public-books.jsonl"), ("eligibility", "account-eligibility.jsonl"),
        ("trades", "anonymous-trades.jsonl")];
    let mut buffers: Vec<(&str, Vec<u8>)> = Vec::new();
    let mut bindings = serde_json::Map::new();
    for (name, file) in names {
        let target = runtime_dir.join(file);
        let prefix = crate::native_dip_host_v1::sealed_native_prefix(&target)?;
        let data = fs::read(&target).map_err(|e| e.to_string())?;
        let bytes = &data[..(prefix.bytes as usize).min(data.len())];
        need(bytes.len() as u64 == prefix.bytes, "TRUNCATED")?;
        bindings.insert(name.to_string(), json!({"bytes": prefix.bytes, "hash": sha(bytes),
            "dev": prefix.dev, "ino": prefix.ino}));
        buffers.push((name, bytes.to_vec()));
    }
    let books_bytes = &buffers[0].1;
    let books_text = js_trim_end(std::str::from_utf8(books_bytes).unwrap_or(""));
    // JS .split('\n').map(JSON.parse) — no empty-line filter; a blank line throws.
    let books: Vec<V> = books_text.split('\n')
        .map(|l| serde_json::from_str::<V>(l).map_err(|e| e.to_string()))
        .collect::<Result<_, _>>()?;
    let scope = books.first().map(|b| b["payload"]["scope"].clone()).unwrap_or(V::Null);
    let scope_ok = match books.first().and_then(|b| b.get("payload"))
        .and_then(|p| p.get("scope")).and_then(|s| s.get("exchanges")) {
        Some(V::Array(a)) => !a.is_empty(),
        _ => false,
    };
    need(books.first().and_then(|b| b["kind"].as_str()) == Some("session_start")
        && scope_ok, "SCOPE")?;
    let scope_file = working_dir.join("scope.json");
    write_private_new(&scope_file, format!("{}\n", js_stringify(&scope)).as_bytes())?;
    let verified = crate::rust_backend::read_native_publication_prefix(
        &runtime_dir.join("public-books.jsonl"), &scope_file,
        bindings["books"]["bytes"].as_u64().unwrap_or(0),
        bindings["books"]["hash"].as_str().unwrap_or(""),
        &working_dir.join("publication.json"), executable, binary_hash)?;
    need(verified["sourceRecords"].as_f64() == Some(books.len() as f64), "BOOK_RECORDS")?;
    let eligibility = verify_chained_rows(&buffers[1].1)?;
    let trades = verify_chained_rows(&buffers[2].1)?;
    for (name, file) in names {
        let stat = fs::metadata(runtime_dir.join(file)).map_err(|e| e.to_string())?;
        let (dev, ino) = crate::native_dip_host_v1::file_dev_ino(&stat);
        let b = &bindings[name];
        need(dev == b["dev"].as_u64().unwrap_or(0) && ino == b["ino"].as_u64().unwrap_or(0)
            && stat.len() >= b["bytes"].as_u64().unwrap_or(0), "SOURCE_REPLACED")?;
    }
    // {engineHash: verified.engineHash, binaryHash, bookRecords: verified.sourceRecords} —
    // a missing engineHash/sourceRecords is undefined in JS and drops out of the object.
    let mut nv = serde_json::Map::new();
    insert_defined(&mut nv, "engineHash", verified.get("engineHash").cloned());
    nv.insert("binaryHash".into(), json!(binary_hash));
    insert_defined(&mut nv, "bookRecords", verified.get("sourceRecords").cloned());
    Ok(json!({"scope": scope, "books": books, "eligibility": eligibility, "trades": trades,
        "bindings": V::Object(bindings), "nativeVerification": V::Object(nv)}))
}

/// events.sort((a,b)=>Date.parse(a.availableAt)-Date.parse(b.availableAt)||eligibility-first).
/// A NaN diff is falsy in JS, so it falls through to the eligibility tiebreak.
pub(crate) fn sort_events(events: &mut Vec<V>) {
    events.sort_by(|a, b| {
        let d = dp(&a["availableAt"]) - dp(&b["availableAt"]);
        if !d.is_nan() && d != 0.0 {
            return if d < 0.0 { std::cmp::Ordering::Less } else { std::cmp::Ordering::Greater };
        }
        if a["kind"].as_str() == Some("eligibility") { std::cmp::Ordering::Less }
        else if b["kind"].as_str() == Some("eligibility") { std::cmp::Ordering::Greater }
        else { std::cmp::Ordering::Equal }
    });
}

/// stableMakerEvents(sources, bank) — flatten chained sources into one sorted event stream.
pub fn stable_maker_events(sources: &V, bank: &V) -> Result<Vec<V>, String> {
    // new Map(scope.exchanges.map(x=>[x.marketId,x.exchangeId])) — SameValueZero keys.
    let scope_exchanges = match sources.get("scope") {
        None => return Err(prop_read_err(None, "exchanges")),
        Some(V::Null) => return Err(prop_read_err(Some(&V::Null), "exchanges")),
        Some(s) => s.get("exchanges"),
    };
    let exchanges = require_iterable(scope_exchanges, "sources.scope.exchanges")?;
    let mut mapping: std::collections::HashMap<String, V> = std::collections::HashMap::new();
    for x in exchanges {
        mapping.insert(set_key(x.get("marketId")), x.get("exchangeId").cloned().unwrap_or(V::Null));
    }
    let legs = require_iterable(bank.get("legs"), "bank.legs")?;
    let scope: std::collections::HashSet<String> =
        legs.iter().map(|l| set_key(l.get("exchangeId"))).collect();
    let mut events: Vec<V> = Vec::new();
    for row in require_for_of(sources.get("eligibility"), "sources.eligibility")? {
        // row.event?.type — optional chain: missing event fails the need, not a TypeError.
        need(row.get("event").and_then(|e| e.get("type"))
            .and_then(|t| t.as_str()) == Some("market_account_eligibility"), "ELIGIBILITY")?;
        // a = row.event?.snapshot; a.complete reads through null/undefined -> TypeError.
        let a = match row["event"].get("snapshot") {
            v @ (None | Some(V::Null)) => return Err(prop_read_err(v, "complete")),
            Some(v) => v,
        };
        need(a["complete"].as_bool() == Some(true)
            && js_strict_eq_opt(a.get("tournamentId"), bank.get("tournamentId")), "ELIGIBILITY")?;
        let excluded = match a.get("excludedMarketIds") {
            v @ (None | Some(V::Null)) => return Err(prop_read_err(v, "every")),
            Some(v) => v.as_array()
                .ok_or_else(|| "a.excludedMarketIds.every is not a function".to_string())?,
        };
        need(excluded.iter().all(|id| mapping.contains_key(&set_key(Some(id)))), "ELIGIBILITY")?;
        let excluded_exchange_ids: Vec<V> = excluded.iter()
            .map(|id| mapping[&set_key(Some(id))].clone()).collect();
        let mut ev = serde_json::Map::new();
        ev.insert("kind".into(), json!("eligibility"));
        insert_defined(&mut ev, "availableAt", row.get("at").cloned());
        let mut snap = serde_json::Map::new();
        snap.insert("schemaVersion".into(), json!(1));
        insert_defined(&mut snap, "positionsAt", a.get("positionsAt").cloned());
        insert_defined(&mut snap, "ordersAt", a.get("ordersAt").cloned());
        snap.insert("excludedExchangeIds".into(), V::Array(excluded_exchange_ids));
        ev.insert("snapshot".into(), V::Object(snap));
        events.push(V::Object(ev));
    }
    for row in require_for_of(sources.get("books"), "sources.books")? {
        let kind = row["kind"].as_str().unwrap_or("");
        if kind == "book" {
            let pl = match row.get("payload") {
                v @ (None | Some(V::Null)) => return Err(prop_read_err(v, "book")),
                Some(v) => v,
            };
            let book = match pl.get("book") {
                v @ (None | Some(V::Null)) => return Err(prop_read_err(v, "exchangeId")),
                Some(v) => v,
            };
            if scope.contains(&set_key(book.get("exchangeId"))) {
                let mut m = book.as_object().cloned().unwrap_or_default();
                m.insert("publicationHash".into(), row["hash"].clone());
                m.insert("publicationSequence".into(), row["sequence"].clone());
                let mut ev = serde_json::Map::new();
                ev.insert("kind".into(), json!("book"));
                insert_defined(&mut ev, "availableAt", row.get("at").cloned());
                ev.insert("book".into(), V::Object(m));
                events.push(V::Object(ev));
            }
        } else if kind == "session_start" || kind == "session_stop" {
            let mut ev = serde_json::Map::new();
            ev.insert("kind".into(), json!("boundary"));
            insert_defined(&mut ev, "availableAt", row.get("at").cloned());
            ev.insert("reason".into(), row["kind"].clone());
            events.push(V::Object(ev));
        } else if kind == "invalidation" {
            let pl = match row.get("payload") {
                v @ (None | Some(V::Null)) => return Err(prop_read_err(v, "marketId")),
                Some(v) => v,
            };
            let market_hit = !js_truthy(pl.get("marketId"))
                || legs.iter().any(|l| js_strict_eq_opt(l.get("marketId"), pl.get("marketId")));
            if market_hit {
                let mut ev = serde_json::Map::new();
                ev.insert("kind".into(), json!("boundary"));
                insert_defined(&mut ev, "availableAt", row.get("at").cloned());
                ev.insert("reason".into(), row["kind"].clone());
                events.push(V::Object(ev));
            }
        }
    }
    for row in require_for_of(sources.get("trades"), "sources.trades")? {
        let page = match row.get("event") {
            v @ (None | Some(V::Null)) => return Err(prop_read_err(v, "exchangeId")),
            Some(v) => v,
        };
        if scope.contains(&set_key(page.get("exchangeId"))) {
            need(page.get("type").and_then(|t| t.as_str()) == Some("anonymous_trade_page")
                && js_strict_eq_opt(page.get("tournamentId"), bank.get("tournamentId")),
                "TRADE_SCOPE")?;
            let mut ev = serde_json::Map::new();
            ev.insert("kind".into(), json!("trades"));
            insert_defined(&mut ev, "availableAt", row.get("at").cloned());
            ev.insert("page".into(), page.clone());
            events.push(V::Object(ev));
        }
    }
    sort_events(&mut events);
    Ok(events)
}

/// Newest eligibility snapshot causally available for a book request.
/// `bound_by_request=false`: `a.availableAt <= e.availableAt` (stable + coupled v1).
/// `bound_by_request=true`: `a.availableAt <= request` (coupled bulk v2).
pub(crate) fn causal_account<'a>(accounts: &'a [V], request: f64, event_at: f64, bound_by_request: bool) -> Option<&'a V> {
    accounts.iter().rev().find(|a| {
        let pos = dp(&a["snapshot"]["positionsAt"]);
        let ord = dp(&a["snapshot"]["ordersAt"]);
        let av = dp(&a["availableAt"]);
        pos <= request && ord <= request && av <= (if bound_by_request { request } else { event_at })
    })
}
/// {...e, accountEligibility: causal?.snapshot || null}
pub(crate) fn attach_account(e: &V, causal: Option<&V>) -> V {
    let mut ev = e.clone();
    let snap = causal.and_then(|a| {
        let s = a.get("snapshot");
        if js_truthy(s) { s.cloned() } else { None }
    }).unwrap_or(V::Null);
    if let Some(m) = ev.as_object_mut() { m.insert("accountEligibility".into(), snap); }
    ev
}

/// Shared replay loop used by stable v1/v2 and coupled v1 (same causal rule:
/// `Date.parse(a.availableAt) <= Date.parse(e.availableAt)`).
pub(crate) fn replay_events_impl(events: &[V], bank: &V,
    create: fn(&V) -> Result<V, String>,
    advance: fn(&V, &V, &V) -> Result<V, String>,
    report: fn(&V, &V, &str) -> Result<V, String>) -> Result<V, String> {
    let mut state = create(bank)?;
    let mut accounts: Vec<V> = Vec::new();
    let mut processed = 0i64;
    let cutoff = dp(&bank["cutoff"]);
    for e in events {
        if e["kind"].as_str() == Some("eligibility") { accounts.push(e.clone()); continue; }
        if dp(&e["availableAt"]) < cutoff { continue; }
        if e["kind"].as_str() == Some("book") {
            // e.book.requestStartedAt — TypeError when book is null/undefined.
            match e.get("book") {
                v @ (None | Some(V::Null)) =>
                    return Err(prop_read_err(v, "requestStartedAt")),
                _ => {}
            }
            let request = dp(&e["book"]["requestStartedAt"]);
            let causal = causal_account(&accounts, request, dp(&e["availableAt"]), false).cloned();
            state = advance(&state, &attach_account(e, causal.as_ref()), bank)?;
        } else {
            state = advance(&state, e, bank)?;
        }
        processed += 1;
    }
    Ok(json!({"state": state, "processed": processed,
        "report": report(&state, bank, state["lastAt"].as_str().unwrap_or(""))?}))
}

/// replayStableMakerEvents(events, bank) — over stable-maker-paper-v2.
pub fn replay_stable_maker_events(events: &[V], bank: &V) -> Result<V, String> {
    replay_events_impl(events, bank,
        crate::stable_maker_paper_v2::create_stable_maker_state,
        crate::stable_maker_paper_v2::advance_stable_maker,
        crate::stable_maker_paper_v2::stable_maker_report)
}

/// Shared record prefix: schemaVersion..report in JS literal order; firstAt/through
/// are dropped (not null) when the event list is empty or availableAt is absent.
pub(crate) fn base_record(mode: &str, bank: &V, sources: &V, events: &[V], replay: &V,
    started: std::time::Instant, loaded: std::time::Instant, finished: std::time::Instant) -> serde_json::Map<String, V> {
    let mut record = serde_json::Map::new();
    record.insert("schemaVersion".into(), json!(1));
    record.insert("at".into(), json!(now_iso()));
    record.insert("mode".into(), json!(mode));
    record.insert("bank".into(), bank.clone());
    record.insert("bindings".into(), sources["bindings"].clone());
    record.insert("nativeVerification".into(), sources["nativeVerification"].clone());
    if let Some(first) = events.first().and_then(|f| f.get("availableAt")) {
        record.insert("firstAt".into(), first.clone());
    }
    if let Some(last) = events.last().and_then(|f| f.get("availableAt")) {
        record.insert("through".into(), last.clone());
    }
    record.insert("processed".into(), replay["processed"].clone());
    record.insert("timing".into(), json!({
        "sourceMs": loaded.duration_since(started).as_secs_f64() * 1000.0,
        "replayMs": finished.duration_since(loaded).as_secs_f64() * 1000.0,
        "totalMs": finished.duration_since(started).as_secs_f64() * 1000.0}));
    record.insert("report".into(), replay["report"].clone());
    record
}
/// fs.writeFileSync(out, JSON.stringify(record,null,2)+'\n', {flag:'wx',mode:0o600})
pub(crate) fn write_record(out: &Path, record: serde_json::Map<String, V>) -> Result<V, String> {
    let v = V::Object(record);
    write_private_new(out, format!("{}\n", js_pretty(&v)).as_bytes())?;
    Ok(v)
}

/// runStableMakerReplay({runtimeDir, bank, executable, binaryHash, out, workingDir})
pub fn run_stable_maker_replay(runtime_dir: &Path, bank: &V, executable: &Path, binary_hash: &str,
    out: &Path, working_dir: Option<&Path>) -> Result<V, String> {
    let owned_dir: PathBuf;
    let working_dir = match working_dir {
        Some(w) => w,
        None => { owned_dir = runtime_dir.join("stable-maker-diagnostics").join(uuid_v4()); &owned_dir }
    };
    let started = std::time::Instant::now();
    let sources = load_stable_maker_sources(runtime_dir, working_dir, executable, binary_hash)?;
    let events = stable_maker_events(&sources, bank)?;
    let loaded = std::time::Instant::now();
    let replay = replay_stable_maker_events(&events, bank)?;
    let finished = std::time::Instant::now();
    let mut record = base_record("stable_maker_actual_capture_replay_v2", bank, &sources, &events,
        &replay, started, loaded, finished);
    // Object.fromEntries(Object.entries(wallets).map(([side,w])=>[side,w.fills]))
    let mut fills = serde_json::Map::new();
    for (side, w) in replay["state"]["wallets"].as_object().map(|m| m.iter()).into_iter().flatten() {
        fills.insert(side.clone(), w["fills"].clone());
    }
    record.insert("fills".into(), V::Object(fills));
    record.insert("networkRequests".into(), json!(0));
    record.insert("competitionOrders".into(), json!(0));
    record.insert("fees".into(), json!(0));
    record.insert("profitabilityEstablished".into(), json!(false));
    write_record(out, record)
}
