// data/paired-price-reversion-20261005.mjs — fixed-policy quote reversion scan
// over party-paired YES markets in market-prices.jsonl.
use baal_service_runtime::journal::{journal_lines, JournalReadOptions};
use baal_service_runtime::json_text::{js_stringify, sha256};
use serde_json::{json, Value as V};
use sha2::{Digest, Sha256};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

fn sha(b: &[u8]) -> String { sha256(b) }
fn read_json(p: &Path) -> Result<V, String> {
    serde_json::from_slice(&std::fs::read(p).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}
fn localappdata() -> Result<PathBuf, String> {
    std::env::var_os("LOCALAPPDATA").map(PathBuf::from).ok_or_else(|| "LOCALAPPDATA".to_string())
}
fn now_iso() -> String {
    baal_service_runtime::contract::ms_to_iso(
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as f64)
}
fn perf_ms() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs_f64() * 1000.0
}
fn need(ok: bool, code: &str) -> Result<(), String> {
    if ok { Ok(()) } else { Err(format!("PAIR_PRICE_{}", code)) }
}
fn parse_iso_ms(s: &str) -> Option<f64> {
    baal_service_runtime::contract::parse_iso_ms(s)
}
fn sealed_end(path: &Path) -> Result<u64, String> {
    let len = std::fs::metadata(path).map_err(|e| e.to_string())?.len();
    let mut fd = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let back = std::cmp::min(len, 1024 * 1024);
    fd.seek(SeekFrom::Start(len - back)).map_err(|e| e.to_string())?;
    let mut tail = vec![0u8; back as usize];
    fd.read_exact(&mut tail).map_err(|e| e.to_string())?;
    match tail.iter().rposition(|c| *c == b'\n') {
        Some(i) => Ok(len - back + i as u64 + 1),
        None => Err("DUAL_TIMELAPSE_EMPTY".into()),
    }
}

// Fixed diagnostic policy, declared before scanning. Bulk quotes have no
// executable depth: this is opportunity selection, never paper fills or P&L.
pub fn policy() -> V {
    json!({"maximumQuoteAgeMs": 20000, "maximumSkewMs": 10000, "latencyMs": 2000,
        "maximumHoldMs": 1800000, "entryCost": "0.980000",
        "targetQuoteMargin": "0.010000", "perLegAdverseTicks": 1,
        "feePerShare": "0.000000"})
}
fn pol() -> V { policy() }

/// /^Will the (Democratic|Republican) Party win the (.+)\?$/
fn party_match(title: &str) -> Option<(String, String)> {
    let rest = title.strip_prefix("Will the ")?;
    let rest = rest.strip_suffix('?')?;
    let (party, rest) = if let Some(r) = rest.strip_prefix("Democratic Party win the ") {
        ("Democratic", r)
    } else if let Some(r) = rest.strip_prefix("Republican Party win the ") {
        ("Republican", r)
    } else { return None };
    if rest.is_empty() { return None; }
    Some((party.to_string(), rest.to_string()))
}

pub fn party_title_pairs(markets: &[V], universe: &V) -> Vec<V> {
    let scope: indexmap::IndexMap<String, V> = universe["exchanges"].as_array()
        .cloned().unwrap_or_default().into_iter()
        .map(|x| (x["marketId"].to_string().trim_matches('"').to_string(), x))
        .collect();
    let mut group: indexmap::IndexMap<String, Vec<V>> = indexmap::IndexMap::new();
    for m in markets {
        let title = m["title"].as_str().unwrap_or("");
        let Some((party, tail)) = party_match(title) else { continue };
        let market_id = m["id"].to_string().trim_matches('"').to_string();
        let Some(e) = scope.get(&market_id) else { continue };
        if e["option"].as_str() != Some("YES") { continue; }
        group.entry(tail).or_default().push(
            json!({"marketId": m["id"], "exchangeId": e["exchangeId"], "party": party}));
    }
    group.into_iter()
        .filter(|(_, x)| x.len() == 2
            && x.iter().map(|y| y["party"].as_str().unwrap_or("")).collect::<std::collections::HashSet<_>>().len() == 2)
        .map(|(title, mut legs)| {
            legs.sort_by(|a, b| a["party"].as_str().unwrap_or("")
                .cmp(b["party"].as_str().unwrap_or("")));
            json!({"title": title, "legs": legs,
                "contractPredicatesVerified": false})
        }).collect()
}

type Quotes = indexmap::IndexMap<String, V>;

