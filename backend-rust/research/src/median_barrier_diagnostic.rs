// research/median-barrier-diagnostic.mjs — causal bulk quote-path diagnosis:
// median barrier entry on verified market price observations. Quote-path only;
// no order depth, executions, wallet or profit claim.
use crate::js::{date_parse, js_number, map_key, num, safe_int};
use baal_service_runtime::json_text::{digest, sha256};
use baal_service_runtime::market::validate_price_observation;
use indexmap::IndexMap;
use serde_json::{json, Value as V};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read};
use std::path::Path;

const HOUR: f64 = 3_600_000.0;
const HOLD: f64 = 7_200_000.0;
const COOLDOWN: f64 = 900_000.0;
const GAP: f64 = 120_000.0;

fn need(x: bool, c: &str) -> Result<(), String> {
    if !x {
        return Err(c.to_string());
    }
    Ok(())
}

// tick(x): finite number on the 1/200 grid → Math.round(x*200), else null.
fn tick(x: Option<&V>) -> Option<f64> {
    let v = x?;
    if !v.is_number() {
        return None;
    }
    let n = js_number(Some(v));
    if !n.is_finite() {
        return None;
    }
    if (n * 200.0 - (n * 200.0).round()).abs() < 1e-8 {
        Some((n * 200.0).round())
    } else {
        None
    }
}
// upper(a): numeric sort, take index floor(len/2) (upper median).
fn upper(mut a: Vec<f64>) -> f64 {
    a.sort_by(|x, y| x.partial_cmp(y).unwrap_or(std::cmp::Ordering::Equal));
    a[a.len() / 2]
}

#[derive(Default)]
struct Market {
    points: Vec<(f64, f64)>, // (at, bid in ticks)
    head: usize,
    active: Option<V>,
    cooldown: f64,
}

pub struct BarrierDiagnostic {
    universe: V,
    members: HashMap<String, V>,
    markets: IndexMap<String, Market>,
    events: Vec<V>,
    counts: IndexMap<String, f64>,
    seq: f64,
    previous: String,
    last_known: f64,
}

// createBarrierDiagnostic(universe)
pub fn create_barrier_diagnostic(universe: &V) -> BarrierDiagnostic {
    let mut members = HashMap::new();
    for x in universe["exchanges"].as_array().cloned().unwrap_or_default() {
        members.insert(map_key(x.get("exchangeId")), x);
    }
    let mut counts = IndexMap::new();
    for k in ["candidates", "target", "stop", "horizon", "data_gap", "missing_quote", "unfinished"]
    {
        counts.insert(k.to_string(), 0.0);
    }
    BarrierDiagnostic {
        universe: universe.clone(),
        members,
        markets: IndexMap::new(),
        events: Vec::new(),
        counts,
        seq: 0.0,
        previous: String::new(),
        last_known: f64::NEG_INFINITY,
    }
}

impl BarrierDiagnostic {
    fn finish(&mut self, key: &str, reason: &str, at: f64, bid: Option<f64>) {
        let e = match self.markets.get_mut(key).and_then(|m| m.active.take()) {
            Some(e) => e,
            None => return,
        };
        *self.counts.entry(reason.to_string()).or_insert(0.0) += 1.0;
        let mut ev = e.as_object().cloned().unwrap_or_default();
        ev.insert("reason".to_string(), json!(reason));
        ev.insert("endAt".to_string(), num(at));
        ev.insert("observedBid".to_string(), bid.map_or(V::Null, num));
        ev.insert(
            "quoteDifferenceTicks".to_string(),
            bid.map_or(V::Null, |b| num(b - js_number(e.get("ask")))),
        );
        self.events.push(V::Object(ev));
        if let Some(m) = self.markets.get_mut(key) {
            m.cooldown = at + COOLDOWN;
        }
    }

