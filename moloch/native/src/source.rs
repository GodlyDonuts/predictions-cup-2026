//! Official REST receipt normalization and complete cursor-window proofs.
//! No page, timestamp, or top-of-book implies a fill or gap-free tape.
use crate::{Result, amount, money, price};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
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
struct TradeFact {
    market: Value,
    price: Value,
    quantity: Value,
    executed: i64,
    stamp: Value,
    namespace: Value,
}
#[derive(Clone)]
pub struct RestSource {
    tournament: String,
    scopes: BTreeMap<String, Scope>,
    tape: TapeCoverage,
    last: Option<i64>,
    revisions: BTreeMap<String, u64>,
    facts: Arc<BTreeMap<String, TradeFact>>,
}
impl RestSource {
    pub fn seed_trade_facts(&mut self, mut events: Vec<Value>) -> Result<()> {
        let mut next = self.clone();
        next.normalize_facts(&mut events)?;
        *self = next;
        Ok(())
    }
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
        // Registry size is independent of the number of concurrently polled books.
        if scopes.is_empty() || scopes.len() > 1000 || tournament.is_empty() {
            return Err("SOURCE_SCOPES");
        }
        Ok(Self {
            tournament: tournament.into(),
            scopes,
            tape: TapeCoverage::default(),
            last: None,
            revisions: BTreeMap::new(),
            facts: Arc::new(BTreeMap::new()),
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
                let at = if time(&b["receivedAt"])? > time(&r["receivedAt"])? {
                    b["receivedAt"].clone()
                } else {
                    r["receivedAt"].clone()
                };
                b["receivedAt"] = at.clone();
                out.push(json!({"type":"book","receivedAt":at,"book":b}));
            }
        }
        self.normalize_facts(&mut out)?;
        self.last = Some(received);
        Ok(out)
    }
    fn normalize_facts(&mut self, events: &mut [Value]) -> Result<()> {
        for e in events.iter_mut().filter(|e| e["type"] == "trade") {
            let key = format!("{}:{}", text(&e["exchangeId"])?, text(&e["id"])?);
            let executed = time(&e["executedAt"])?;
            if let Some(old) = self.facts.get(&key) {
                if old.market != e["marketId"]
                    || old.price != e["yesPrice"]
                    || old.quantity != e["quantity"]
                {
                    return Err("SOURCE_TRADE_FACT_CONFLICT");
                }
                let reporting = if old.namespace == "sig_reporting_tape_only" {
                    Some(old.executed)
                } else if e["idNamespace"] == "sig_reporting_tape_only" {
                    Some(executed)
                } else {
                    None
                };
                if old.executed != executed
                    && !(old.executed / 1_000_000 == executed / 1_000_000
                        && reporting.is_some_and(|n| n % 1_000_000 == 0)
                        && old.namespace != e["idNamespace"])
                {
                    return Err("SOURCE_TRADE_CLOCK_CONFLICT");
                }
                if old.stamp != e["executedAt"] {
                    e["originalExecutedAt"] = e["executedAt"].clone();
                    e["executedAt"] = old.stamp.clone();
                    e["executionClockUse"] = json!(
                        "first verified representation for duplicate only; original wire clock retained"
                    );
                }
            } else {
                Arc::make_mut(&mut self.facts).insert(
                    key,
                    TradeFact {
                        market: e["marketId"].clone(),
                        price: e["yesPrice"].clone(),
                        quantity: e["quantity"].clone(),
                        executed,
                        stamp: e["executedAt"].clone(),
                        namespace: e["idNamespace"].clone(),
                    },
                );
            }
        }
        Ok(())
    }
    fn invalidate(&mut self, market: &str, received: &Value) -> Result<Vec<Value>> {
        let s = self.scopes.get_mut(market).ok_or("SOURCE_STREAM_SCOPE")?;
        s.invalidated = time(received)?;
        s.blocked = true;
        s.book_ready = false;
        s.tape_ready = false;
        Ok(vec![
            json!({"type":"gap","exchangeId":s.exchange,"receivedAt":received}),
        ])
    }
    pub fn subscribed(&mut self, received: &str) -> Result<Vec<Value>> {
        let mut next = self.clone();
        let mut events = Vec::new();
        for market in self.scopes.keys() {
            events.extend(next.invalidate(market, &json!(received))?);
        }
        *self = next;
        Ok(events)
    }
    pub fn subscribed_market(&mut self, market: &str, received: &str) -> Result<Vec<Value>> {
        let mut next = self.clone();
        let out = next.invalidate(market, &json!(received))?;
        *self = next;
        Ok(out)
    }
    pub fn broadcast(
        &mut self,
        event: &str,
        payload: &Value,
        received: &str,
    ) -> Result<Vec<Value>> {
        let mut next = self.clone();
        let received = json!(received);
        time(&received)?;
        let mut events = Vec::new();
        if event == "markets_batch" {
            let markets = payload["markets"]
                .as_array()
                .filter(|xs| xs.len() <= 1000)
                .ok_or("SOURCE_STREAM_AGGREGATE")?;
            let mut seen = BTreeSet::new();
            for m in markets {
                let market = numeric_id(&m["marketId"])?;
                if !seen.insert(market.clone()) {
                    return Err("SOURCE_STREAM_DUPLICATE_MARKET");
                }
                if next.scopes.contains_key(&market) {
                    events.extend(next.batch(&market, &m["batch"], &received)?);
                }
            }
        } else if ["book_dirty", "market_settled"].contains(&event) {
            let market = numeric_id(&payload["marketId"])?;
            if next.scopes.contains_key(&market) {
                next.check_scope(payload, &market)?;
                if event == "book_dirty" {
                    if numeric_id(&payload["exchangeId"])? != next.scopes[&market].exchange {
                        return Err("SOURCE_STREAM_SCOPE");
                    }
                }
                time(&payload["at"])?;
                events.extend(next.invalidate(&market, &received)?);
            }
        } else {
            return Err("SOURCE_STREAM_EVENT");
        }
        next.normalize_facts(&mut events)?;
        *self = next;
        Ok(events)
    }
    fn check_scope(&self, v: &Value, market: &str) -> Result<()> {
        if numeric_id(&v["marketId"])? != market || v["tournamentId"] != self.tournament {
            return Err("SOURCE_STREAM_SCOPE");
        }
        Ok(())
    }
    fn batch(&mut self, market: &str, b: &Value, received: &Value) -> Result<Vec<Value>> {
        let d = &b["delivery"];
        let revision = integer(&d["revision"])?;
        let previous = integer(&d["previousRevision"])?;
        if d["model"] != "best-effort-authoritative-resync" || revision == 0 || revision <= previous
        {
            return Err("SOURCE_STREAM_REVISION");
        }
        match (
            d["sourceSequenceFrom"].is_null(),
            d["sourceSequenceThrough"].is_null(),
        ) {
            (true, true) => {}
            (false, false) => {
                if integer(&d["sourceSequenceFrom"])? > integer(&d["sourceSequenceThrough"])? {
                    return Err("SOURCE_STREAM_SEQUENCE");
                }
            }
            _ => return Err("SOURCE_STREAM_SEQUENCE"),
        }
        if !d["correlationId"].as_str().is_some_and(|s| s.len() <= 256)
            || b.get("resyncRequired").is_some_and(|v| !v.is_boolean())
        {
            return Err("SOURCE_STREAM_DELIVERY");
        }
        let compact = b["resyncRequired"] == true;
        let empty = Vec::new();
        let array = |k: &str| -> Result<&Vec<Value>> {
            if let Some(v) = b.get(k) {
                v.as_array()
                    .filter(|v| v.len() <= 100000)
                    .ok_or("SOURCE_STREAM_ARRAY")
            } else if compact || k == "books" {
                Ok(&empty)
            } else {
                Err("SOURCE_STREAM_ARRAY")
            }
        };
        let books = array("books")?;
        let trades = array("trades")?;
        let dirty = array("bookDirty")?;
        let settled = array("marketSettled")?;
        let exchange = self.scopes[market].exchange.clone();
        let mut normalized_books = Vec::new();
        let mut normalized_trades = Vec::new();
        let mut seen = BTreeSet::new();
        let stamp = |v: &Value| -> Result<String> {
            Ok(chrono::DateTime::from_timestamp_nanos(time(v)?)
                .to_rfc3339_opts(chrono::SecondsFormat::Nanos, true))
        };
        for book in books {
            let ex = numeric_id(&book["exchangeId"])?;
            if ex != exchange || !seen.insert(ex) {
                return Err("SOURCE_STREAM_BOOK_SCOPE");
            }
            if book.get("nextExpiryAt").is_none() {
                return Err("SOURCE_STREAM_EXPIRY");
            }
            if !book["nextExpiryAt"].is_null() {
                time(&book["nextExpiryAt"])?;
            }
            let sequence = integer(&book["asOf"]["sequence"])?;
            let source = stamp(&book["asOf"]["at"])?;
            if time(&json!(source))? > time(received)? {
                return Err("SOURCE_STREAM_FUTURE_BOOK");
            }
            let levels = |key: &str, bid: bool| -> Result<Value> {
                let mut rows = Vec::new();
                let mut last = None;
                for l in book[key]
                    .as_array()
                    .filter(|v| v.len() <= 100000)
                    .ok_or("SOURCE_STREAM_LEVELS")?
                {
                    let value = decimal(&l["price"])?;
                    let p = amount(&value)?;
                    let n = integer(&l["quantity"])?;
                    if n == 0 || last.is_some_and(|old| if bid { old <= p } else { old >= p }) {
                        return Err("SOURCE_STREAM_LEVELS");
                    }
                    last = Some(p);
                    rows.push(json!({"price":value,"quantity":n}));
                }
                Ok(json!(rows))
            };
            normalized_books.push(json!({"tournamentId":self.tournament,"marketId":market,"exchangeId":exchange,"sequence":sequence.to_string(),"sourceAt":source,"requestStartedAt":received,"receivedAt":received,"transport":"official_sig_realtime","bids":levels("bids",true)?,"asks":levels("asks",false)?}));
        }
        let mut unsequenced = false;
        for t in trades {
            self.check_scope(t, market)?;
            if numeric_id(&t["exchangeId"])? != exchange {
                return Err("SOURCE_STREAM_TRADE_SCOPE");
            }
            if t["id"].is_null() != t["sequence"].is_null() {
                return Err("SOURCE_STREAM_TRADE_SEQUENCE");
            }
            let size = integer(&t["quantity"])?;
            if size == 0 || size > 2_147_483_647 {
                return Err("SOURCE_STREAM_TRADE_SIZE");
            }
            let executed = stamp(&t["executedAt"])?;
            if time(&json!(executed))? > time(received)? {
                return Err("SOURCE_STREAM_FUTURE_TRADE");
            }
            let yes = decimal(&t["price"])?;
            if t["sequence"].is_null() {
                unsequenced = true;
                continue;
            }
            integer(&t["sequence"])?;
            let id = text(&t["id"])?;
            if id.len() > 128
                || !id
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
            {
                return Err("SOURCE_STREAM_TRADE_ID");
            }
            normalized_trades.push(json!({"type":"trade","tournamentId":self.tournament,"marketId":market,"exchangeId":exchange,"id":id,"idNamespace":"sig_engine_match","executedAt":executed,"receivedAt":received,"yesPrice":yes,"quantity":size}));
        }
        for v in dirty {
            self.check_scope(v, market)?;
            if numeric_id(&v["exchangeId"])? != exchange {
                return Err("SOURCE_STREAM_DIRTY_SCOPE");
            }
            time(&v["at"])?;
        }
        for v in settled {
            self.check_scope(v, market)?;
            if !v["settledWith"].as_str().is_some_and(|s| s.len() <= 256) {
                return Err("SOURCE_STREAM_SETTLEMENT");
            }
            time(&v["at"])?;
        }
        let last = self.revisions.get(market).copied();
        let duplicate = last.is_some_and(|n| revision <= n);
        let gap = last.is_some_and(|n| previous > n);
        if !duplicate {
            self.revisions.insert(market.into(), revision);
        }
        let bad = gap || compact || unsequenced || !settled.is_empty();
        let mut out = if bad {
            self.invalidate(market, received)?
        } else {
            Vec::new()
        };
        let s = self.scopes.get_mut(market).unwrap();
        if !duplicate && !bad && !s.blocked {
            out.extend(normalized_trades)
        }
        for book in normalized_books {
            if book["bids"].as_array().unwrap().is_empty()
                || book["asks"].as_array().unwrap().is_empty()
            {
                s.invalidated = time(received)?;
                s.blocked = true;
                s.book_ready = false;
                s.tape_ready = false;
                out.push(json!({"type":"gap","exchangeId":exchange,"receivedAt":received}));
                continue;
            }
            let sequence = text(&book["sequence"])?
                .parse::<u128>()
                .map_err(|_| "SOURCE_SEQUENCE")?;
            let accepted = if let Some(old) = &s.book {
                let old_sequence = text(&old["sequence"])?
                    .parse::<u128>()
                    .map_err(|_| "SOURCE_SEQUENCE")?;
                if sequence == old_sequence
                    && time(&book["sourceAt"])? == time(&old["sourceAt"])?
                    && (book["bids"] != old["bids"] || book["asks"] != old["asks"])
                {
                    return Err("SOURCE_BOOK_CONFLICT");
                }
                sequence > old_sequence
                    || sequence == old_sequence
                        && time(&book["sourceAt"])? >= time(&old["sourceAt"])?
            } else {
                true
            };
            if accepted {
                s.book = Some(book.clone());
                if !s.blocked {
                    out.push(json!({"type":"book","receivedAt":received,"book":book}));
                }
            }
        }
        Ok(out)
    }
    pub fn scopes(&self) -> Value {
        json!(self.scopes.values().map(|s|json!({"marketId":s.market,"exchangeId":s.exchange,"blocked":s.blocked,"bookReady":s.book_ready,"tapeReady":s.tape_ready})).collect::<Vec<_>>())
    }
    /// A late socket gap must not fall behind the next rolling REST window.
    pub fn recovery_start_ms(&self,market:&str)->Option<u64>{
        let scope=self.scopes.get(market)?;
        if scope.blocked {u64::try_from(scope.invalidated/1_000_000).ok().map(|t|t.saturating_sub(1))}else{None}
    }
}
#[cfg(test)]
mod tests {
    #[test]
    fn market_registry_can_watch_the_venue_without_expanding_read_concurrency() {
        let pairs: serde_json::Value = (1..=237u64).map(|id| serde_json::json!({"marketId":id.to_string(),"exchangeId":(id+1000).to_string()})).collect::<Vec<_>>().into();
        let source = super::RestSource::new(&pairs,"cup","2026-10-15T00:00:00Z").unwrap();
        assert_eq!(source.scopes().as_array().unwrap().len(),237);
        let too_many: serde_json::Value = (1..=1001u64).map(|id| serde_json::json!({"marketId":id.to_string(),"exchangeId":(id+2000).to_string()})).collect::<Vec<_>>().into();
        assert!(super::RestSource::new(&too_many,"cup","2026-10-15T00:00:00Z").is_err());
    }
    use super::*;
    #[test]
    fn late_socket_gap_retains_the_actual_recovery_start(){
        let mut source=RestSource::new(&json!([{"marketId":"1","exchangeId":"2"}]),"cup","2026-10-14T00:00:00Z").unwrap();
        let at=chrono::DateTime::parse_from_rfc3339("2026-10-14T00:00:10Z").unwrap().timestamp_millis() as u64;
        source.subscribed_market("1","2026-10-14T00:00:10Z").unwrap();
        assert_eq!(source.recovery_start_ms("1"),Some(at-1));
        assert_eq!(source.recovery_start_ms("missing"),None);
        // A rolling window already beyond this instant must be extended backwards.
        assert!(source.recovery_start_ms("1").unwrap()<at+120000);
    }
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