struct Packet {
    buy: num_bigint::BigInt, sell: num_bigint::BigInt,
    requests: Vec<V>, received: Vec<V>,
}
/// pairedQuotePacket — None when unusable (JS returns null).
fn paired_quote_packet(pair: &V, cache: &Quotes, at: f64) -> Result<Option<Packet>, String> {
    let quotes: Vec<V> = pair["legs"].as_array().cloned().unwrap_or_default().iter()
        .map(|l| cache.get(l["exchangeId"].as_str().unwrap_or(""))
            .cloned().unwrap_or(V::Null))
        .collect();
    if quotes.iter().any(|q| q.is_null()) { return Ok(None); }
    let max_age = pol()["maximumQuoteAgeMs"].as_f64().unwrap_or(0.0);
    for q in &quotes {
        let started = parse_iso_ms(q["requestStartedAt"].as_str().unwrap_or(""))
            .unwrap_or(f64::NAN);
        let received = parse_iso_ms(q["receivedAt"].as_str().unwrap_or(""))
            .unwrap_or(f64::NAN);
        if started > at || received > at || at - started > max_age
            || q["bestBid"].is_null() || q["bestAsk"].is_null()
            || q["bestBid"].as_f64().unwrap_or(f64::NAN)
                > q["bestAsk"].as_f64().unwrap_or(f64::NAN) {
            return Ok(None);
        }
    }
    let skew = quotes.iter().map(|q| parse_iso_ms(q["receivedAt"].as_str().unwrap_or(""))
            .unwrap_or(f64::NAN)).fold(f64::NEG_INFINITY, f64::max)
        - quotes.iter().map(|q| parse_iso_ms(q["requestStartedAt"].as_str().unwrap_or(""))
            .unwrap_or(f64::NAN)).fold(f64::INFINITY, f64::min);
    if skew > pol()["maximumSkewMs"].as_f64().unwrap_or(0.0) { return Ok(None); }
    let scale = num_bigint::BigInt::from(baal_paper::fixed::SCALE);
    let tick = num_bigint::BigInt::from(baal_paper::fixed::TICK);
    let fixed = |n: f64| -> Result<num_bigint::BigInt, String> {
        need(n.is_finite() && n >= 0.0 && n <= 1.0, "QUOTE")?;
        baal_paper::fixed::big_amount(&format!("{:.6}", n))
    };
    let zero = num_bigint::BigInt::from(0);
    let mut buy = zero.clone();
    let mut sell = zero.clone();
    for q in &quotes {
        buy += &scale - fixed(q["bestBid"].as_f64().unwrap_or(f64::NAN))? + &tick;
        sell += &scale - fixed(q["bestAsk"].as_f64().unwrap_or(f64::NAN))? - &tick;
    }
    for q in &quotes {
        if &scale - fixed(q["bestBid"].as_f64().unwrap_or(f64::NAN))? + &tick >= scale
            || &scale - fixed(q["bestAsk"].as_f64().unwrap_or(f64::NAN))? - &tick <= zero {
            return Ok(None);
        }
    }
    Ok(Some(Packet { buy, sell,
        requests: quotes.iter().map(|q| q["requestStartedAt"].clone()).collect::<Vec<_>>(),
        received: quotes.iter().map(|q| q["receivedAt"].clone()).collect::<Vec<_>>() }))
}

