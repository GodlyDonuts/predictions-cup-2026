// execution/reference-quality-audit.mjs — replay of the collector journal to
// prove every attributed entry's reference inputs reconstruct byte-for-byte.
// Pure script in JS; exposed here as reference_quality_audit(dir) returning the
// report value, plus run(dir, out) performing the file write + console line.
use baal_service_runtime::contract::parse_iso_ms;
use baal_service_runtime::fixed::amount;
use baal_service_runtime::history::historical_reference;
use baal_service_runtime::json_text::{digest, js_int, js_truthy};
use baal_service_runtime::market::normalize_book;
use serde_json::{json, Map, Value as V};
use sha2::{Digest, Sha256};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::Path;

fn require_that(x: bool, m: &str) -> Result<(), String> {
    if x { Ok(()) } else { Err(m.to_string()) }
}
fn parse(v: &V) -> f64 {
    v.as_str().and_then(parse_iso_ms).unwrap_or(f64::NAN)
}
fn num(x: f64) -> V {
    if x.is_finite() && x.fract() == 0.0 && x.abs() <= 9.0e15 { json!(x as i64) } else { json!(x) }
}
fn median(values: &[f64]) -> Option<f64> {
    let mut a = values.to_vec();
    a.sort_by(|x, y| x.partial_cmp(y).unwrap_or(std::cmp::Ordering::Equal));
    let n = a.len();
    if n == 0 { None } else if n % 2 == 1 { Some(a[(n - 1) / 2]) } else { Some((a[n / 2 - 1] + a[n / 2]) / 2.0) }
}

// readline-style line iterator: every line including an unterminated tail,
// '\r\n' merged. An empty line triggers EMPTY_SOURCE_LINE at consume time.
fn source_lines(file: &Path) -> Result<Vec<String>, String> {
    let raw = std::fs::read(file).map_err(|e| e.to_string())?;
    let text = String::from_utf8_lossy(&raw);
    let mut lines: Vec<String> = text.split('\n').map(|l| l.strip_suffix('\r').unwrap_or(l).to_string()).collect();
    if lines.last().is_some_and(|l| l.is_empty()) {
        lines.pop();
    }
    Ok(lines)
}
// verified(file): chain + clock verification, yielding (row, prefixHash) per
// line. Laziness matters: callers interleave body processing, so a caller error
// on row N precedes a chain error on row N+1. We keep that ordering by making
// verified_next a per-line step driven from the caller loop.
struct Verified {
    lines: std::vec::IntoIter<String>,
    sequence: i64,
    previous: String,
    clock: f64,
    prefix: Sha256,
}
impl Verified {
    fn open(file: &Path) -> Result<Verified, String> {
        Ok(Verified {
            lines: source_lines(file)?.into_iter(),
            sequence: 0,
            previous: String::new(),
            clock: f64::NEG_INFINITY,
            prefix: Sha256::new(),
        })
    }
    fn next_row(&mut self) -> Result<Option<(V, String)>, String> {
        let Some(line) = self.lines.next() else { return Ok(None) };
        require_that(!line.is_empty(), "EMPTY_SOURCE_LINE")?;
        let row: V = serde_json::from_str(&line).map_err(|e| e.to_string())?;
        let mut body = row.clone();
        let hash = body.as_object_mut().and_then(|b| b.remove("hash")).unwrap_or(V::Null);
        self.sequence += 1;
        require_that(
            js_int(&row["sequence"]) == Some(self.sequence)
                && row["previous"].as_str().unwrap_or("\u{0}") == self.previous
                && hash.as_str().unwrap_or("") == digest(&body),
            "SOURCE_CHAIN_FAILED",
        )?;
        let clock_at = row["event"].get("clockAt").filter(|x| js_truthy(Some(x))).cloned();
        let t = parse(clock_at.as_ref().unwrap_or(&row["at"]));
        require_that(t.is_finite() && t >= self.clock, "SOURCE_CLOCK_FAILED")?;
        self.clock = t;
        self.previous = hash.as_str().unwrap().to_string();
        self.prefix.update(format!("{line}\n").as_bytes());
        let prefix_hash = format!("{:x}", self.prefix.clone().finalize());
        Ok(Some((row, prefix_hash)))
    }
}

