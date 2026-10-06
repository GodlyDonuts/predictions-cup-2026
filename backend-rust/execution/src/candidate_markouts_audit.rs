// execution/candidate-markouts-audit.mjs — script replaying the decision journal
// and verified public-book publication, then measuring candidate markouts.
// The live-paper/book-publication.mjs reducer is inlined here (pub(crate)) for
// the sibling audit modules; collector's book_publication_v2.rs is not reachable
// from this crate.
use crate::candidate_markouts::measure_candidate_markouts;
use baal_service_runtime::json_text::canonical_hash;
use serde_json::{json, Map, Value as V};
use std::path::Path;

/// live-paper/book-publication.mjs — incremental consumer reducer (replay half).
pub(crate) mod book_publication {
    use super::*;
    use baal_service_runtime::contract::{instant_ns, timestamp_str};
    use baal_service_runtime::fixed::{amount, instant, price};
    use baal_service_runtime::json_text::{canonical_json, js_int};
    const REASONS: [&str; 6] = ["BOOK_READ_FAILED", "INVALID_BOOK", "ACCOUNT_OUTAGE", "STOP", "COLLECTOR_FAILURE", "MANUAL_INVALIDATION"];
    const STOP_REASONS: [&str; 3] = ["STOP", "COLLECTOR_FAILURE", "NORMAL_SHUTDOWN"];
    fn check(ok: bool, code: &str) -> Result<(), String> {
        if ok { Ok(()) } else { Err(code.to_string()) }
    }
    fn id(v: &V) -> bool {
        v.as_str().is_some_and(|s| {
            !s.is_empty() && s.as_bytes()[0] != b'0' && s.bytes().all(|b| b.is_ascii_digit())
                && s.parse::<u64>().map(|n| n <= 9_007_199_254_740_991).unwrap_or(false)
        })
    }
    fn plain(v: &V) -> bool {
        v.as_object().is_some_and(|o| {
            !o.contains_key("__proto__") && !o.contains_key("constructor") && !o.contains_key("prototype")
        })
    }
    fn exact(v: &V, keys: &[&str]) -> bool {
        v.as_object().is_some_and(|o| o.len() == keys.len() && keys.iter().all(|k| o.contains_key(*k)))
            && plain(v)
    }
    fn hash(v: &V) -> String {
        canonical_hash(v).unwrap_or_default()
    }
    fn canonical(v: &V) -> String {
        canonical_json(v).unwrap_or_default()
    }
    fn uuid(s: &str) -> bool {
        let b = s.as_bytes();
        s.len() == 36 && b[8] == b'-' && b[13] == b'-' && b[18] == b'-' && b[23] == b'-'
            && b[..8].iter().chain(b[9..13].iter()).chain(b[14..18].iter()).chain(b[19..23].iter()).chain(b[24..].iter()).all(u8::is_ascii_hexdigit)
    }
    fn session(v: &V) -> bool {
        v.as_str().is_some_and(|s| {
            !s.is_empty() && s.len() <= 128 && s.as_bytes()[0].is_ascii_alphanumeric()
                && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b':' || b == b'_' || b == b'.' || b == b'-')
        })
    }
    fn sequence_ok(v: &V) -> bool {
        v.as_str().is_some_and(|s| {
            (s == "0" || (!s.is_empty() && !s.starts_with('0') && s.bytes().all(|b| b.is_ascii_digit())))
                && s.parse::<u64>().map(|n| n <= 9_007_199_254_740_991).unwrap_or(false)
        })
    }
    pub fn scope_of(universe: &V) -> Result<V, String> {
        check(
            plain(universe)
                && universe.get("tournamentId").and_then(V::as_str).is_some_and(uuid)
                && universe.get("exchanges").and_then(V::as_array).is_some_and(|e| !e.is_empty() && e.len() <= 10000),
            "PUBLIC_BOOK_SCOPE_INVALID",
        )?;
        let mut markets = std::collections::HashSet::new();
        let mut exchanges = std::collections::HashSet::new();
        let mut rows = Vec::new();
        for row in universe["exchanges"].as_array().unwrap() {
            check(
                plain(row)
                    && id(row.get("marketId").unwrap_or(&V::Null))
                    && id(row.get("exchangeId").unwrap_or(&V::Null))
                    && row.get("option").and_then(V::as_str) == Some("YES")
                    && !markets.contains(row["marketId"].as_str().unwrap())
                    && !exchanges.contains(row["exchangeId"].as_str().unwrap()),
                "PUBLIC_BOOK_SCOPE_INVALID",
            )?;
            markets.insert(row["marketId"].as_str().unwrap().to_string());
            exchanges.insert(row["exchangeId"].as_str().unwrap().to_string());
            rows.push(json!({"marketId": row["marketId"], "exchangeId": row["exchangeId"], "option": "YES"}));
        }
        rows.sort_by(|a, b| {
            let (x, y) = (a["marketId"].as_str().unwrap().parse::<f64>().unwrap_or(0.0), b["marketId"].as_str().unwrap().parse::<f64>().unwrap_or(0.0));
            x.partial_cmp(&y).unwrap_or(std::cmp::Ordering::Equal)
        });
        Ok(json!({"tournamentId": universe["tournamentId"], "exchanges": rows}))
    }
    pub fn create_book_publication_state(universe: &V) -> Result<V, String> {
        let scope = scope_of(universe)?;
        Ok(json!({
            "schemaVersion": 1,
            "tournamentId": scope["tournamentId"],
            "scope": scope,
            "scopeHash": hash(&scope),
            "sessionId": V::Null,
            "sessions": [],
            "closed": true,
            "sequence": 0,
            "hash": "",
            "lastAt": V::Null,
            "books": {},
            "sourceVersions": {},
            "invalidations": {"global": V::Null, "markets": {}},
        }))
    }
    fn inst(v: &V) -> Result<i64, String> {
        instant(v.as_str().unwrap_or("\u{0}"))
    }
    fn validate_book(book: &V, state: &V, published_at: &V) -> Result<V, String> {
        let fields = ["tournamentId", "marketId", "exchangeId", "instrument", "receivedAt", "requestStartedAt",
            "sourceAt", "sequence", "versionKey", "bids", "asks", "freshnessPolicy", "sourceEventAgeMs"];
        check(exact(book, &fields), "PUBLIC_BOOK_RECORD_INVALID")?;
        let spec = state["scope"]["exchanges"].as_array().and_then(|rows| {
            rows.iter().find(|r| r.get("marketId") == book.get("marketId"))
        });
        check(
            spec.is_some()
                && spec.unwrap().get("exchangeId") == book.get("exchangeId")
                && book.get("tournamentId") == state.get("tournamentId")
                && book.get("instrument").and_then(V::as_str) == Some(format!("{}:YES", book["exchangeId"].as_str().unwrap_or("")).as_str())
                && book.get("freshnessPolicy").and_then(V::as_str) == Some("authoritative_receipt_v1"),
            "PUBLIC_BOOK_RECORD_INVALID",
        )?;
        let started = inst(book.get("requestStartedAt").unwrap_or(&V::Null))?;
        let received = inst(book.get("receivedAt").unwrap_or(&V::Null))?;
        let published = inst(published_at)?;
        let source = instant_ns(book.get("sourceAt").and_then(V::as_str).unwrap_or("\u{0}")).map_err(|e| e.0)?;
        check(
            started <= received && received <= published && published - received <= 20000 && received - started <= 20000,
            "PUBLIC_BOOK_RECORD_INVALID",
        )?;
        let source_at = book.get("sourceAt").and_then(V::as_str).unwrap_or("");
        let source_ms = baal_service_runtime::contract::parse_iso_ms(source_at).unwrap_or(f64::NAN);
        check(
            timestamp_str(source_at).map(|t| t == source_at).unwrap_or(false)
                && source <= (received as i128 + 2000) * 1_000_000
                && sequence_ok(book.get("sequence").unwrap_or(&V::Null))
                && book.get("versionKey").and_then(V::as_str) == Some(format!("{}:{}", book["sequence"].as_str().unwrap_or(""), source_at).as_str())
                && book.get("sourceEventAgeMs").and_then(V::as_f64) == Some(received as f64 - source_ms),
            "PUBLIC_BOOK_RECORD_INVALID",
        )?;
        for barrier in [
            state["invalidations"]["global"].clone(),
            state["invalidations"]["markets"].get(book["marketId"].as_str().unwrap_or("")).cloned().unwrap_or(V::Null),
        ] {
            if !barrier.is_null() {
                check(started >= inst(barrier.get("at").unwrap_or(&V::Null))?, "PUBLIC_BOOK_REQUEST_PRECEDES_INVALIDATION")?;
            }
        }
        let levels = |rows: &V, descending: bool| -> Result<(), String> {
            let rows = rows.as_array();
            check(rows.is_some_and(|r| !r.is_empty() && r.len() <= 20), "PUBLIC_BOOK_RECORD_INVALID")?;
            let mut previous: Option<i128> = None;
            for row in rows.unwrap() {
                check(
                    exact(row, &["price", "quantity"])
                        && js_int(row.get("quantity").unwrap_or(&V::Null)).is_some_and(|q| q > 0),
                    "PUBLIC_BOOK_RECORD_INVALID",
                )?;
                let p = price(row["price"].as_str().unwrap_or("\u{0}"))?;
                check(previous.is_none() || (if descending { p < previous.unwrap() } else { p > previous.unwrap() }), "PUBLIC_BOOK_RECORD_INVALID")?;
                previous = Some(p);
            }
            Ok(())
        };
        levels(book.get("bids").unwrap_or(&V::Null), true)?;
        levels(book.get("asks").unwrap_or(&V::Null), false)?;
        check(
            amount(book["bids"][0]["price"].as_str().unwrap_or("\u{0}"))? < amount(book["asks"][0]["price"].as_str().unwrap_or("\u{0}"))?,
            "PUBLIC_BOOK_RECORD_INVALID",
        )?;
        let depth_hash = hash(&json!({"bids": book["bids"], "asks": book["asks"]}));
        let prior = state["sourceVersions"].get(book["instrument"].as_str().unwrap_or("")).cloned();
        if let Some(prior) = prior.filter(|p| !p.is_null()) {
            check(
                book["sequence"].as_str().unwrap_or("0").parse::<i128>().unwrap_or(0) >= prior["sequence"].as_str().unwrap_or("0").parse::<i128>().unwrap_or(0)
                    && source >= instant_ns(prior["sourceAt"].as_str().unwrap_or("\u{0}")).map_err(|e| e.0)?
                    && received >= inst(prior.get("receivedAt").unwrap_or(&V::Null))?,
                "PUBLIC_BOOK_SOURCE_ROLLBACK",
            )?;
            check(
                book.get("sequence") != prior.get("sequence")
                    || (book.get("versionKey") == prior.get("versionKey") && depth_hash == prior["depthHash"].as_str().unwrap_or("")),
                "PUBLIC_BOOK_VERSION_CONFLICT",
            )?;
        }
        Ok(json!({
            "sequence": book["sequence"],
            "sourceAt": book["sourceAt"],
            "versionKey": book["versionKey"],
            "receivedAt": book["receivedAt"],
            "depthHash": depth_hash,
        }))
    }
    /// applyBookPublication(state, record) -> next state
    pub fn apply_book_publication(state: &V, record: &V) -> Result<V, String> {
        let state_scope_hash = hash(&scope_of(state.get("scope").unwrap_or(&V::Null))?);
        check(
            state.get("schemaVersion") == Some(&json!(1))
                && state.get("scopeHash").and_then(V::as_str) == Some(state_scope_hash.as_str()),
            "PUBLIC_BOOK_STATE_INVALID",
        )?;
        check(
            exact(record, &["schemaVersion", "kind", "sequence", "previousHash", "sessionId", "tournamentId", "scopeHash", "at", "payload", "hash"]),
            "PUBLIC_BOOK_RECORD_INVALID",
        )?;
        let received_hash = record["hash"].clone();
        let mut body = record.clone();
        body.as_object_mut().unwrap().remove("hash");
        check(
            body["schemaVersion"] == json!(1)
                && js_int(&body["sequence"]).is_some_and(|s| s == state["sequence"].as_i64().unwrap_or(-1) + 1)
                && body["previousHash"].as_str().unwrap_or("\u{0}") == state["hash"].as_str().unwrap_or("")
                && received_hash.as_str().unwrap_or("\u{0}") == hash(&body)
                && body["tournamentId"] == state["tournamentId"]
                && body["scopeHash"] == state["scopeHash"]
                && session(&body["sessionId"]),
            "PUBLIC_BOOK_CHAIN_INVALID",
        )?;
        let at = inst(&body["at"])?;
        check(
            state.get("lastAt") == Some(&V::Null) || at >= inst(&state["lastAt"])?,
            "PUBLIC_BOOK_CLOCK_ROLLBACK",
        )?;
        let mut next = state.clone();
        let kind = body["kind"].as_str().unwrap_or("");
        if kind == "session_start" {
            check(
                exact(&body["payload"], &["scope"])
                    && canonical(&body["payload"]["scope"]) == canonical(&state["scope"])
                    && !state["sessions"].as_array().is_some_and(|s| s.contains(&body["sessionId"])),
                "PUBLIC_BOOK_SESSION_INVALID",
            )?;
            next["sessionId"] = body["sessionId"].clone();
            next["sessions"].as_array_mut().unwrap().push(body["sessionId"].clone());
            next["closed"] = json!(false);
            next["books"] = json!({});
            next["invalidations"] = json!({"global": {"at": body["at"], "reason": "SESSION_START", "sequence": body["sequence"]}, "markets": {}});
        } else {
            check(
                !baal_service_runtime::json_text::js_truthy(state.get("closed")) && body["sessionId"] == state["sessionId"],
                "PUBLIC_BOOK_SESSION_CLOSED",
            )?;
            if kind == "book" {
                check(exact(&body["payload"], &["book"]), "PUBLIC_BOOK_RECORD_INVALID")?;
                let book = &body["payload"]["book"];
                let version = validate_book(book, state, &body["at"])?;
                let instrument = book["instrument"].as_str().unwrap_or("").to_string();
                next["sourceVersions"].as_object_mut().unwrap().insert(instrument.clone(), version);
                let mut stored = book.clone();
                let m = stored.as_object_mut().unwrap();
                m.insert("publicationSequence".into(), body["sequence"].clone());
                m.insert("publicationHash".into(), received_hash.clone());
                m.insert("publicationSessionId".into(), body["sessionId"].clone());
                m.insert("publishedAt".into(), body["at"].clone());
                next["books"].as_object_mut().unwrap().insert(instrument, stored);
            } else if kind == "invalidation" {
                check(
                    exact(&body["payload"], &["marketId", "reason"])
                        && body["payload"]["reason"].as_str().is_some_and(|r| REASONS.contains(&r))
                        && (body["payload"].get("marketId") == Some(&V::Null)
                            || state["scope"]["exchanges"].as_array().is_some_and(|rows| {
                                rows.iter().any(|r| r.get("marketId") == body["payload"].get("marketId"))
                            })),
                    "PUBLIC_BOOK_RECORD_INVALID",
                )?;
                let barrier = json!({"at": body["at"], "reason": body["payload"]["reason"], "sequence": body["sequence"]});
                if body["payload"].get("marketId") == Some(&V::Null) {
                    next["books"] = json!({});
                    next["invalidations"]["global"] = barrier;
                } else {
                    let mid = body["payload"]["marketId"].as_str().unwrap_or("").to_string();
                    next["invalidations"]["markets"].as_object_mut().unwrap().insert(mid.clone(), barrier);
                    let keys: Vec<String> = next["books"]
                        .as_object()
                        .map(|o| {
                            o.iter()
                                .filter(|(_, b)| b.get("marketId") == body["payload"].get("marketId"))
                                .map(|(k, _)| k.clone())
                                .collect()
                        })
                        .unwrap_or_default();
                    for k in keys {
                        next["books"].as_object_mut().unwrap().remove(&k);
                    }
                }
            } else if kind == "session_stop" {
                check(
                    exact(&body["payload"], &["reason"])
                        && body["payload"]["reason"].as_str().is_some_and(|r| STOP_REASONS.contains(&r)),
                    "PUBLIC_BOOK_RECORD_INVALID",
                )?;
                next["closed"] = json!(true);
                next["books"] = json!({});
                next["invalidations"]["global"] = json!({"at": body["at"], "reason": body["payload"]["reason"], "sequence": body["sequence"]});
            } else {
                check(false, "PUBLIC_BOOK_RECORD_INVALID")?;
            }
        }
        next["sequence"] = body["sequence"].clone();
        next["hash"] = received_hash;
        next["lastAt"] = body["at"].clone();
        Ok(next)
    }
}

