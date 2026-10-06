use crate::money::price;
use chrono::{DateTime, SecondsFormat, Timelike};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use std::{collections::HashSet, fs::File, io::{BufRead, BufReader, Read}, path::Path, time::Instant};

const SAFE: i64 = 9_007_199_254_740_991;
const MAX_RECORD: usize = 8 * 1024 * 1024;
fn need(ok: bool, code: &str) -> Result<(), String> {
    if ok { Ok(()) } else { Err(code.to_owned()) }
}
fn object(v: &Value) -> Result<&Map<String, Value>, String> {
    let o = v.as_object().ok_or("PUBLIC_BOOK_RECORD_INVALID")?;
    need(!["__proto__", "constructor", "prototype"].iter().any(|k| o.contains_key(*k)), "PUBLIC_BOOK_RECORD_INVALID")?;
    Ok(o)
}
fn exact(v: &Value, keys: &[&str]) -> Result<(), String> {
    let o = object(v)?;
    need(o.len() == keys.len() && keys.iter().all(|k| o.contains_key(*k)), "PUBLIC_BOOK_RECORD_INVALID")
}
fn text<'a>(v: &'a Value, key: &str) -> Result<&'a str, String> {
    v.get(key).and_then(Value::as_str).ok_or_else(|| "PUBLIC_BOOK_RECORD_INVALID".into())
}
fn integer(v: &Value) -> Result<i64, String> {
    let n = v.as_f64().ok_or("PUBLIC_BOOK_RECORD_INVALID")?;
    need(n.is_finite() && n.fract() == 0.0 && n.abs() <= SAFE as f64, "PUBLIC_BOOK_RECORD_INVALID")?;
    Ok(n as i64)
}
fn id(s: &str) -> bool {
    !s.is_empty() && !s.starts_with('0') && s.bytes().all(|b| b.is_ascii_digit()) && s.parse::<i64>().is_ok_and(|n| n <= SAFE)
}
fn sequence(s: &str) -> Result<i64, String> {
    need(s == "0" || id(s), "PUBLIC_BOOK_RECORD_INVALID")?;
    s.parse().map_err(|_| "PUBLIC_BOOK_RECORD_INVALID".into())
}
fn session(s: &str) -> bool {
    let b = s.as_bytes();
    !b.is_empty() && b.len() <= 128 && b[0].is_ascii_alphanumeric()
        && b.iter().all(|c| c.is_ascii_alphanumeric() || b":_.-".contains(c))
}
fn clock(s: &str, nanos: bool) -> Result<(i64, i128), String> {
    let b = s.as_bytes();
    let length = if nanos { 30 } else { 24 };
    need(b.len() == length && [4, 7].iter().all(|i| b[*i] == b'-') && b[10] == b'T'
        && b[13] == b':' && b[16] == b':' && b[19] == b'.' && b[length - 1] == b'Z'
        && b.iter().enumerate().all(|(i,c)| [4,7,10,13,16,19,length-1].contains(&i) || c.is_ascii_digit()), "PUBLIC_BOOK_CLOCK_INVALID")?;
    let dt = DateTime::parse_from_rfc3339(s).map_err(|_| "PUBLIC_BOOK_CLOCK_INVALID")?;
    need(dt.second() < 60 && dt.timestamp_subsec_nanos() < 1_000_000_000
        && (!nanos || &s[..4] >= "1970")
        && dt.to_rfc3339_opts(if nanos { SecondsFormat::Nanos } else { SecondsFormat::Millis }, true) == s,
        "PUBLIC_BOOK_CLOCK_INVALID")?;
    Ok((dt.timestamp_millis(), dt.timestamp() as i128 * 1_000_000_000 + dt.timestamp_subsec_nanos() as i128))
}
// All accepted numeric fields are safe integers. Normalize JSON 1.0 and -0
// exactly as JSON.parse/JSON.stringify do before canonical record hashing.
fn canonical(v: &Value) -> Value {
    match v {
        Value::Object(o) => {
            let mut keys: Vec<_> = o.keys().collect(); keys.sort();
            let mut m = Map::new();
            for k in keys { m.insert(k.clone(), canonical(&o[k])); }
            Value::Object(m)
        }
        Value::Array(a) => Value::Array(a.iter().map(canonical).collect()),
        Value::Number(n) => {
            if let Some(f) = n.as_f64() && f.fract() == 0.0 && f.abs() <= SAFE as f64 { json!(f as i64) }
            else { v.clone() }
        }
        _ => v.clone(),
    }
}
fn hash(v: &Value) -> Result<String, String> {
    Ok(format!("{:x}", Sha256::digest(serde_json::to_vec(&canonical(v)).map_err(|_| "PUBLIC_BOOK_JSON_INVALID")?)))
}
fn scope(v: &Value) -> Result<Value, String> {
    object(v)?;
    let tournament = text(v,"tournamentId")?;
    let b = tournament.as_bytes();
    need(b.len() == 36 && b.iter().enumerate().all(|(i,c)| if [8,13,18,23].contains(&i) { *c == b'-' } else { c.is_ascii_hexdigit() }), "PUBLIC_BOOK_SCOPE_INVALID")?;
    let rows = v["exchanges"].as_array().ok_or("PUBLIC_BOOK_SCOPE_INVALID")?;
    need(!rows.is_empty() && rows.len() <= 10_000, "PUBLIC_BOOK_SCOPE_INVALID")?;
    let mut markets = HashSet::new();let mut exchanges = HashSet::new();let mut result = Vec::new();
    for row in rows {
        object(row)?;let market = text(row,"marketId")?;let exchange = text(row,"exchangeId")?;
        need(id(market) && id(exchange) && row["option"] == "YES" && markets.insert(market.to_owned()) && exchanges.insert(exchange.to_owned()), "PUBLIC_BOOK_SCOPE_INVALID")?;
        result.push(json!({"marketId":market,"exchangeId":exchange,"option":"YES"}));
    }
    result.sort_by_key(|r| r["marketId"].as_str().unwrap().parse::<i64>().unwrap());
    Ok(json!({"tournamentId":tournament,"exchanges":result}))
}
fn validate_levels(v: &Value, descending: bool) -> Result<(), String> {
    let rows = v.as_array().ok_or("PUBLIC_BOOK_RECORD_INVALID")?;
    need(!rows.is_empty() && rows.len() <= 20, "PUBLIC_BOOK_RECORD_INVALID")?;
    let mut prior = None;
    for row in rows {
        exact(row,&["price","quantity"])?;let p = price(text(row,"price")?)?;
        need(integer(&row["quantity"])? > 0 && prior.is_none_or(|previous| if descending { p < previous } else { p > previous }), "PUBLIC_BOOK_RECORD_INVALID")?;
        prior = Some(p);
    }
    Ok(())
}
pub struct Publication {
    scope: Value, scope_hash: String, tournament: String, session: Option<String>, sessions: Vec<String>,
    closed: bool, sequence: i64, hash: String, last_at: Option<String>, books: Map<String,Value>, versions: Map<String,Value>,
    global: Value, markets: Map<String,Value>,
}
impl Publication {
    pub fn new(input: &Value) -> Result<Self,String> {
        let scope = scope(input)?;let scope_hash = hash(&scope)?;let tournament = text(&scope,"tournamentId")?.to_owned();
        Ok(Self {scope,scope_hash,tournament,session:None,sessions:Vec::new(),closed:true,sequence:0,hash:String::new(),last_at:None,
            books:Map::new(),versions:Map::new(),global:Value::Null,markets:Map::new()})
    }
    pub fn sequence(&self)->i64{self.sequence}
    pub fn tail_hash(&self)->&str{&self.hash}
    fn validate_book(&self, book: &Value, at: &str) -> Result<Value,String> {
        exact(book,&["tournamentId","marketId","exchangeId","instrument","receivedAt","requestStartedAt","sourceAt","sequence","versionKey","bids","asks","freshnessPolicy","sourceEventAgeMs"])?;
        let market = text(book,"marketId")?;let exchange = text(book,"exchangeId")?;
        let spec = self.scope["exchanges"].as_array().unwrap().iter().find(|r| r["marketId"] == market).ok_or("PUBLIC_BOOK_RECORD_INVALID")?;
        need(spec["exchangeId"] == exchange && book["tournamentId"] == self.tournament && book["instrument"] == format!("{exchange}:YES")
            && book["freshnessPolicy"] == "authoritative_receipt_v1", "PUBLIC_BOOK_RECORD_INVALID")?;
        let started = clock(text(book,"requestStartedAt")?,false)?.0;let received = clock(text(book,"receivedAt")?,false)?.0;
        let published = clock(at,false)?.0;let (source_ms,source_ns) = clock(text(book,"sourceAt")?,true)?;
        need(started <= received && received <= published && published - received <= 20_000 && received - started <= 20_000
            && source_ns <= (received as i128 + 2000) * 1_000_000
            && integer(&book["sourceEventAgeMs"])? == received - source_ms, "PUBLIC_BOOK_RECORD_INVALID")?;
        let seq = sequence(text(book,"sequence")?)?;
        need(book["versionKey"] == format!("{}:{}",text(book,"sequence")?,text(book,"sourceAt")?), "PUBLIC_BOOK_RECORD_INVALID")?;
        for barrier in [&self.global, self.markets.get(market).unwrap_or(&Value::Null)] {
            if !barrier.is_null() { need(started >= clock(text(barrier,"at")?,false)?.0,"PUBLIC_BOOK_REQUEST_PRECEDES_INVALIDATION")?; }
        }
        validate_levels(&book["bids"],true)?;validate_levels(&book["asks"],false)?;
        need(price(text(&book["bids"][0],"price")?)? < price(text(&book["asks"][0],"price")?)?,"PUBLIC_BOOK_RECORD_INVALID")?;
        let depth_hash = hash(&json!({"bids":book["bids"],"asks":book["asks"]}))?;
        if let Some(prior) = self.versions.get(text(book,"instrument")?) {
            need(seq >= sequence(text(prior,"sequence")?)? && source_ns >= clock(text(prior,"sourceAt")?,true)?.1
                && received >= clock(text(prior,"receivedAt")?,false)?.0,"PUBLIC_BOOK_SOURCE_ROLLBACK")?;
            need(book["sequence"] != prior["sequence"] || book["versionKey"] == prior["versionKey"] && depth_hash == text(prior,"depthHash")?,"PUBLIC_BOOK_VERSION_CONFLICT")?;
        }
        Ok(json!({"sequence":book["sequence"],"sourceAt":book["sourceAt"],"versionKey":book["versionKey"],"receivedAt":book["receivedAt"],"depthHash":depth_hash}))
    }
    pub fn apply(&mut self, record: &Value) -> Result<(),String> {
        exact(record,&["schemaVersion","kind","sequence","previousHash","sessionId","tournamentId","scopeHash","at","payload","hash"])?;
        let mut body = object(record)?.clone();body.remove("hash");let seq = integer(&record["sequence"])?;
        let sess = text(record,"sessionId")?;
        need(integer(&record["schemaVersion"])? == 1 && seq == self.sequence + 1 && record["previousHash"] == self.hash
            && text(record,"hash")? == hash(&Value::Object(body))? && record["tournamentId"] == self.tournament
            && record["scopeHash"] == self.scope_hash && session(sess),"PUBLIC_BOOK_CHAIN_INVALID")?;
        let at = text(record,"at")?;let time = clock(at,false)?.0;
        need(self.last_at.as_ref().is_none_or(|old| clock(old,false).is_ok_and(|t| time >= t.0)),"PUBLIC_BOOK_CLOCK_ROLLBACK")?;
        let payload = &record["payload"];
        if record["kind"] == "session_start" {
            exact(payload,&["scope"])?;
            need(canonical(&payload["scope"]) == canonical(&self.scope) && !self.sessions.iter().any(|s| s == sess),"PUBLIC_BOOK_SESSION_INVALID")?;
            self.session = Some(sess.to_owned());self.sessions.push(sess.to_owned());self.closed = false;self.books.clear();self.markets.clear();
            self.global = json!({"at":at,"reason":"SESSION_START","sequence":seq});
        } else {
            need(!self.closed && self.session.as_deref() == Some(sess),"PUBLIC_BOOK_SESSION_CLOSED")?;
            match text(record,"kind")? {
                "book" => {
                    exact(payload,&["book"])?;let book = &payload["book"];let version = self.validate_book(book,at)?;
                    let instrument = text(book,"instrument")?.to_owned();let mut published = object(book)?.clone();
                    published.insert("publicationSequence".into(),json!(seq));published.insert("publicationHash".into(),record["hash"].clone());
                    published.insert("publicationSessionId".into(),json!(sess));published.insert("publishedAt".into(),json!(at));
                    self.versions.insert(instrument.clone(),version);self.books.insert(instrument,Value::Object(published));
                }
                "invalidation" => {
                    exact(payload,&["marketId","reason"])?;let reason = text(payload,"reason")?;
                    need(["BOOK_READ_FAILED","INVALID_BOOK","ACCOUNT_OUTAGE","STOP","COLLECTOR_FAILURE","MANUAL_INVALIDATION"].contains(&reason),"PUBLIC_BOOK_RECORD_INVALID")?;
                    let barrier = json!({"at":at,"reason":reason,"sequence":seq});
                    if payload["marketId"].is_null() { self.books.clear();self.global = barrier; }
                    else {
                        let market = text(payload,"marketId")?;
                        need(self.scope["exchanges"].as_array().unwrap().iter().any(|r| r["marketId"] == market),"PUBLIC_BOOK_RECORD_INVALID")?;
                        self.markets.insert(market.to_owned(),barrier);self.books.retain(|_,b| b["marketId"] != market);
                    }
                }
                "session_stop" => {
                    exact(payload,&["reason"])?;let reason = text(payload,"reason")?;
                    need(["STOP","COLLECTOR_FAILURE","NORMAL_SHUTDOWN"].contains(&reason),"PUBLIC_BOOK_RECORD_INVALID")?;
                    self.closed = true;self.books.clear();self.global = json!({"at":at,"reason":reason,"sequence":seq});
                }
                _ => return Err("PUBLIC_BOOK_RECORD_INVALID".into()),
            }
        }
        self.sequence = seq;self.hash = text(record,"hash")?.to_owned();self.last_at = Some(at.to_owned());Ok(())
    }
    pub fn value(&self) -> Value {
        json!({"schemaVersion":1,"tournamentId":self.tournament,"scope":self.scope,"scopeHash":self.scope_hash,"sessionId":self.session,
            "sessions":self.sessions,"closed":self.closed,"sequence":self.sequence,"hash":self.hash,"lastAt":self.last_at,
            "books":self.books,"sourceVersions":self.versions,"invalidations":{"global":self.global,"markets":self.markets}})
    }
}
pub fn read_prefix(path: &Path, end: u64, expected: &str, input: &Value) -> Result<Value,String> {
    let start = Instant::now();
    need(end > 0 && end <= SAFE as u64 && expected.len() == 64 && expected.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),"INVALID_SOURCE_PREFIX")?;
    let file = File::open(path).map_err(|_| "SOURCE_OPEN_FAILED")?;
    need(file.metadata().map_err(|_| "SOURCE_METADATA_FAILED")?.len() >= end,"INVALID_SOURCE_PREFIX")?;
    let mut reader = BufReader::with_capacity(1024 * 1024,file.take(end));let mut prefix = Sha256::new();let mut bytes = 0u64;
    let mut state = Publication::new(input)?;let mut line = Vec::new();
    loop {
        line.clear();let n = (&mut reader).take((MAX_RECORD + 1) as u64).read_until(b'\n',&mut line).map_err(|_| "SOURCE_READ_FAILED")?;if n == 0 { break; }
        need(n <= MAX_RECORD && line.last() == Some(&b'\n'),"PUBLIC_BOOK_PARTIAL_OR_OVERSIZE_RECORD")?;
        bytes += n as u64;prefix.update(&line);
        let row: Value = serde_json::from_slice(&line).map_err(|_| "PUBLIC_BOOK_JSON_INVALID")?;state.apply(&row)?;
    }
    let sha = format!("{:x}",prefix.finalize());need(bytes == end && sha == expected,"SOURCE_PREFIX_HASH_MISMATCH")?;
    Ok(json!({"schemaVersion":1,"mode":"native_public_book_prefix_v1","sourceHash":sha,"sourceBytes":bytes,"sourceRecords":state.sequence,
        "scopeHash":state.scope_hash,"engineHash":env!("BAAL_ENGINE_HASH"),"verifyMs":start.elapsed().as_secs_f64()*1000.0,"state":state.value(),"networkRequests":0,"competitionOrders":0}))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn clocks_and_js_integer_hashing() {
        assert!(clock("2026-10-04T10:00:00.000Z",false).is_ok());
        assert!(clock("2026-10-04T10:00:00.000000001Z",true).is_ok());
        for s in ["2026-02-30T10:00:00.000Z","2026-10-04T10:00:60.000Z","2026-10-04T10:00:00Z"] { assert!(clock(s,false).is_err()); }
        assert_eq!(hash(&serde_json::from_str::<Value>("{\"x\":1.0,\"y\":-0.0}").unwrap()).unwrap(),hash(&json!({"x":1,"y":0})).unwrap());
        assert!(!id("01"));assert!(!id("9007199254740992"));assert_eq!(sequence("0").unwrap(),0);
    }
}