// live-paper/reference-quality.mjs historicalReferenceQuality.
fn historical_reference_quality(history: Option<&V>, books: &[V], at: &str, policy: &V) -> Result<V, String> {
    let reference = historical_reference(history, books, at, policy)?;
    let now = parse(&json!(at));
    if !js_truthy(reference.get("ready")) {
        return Ok(json!({"reference": reference, "quality": V::Null}));
    }
    let candles = reference.get("source").and_then(V::as_str) == Some("closed_trade_candle_vwap_median");
    let lookback = policy.get("lookbackMs").and_then(V::as_f64).unwrap_or(0.0);
    let source_rows: Vec<V> = if candles {
        history.map(|h| h["candles"].as_array().cloned().unwrap_or_default()).unwrap_or_default()
    } else {
        books.to_vec()
    };
    let mut points: Vec<(f64, f64)> = Vec::new();
    for x in &source_rows {
        let t = if candles { parse(&x["time"]) } else { parse(&x["at"]) };
        let keep = if candles {
            t + 60000.0 <= now && t >= now - lookback
        } else {
            t < now && parse(&x["receivedAt"]) <= now && t >= now - lookback
        };
        if keep {
            points.push((t + if candles { 60000.0 } else { 0.0 }, x[if candles { "vwap" } else { "mid" }].as_f64().unwrap_or(f64::NAN)));
        }
    }
    if points.iter().any(|(at, price)| !price.is_finite() || *price < 0.0 || *price > 1.0 || !at.is_finite()) {
        return Err("INVALID_REFERENCE_POINTS".into());
    }
    points.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    let mut minutes: Vec<(i64, Vec<f64>)> = Vec::new();
    for (at, price) in &points {
        let minute = (at / 60000.0).floor() as i64;
        match minutes.iter_mut().last() {
            Some((m, ps)) if *m == minute => ps.push(*price),
            _ => minutes.push((minute, vec![*price])),
        }
    }
    minutes.sort_by_key(|(m, _)| *m);
    let balanced: Vec<(i64, f64, i64)> = minutes
        .iter()
        .map(|(m, prices)| (*m, median(prices).unwrap_or(f64::NAN), prices.len() as i64))
        .collect();
    let edge = std::cmp::max(1, (balanced.len() as f64 / 4.0).ceil() as usize);
    let older = median(&balanced[..edge.min(balanced.len())].iter().map(|x| x.1).collect::<Vec<_>>());
    let recent = median(&balanced[balanced.len().saturating_sub(edge)..].iter().map(|x| x.1).collect::<Vec<_>>());
    let max_count = balanced.iter().map(|x| x.2 as f64).fold(f64::NEG_INFINITY, f64::max);
    let last = points.last();
    let Some(&last_point) = last else {
        // JS: points.at(-1).at on an empty array -> TypeError.
        return Err("Cannot read properties of undefined (reading 'at')".into());
    };
    let first = points[0];
    let quality = json!({
        "rawSamples": points.len(),
        "distinctMinutes": balanced.len(),
        "largestMinuteWeight": num(max_count / points.len() as f64),
        "minuteBalancedCenter": median(&balanced.iter().map(|x| x.1).collect::<Vec<_>>()).map_or(V::Null, |v| num(v)),
        "firstQuartileCenter": older.map_or(V::Null, |v| num(v)),
        "lastQuartileCenter": recent.map_or(V::Null, |v| num(v)),
        "recentMinusOlder": num(recent.unwrap_or(0.0) - older.unwrap_or(0.0)),
        "referenceMinusRecent": num(reference["center"].as_f64().unwrap_or(0.0) - recent.unwrap_or(0.0)),
        "lastObservationAgeMs": num(now - last_point.0),
        "spanMs": num(last_point.0 - first.0),
        "interpretation": "Descriptive causal history only; repeated observations are not independent trials and a discount is not a probability of recovery.",
    });
    let mut out = Map::new();
    out.insert("reference".into(), reference);
    out.insert("quality".into(), quality);
    Ok(V::Object(out))
}