use book_publication::{apply_book_publication, create_book_publication_state};

/// candidate-markouts-audit.mjs main script body. Returns the console.log line:
/// None when decisions.jsonl is absent (status 'awaiting_decisions').
pub fn candidate_markouts_audit(runtime_dir: &Path, universe: &V, now: i64, report_out: &Path) -> Result<Option<V>, String> {
    let file = runtime_dir.join("competition/decisions.jsonl");
    if !file.exists() {
        return Ok(Some(json!({"status": "awaiting_decisions", "networkRequests": 0})));
    }
    let journal = baal_service_runtime::journal::Journal::open(&file).map_err(|e| {
        e.strip_prefix("JOURNAL_INCOMPLETE_TAIL: ").map(str::to_string).unwrap_or(e)
    })?;
    let text = std::fs::read_to_string(&file).map_err(|e| e.to_string())?;
    let rows: Vec<V> = text
        .trim()
        .split('\n')
        .filter(|l| !l.is_empty())
        .map(|l| serde_json::from_str(l).map_err(|e| e.to_string()))
        .collect::<Result<_, _>>()?;
    // JS rows.map(x => x.event.decision): non-object event -> TypeError.
    let mut decisions = Vec::new();
    for x in &rows {
        match &x["event"] {
            V::Object(_) => decisions.push(x["event"]["decision"].clone()),
            V::Null => return Err("Cannot read properties of null (reading 'decision')".into()),
            _ => return Err("Cannot read properties of undefined (reading 'decision')".into()),
        }
    }
    let markets: std::collections::HashSet<String> = decisions
        .iter()
        .filter_map(|x| x.get("marketId").and_then(V::as_str).map(str::to_string))
        .collect();
    // JS Math.min(...): NaN propagates; empty -> Infinity.
    let mut earliest = f64::INFINITY;
    for x in &decisions {
        let t = baal_service_runtime::contract::parse_iso_ms(x["receivedAt"].as_str().unwrap_or("\u{0}")).unwrap_or(f64::NAN);
        earliest = if earliest.is_nan() || t.is_nan() { f64::NAN } else { earliest.min(t) };
    }
    let mut books: Vec<V> = Vec::new();
    let source = runtime_dir.join("public-books.jsonl");
    let size = std::fs::metadata(&source).map_err(|e| e.to_string())?.len() as usize;
    let mut consumed = 0usize;
    let mut publication = create_book_publication_state(universe)?;
    // readline over complete lines; a trailing unterminated tail is skipped by
    // the consumed+bytes>size guard.
    let raw = std::fs::read(&source).map_err(|e| e.to_string())?;
    let text = String::from_utf8_lossy(&raw);
    for line in text.split('\n').map(|l| l.strip_suffix('\r').unwrap_or(l)) {
        let bytes = line.len() + 1;
        if consumed + bytes > size {
            break;
        }
        let row: V = serde_json::from_str(line).map_err(|e| e.to_string())?;
        publication = apply_book_publication(&publication, &row)?;
        consumed += bytes;
        if row["kind"] == json!("book")
            && markets.contains(row["payload"]["book"]["marketId"].as_str().unwrap_or(""))
            && baal_service_runtime::contract::parse_iso_ms(row["payload"]["book"]["receivedAt"].as_str().unwrap_or("\u{0}")).unwrap_or(f64::NAN) > earliest
            && baal_service_runtime::contract::parse_iso_ms(row["at"].as_str().unwrap_or("\u{0}")).unwrap_or(f64::NAN) <= now as f64
        {
            let mut book = row["payload"]["book"].clone();
            book.as_object_mut().unwrap().insert("tournamentId".into(), row["tournamentId"].clone());
            books.push(book);
        }
    }
    let measured = measure_candidate_markouts(&json!({"decisions": decisions, "books": books, "now": now}))?;
    let mut report = Map::new();
    report.insert("schemaVersion".into(), json!(1));
    report.insert("checkedAt".into(), json!(baal_service_runtime::contract::ms_to_iso(now as f64)));
    report.insert("decisionJournal".into(), json!({"sequence": journal.sequence, "hash": journal.previous}));
    report.insert(
        "bookSource".into(),
        json!({"sequence": publication["sequence"], "hash": publication["hash"], "bytes": consumed}),
    );
    for (k, v) in measured.as_object().unwrap() {
        report.insert(k.clone(), v.clone());
    }
    std::fs::write(report_out, format!("{}\n", serde_json::to_string_pretty(&V::Object(report.clone())).map_err(|e| e.to_string())?))
        .map_err(|e| e.to_string())?;
    let report = V::Object(report);
    Ok(Some(json!({
        "counts": report["counts"],
        "episodes": report["episodes"],
        "networkRequests": 0,
        "profitabilityEstablished": false,
    })))
}
