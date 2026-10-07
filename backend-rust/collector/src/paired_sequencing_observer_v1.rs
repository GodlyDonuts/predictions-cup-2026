// live-paper/paired-sequencing-observer-v1.mjs — prospective observation only:
// no timers, credentials, API, wallet fills or orders.
use crate::breadth::validate_universe;
use crate::paired_entry_leg_risk_v1::compare_paired_entry_leg_risk_v1;
use baal_execution::core::payload_hash;
use baal_paper::fixed::{big_amount, money, SCALE, TICK};
use baal_service_runtime::atomic::atomic_json_canonical;
use baal_service_runtime::contract::{ms_to_iso, parse_iso_ms};
use baal_service_runtime::journal::Journal;
use baal_service_runtime::json_text::js_truthy;
use num_bigint::BigInt;
use serde_json::{json, Map, Value as V};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

// JS Math.min/Math.max: NaN propagates; -0/+0 handled by f64::min/max.
fn js_min(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() { f64::NAN } else { a.min(b) }
}
fn js_max(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() { f64::NAN } else { a.max(b) }
}
fn parse_ms(v: &V) -> f64 {
    v.as_str().and_then(parse_iso_ms).unwrap_or(f64::NAN)
}
// JS Number() coercion used by Math.min on book quantities.
fn js_number(v: Option<&V>) -> f64 {
    match v {
        None => f64::NAN,
        Some(V::Number(n)) => n.as_f64().unwrap_or(f64::NAN),
        Some(V::String(s)) => s.trim().parse::<f64>().unwrap_or(f64::NAN),
        Some(V::Bool(b)) => {
            if *b { 1.0 } else { 0.0 }
        }
        Some(V::Null) => 0.0,
        Some(_) => f64::NAN,
    }
}
fn bigint_f64(x: &BigInt) -> f64 {
    x.to_string().parse::<f64>().unwrap_or(if x.sign() == num_bigint::Sign::Minus {
        f64::NEG_INFINITY
    } else {
        f64::INFINITY
    })
}
// JS `x++`: Number-coerce, add 1. A non-finite result stores as NaN in JS which
// JSON.stringify renders as null — the V stores Null directly (same bytes out).
fn js_inc(v: Option<&V>) -> V {
    let r = js_number(v) + 1.0;
    if !r.is_finite() {
        V::Null
    } else if r.fract() == 0.0 && r.abs() <= 9_007_199_254_740_991.0 {
        json!(r as i64)
    } else {
        json!(r)
    }
}
fn cohort_ok(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() >= 2
        && b.len() <= 64
        && b[0].is_ascii_lowercase()
        && b[1..].iter().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'-')
}

/// The `{status,onBook,close}` object the JS returns.
pub struct PairedSequencingObserverV1 {
    file: PathBuf,
    request_bytes: Vec<u8>,
    request: V,
    cohort: String,
    stats_file: PathBuf,
    journal: Journal,
    state: V,
    now: Arc<dyn Fn() -> f64 + Send + Sync>,
}

