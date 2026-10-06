// data/paired-price-reversion-20261005.mjs — title-paired quote reversion scan.
use crate::replay_dual_no_timelapse_20261005::sealed_end;
use baal_collector::journal_lines::{journal_lines, JournalReadOptions};
use baal_paper::fixed::{big_amount as big_amount_of, money, SCALE, TICK};
use baal_service_runtime::json_text::{js_stringify, sha256};
use num_bigint::BigInt;
use serde_json::{json, Value as V};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

fn sha(b: &[u8]) -> String { sha256(b) }
fn need(ok: bool, code: &str) -> Result<(), String> {
    if !ok { return Err(format!("PAIR_PRICE_{}", code)); }
    Ok(())
}
fn amount(s: &str) -> BigInt { big_amount_of(s).unwrap_or_else(|_| BigInt::from(0)) }
fn amount_v(v: &V) -> BigInt { amount(v.as_str().unwrap_or("")) }

pub fn paired_reversion_policy() -> V {
    json!({"maximumQuoteAgeMs": 20000, "maximumSkewMs": 10000, "latencyMs": 2000,
        "maximumHoldMs": 1800000, "entryCost": "0.980000", "targetQuoteMargin": "0.010000",
        "perLegAdverseTicks": 1, "feePerShare": "0.000000"})
}

/// partyTitlePairs(markets, universe) — groups titles like
/// "Will the (Democratic|Republican) Party win the (X)?".
pub fn party_title_pairs(markets: &[V], universe: &V) -> Vec<V> {
    let scope: std::collections::HashMap<String, V> = universe["exchanges"].as_array().cloned().unwrap_or_default().iter()
        .filter_map(|x| x["marketId"].as_str().map(|id| (id.to_string(), x.clone()))).collect();
    let mut group: indexmap::IndexMap<String, Vec<V>> = indexmap::IndexMap::new();
    for m in markets {
        let title = m["title"].as_str().unwrap_or("");
        let caps = match party_title_match(title) { Some(c) => c, None => continue };
        let Some(e) = scope.get(m["id"].as_str().unwrap_or("")) else { continue };
        if e["option"].as_str() != Some("YES") { continue; }
        group.entry(caps.1.to_string()).or_default().push(json!({
            "marketId": m["id"], "exchangeId": e["exchangeId"], "party": caps.0}));
    }
    group.into_iter()
        .filter(|(_, x)| x.len() == 2 && {
            let parties: std::collections::HashSet<&str> = x.iter().filter_map(|y| y["party"].as_str()).collect();
            parties.len() == 2
        })
        .map(|(title, mut legs)| {
            legs.sort_by(|a, b| a["party"].as_str().unwrap_or("").cmp(b["party"].as_str().unwrap_or("")));
            json!({"title": title, "legs": legs, "contractPredicatesVerified": false})
        })
        .collect()
}

fn party_title_match(title: &str) -> Option<(&str, &str)> {
    // /^Will the (Democratic|Republican) Party win the (.+)\?$/
    let rest = title.strip_prefix("Will the ")?;
    let (party, rest) = rest.split_once(" Party win the ")?;
    if party != "Democratic" && party != "Republican" { return None; }
    let name = rest.strip_suffix('?')?;
    if name.is_empty() { return None; }
    Some((party, name))
}