struct ScanState {
    pair: V, usable_packets: i64, quoted_entries: i64,
    quoted_profitable_exits: i64, quoted_timeouts: i64,
    entry: Option<V>, quote_margins: Vec<String>, holding_ms: Vec<f64>,
    min_buy: Option<num_bigint::BigInt>, last: V,
}
pub struct QuoteReversionScan {
    cache: Quotes,
    states: Vec<ScanState>,
}
pub fn create_quote_reversion_scan(pairs: &[V]) -> QuoteReversionScan {
    QuoteReversionScan { cache: indexmap::IndexMap::new(),
        states: pairs.iter().map(|pair| ScanState { pair: pair.clone(),
            usable_packets: 0, quoted_entries: 0, quoted_profitable_exits: 0,
            quoted_timeouts: 0, entry: None, quote_margins: Vec::new(),
            holding_ms: Vec::new(), min_buy: None, last: V::Null }).collect() }
}
impl QuoteReversionScan {
    pub fn observe(&mut self, snapshot: &V) -> Result<(), String> {
        let at = parse_iso_ms(snapshot["receivedAt"].as_str().unwrap_or(""))
            .unwrap_or(f64::NAN);
        let start = parse_iso_ms(snapshot["requestStartedAt"].as_str().unwrap_or(""))
            .unwrap_or(f64::NAN);
        need(at.fract() == 0.0 && at.is_finite() && start.fract() == 0.0
            && start.is_finite() && at >= start, "CLOCK")?;
        let mut changed: std::collections::HashSet<String> = std::collections::HashSet::new();
        for q in snapshot["quotes"].as_array().cloned().unwrap_or_default() {
            let id = q["exchangeId"].as_str().unwrap_or("").to_string();
            let mut q = q.clone();
            q["requestStartedAt"] = snapshot["requestStartedAt"].clone();
            q["receivedAt"] = snapshot["receivedAt"].clone();
            self.cache.insert(id.clone(), q);
            changed.insert(id);
        }
        let p = pol();
        let latency = p["latencyMs"].as_f64().unwrap_or(0.0);
        let max_hold = p["maximumHoldMs"].as_f64().unwrap_or(0.0);
        let entry_cost = baal_paper::fixed::big_amount(
            p["entryCost"].as_str().unwrap_or(""))?;
        let target = baal_paper::fixed::big_amount(
            p["targetQuoteMargin"].as_str().unwrap_or(""))?;
        for s in &mut self.states {
            if !s.pair["legs"].as_array().cloned().unwrap_or_default().iter()
                .any(|l| changed.contains(l["exchangeId"].as_str().unwrap_or(""))) {
                continue;
            }
            let Some(packet) = paired_quote_packet(&s.pair, &self.cache, at)? else { continue };
            s.usable_packets += 1;
            let buy_i = packet.buy.clone();
            let sell_i = packet.sell.clone();
            s.last = json!({"at": baal_service_runtime::contract::ms_to_iso(at),
                "buy": baal_paper::fixed::money(&buy_i),
                "sell": baal_paper::fixed::money(&sell_i)});
            s.min_buy = Some(match &s.min_buy {
                None => buy_i.clone(),
                Some(m) => if buy_i < *m { buy_i.clone() } else { m.clone() },
            });
            if let Some(e) = s.entry.clone() {
                let entry_at = e["at"].as_f64().unwrap_or(f64::NAN);
                let ready = packet.requests.iter()
                    .all(|t| parse_iso_ms(t.as_str().unwrap_or(""))
                        .unwrap_or(f64::NAN) >= entry_at + latency);
                if !ready { continue; }
                let margin = &sell_i - num_bigint::BigInt::parse_bytes(
                    e["cost"].as_str().unwrap_or("").as_bytes(), 10).unwrap_or_default();
                if margin >= target || at - entry_at >= max_hold {
                    s.quote_margins.push(baal_paper::fixed::money(&margin));
                    s.holding_ms.push(at - entry_at);
                    if margin >= target { s.quoted_profitable_exits += 1; }
                    else { s.quoted_timeouts += 1; }
                    s.entry = None;
                }
                continue;
            }
            if buy_i <= entry_cost {
                s.entry = Some(json!({"at": at, "cost": buy_i.to_string()}));
                s.quoted_entries += 1;
            }
        }
        Ok(())
    }
    pub fn report(&self) -> Vec<V> {
        self.states.iter().map(|s| {
            let total = s.quote_margins.iter().try_fold(num_bigint::BigInt::from(0),
                |n, v| -> Result<num_bigint::BigInt, String> {
                    Ok(n + baal_paper::fixed::big_amount(v)?)
                }).unwrap_or_default();
            json!({"title": s.pair["title"], "legs": s.pair["legs"],
                "contractPredicatesVerified": false,
                "usablePackets": s.usable_packets,
                "minimumQuotedAcquisition": s.min_buy.as_ref()
                    .map(|b| json!(baal_paper::fixed::money(b))).unwrap_or(V::Null),
                "quotedEntrySignals": s.quoted_entries,
                "quotedProfitableExitSignals": s.quoted_profitable_exits,
                "quotedTimeoutExitSignals": s.quoted_timeouts,
                "quotedRoundtripMargins": s.quote_margins,
                "totalClosedQuoteMargin": baal_paper::fixed::money(&total),
                "quoteHoldingMs": s.holding_ms,
                "openQuotedSignal": s.entry.as_ref().map(|e| json!({
                    "at": baal_service_runtime::contract::ms_to_iso(
                        e["at"].as_f64().unwrap_or(f64::NAN)),
                    "cost": baal_paper::fixed::money(&num_bigint::BigInt::parse_bytes(
                        e["cost"].as_str().unwrap_or("").as_bytes(), 10)
                        .unwrap_or_default())
                })).unwrap_or(V::Null),
                "last": s.last, "depthAvailable": false, "executionEvidence": false,
                "simulatedFills": 0, "realizedPnl": V::Null})
        }).collect()
    }
}