    // accept(row): chain/hash/state/event gates, then per-quote processing.
    pub fn accept(&mut self, row: &V) -> Result<(), String> {
        let hash_v = row.get("hash").cloned().unwrap_or(V::Null);
        let mut body = row.as_object().cloned().unwrap_or_default();
        body.shift_remove("hash");
        let seq_plus = self.seq + 1.0;
        need(
            row.get("sequence").and_then(V::as_f64) == Some(seq_plus)
                && row.get("previous").and_then(V::as_str) == Some(self.previous.as_str())
                && digest(&V::Object(body)) == hash_v.as_str().unwrap_or(""),
            "PRICE_CHAIN",
        )?;
        need(
            row.as_object()
                .is_some_and(|o| o.get("state").is_some_and(V::is_null))
                && row.get("event").and_then(|e| e.get("type")).and_then(V::as_str)
                    == Some("market_price_observation"),
            "PRICE_ROW",
        )?;
        let s = validate_price_observation(&row["event"]["snapshot"], &self.universe)?;
        let at = date_parse(&crate::js::js_string(row.get("at").unwrap_or(&V::Null)));
        let request = date_parse(&crate::js::js_string(
            s.get("requestStartedAt").unwrap_or(&V::Null),
        ));
        need(
            at.is_finite()
                && at >= self.last_known
                && date_parse(&crate::js::js_string(
                    s.get("receivedAt").unwrap_or(&V::Null),
                )) <= at,
            "PRICE_CLOCK",
        )?;
        let missing: Vec<String> = s["missingIds"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .iter()
            .map(|id| map_key(Some(id)))
            .collect();
        for id in &missing {
            if self.markets.get(id).is_some_and(|m| m.active.is_some()) {
                self.finish(id, "missing_quote", at, None);
            }
        }
        for q in s["quotes"].as_array().cloned().unwrap_or_default() {
            let exchange_key = map_key(q.get("exchangeId"));
            let member = self.members.get(&exchange_key);
            need(
                member.is_some_and(|m| m.get("marketId") == q.get("marketId"))
                    && q.get("option").and_then(V::as_str) == Some("YES"),
                "SCOPE",
            )?;
            let bid = tick(q.get("bestBid"));
            let ask = tick(q.get("bestAsk"));
            if !self.markets.contains_key(&exchange_key) {
                self.markets.insert(
                    exchange_key.clone(),
                    Market {
                        points: Vec::new(),
                        head: 0,
                        active: None,
                        cooldown: 0.0,
                    },
                );
            }
            if bid.is_none()
                || ask.is_none()
                || bid.unwrap() <= 0.0
                || ask.unwrap() >= 200.0
                || bid.unwrap() >= ask.unwrap()
            {
                let key = exchange_key.clone();
                if self.markets.get(&key).is_some_and(|m| m.active.is_some()) {
                    self.finish(&key, "missing_quote", at, None);
                }
                continue;
            }
            let bid = bid.unwrap();
            let ask = ask.unwrap();
            let mut finish_action: Option<(&'static str, Option<f64>)> = None;
            {
                let m = self.markets.get_mut(&exchange_key).unwrap();
                if m.active.is_some() {
                    let e_at = js_number(m.active.as_ref().unwrap().get("at"));
                    let e_last_at = js_number(m.active.as_ref().unwrap().get("lastAt"));
                    let e_ask = js_number(m.active.as_ref().unwrap().get("ask"));
                    if at - e_last_at > GAP {
                        finish_action = Some(("data_gap", None));
                    } else if at >= e_at + HOLD {
                        finish_action = Some(("horizon", Some(bid)));
                    } else if request >= e_at + 3000.0 {
                        if bid <= e_ask - 4.0 {
                            finish_action = Some(("stop", Some(bid)));
                        } else if bid >= e_ask + 1.0 {
                            finish_action = Some(("target", Some(bid)));
                        }
                    }
                }
            }
            if let Some((reason, b)) = finish_action {
                self.finish(&exchange_key, reason, at, b);
            }
            {
                let m = self.markets.get_mut(&exchange_key).unwrap();
                if let Some(e) = m.active.as_mut() {
                    e["lastAt"] = num(at);
                }
                while m.head < m.points.len() && m.points[m.head].0 < at - HOUR {
                    m.head += 1;
                }
                if m.head > 256 && m.head * 2 > m.points.len() {
                    m.points.drain(..m.head);
                    m.head = 0;
                }
            }
            let do_candidate = {
                let m = self.markets.get(&exchange_key).unwrap();
                m.active.is_none() && at >= m.cooldown
            };
            if do_candidate {
                let (prior, market_id, exchange_id) = {
                    let m = self.markets.get(&exchange_key).unwrap();
                    let mut prior: Vec<(f64, f64)> = m.points[m.head..]
                        .iter()
                        .copied()
                        .filter(|p| p.0 < request)
                        .collect();
                    prior.pop();
                    (
                        prior,
                        q.get("marketId").cloned().unwrap_or(V::Null),
                        q.get("exchangeId").cloned().unwrap_or(V::Null),
                    )
                };
                if prior.len() >= 20
                    && prior.last().map(|p| p.0).unwrap_or(0.0)
                        - prior.first().map(|p| p.0).unwrap_or(0.0)
                        >= 1_800_000.0
                {
                    let median = upper(prior.iter().map(|p| p.1).collect());
                    if median - ask >= 1.0 {
                        let trend = |split: f64| -> Option<f64> {
                            let older: Vec<f64> = prior
                                .iter()
                                .filter(|p| p.0 < at - split)
                                .map(|p| p.1)
                                .collect();
                            let recent: Vec<f64> = prior
                                .iter()
                                .filter(|p| p.0 >= at - split)
                                .map(|p| p.1)
                                .collect();
                            if older.len() >= 5 && recent.len() >= 5 {
                                Some(upper(recent) - upper(older))
                            } else {
                                None
                            }
                        };
                        *self.counts.entry("candidates".to_string()).or_insert(0.0) += 1.0;
                        self.markets
                            .get_mut(&exchange_key)
                            .unwrap()
                            .active = Some(json!({
                            "marketId": market_id,
                            "exchangeId": exchange_id,
                            "at": num(at),
                            "lastAt": num(at),
                            "ask": num(ask),
                            "bid": num(bid),
                            "reference": num(median),
                            "trend30Ticks": trend(1_800_000.0).map_or(V::Null, num),
                            "trend15Ticks": trend(900_000.0).map_or(V::Null, num),
                            "sourceSequence": row["sequence"],
                            "sourceHash": hash_v,
                            "historyPoints": num(prior.len() as f64),
                        }));
                    }
                }
            }
            self.markets
                .get_mut(&exchange_key)
                .unwrap()
                .points
                .push((at, bid));
        }
        self.seq = seq_plus;
        self.previous = hash_v.as_str().unwrap_or("").to_string();
        self.last_known = at;
        Ok(())
    }

    // report(): unfinished actives plus thresholds — same key order as JS.
    pub fn report(&self) -> V {
        let mut unfinished: Vec<V> = Vec::new();
        for m in self.markets.values() {
            if let Some(e) = &m.active {
                let mut o = e.as_object().cloned().unwrap_or_default();
                o.insert("reason".to_string(), json!("unfinished"));
                unfinished.push(V::Object(o));
            }
        }
        let mut counts = serde_json::Map::new();
        for (k, v) in &self.counts {
            let v = if k == "unfinished" {
                unfinished.len() as f64
            } else {
                *v
            };
            counts.insert(k.clone(), num(v));
        }
        json!({
            "schemaVersion": 1,
            "mode": "causal_bulk_quote_barrier_diagnostic",
            "counts": V::Object(counts),
            "events": self.events,
            "unfinished": unfinished,
            "source": {
                "sequence": num(self.seq),
                "hash": self.previous,
                "lastKnownAt": if self.last_known.is_finite() { json!(crate::js::iso(self.last_known)) } else { V::Null },
            },
            "thresholds": {
                "historyMs": num(HOUR),
                "minimumHistoryPoints": 20,
                "minimumHistorySpanMs": 1_800_000,
                "dropNewest": true,
                "targetTicks": 1,
                "stopTicks": 4,
                "maximumHoldMs": num(HOLD),
                "cooldownMs": num(COOLDOWN),
                "maximumQuoteGapMs": num(GAP),
            },
            "fills": 0,
            "orders": 0,
            "profitabilityEstablished": false,
            "limitations": [
                "Quoted bid paths are not executable fills or wallet returns.",
                "Entry is the observed bulk ask, not a simulated delayed purchase.",
                "Gaps and unfinished paths remain explicit; markets/episodes are correlated.",
            ],
        })
    }
}

// diagnosePrefix({file,bytes,sha256,universe}): hash and line-consume exactly
// the [0, bytes) prefix of the append-only tape.
pub fn diagnose_prefix(args: &V) -> Result<V, String> {
    let file_name = crate::js::js_string(args.get("file").unwrap_or(&V::Null));
    let file = Path::new(&file_name);
    let bytes = safe_int(args.get("bytes"));
    let want_hash = crate::js::js_string(args.get("sha256").unwrap_or(&V::Null));
    let universe = args.get("universe").cloned().unwrap_or(V::Null);
    need(
        bytes.is_some_and(|b| b > 0)
            && crate::files::read_bytes(file)
                .map(|b| b.len() as i64 >= bytes.unwrap())
                .unwrap_or(false),
        "PREFIX_BYTES",
    )?;
    let bytes_n = bytes.unwrap() as usize;
    let f = std::fs::File::open(file).map_err(|e| e.to_string())?;
    let mut limited = f.take(bytes_n as u64);
    let mut prefix = Vec::new();
    limited.read_to_end(&mut prefix).map_err(|e| e.to_string())?;
    let mut d = create_barrier_diagnostic(&universe);
    let mut consumed: usize = 0;
    // readline splits on \n and strips one trailing \r; each consumed row
    // counts Buffer.byteLength(line)+1 regardless of a final terminator.
    let reader = BufReader::new(&prefix[..]);
    let mut raw = Vec::new();
    let mut r = reader;
    loop {
        raw.clear();
        let n = r.read_until(b'\n', &mut raw).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        if raw.last() == Some(&b'\n') {
            raw.pop();
        }
        if raw.last() == Some(&b'\r') {
            raw.pop();
        }
        consumed += raw.len() + 1;
        let text = String::from_utf8_lossy(&raw).to_string();
        let line: V = serde_json::from_str(&text).map_err(|e| e.to_string())?;
        d.accept(&line)?;
    }
    need(
        consumed == bytes_n && sha256(&prefix) == want_hash,
        "PREFIX_HASH_OR_BOUNDARY",
    )?;
    Ok(d.report())
}