/// pairedQuotePacket(pair, cache, at) — cache: exchangeId -> quote object.
pub fn paired_quote_packet(pair: &V, cache: &std::collections::HashMap<String, V>, at: i64) -> Option<V> {
    let policy = paired_reversion_policy();
    let quotes: Vec<V> = pair["legs"].as_array()?.iter()
        .map(|l| cache.get(l["exchangeId"].as_str().unwrap_or("")).cloned().unwrap_or(V::Null)).collect();
    if quotes.iter().any(|q| q.is_null()) { return None; }
    let stale = quotes.iter().any(|q| {
        q["requestStartedAt"].as_i64().unwrap_or(i64::MAX) > at
            || q["receivedAt"].as_i64().unwrap_or(i64::MAX) > at
            || at - q["requestStartedAt"].as_i64().unwrap_or(0) > policy["maximumQuoteAgeMs"].as_i64().unwrap_or(20000)
            || q["bestBid"].is_null() || q["bestAsk"].is_null()
            || q["bestBid"].as_f64().unwrap_or(f64::NAN) > q["bestAsk"].as_f64().unwrap_or(f64::NAN)
    });
    if stale { return None; }
    let max_recv = quotes.iter().map(|q| q["receivedAt"].as_i64().unwrap_or(i64::MIN)).max().unwrap_or(0);
    let min_req = quotes.iter().map(|q| q["requestStartedAt"].as_i64().unwrap_or(i64::MAX)).min().unwrap_or(0);
    if max_recv - min_req > policy["maximumSkewMs"].as_i64().unwrap_or(10000) { return None; }
    let fixed = |n: f64| -> BigInt {
        if !(n.is_finite() && (0.0..=1.0).contains(&n)) { return BigInt::from(-1); }
        amount(&format!("{:.6}", n))
    };
    let scale = BigInt::from(SCALE);
    let tick = BigInt::from(TICK);
    let buy = quotes.iter().fold(BigInt::from(0), |v, q| v + &scale - fixed(q["bestBid"].as_f64().unwrap_or(f64::NAN)) + &tick);
    let sell = quotes.iter().fold(BigInt::from(0), |v, q| v + &scale - fixed(q["bestAsk"].as_f64().unwrap_or(f64::NAN)) - &tick);
    if quotes.iter().any(|q| {
        &scale - fixed(q["bestBid"].as_f64().unwrap_or(f64::NAN)) + &tick >= scale
            || &scale - fixed(q["bestAsk"].as_f64().unwrap_or(f64::NAN)) - &tick <= BigInt::from(0)
    }) {
        return None;
    }
    // JS returns the raw BigInt micros; callers apply money()/Number()/1e6 themselves.
    Some(json!({"buy": buy.to_string().parse::<i64>().unwrap_or(0),
        "sell": sell.to_string().parse::<i64>().unwrap_or(0),
        "requests": quotes.iter().map(|q| q["requestStartedAt"].clone()).collect::<Vec<V>>(),
        "received": quotes.iter().map(|q| q["receivedAt"].clone()).collect::<Vec<V>>()}))
}

struct PairState {
    pair: V,
    usable_packets: i64,
    quoted_entries: i64,
    quoted_profitable_exits: i64,
    quoted_timeouts: i64,
    entry: Option<(i64, BigInt)>,
    quote_margins: Vec<BigInt>,
    holding_ms: Vec<i64>,
    min_buy: Option<BigInt>,
    last: V,
}

/// createQuoteReversionScan(pairs) — the {observe, report} closure pair.
pub struct QuoteReversionScan {
    cache: std::collections::HashMap<String, V>,
    states: Vec<PairState>,
}

impl QuoteReversionScan {
    pub fn observe(&mut self, snapshot: &V) -> Result<(), String> {
        let policy = paired_reversion_policy();
        let at = baal_service_runtime::contract::parse_iso_ms(snapshot["receivedAt"].as_str().unwrap_or("")).unwrap_or(f64::NAN);
        let start = baal_service_runtime::contract::parse_iso_ms(snapshot["requestStartedAt"].as_str().unwrap_or("")).unwrap_or(f64::NAN);
        need(at.fract() == 0.0 && at.abs() <= 9007199254740991.0
            && start.fract() == 0.0 && start.abs() <= 9007199254740991.0 && at >= start, "CLOCK")?;
        let at = at as i64;
        let start = start as i64;
        let mut changed = std::collections::HashSet::new();
        for q in snapshot["quotes"].as_array().cloned().unwrap_or_default() {
            let mut q = q;
            q["requestStartedAt"] = json!(start);
            q["receivedAt"] = json!(at);
            let id = q["exchangeId"].as_str().unwrap_or("").to_string();
            self.cache.insert(id.clone(), q);
            changed.insert(id);
        }
        for s in &mut self.states {
            if !s.pair["legs"].as_array().cloned().unwrap_or_default().iter()
                .any(|l| changed.contains(l["exchangeId"].as_str().unwrap_or(""))) {
                continue;
            }
            let Some(p) = paired_quote_packet(&s.pair, &self.cache, at) else { continue };
            s.usable_packets += 1;
            let buy = BigInt::from(p["buy"].as_i64().unwrap_or(0));
            let sell = BigInt::from(p["sell"].as_i64().unwrap_or(0));
            s.last = json!({"at": baal_service_runtime::contract::ms_to_iso(at as f64),
                "buy": money(&buy), "sell": money(&sell)});
            s.min_buy = Some(match &s.min_buy { None => buy.clone(), Some(m) => if buy < *m { buy.clone() } else { m.clone() } });
            if let Some((e_at, e_cost)) = s.entry.clone() {
                if !p["requests"].as_array().cloned().unwrap_or_default().iter()
                    .all(|t| t.as_i64().unwrap_or(0) >= e_at + policy["latencyMs"].as_i64().unwrap_or(2000)) {
                    continue;
                }
                let margin = &sell - &e_cost;
                if margin >= amount("0.010000") || at - e_at >= policy["maximumHoldMs"].as_i64().unwrap_or(1800000) {
                    s.quote_margins.push(margin.clone());
                    s.holding_ms.push(at - e_at);
                    if margin >= amount("0.010000") { s.quoted_profitable_exits += 1; } else { s.quoted_timeouts += 1; }
                    s.entry = None;
                }
                continue;
            }
            if buy <= amount("0.980000") {
                s.entry = Some((at, buy));
                s.quoted_entries += 1;
            }
        }
        Ok(())
    }