pub fn main() -> Result<V, String> {
    let rt = localappdata()?.join("SigTradingCup/paper-runtime");
    let file = rt.join("market-prices.jsonl");
    let universe = read_json(Path::new("live-paper/price-universe.json"))?;
    let markets = read_json(Path::new("data/markets.json"))?;
    let pairs = party_title_pairs(markets.as_array().map(|a| a.as_slice())
        .unwrap_or(&[]), &universe);
    let mut scan = create_quote_reversion_scan(&pairs);
    let end = sealed_end(&file)?;
    let mut raw = Sha256::new();
    let mut sequence = 0i64;
    let mut previous = String::new();
    let mut first = V::Null;
    let mut last = V::Null;
    let started = perf_ms();
    for line in journal_lines(&file, &JournalReadOptions {
            end_byte: Some(end), ..Default::default() })? {
        raw.update(line.as_bytes());
        raw.update(b"\n");
        let row: V = serde_json::from_str(&line).map_err(|e| e.to_string())?;
        let mut body = row.clone();
        let hash = row["hash"].as_str().unwrap_or("").to_string();
        if let V::Object(m) = &mut body { m.shift_remove("hash"); }
        sequence += 1;
        need(row["sequence"].as_i64() == Some(sequence)
            && row["previous"].as_str().unwrap_or("") == previous
            && sha(js_stringify(&body).as_bytes()) == hash, "CHAIN")?;
        previous = hash;
        need(row["event"]["type"].as_str() == Some("market_price_observation"), "SOURCE")?;
        if first.is_null() { first = row["event"]["snapshot"]["receivedAt"].clone(); }
        last = row["event"]["snapshot"]["receivedAt"].clone();
        scan.observe(&row["event"]["snapshot"])?;
    }
    let mut results = scan.report();
    results.sort_by(|a, b| {
        let av = baal_paper::fixed::big_amount(
            a["totalClosedQuoteMargin"].as_str().unwrap_or("")).unwrap_or_default();
        let bv = baal_paper::fixed::big_amount(
            b["totalClosedQuoteMargin"].as_str().unwrap_or("")).unwrap_or_default();
        bv.cmp(&av).then_with(|| b["quotedRoundtripMargins"].as_array()
            .map(|x| x.len()).unwrap_or(0)
            .cmp(&a["quotedRoundtripMargins"].as_array().map(|x| x.len()).unwrap_or(0)))
    });
    let amount = |v: &V| baal_paper::fixed::big_amount(v.as_str().unwrap_or(""))
        .unwrap_or_default();
    let total_margin: num_bigint::BigInt = results.iter()
        .map(|r| amount(&r["totalClosedQuoteMargin"]))
        .fold(num_bigint::BigInt::from(0), |a, b| a + b);
    let summary = json!({"hours": (parse_iso_ms(last.as_str().unwrap_or(""))
            .unwrap_or(f64::NAN)
            - parse_iso_ms(first.as_str().unwrap_or("")).unwrap_or(f64::NAN)) / 3600000.0,
        "quotedEntrySignals": results.iter()
            .map(|r| r["quotedEntrySignals"].as_i64().unwrap_or(0)).sum::<i64>(),
        "quotedClosedSignals": results.iter()
            .map(|r| r["quotedRoundtripMargins"].as_array().map(|x| x.len()).unwrap_or(0))
            .sum::<usize>(),
        "quotedProfitableExits": results.iter()
            .map(|r| r["quotedProfitableExitSignals"].as_i64().unwrap_or(0)).sum::<i64>(),
        "quotedTimeouts": results.iter()
            .map(|r| r["quotedTimeoutExitSignals"].as_i64().unwrap_or(0)).sum::<i64>(),
        "totalClosedQuoteMargin": baal_paper::fixed::money(&total_margin),
        "positiveNetQuotePairs": results.iter()
            .filter(|r| amount(&r["totalClosedQuoteMargin"]) > num_bigint::BigInt::from(0))
            .count()});
    let out = json!({"schemaVersion": 1, "at": now_iso(), "policy": pol(),
        "source": {"prefixBytes": end, "sha256": format!("{:x}", raw.finalize()),
            "records": sequence, "tailHash": previous, "first": first, "last": last},
        "wallMs": perf_ms() - started, "pairsScreened": pairs.len(),
        "summary": summary, "results": results, "competitionOrders": 0,
        "networkRequests": 0, "simulatedFills": 0, "profitabilityEstablished": false,
        "selectionIsExploratory": true,
        "limitations": ["Historical bulk bid/ask quotes contain no executable depth and do not establish fills, order acceptance, queue priority or capital returns.",
            "Title-paired catalogue groups require exact contract-root review; administrative resolution or differing predicates can invalidate payout assumptions.",
            "Quote exit signals omit sequential partial-fill and failed-hedge risk. They rank future exact depth capture only.",
            "Summed quote margins across overlapping opportunities are not a capital-constrained wallet, account return or realized P&L.",
            "No current account eligibility or later contract review was backfilled into historical sources.",
            "Fixed policy is declared before this scan; results are exploratory and require untouched later depth and prospective policy validation."]});
    std::fs::write("data/paired-price-reversion-results-20261005.json",
        format!("{}\n", serde_json::to_string_pretty(&out).map_err(|e| e.to_string())?))
        .map_err(|e| e.to_string())?;
    Ok(json!({"records": sequence, "pairs": pairs.len(), "wallMs": out["wallMs"],
        "summary": out["summary"], "profitabilityEstablished": false}))
}