/// openPairedSequencingObserverV1({runtimeDir,universe,now=Date.now})
pub fn open_paired_sequencing_observer_v1(
    runtime_dir: &Path,
    universe: &V,
    now: Option<Arc<dyn Fn() -> f64 + Send + Sync>>,
) -> Result<Option<PairedSequencingObserverV1>, String> {
    let now = now.unwrap_or_else(|| Arc::new(|| chrono::Utc::now().timestamp_millis() as f64));
    let file = runtime_dir.join("paired-sequencing-observer-request.json");
    if !file.exists() {
        return Ok(None);
    }
    let request_bytes = fs::read(&file).map_err(|e| e.to_string())?;
    let r: V = serde_json::from_slice(&request_bytes).map_err(|e| e.to_string())?;
    // const {hash:declaredHash,...rawUniverse}=universe;
    // universeHash=declaredHash??validateUniverse(rawUniverse,universe.tournamentId).hash
    if !universe.is_object() {
        return Err("SEQUENCING_OBSERVER_REQUEST".into());
    }
    let universe_hash = match universe.get("hash").filter(|h| !h.is_null()) {
        Some(h) => h.clone(),
        None => {
            let mut raw = universe.as_object().cloned().unwrap_or_default();
            raw.shift_remove("hash");
            let checked = validate_universe(
                &V::Object(raw),
                universe.get("tournamentId").and_then(V::as_str).unwrap_or(""),
            )?;
            checked["hash"].clone()
        }
    };
    if r.get("kind").and_then(V::as_str) != Some("paired_sequencing_prospective_observer_v1")
        || r.get("tournamentId") != universe.get("tournamentId")
        || r.get("universeHash") != Some(&universe_hash)
        || !parse_ms(r.get("cutoff").unwrap_or(&V::Null)).is_finite()
        || !r.get("pairs").is_some_and(V::is_array)
    {
        return Err("SEQUENCING_OBSERVER_REQUEST".into());
    }
    let exchanges = universe["exchanges"]
        .as_array()
        .cloned()
        .ok_or("Cannot read properties of undefined (reading 'some')")?;
    for pair in r["pairs"].as_array().unwrap() {
        let legs = match pair.as_array() {
            Some(p) if p.len() == 2 => p,
            _ => return Err("SEQUENCING_OBSERVER_SCOPE".into()),
        };
        if legs[0].get("exchangeId") == legs[1].get("exchangeId")
            || legs.iter().any(|l| {
                !exchanges.iter().any(|e| {
                    e.get("exchangeId") == l.get("exchangeId") && e.get("marketId") == l.get("marketId")
                })
            })
        {
            return Err("SEQUENCING_OBSERVER_SCOPE".into());
        }
    }
    let cohort = match r.get("cohort").filter(|c| !c.is_null()) {
        Some(c) => c.as_str().unwrap_or("").to_string(),
        None => "original".to_string(),
    };
    if !cohort_ok(&cohort) {
        return Err("SEQUENCING_OBSERVER_COHORT".into());
    }
    let dir = runtime_dir.join("paired-sequencing-observer-cohorts").join(&cohort);
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(&dir).map_err(|e| e.to_string())?;
    let journal_file = dir.join("journal.jsonl");
    let mut journal = Journal::open(&journal_file)?;
    let state = journal
        .last
        .as_ref()
        .and_then(|b| b.get("state"))
        .filter(|s| !s.is_null())
        .cloned()
        .unwrap_or_else(|| {
            json!({
                "requestHash": payload_hash(&r).unwrap_or_default(),
                "observations": 0,
                "conditionalPositive": 0,
                "sequence": 0,
                "pairs": {},
                "seen": {},
            })
        });
    if state.get("requestHash").and_then(V::as_str) != Some(payload_hash(&r)?.as_str()) {
        return Err("SEQUENCING_OBSERVER_IMMUTABLE_REQUEST".into());
    }
    let stats_file = dir.join("stats.json");
    if journal.last.is_none() {
        journal.commit(&state, &json!({"kind":"genesis","requestHash":state["requestHash"]}))?;
    }
    Ok(Some(PairedSequencingObserverV1 {
        file,
        request_bytes,
        request: r,
        cohort,
        stats_file,
        journal,
        state,
        now,
    }))
}