// live-paper/dip-shadow.mjs dipSignal.
fn dip_signal(reference: &V, ask: f64, bid: f64, p: &V) -> Result<V, String> {
    if !reference.is_object() {
        return Err(format!("Cannot read properties of {} (reading 'ready')", if reference.is_null() { "null" } else { "undefined" }));
    }
    if !js_truthy(reference.get("ready")) {
        // {admitted:false, reason:reference.reason, ...reference}
        let mut out = Map::new();
        out.insert("admitted".into(), json!(false));
        out.insert("reason".into(), reference.get("reason").cloned().unwrap_or(V::Null));
        if let Some(m) = reference.as_object() {
            for (k, v) in m {
                out.insert(k.clone(), v.clone());
            }
        }
        return Ok(V::Object(out));
    }
    let center = reference["center"].as_f64().unwrap_or(f64::NAN);
    let deviation = reference["robustDeviation"].as_f64().unwrap_or(f64::NAN);
    let discount = center - ask;
    let z = discount / deviation;
    let get = |k: &str| p.get(k).and_then(V::as_f64).unwrap_or(f64::NAN);
    let admitted = ask >= get("minimumPrice")
        && ask <= get("maximumPrice")
        && ask > bid
        && ask - bid <= get("maximumSpread")
        && discount >= get("minimumDip")
        && discount / f64::max(center, 0.001) >= get("minimumRelativeDip")
        && discount <= get("maximumDip")
        && z >= get("minimumZ");
    let mut out = reference.as_object().cloned().unwrap_or_default();
    out.insert("discount".into(), num(discount));
    out.insert("relativeDiscount".into(), num(discount / f64::max(center, 0.001)));
    out.insert("z".into(), num(z));
    out.insert("ask".into(), num(ask));
    out.insert("bid".into(), num(bid));
    out.insert("admitted".into(), json!(admitted));
    out.insert("reason".into(), json!(if admitted { "historical_discount" } else { "discount_spread_or_regime_gate" }));
    Ok(V::Object(out))
}

struct Target {
    intent: V,
    proof: V,
    core_hash: V,
    entry_book: Option<V>,
    reference_inputs: Option<(V, Vec<V>)>,
}