    pub fn report(&self) -> Vec<V> {
        self.states.iter().map(|s| json!({
            "title": s.pair["title"], "legs": s.pair["legs"], "contractPredicatesVerified": false,
            "usablePackets": s.usable_packets,
            "minimumQuotedAcquisition": s.min_buy.as_ref().map(|m| json!(money(m))).unwrap_or(V::Null),
            "quotedEntrySignals": s.quoted_entries,
            "quotedProfitableExitSignals": s.quoted_profitable_exits,
            "quotedTimeoutExitSignals": s.quoted_timeouts,
            "quotedRoundtripMargins": s.quote_margins.iter().map(|m| json!(money(m))).collect::<Vec<V>>(),
            "totalClosedQuoteMargin": money(&s.quote_margins.iter().fold(BigInt::from(0), |n, v| n + v)),
            "quoteHoldingMs": s.holding_ms,
            "openQuotedSignal": s.entry.as_ref().map(|(at, cost)| json!({
                "at": baal_service_runtime::contract::ms_to_iso(*at as f64), "cost": money(cost)})).unwrap_or(V::Null),
            "last": s.last, "depthAvailable": false, "executionEvidence": false,
            "simulatedFills": 0, "realizedPnl": V::Null})).collect()
    }
}

pub fn create_quote_reversion_scan(pairs: &[V]) -> QuoteReversionScan {
    QuoteReversionScan {
        cache: std::collections::HashMap::new(),
        states: pairs.iter().map(|pair| PairState {
            pair: pair.clone(), usable_packets: 0, quoted_entries: 0, quoted_profitable_exits: 0,
            quoted_timeouts: 0, entry: None, quote_margins: Vec::new(), holding_ms: Vec::new(),
            min_buy: None, last: V::Null,
        }).collect(),
    }
}

