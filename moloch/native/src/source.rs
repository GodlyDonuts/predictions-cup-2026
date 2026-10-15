//! Official REST receipt normalization and complete cursor-window proofs.
//! No page, timestamp, or top-of-book implies a fill or gap-free tape.
use crate::{Result, amount, money, price};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
fn time(v: &Value) -> Result<i64> {
    chrono::DateTime::parse_from_rfc3339(v.as_str().ok_or("SOURCE_TIMESTAMP")?)
        .map_err(|_| "SOURCE_TIMESTAMP")?
        .timestamp_nanos_opt()
        .ok_or("SOURCE_TIMESTAMP_RANGE")
}
fn text(v: &Value) -> Result<&str> {
    v.as_str().filter(|s| !s.is_empty()).ok_or("SOURCE_STRING")
}
fn integer(v: &Value) -> Result<u64> {
    v.as_u64()
        .filter(|v| *v <= 9_007_199_254_740_991)
        .ok_or("SOURCE_INTEGER")
}
fn numeric_id(v: &Value) -> Result<String> {
    let s = if let Some(s) = v.as_str() {
        s.to_owned()
    } else {
        integer(v)?.to_string()
    };
    if s.is_empty() || s.starts_with('0') || !s.bytes().all(|b| b.is_ascii_digit()) {
        return Err("SOURCE_ID");
    }
    Ok(s)
}
fn decimal(v: &Value) -> Result<String> {
    if let Some(s) = v.as_str() {
        return Ok(money(price(amount(s)?)?));
    }
    let n = v.as_f64().ok_or("SOURCE_PRICE")?;
    let ticks = n * 200.0;
    if !n.is_finite() || n <= 0.0 || n >= 1.0 || (ticks - ticks.round()).abs() > 1e-7 {
        return Err("SOURCE_PRICE");
    }
    Ok(money(price(ticks.round() as i128 * 5000)?))
}
fn hash(v: &Value) -> String {
    format!("{:x}", Sha256::digest(serde_json::to_vec(v).expect("JSON")))
}
fn url(r: &Value) -> Result<reqwest::Url> {
    let endpoint = text(&r["endpoint"])?;
    if !endpoint.starts_with('/')
        || endpoint.starts_with("//")
        || endpoint.contains(['\\', '\r', '\n', '#'])
    {
        return Err("SOURCE_ENDPOINT");
    }
    let u = reqwest::Url::parse(&format!("https://sig.thesuper.market/api/v1{endpoint}"))
        .map_err(|_| "SOURCE_ENDPOINT")?;
    let mut seen = BTreeSet::new();
    for (k, v) in u.query_pairs() {
        if !seen.insert(k.to_string()) || v.is_empty() {
            return Err("SOURCE_QUERY");
        }
    }
    if u.query_pairs()
        .find(|(k, _)| k == "tournamentId")
        .map(|(_, v)| v.to_string())
        .as_deref()
        != r["tournamentId"].as_str()
    {
        return Err("SOURCE_TOURNAMENT");
    }
    Ok(u)
}
fn scope(r: &Value) -> Result<Value> {
    text(&r["tournamentId"])?;
    Ok(
        json!({"tournamentId":r["tournamentId"],"marketId":numeric_id(&r["marketId"])? ,"exchangeId":numeric_id(&r["exchangeId"])?}),
    )
}
fn scoped_event(kind: &str, r: &Value) -> Result<Value> {
    let mut e = scope(r)?;
    e["type"] = json!(kind);
    e["receivedAt"] = r["receivedAt"].clone();
    Ok(e)
}
pub fn receipt_events(r: &Value) -> Result<Vec<Value>> {
    if r["source"] != "official_sig_api_v1"
        || r["status"] != 200
        || time(&r["requestStartedAt"])? > time(&r["receivedAt"])?
    {
        return Err("SOURCE_RECEIPT");
    }
    let u = url(r)?;
    let s = scope(r)?;
    match r["kind"].as_str() {
        Some("book") => {
            if u.path() != format!("/api/v1/markets/{}/orderbook", text(&s["marketId"])?) {
                return Err("SOURCE_BOOK_ENDPOINT");
            }
            let context = r["data"]["contexts"]
                .as_array()
                .ok_or("SOURCE_CONTEXT")?
                .iter()
                .find(|c| c["type"] == "tournament" && c["tournament"]["id"] == r["tournamentId"])
                .ok_or("SOURCE_CONTEXT")?;
            let b = context["orderbook"]["exchanges"]
                .as_array()
                .ok_or("SOURCE_BOOK")?
                .iter()
                .find(|b| numeric_id(&b["exchangeId"]).ok().as_deref() == s["exchangeId"].as_str())
                .ok_or("SOURCE_BOOK")?;
            let seq = integer(&b["asOf"]["sequence"])?;
            time(&b["asOf"]["at"])?;
            let bids = b["bids"].as_array().ok_or("SOURCE_DEPTH")?;
            let asks = b["asks"].as_array().ok_or("SOURCE_DEPTH")?;
            if bids.is_empty() || asks.is_empty() {
                return Ok(vec![
                    json!({"type":"gap","exchangeId":s["exchangeId"],"receivedAt":r["receivedAt"]}),
                ]);
            }
            let levels = |xs: &Vec<Value>| -> Result<Value> {
                Ok(json!(xs.iter().map(|l|Ok(json!({"price":decimal(&l["price"])?,"quantity":integer(&l["quantity"])?}))).collect::<Result<Vec<_>>>()?))
            };
            let mut book = s;
            book["requestStartedAt"] = r["requestStartedAt"].clone();
            book["receivedAt"] = r["receivedAt"].clone();
            book["sourceAt"] = b["asOf"]["at"].clone();
            book["sequence"] = json!(seq.to_string());
            book["bids"] = levels(bids)?;
            book["asks"] = levels(asks)?;
            Ok(vec![
                json!({"type":"book","receivedAt":r["receivedAt"],"book":book}),
            ])
        }
        Some("trades") => {
            if u.path() != format!("/api/v1/exchanges/{}/trades", text(&s["exchangeId"])?)
                || r["data"]["marketId"] != s["marketId"]
                || r["data"]["exchangeId"] != s["exchangeId"]
            {
                return Err("SOURCE_TAPE_SCOPE");
            }
            let mut trades = Vec::new();
            for t in r["data"]["data"].as_array().ok_or("SOURCE_TAPE")? {
                let p = &t["price"];
                if p.is_null() {
                    continue;
                }
                let raw = p
                    .as_f64()
                    .or_else(|| p.as_str().and_then(|s| s.parse().ok()))
                    .ok_or("SOURCE_PRICE")?;
                if raw <= 0.0 || raw >= 1.0 {
                    continue;
                }
                let executed = time(&t["createdAt"])?;
                if executed > time(&r["receivedAt"])? {
                    return Err("SOURCE_FUTURE_TRADE");
                }
                let mut event = scoped_event("trade", r)?;
                event["id"] = json!(text(&t["id"])?);
                event["executedAt"] = t["createdAt"].clone();
                event["yesPrice"] = json!(decimal(p)?);
                event["quantity"] = json!(integer(&t["size"])?);
                event["idNamespace"] = json!("sig_reporting_tape_only");
                event["sourceRequestStartedAt"] = r["requestStartedAt"].clone();
                trades.push((executed, text(&t["id"])?.to_owned(), event));
            }
            trades.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
            let mut events = trades.into_iter().map(|(_, _, e)| e).collect::<Vec<_>>();
            let p = &r["data"]["pagination"];
            let c = &r["data"]["coverage"];
            if !u.query_pairs().any(|(k, _)| k == "cursor")
                && p["hasMore"] == false
                && p["nextCursor"].is_null()
                && c["complete"] == true
                && integer(&c["projectedThroughSequence"]).is_ok()
            {
                events.push(coverage(r, &u, integer(&c["projectedThroughSequence"])?)?)
            }
            Ok(events)
        }
        Some("history") => Ok(vec![]),
        _ => Err("SOURCE_KIND"),
    }
}
fn window(r: &Value, u: &reqwest::Url) -> Result<(String, String)> {
    let get = |name: &str| {
        u.query_pairs()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.to_string())
            .ok_or("SOURCE_WINDOW")
    };
    let from = get("from")?;
    let to = get("to")?;
    if time(&json!(from))? != time(&r["data"]["from"])?
        || time(&json!(to))? != time(&r["data"]["to"])?
        || time(&json!(from))? >= time(&json!(to))?
        || time(&json!(to))? > time(&r["receivedAt"])?
    {
        return Err("SOURCE_SERVED_WINDOW");
    }
    Ok((from, to))
}
fn coverage(r: &Value, u: &reqwest::Url, sequence: u64) -> Result<Value> {
    window(r, u)?;
    let mut e = scoped_event("coverage", r)?;
    e["from"] = r["data"]["from"].clone();
    e["to"] = r["data"]["to"].clone();
    e["complete"] = json!(true);
    e["projectedThroughSequence"] = json!(sequence.to_string());
    Ok(e)
}
#[derive(Clone)]
struct PageWindow {
    next: String,
    cursors: BTreeSet<String>,
    minimum: Option<u64>,
    complete: bool,
    pages: u64,
    hashes: Vec<String>,
}
#[derive(Clone, Default)]
pub struct TapeCoverage {
    pending: BTreeMap<String, PageWindow>,
}
impl TapeCoverage {
    pub fn accept(&mut self, r: &Value) -> Result<Vec<Value>> {
        let mut next = self.clone();
        let events = next.apply(r)?;
        *self = next;
        Ok(events)
    }
    fn apply(&mut self, r: &Value) -> Result<Vec<Value>> {
        if r["kind"] != "trades" {
            return Ok(vec![]);
        }
        receipt_events(r)?;
        let u = url(r)?;
        let (from, to) = window(r, &u)?;
        let key = format!(
            "{}:{}:{}:{}:{}",
            r["tournamentId"], r["marketId"], r["exchangeId"], from, to
        );
        let cursor = u
            .query_pairs()
            .find(|(k, _)| k == "cursor")
            .map(|(_, v)| v.to_string());
        let p = &r["data"]["pagination"];
        let more = p["hasMore"].as_bool().ok_or("SOURCE_PAGE_CURSOR")?;
        let next = if more {
            Some(text(&p["nextCursor"])?.to_owned())
        } else {
            if !p["nextCursor"].is_null() {
                return Err("SOURCE_PAGE_CURSOR");
            }
            None
        };
        if cursor.is_none() {
            if !more {
                return Ok(vec![]);
            }
            if self.pending.len() >= 64 {
                return Err("SOURCE_WINDOW_LIMIT");
            }
            self.pending.insert(
                key.clone(),
                PageWindow {
                    next: next.clone().unwrap(),
                    cursors: BTreeSet::new(),
                    minimum: None,
                    complete: true,
                    pages: 0,
                    hashes: Vec::new(),
                },
            );
        }
        let Some(w) = self.pending.get_mut(&key) else {
            return Ok(vec![]);
        };
        if let Some(c) = &cursor {
            if *c != w.next || !w.cursors.insert(c.clone()) {
                return Err("SOURCE_CURSOR_CHAIN");
            }
        }
        w.pages += 1;
        if w.pages > 5 {
            return Err("SOURCE_PAGE_BOUND");
        }
        let checkpoint = integer(&r["data"]["coverage"]["projectedThroughSequence"]).ok();
        w.complete &= r["data"]["coverage"]["complete"] == true && checkpoint.is_some();
        if let Some(s) = checkpoint {
            w.minimum = Some(w.minimum.map_or(s, |m| m.min(s)));
        }
        w.hashes.push(hash(r));
        if let Some(n) = next {
            if w.cursors.contains(&n) || cursor.as_ref() == Some(&n) {
                return Err("SOURCE_CURSOR_LOOP");
            }
            w.next = n;
            return Ok(vec![]);
        }
        let w = self.pending.remove(&key).unwrap();
        if !w.complete {
            return Ok(vec![]);
        }
        let mut event = coverage(r, &u, w.minimum.ok_or("SOURCE_CHECKPOINT")?)?;
        event["sourcePagination"] = json!({"pages":w.pages,"receiptHashes":w.hashes,"checkpointUse":"minimum verified page projection sequence"});
        Ok(vec![event])
    }
}
#[derive(Clone)]
struct Scope {
    market: String,
    exchange: String,
    invalidated: i64,
    blocked: bool,
    book_ready: bool,
    tape_ready: bool,
    book: Option<Value>,
}
#[derive(Clone)]
pub struct RestSource {
    tournament: String,
    scopes: BTreeMap<String, Scope>,
    tape: TapeCoverage,
    last: Option<i64>,
}
impl RestSource {
    pub fn new(pairs: &Value, tournament: &str, started: &str) -> Result<Self> {
        let started = time(&json!(started))?;
        let mut scopes = BTreeMap::new();
        let mut exchanges = BTreeSet::new();
        for p in pairs.as_array().ok_or("SOURCE_SCOPES")? {
            let market = numeric_id(&p["marketId"])?;
            let exchange = numeric_id(&p["exchangeId"])?;
            if scopes.contains_key(&market) || !exchanges.insert(exchange.clone()) {
                return Err("SOURCE_DUPLICATE_SCOPE");
            }
            scopes.insert(
                market.clone(),
                Scope {
                    market,
                    exchange,
                    invalidated: started,
                    blocked: true,
                    book_ready: false,
                    tape_ready: false,
                    book: None,
                },
            );
        }
        if scopes.is_empty() || scopes.len() > 4 || tournament.is_empty() {
            return Err("SOURCE_SCOPES");
        }
        Ok(Self {
            tournament: tournament.into(),
            scopes,
            tape: TapeCoverage::default(),
            last: None,
        })
    }
    pub fn accept(&mut self, r: &Value) -> Result<Vec<Value>> {
        let mut next = self.clone();
        let events = next.apply(r)?;
        *self = next;
        Ok(events)
    }
    fn apply(&mut self, r: &Value) -> Result<Vec<Value>> {
        let received = time(&r["receivedAt"])?;
        if self.last.is_some_and(|t| received < t) {
            return Err("SOURCE_RECEIPT_ORDER");
        }
        let market = text(&r["marketId"])?;
        let s = self.scopes.get_mut(market).ok_or("SOURCE_SCOPE")?;
        if r["exchangeId"] != s.exchange || r["tournamentId"] != self.tournament {
            return Err("SOURCE_SCOPE");
        }
        let mut events = receipt_events(r)?;
        events.extend(self.tape.accept(r)?);
        let mut out = Vec::new();
        for e in &events {
            match e["type"].as_str() {
                Some("book") => {
                    let b = &e["book"];
                    let mut accepted = true;
                    if let Some(old) = &s.book {
                        let old_sequence = text(&old["sequence"])?
                            .parse::<u128>()
                            .map_err(|_| "SOURCE_SEQUENCE")?;
                        let sequence = text(&b["sequence"])?
                            .parse::<u128>()
                            .map_err(|_| "SOURCE_SEQUENCE")?;
                        accepted = sequence > old_sequence
                            || sequence == old_sequence
                                && time(&b["sourceAt"])? >= time(&old["sourceAt"])?;
                        if sequence == old_sequence
                            && time(&b["sourceAt"])? == time(&old["sourceAt"])?
                            && (b["bids"] != old["bids"] || b["asks"] != old["asks"])
                        {
                            return Err("SOURCE_BOOK_CONFLICT");
                        }
                    }
                    if accepted {
                        s.book = Some(b.clone());
                        if s.blocked {
                            let mut invalid = e.clone();
                            invalid["book"]["invalid"] = json!(true);
                            out.push(invalid)
                        }
                    }
                    if time(&r["requestStartedAt"])? >= s.invalidated {
                        s.book_ready = true
                    }
                }
                Some("coverage") => {
                    out.push(e.clone());
                    if time(&e["from"])? <= s.invalidated && time(&e["to"])? >= s.invalidated {
                        s.tape_ready = true
                    }
                }
                Some("trade") => out.push(e.clone()),
                Some("gap") => {
                    s.invalidated = received;
                    s.blocked = true;
                    s.book_ready = false;
                    s.tape_ready = false;
                    out.push(e.clone())
                }
                _ => {}
            }
        }
        if s.blocked && s.book_ready && s.tape_ready {
            s.blocked = false
        }
        if !s.blocked
            && events
                .iter()
                .any(|e| e["type"] == "book" || e["type"] == "coverage")
        {
            if let Some(b) = &s.book {
                let mut b = b.clone();
                b["receivedAt"] = r["receivedAt"].clone();
                out.push(json!({"type":"book","receivedAt":r["receivedAt"],"book":b}));
            }
        }
        self.last = Some(received);
        Ok(out)
    }
    pub fn scopes(&self) -> Value {
        json!(self.scopes.values().map(|s|json!({"marketId":s.market,"exchangeId":s.exchange,"blocked":s.blocked,"bookReady":s.book_ready,"tapeReady":s.tape_ready})).collect::<Vec<_>>())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn page(cursor: Option<&str>, next: Option<&str>, seq: Value) -> Value {
        json!({"kind":"trades","source":"official_sig_api_v1","status":200,"marketId":"1","exchangeId":"2","tournamentId":"cup","requestStartedAt":"2026-10-14T00:01:00Z","receivedAt":"2026-10-14T00:01:01Z","endpoint":format!("/exchanges/2/trades?tournamentId=cup&from=2026-10-14T00:00:00Z&to=2026-10-14T00:01:00Z{}",cursor.map(|c|format!("&cursor={c}")).unwrap_or_default()),"data":{"marketId":"1","exchangeId":"2","from":"2026-10-14T00:00:00Z","to":"2026-10-14T00:01:00Z","data":[],"pagination":{"hasMore":next.is_some(),"nextCursor":next},"coverage":{"complete":true,"projectedThroughSequence":seq}}})
    }
    #[test]
    fn full_chain_minimum_checkpoint() {
        let mut c = TapeCoverage::default();
        assert!(
            c.accept(&page(None, Some("a"), json!(100)))
                .unwrap()
                .is_empty()
        );
        assert!(
            c.accept(&page(Some("a"), Some("b"), json!(105)))
                .unwrap()
                .is_empty()
        );
        let e = c.accept(&page(Some("b"), None, json!(103))).unwrap();
        assert_eq!(e[0]["projectedThroughSequence"], "100");
        assert_eq!(e[0]["sourcePagination"]["pages"], 3);
        assert!(
            TapeCoverage::default()
                .accept(&page(Some("b"), None, json!(103)))
                .unwrap()
                .is_empty()
        )
    }
    #[test]
    fn missing_reordered_cyclic_incomplete_pages() {
        let mut c = TapeCoverage::default();
        c.accept(&page(None, Some("a"), json!(100))).unwrap();
        assert!(c.accept(&page(Some("b"), None, json!(100))).is_err());
        assert!(c.accept(&page(Some("a"), Some("a"), json!(100))).is_err());
        assert_eq!(
            c.accept(&page(Some("a"), None, json!(100))).unwrap().len(),
            1
        );
        let mut d = TapeCoverage::default();
        d.accept(&page(None, Some("a"), Value::Null)).unwrap();
        assert!(
            d.accept(&page(Some("a"), None, json!(100)))
                .unwrap()
                .is_empty()
        )
    }
    #[test]
    fn served_window_and_foreign_context_fail() {
        let mut r = page(None, None, json!(100));
        r["data"]["from"] = r["data"]["to"].clone();
        assert!(receipt_events(&r).is_err());
        r = page(None, None, json!(100));
        r["tournamentId"] = json!("foreign");
        assert!(receipt_events(&r).is_err())
    }
    #[test]
    fn initial_resync_requires_book_and_complete_tape() {
        let book = json!({"kind":"book","source":"official_sig_api_v1","status":200,"marketId":"1","exchangeId":"2","tournamentId":"cup","requestStartedAt":"2026-10-14T00:00:00Z","receivedAt":"2026-10-14T00:00:01Z","endpoint":"/markets/1/orderbook?tournamentId=cup&depth=200","data":{"contexts":[{"type":"tournament","tournament":{"id":"cup"},"orderbook":{"exchanges":[{"exchangeId":"2","asOf":{"sequence":100,"at":"2026-10-14T00:00:00Z"},"bids":[{"price":0.45,"quantity":100}],"asks":[{"price":0.455,"quantity":100}]}]}}]}});
        let mut s = RestSource::new(
            &json!([{"marketId":"1","exchangeId":"2"}]),
            "cup",
            "2026-10-14T00:00:00Z",
        )
        .unwrap();
        assert_eq!(s.accept(&book).unwrap()[0]["book"]["invalid"], true);
        let out = s.accept(&page(None, None, json!(100))).unwrap();
        assert_eq!(out.len(), 2);
        assert_eq!(out[0]["type"], "coverage");
        assert!(out[1]["book"]["invalid"].is_null());
        assert_eq!(s.scopes()[0]["blocked"], false)
    }
}