impl PairedSequencingObserverV1 {
    /// status()
    pub fn status(&self) -> V {
        json!({
            "kind": self.request["kind"],
            "cohort": self.cohort,
            "cutoff": self.request["cutoff"],
            "observations": self.state["observations"],
            "conditionalPositive": self.state["conditionalPositive"],
            "sourceSequence": self.state["sequence"],
            "latestPairs": self.state["pairs"],
            "orders": 0,
            "paperFills": 0,
            "probabilityEstimated": false,
            "executionAuthority": false,
            "additionalApiClients": 0,
        })
    }
    /// onBook(publication)
    pub fn on_book(&mut self, publication: &V) -> Result<(), String> {
        if fs::read(&self.file).map_err(|e| e.to_string())? != self.request_bytes {
            return Err("SEQUENCING_OBSERVER_REQUEST_CHANGED".into());
        }
        let at = (self.now)();
        if js_truthy(publication.get("closed"))
            || publication.get("tournamentId") != self.request.get("tournamentId")
        {
            return Ok(());
        }
        let pairs = self.request["pairs"].as_array().cloned().unwrap_or_default();
        let cutoff = parse_ms(&self.request["cutoff"]);
        for pair in &pairs {
            let books: Vec<V> = pair
                .as_array()
                .unwrap()
                .iter()
                .map(|l| {
                    publication["books"][format!("{}:YES", l["exchangeId"].as_str().unwrap_or(""))].clone()
                })
                .collect();
            if books.iter().any(|b| {
                !js_truthy(Some(b))
                    || parse_ms(&b["requestStartedAt"]) <= cutoff
                    || parse_ms(&b["receivedAt"]) > at
                    || at - parse_ms(&b["requestStartedAt"]) > 20000.0
            }) {
                continue;
            }
            let max_received = books.iter().map(|b| parse_ms(&b["receivedAt"])).fold(f64::NEG_INFINITY, js_max);
            let min_started = books.iter().map(|b| parse_ms(&b["requestStartedAt"])).fold(f64::INFINITY, js_min);
            if max_received - min_started > 10000.0 {
                continue;
            }
            let key = pair
                .as_array()
                .unwrap()
                .iter()
                .map(|l| l["exchangeId"].as_str().unwrap_or("").to_string())
                .collect::<Vec<_>>()
                .join(":");
            let source: Vec<V> = books
                .iter()
                .map(|b| json!({"hash": b["publicationHash"], "version": b["versionKey"]}))
                .collect();
            let source_hash = payload_hash(&V::Array(source))?;
            match self.state.get("seen") {
                None | Some(V::Null) => {
                    return Err(format!("Cannot read properties of undefined (reading '{key}')"));
                }
                Some(seen) if seen.is_object() => {
                    if seen.get(&key).and_then(V::as_str) == Some(source_hash.as_str()) {
                        continue;
                    }
                }
                _ => {} // non-object seen indexes to undefined in JS -> not equal, proceed
            }
            let scale = BigInt::from(SCALE);
            let tick = BigInt::from(TICK);
            let mut legs: Vec<V> = Vec::with_capacity(2);
            for b in &books {
                if b["bids"].is_null() {
                    return Err("Cannot read properties of undefined (reading '0')".into());
                }
                let bid0 = &b["bids"][0];
                if bid0.is_null() {
                    return Err("Cannot read properties of undefined (reading 'price')".into());
                }
                let no_buy = money(&(&scale - big_amount(bid0["price"].as_str().unwrap_or(""))? + &tick));
                if b["asks"].is_null() {
                    return Err("Cannot read properties of undefined (reading '0')".into());
                }
                let ask0 = &b["asks"][0];
                if ask0.is_null() {
                    return Err("Cannot read properties of undefined (reading 'price')".into());
                }
                let no_sell = money(&(&scale - big_amount(ask0["price"].as_str().unwrap_or(""))?));
                legs.push(json!({"exchangeId": b["exchangeId"], "noBuyLimit": no_buy, "noSellLimit": no_sell}));
            }
            let mut depth = 100.0f64;
            for b in &books {
                depth = js_min(
                    depth,
                    js_min(js_number(b["bids"][0].get("quantity")), js_number(b["asks"][0].get("quantity"))),
                );
            }
            let mut quantity = depth;
            for l in &legs {
                let buy = big_amount(l["noBuyLimit"].as_str().unwrap_or(""))?;
                if buy == BigInt::from(0) {
                    return Err("Division by zero".into()); // JS BigInt RangeError
                }
                quantity = js_min(quantity, bigint_f64(&(BigInt::from(25_000_000i64) / &buy)));
            }
            if quantity < 1.0 {
                continue;
            }
            let result = match compare_paired_entry_leg_risk_v1(&json!({"legs": legs, "quantity": quantity})) {
                Ok(x) => x,
                Err(_) => continue,
            };
            let mut next = self.state.clone(); // structuredClone(state)
            let obs = js_inc(next.get("observations"));
            next["observations"] = obs;
            next["sequence"] = publication["sequence"].clone();
            match next.get_mut("seen") {
                Some(V::Object(m)) => {
                    m.insert(key.clone(), json!(source_hash));
                }
                Some(V::Null) | None => {
                    return Err(format!("Cannot set properties of undefined (setting '{key}')"));
                }
                _ => {
                    return Err(format!("Cannot create property '{key}' on a non-object"));
                }
            }
            let surplus_str = result["scenarios"][0]["conditionalOrdinarySurplusIfBothFill"]
                .as_str()
                .unwrap_or("");
            if !surplus_str.starts_with('-') && big_amount(surplus_str)? > BigInt::from(0) {
                let cp = js_inc(next.get("conditionalPositive"));
                next["conditionalPositive"] = cp;
            }
            let mut entry = Map::new();
            entry.insert("at".into(), json!(ms_to_iso(at)));
            entry.insert("sourceHash".into(), json!(source_hash));
            entry.insert("sourceSequence".into(), publication["sequence"].clone());
            entry.insert("rootsReviewed".into(), json!(false));
            for (k, v) in result.as_object().unwrap().iter() {
                entry.insert(k.clone(), v.clone()); // ...result spread
            }
            match next.get_mut("pairs") {
                Some(V::Object(m)) => {
                    m.insert(key.clone(), V::Object(entry));
                }
                Some(V::Null) | None => {
                    return Err(format!("Cannot set properties of undefined (setting '{key}')"));
                }
                _ => {
                    return Err(format!("Cannot create property '{key}' on a non-object"));
                }
            }
            self.journal.commit(
                &next,
                &json!({"kind":"prospective_pair_depth","sourceHash":source_hash,
                    "sourceSequence":publication["sequence"],"at":ms_to_iso(at)}),
            )?;
            self.state = next;
        }
        atomic_json_canonical(&self.stats_file, &self.status())?;
        Ok(())
    }
    /// close() — no-op, same as the JS.
    pub fn close(&mut self) {}
}