/// `main()` — reads market-prices.jsonl from the paper runtime, screens
/// title pairs, writes data/paired-price-reversion-results-20261005.json.
pub fn main(root: &Path) -> Result<V, String> {
    let started = std::time::Instant::now();
    let rt = Path::new(&std::env::var("LOCALAPPDATA").unwrap_or_default()).join("SigTradingCup").join("paper-runtime");
    let file = rt.join("market-prices.jsonl");
    let universe: V = serde_json::from_slice(&std::fs::read(root.join("live-paper/price-universe.json")).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    let markets: V = serde_json::from_slice(&std::fs::read(root.join("data/markets.json")).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    let markets_arr = markets.as_array().cloned().unwrap_or_default();
    let pairs = party_title_pairs(&markets_arr, &universe);
    let mut scan = create_quote_reversion_scan(&pairs);
    let end = sealed_end(&file)?;
    let mut raw = Sha256::new();
    let mut sequence = 0i64;
    let mut previous = String::new();
    let mut first: Option<V> = None;
    let mut last: Option<V> = None;
    for line in journal_lines(&file, &JournalReadOptions { end_byte: Some(end), ..Default::default() })? {
        raw.update(format!("{}\n", line).as_bytes());
        let row: V = serde_json::from_str(&line).map_err(|e| e.to_string())?;
        let hash = row["hash"].as_str().unwrap_or("").to_string();
        let mut body = row.clone();
        body.as_object_mut().unwrap().shift_remove("hash");
        sequence += 1;
        need(row["sequence"].as_i64() == Some(sequence) && row["previous"].as_str() == Some(previous.as_str())
            && sha(js_stringify(&body).as_bytes()) == hash, "CHAIN")?;
        previous = hash;
        need(row["event"]["type"].as_str() == Some("market_price_observation"), "SOURCE")?;
        if first.is_none() { first = Some(row["event"]["snapshot"]["receivedAt"].clone()); }
        last = Some(row["event"]["snapshot"]["receivedAt"].clone());
        scan.observe(&row["event"]["snapshot"])?;
    }
    let mut results = scan.report();
    results.sort_by(|a, b| {
        b["totalClosedQuoteMargin"].as_str().unwrap_or("").parse::<f64>().unwrap_or(f64::NAN)
            .partial_cmp(&a["totalClosedQuoteMargin"].as_str().unwrap_or("").parse::<f64>().unwrap_or(f64::NAN))
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(b["quotedRoundtripMargins"].as_array().map(|x| x.len()).unwrap_or(0)
                .cmp(&a["quotedRoundtripMargins"].as_array().map(|x| x.len()).unwrap_or(0)))
    });
    let parse = |v: &V| baal_service_runtime::contract::parse_iso_ms(v.as_str().unwrap_or("")).unwrap_or(f64::NAN);
    let hours = if first.is_some() { (parse(last.as_ref().unwrap()) - parse(first.as_ref().unwrap())) / 3600000.0 } else { f64::NAN };
    let sum_i64 = |k: &str| -> i64 { results.iter().map(|r| r[k].as_i64().unwrap_or(0)).sum() };
    let sum_len = |k: &str| -> i64 { results.iter().map(|r| r[k].as_array().map(|a| a.len() as i64).unwrap_or(0)).sum() };
    let total_margin = results.iter().fold(BigInt::from(0), |n, r| n + amount_v(&r["totalClosedQuoteMargin"]));
    let summary = json!({"hours": hours, "quotedEntrySignals": sum_i64("quotedEntrySignals"),
        "quotedClosedSignals": sum_len("quotedRoundtripMargins"),
        "quotedProfitableExits": sum_i64("quotedProfitableExitSignals"),
        "quotedTimeouts": sum_i64("quotedTimeoutExitSignals"),
        "totalClosedQuoteMargin": money(&total_margin),
        "positiveNetQuotePairs": results.iter().filter(|r| amount_v(&r["totalClosedQuoteMargin"]) > BigInt::from(0)).count()});
    let out = json!({"schemaVersion": 1,
        "at": baal_service_runtime::contract::ms_to_iso(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as f64).unwrap_or(0.0)),
        "policy": paired_reversion_policy(),
        "source": {"prefixBytes": end, "sha256": format!("{:x}", raw.finalize()), "records": sequence,
            "tailHash": previous, "first": first, "last": last},
        "wallMs": started.elapsed().as_secs_f64() * 1000.0,
        "pairsScreened": pairs.len(), "summary": summary, "results": results,
        "competitionOrders": 0, "networkRequests": 0, "simulatedFills": 0,
        "profitabilityEstablished": false, "selectionIsExploratory": true,
        "limitations": ["Historical bulk bid/ask quotes contain no executable depth and do not establish fills, order acceptance, queue priority or capital returns.",
            "Title-paired catalogue groups require exact contract-root review; administrative resolution or differing predicates can invalidate payout assumptions.",
            "Quote exit signals omit sequential partial-fill and failed-hedge risk. They rank future exact depth capture only.",
            "Summed quote margins across overlapping opportunities are not a capital-constrained wallet, account return or realized P&L.",
            "No current account eligibility or later contract review was backfilled into historical sources.",
            "Fixed policy is declared before this scan; results are exploratory and require untouched later depth and prospective policy validation."]});
    let out_file = root.join("data").join("paired-price-reversion-results-20261005.json");
    std::fs::write(&out_file, format!("{}\n", serde_json::to_string_pretty(&out).map_err(|e| e.to_string())?)).map_err(|e| e.to_string())?;
    println!("{}", js_stringify(&json!({"records": sequence, "pairs": pairs.len(),
        "wallMs": out["wallMs"], "summary": summary, "profitabilityEstablished": false})));
    Ok(out)
}

pub fn run(root: &Path) -> Result<V, String> { main(root) }