/// reference-quality-audit.mjs script body -> the report value.
pub fn reference_quality_audit(dir: &Path) -> Result<V, String> {
    let bank: V = serde_json::from_str(
        &std::fs::read_to_string(dir.join("dip-responsive-bank.json")).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    let bank_hash = digest(&bank);
    let mut targets: Vec<Target> = Vec::new();
    {
        let mut verified = Verified::open(&dir.join("competition/core-journal.jsonl"))?;
        while let Some((row, _prefix_hash)) = verified.next_row()? {
            if row["event"]["type"] != json!("intent_prepared") || row["event"]["reason"] != json!("historical_discount") {
                continue;
            }
            let intent_key = match row["event"].get("intentId") {
                Some(V::String(s)) => s.clone(),
                Some(V::Number(_)) => baal_service_runtime::json_text::js_stringify(&row["event"]["intentId"]),
                _ => "undefined".to_string(),
            };
            let intent = row["state"]["intents"].get(&intent_key).cloned().unwrap_or(V::Null);
            require_that(
                intent["action"] == json!("buy")
                    && row["event"]["referenceProof"]["bankHash"].as_str().unwrap_or("") == bank_hash,
                "ENTRY_PROOF_FAILED",
            )?;
            targets.push(Target {
                intent,
                proof: row["event"]["referenceProof"].clone(),
                core_hash: row["hash"].clone(),
                entry_book: None,
                reference_inputs: None,
            });
        }
    }
    require_that(!targets.is_empty(), "NO_ATTRIBUTED_ENTRIES")?;
    let mut histories: HashMap<String, V> = HashMap::new();
    let mut book_history: HashMap<String, Vec<V>> = HashMap::new();
    let mut source_sequence = V::Null;
    let mut source_hash = V::Null;
    let wanted: HashSet<i64> = targets.iter().filter_map(|t| js_int(&t.proof["sourceSequence"])).collect();
    let scope: HashMap<String, String> = bank["universe"]["exchanges"]
        .as_array()
        .map(|rows| {
            rows.iter()
                .filter_map(|x| {
                    Some((
                        x.get("marketId").and_then(V::as_str)?.to_string(),
                        x.get("exchangeId").and_then(V::as_str)?.to_string(),
                    ))
                })
                .collect()
        })
        .unwrap_or_default();
    {
        let mut verified = Verified::open(&dir.join("journal.jsonl"))?;
        while let Some((row, prefix_hash)) = verified.next_row()? {
            let e = &row["event"];
            let at = e.get("clockAt").filter(|x| js_truthy(Some(x))).cloned().unwrap_or_else(|| row["at"].clone());
            source_sequence = row["sequence"].clone();
            source_hash = row["hash"].clone();
            if js_int(&row["sequence"]).is_some() && js_int(&row["sequence"]) == js_int(&bank["sourceRecords"]) {
                require_that(prefix_hash == bank["sourceHash"].as_str().unwrap_or(""), "BANK_PREFIX_FAILED")?;
            }
            if e["type"] == json!("market_history") {
                // JS e.history.exchangeId member access: null/undefined throw,
                // other primitives read as undefined.
                let history = &e["history"];
                let raw_key = if history.is_object() {
                    history.get("exchangeId").cloned().unwrap_or(V::Null)
                } else if history.is_null() {
                    return Err("Cannot read properties of null (reading 'exchangeId')".into());
                } else if !e.as_object().is_some_and(|o| o.contains_key("history")) {
                    return Err("Cannot read properties of undefined (reading 'exchangeId')".into());
                } else {
                    V::Null
                };
                let key = match &raw_key {
                    V::String(s) => s.clone(),
                    V::Number(_) => baal_service_runtime::json_text::js_stringify(&raw_key),
                    _ => "undefined".to_string(),
                };
                histories.insert(key, history.clone());
            }
            if e["type"] == json!("cycle") {
                let mut snaps: Vec<V> = Vec::new();
                for field in ["sourceSnapshots", "supplementalDepth"] {
                    match e.get(field) {
                        Some(V::Array(a)) => snaps.extend(a.iter().cloned()),
                        Some(v) if js_truthy(Some(v)) => return Err(format!("e.{field} is not iterable")),
                        _ => {}
                    }
                }
                let at_ms = parse(&at);
                snaps.retain(|x| !js_truthy(x.get("skipped")) && parse(&x["receivedAt"]).is_finite() && parse(&x["receivedAt"]) <= at_ms);
                snaps.sort_by(|a, b| parse(&a["receivedAt"]).partial_cmp(&parse(&b["receivedAt"])).unwrap_or(std::cmp::Ordering::Equal));
                for snap in &snaps {
                    let endpoint = snap.get("endpoint").and_then(V::as_str).unwrap_or("");
                    let market_id = endpoint
                        .strip_prefix("/markets/")
                        .and_then(|rest| rest.find('/').map(|i| &rest[..i]))
                        .filter(|digits| !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()))
                        .filter(|_| endpoint.contains("/orderbook?"));
                    let Some(market_id) = market_id.filter(|m| scope.contains_key(*m)) else { continue };
                    let b = (|| -> Result<V, String> {
                        // new Set(e.accountExclusions): undefined/null -> empty,
                        // strings iterate chars (can never equal an exchange id),
                        // other non-iterables throw (caught -> continue).
                        let exclusions: BTreeSet<String> = match e.get("accountExclusions") {
                            None | Some(V::Null) => BTreeSet::new(),
                            Some(V::Array(a)) => a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect(),
                            Some(V::String(_)) => BTreeSet::new(),
                            Some(_) => return Err("e.accountExclusions is not iterable".into()),
                        };
                        let book = normalize_book(
                            &snap["data"],
                            market_id,
                            bank["config"]["tournamentId"].as_str().unwrap_or("\u{0}"),
                            snap["receivedAt"].as_str().unwrap_or("\u{0}"),
                            snap["requestStartedAt"].as_str().unwrap_or("\u{0}"),
                            bank["config"]["maxDataAgeMs"].as_f64().unwrap_or(f64::NAN),
                            &exclusions,
                            "authoritative_receipt_v1",
                        )?;
                        if book["exchangeId"].as_str() != scope.get(market_id).map(String::as_str) {
                            // JS `continue` inside try — skip this snapshot.
                            return Err("SCOPE_CONTINUE".into());
                        }
                        Ok(book)
                    })();
                    let Ok(b) = b else { continue };
                    for target in targets.iter_mut() {
                        let attribution = target.intent.get("attribution");
                        // JS intent.attribution.signalId: null -> "of null", absent ->
                        // "of undefined" TypeError; primitives -> undefined (no match).
                        let signal_id = match attribution {
                            None => return Err("Cannot read properties of undefined (reading 'signalId')".into()),
                            Some(V::Null) => return Err("Cannot read properties of null (reading 'signalId')".into()),
                            Some(V::Object(_)) => attribution.unwrap().get("signalId").cloned(),
                            _ => None, // JS: property read on a primitive -> undefined
                        };
                        if b.get("versionKey").cloned() == signal_id
                            && b.get("exchangeId") == target.intent.get("exchangeId")
                            && parse(&b["receivedAt"]) <= parse(&target.intent["preparedAt"])
                        {
                            target.entry_book = Some(b.clone());
                        }
                    }
                    let exchange_id = b["exchangeId"].as_str().unwrap_or("").to_string();
                    let list = book_history.entry(exchange_id.clone()).or_default();
                    if list.last().and_then(|x| x.get("versionKey")) == b.get("versionKey") {
                        continue;
                    }
                    let mid = (amount(b["bids"].as_array().and_then(|a| a.first()).and_then(|l| l["price"].as_str()).unwrap_or("\u{0}"))? as f64
                        + amount(b["asks"].as_array().and_then(|a| a.first()).and_then(|l| l["price"].as_str()).unwrap_or("\u{0}"))? as f64)
                        / 2e6;
                    list.push(json!({"at": b["sourceAt"], "receivedAt": b["receivedAt"], "mid": mid, "versionKey": b["versionKey"]}));
                    let cutoff = parse(&b["receivedAt"]) - bank["policy"]["lookbackMs"].as_f64().unwrap_or(0.0);
                    list.retain(|x| parse(&x["at"]) >= cutoff);
                    if list.len() > 512 {
                        let drop = list.len() - 512;
                        list.drain(..drop);
                    }
                }
            }
            if js_int(&row["sequence"]).is_some_and(|s| wanted.contains(&s)) {
                for target in targets.iter_mut().filter(|t| js_int(&t.proof["sourceSequence"]) == js_int(&row["sequence"])) {
                    let exchange_id = target.intent["exchangeId"].as_str().unwrap_or("").to_string();
                    let history = histories.get(&exchange_id).cloned().unwrap_or(V::Null);
                    let books = book_history.get(&exchange_id).cloned().unwrap_or_default();
                    require_that(
                        row["hash"] == target.proof["sourceHash"]
                            && digest(&json!({"exchangeId": exchange_id, "history": history, "bookHistory": books}))
                                == target.proof["referenceHash"].as_str().unwrap_or("").to_string(),
                        "REFERENCE_RECONSTRUCTION_FAILED",
                    )?;
                    target.reference_inputs = Some((history, books));
                }
            }
        }
    }
    let mut entries = Vec::new();
    for t in &targets {
        require_that(t.entry_book.is_some() && t.reference_inputs.is_some(), "ENTRY_BOOK_OR_REFERENCE_MISSING")?;
        let (history, books) = t.reference_inputs.clone().unwrap();
        let b = t.entry_book.clone().unwrap();
        let diagnostic = historical_reference_quality(
            if history.is_null() { None } else { Some(&history) },
            &books,
            b["requestStartedAt"].as_str().unwrap_or("\u{0}"),
            &bank["policy"],
        )?;
        let ask = (amount(b["asks"].as_array().and_then(|a| a.first()).and_then(|l| l["price"].as_str()).unwrap_or("\u{0}"))? + 5000) as f64 / 1e6;
        let bid = (amount(b["bids"].as_array().and_then(|a| a.first()).and_then(|l| l["price"].as_str()).unwrap_or("\u{0}"))? - 5000) as f64 / 1e6;
        let signal = dip_signal(&diagnostic["reference"], ask, bid, &bank["policy"])?;
        require_that(js_truthy(signal.get("admitted")), "ORIGINAL_SIGNAL_DID_NOT_REPRODUCE")?;
        let mut out = Map::new();
        out.insert("marketId".into(), t.intent["marketId"].clone());
        out.insert("exchangeId".into(), t.intent["exchangeId"].clone());
        out.insert("preparedAt".into(), t.intent["preparedAt"].clone());
        out.insert("price".into(), t.intent["price"].clone());
        out.insert("quantity".into(), t.intent["quantity"].clone());
        out.insert("referenceProof".into(), t.proof.clone());
        out.insert("coreHash".into(), t.core_hash.clone());
        out.insert("bookVersion".into(), b["versionKey"].clone());
        out.insert("bookReceivedAt".into(), b["receivedAt"].clone());
        out.insert("decisionInputAt".into(), b["requestStartedAt"].clone());
        out.insert("signal".into(), signal);
        out.insert("quality".into(), diagnostic["quality"].clone());
        entries.push(V::Object(out));
    }
    Ok(json!({
        "schemaVersion": 1,
        "at": baal_service_runtime::contract::ms_to_iso(
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as f64).unwrap_or(0.0)
        ),
        "status": "exact_reference_proofs_reproduced",
        "sourceSequence": source_sequence,
        "sourceHash": source_hash,
        "entries": entries,
        "networkRequests": 0,
        "competitionOrdersChanged": 0,
        "profitabilityEstablished": false,
        "limitations": [
            "All recorded admitted entries retained, including losses.",
            "Descriptive pre-entry history is not a forecast or strategy validation.",
            "Minute-balanced benchmark is a comparison, not a retroactive replacement of executed decisions.",
        ],
    }))
}
/// Full script behavior: compute the report, persist it, and return the
/// console.log'd line.
pub fn run(dir: &Path, out: &Path) -> Result<V, String> {
    let report = reference_quality_audit(dir)?;
    std::fs::write(out, format!("{}\n", serde_json::to_string_pretty(&report).map_err(|e| e.to_string())?))
        .map_err(|e| e.to_string())?;
    Ok(report)
}
