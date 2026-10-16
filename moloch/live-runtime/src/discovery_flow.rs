//! Original observed-public-print READ nomination, never tape completeness,
//! fill probability, directional alpha, a current book or execution authority.
use crate::feedback::Result;
use baal_transport_service::prices::official_tick_units;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

pub const FLOW_WINDOW_MS: u64 = 180_000;
pub const MAX_REMEMBERED_PRINTS: usize = 32_768;
const MAX_ARRAY: usize = 100_000;

#[derive(PartialEq, Eq)]
struct Print { market: String, exchange: String, executed: i64, received: i64,
    sequence: u64, price: i128, quantity: u64 }
pub struct Flow { mapping: BTreeMap<String,String>, tournament: Option<String>,
    seen: BTreeMap<(String,String),Print>, settled: BTreeSet<String>, last_received: Option<i64>, last_ranked: Option<u64> }
fn id(v: &Value) -> Result<String> {
    let s = v.as_str().map(str::to_owned).or_else(|| v.as_u64().map(|n|n.to_string()))
        .ok_or("DISCOVERY_FLOW_ID")?;
    if s.is_empty() || s.starts_with('0') || !s.bytes().all(|b|b.is_ascii_digit()) {
        return Err("DISCOVERY_FLOW_ID".into());
    } Ok(s)
}
fn clock(s: &str) -> Result<i64> {
    chrono::DateTime::parse_from_rfc3339(s)?.timestamp_nanos_opt()
        .filter(|n|*n>=0).ok_or("DISCOVERY_FLOW_CLOCK".into())
}
impl Flow {
    pub fn new(mapping: &BTreeMap<String,String>) -> Result<Self> {
        if mapping.len()>4096 || mapping.iter().any(|(e,m)|
            id(&Value::String(e.clone())).is_err() || id(&Value::String(m.clone())).is_err()) {
            return Err("DISCOVERY_FLOW_SCOPE".into());
        }
        Ok(Self {mapping:mapping.clone(),tournament:None,seen:BTreeMap::new(),
            settled:BTreeSet::new(),last_received:None,last_ranked:None})
    }
    pub fn with_tournament(mut self, tournament: &str) -> Result<Self> {
        if tournament.is_empty(){return Err("DISCOVERY_FLOW_TOURNAMENT".into())}
        self.tournament=Some(tournament.into());Ok(self)
    }
    fn tournament(&self,v:&Value)->Result<()> {
        if self.tournament.as_ref().is_some_and(|t|v.get("tournamentId").is_some_and(|v|v!=t.as_str())) {
            return Err("DISCOVERY_FLOW_TOURNAMENT".into());
        } Ok(())
    }
    fn market(&self,v:&Value)->Result<String>{
        self.tournament(v)?;let m=id(&v["marketId"])?;
        if !self.mapping.values().any(|known|known==&m){return Err("DISCOVERY_FLOW_MARKET_SCOPE".into())}
        Ok(m)
    }
    fn optional_clock(v:&Value,key:&str,received:i64)->Result<()> {
        if let Some(v)=v.get(key){if clock(v.as_str().ok_or("DISCOVERY_FLOW_CLOCK")?)?>received{
            return Err("DISCOVERY_FLOW_FUTURE".into());
        }} Ok(())
    }
    fn settle(&mut self,market:&str){
        self.settled.insert(market.into());self.seen.retain(|_,p|p.market!=market);
    }
    /// Clear hypotheses after another discovery parser fails, without minting
    /// a reset receipt or erasing clocks/settlement tombstones.
    pub fn clear_observations(&mut self){self.seen.clear();}
    /// Failure clears all observed flow and remembered IDs; it never admits a
    /// valid prefix of a malformed frame. Settlement tombstones remain. The
    /// caller uses its existing fallback read rotation on an error.
    pub fn observe(&mut self,event:&str,payload:&Value,received:&str)->Result<()> {
        let result:Result<()>=(|| {let at=clock(received)?;
            if self.last_received.is_some_and(|last|at<last){return Err("DISCOVERY_FLOW_RECEIPT_ORDER".into())}
            self.apply(event,payload,at)?;self.last_received=Some(at);Ok(())})();
        if result.is_err(){self.clear_observations();}result
    }
    fn apply(&mut self,event:&str,payload:&Value,received:i64)->Result<()> {
        self.tournament(payload)?;
        if event=="stream_reset" {
            if payload["localBoundary"]!=true{return Err("DISCOVERY_FLOW_RESET_PROOF".into())}
            self.clear_observations();return Ok(());
        }
        if event=="book_dirty" || event=="market_settled" {
            let m=self.market(payload)?;Self::optional_clock(payload,"at",received)?;
            Self::optional_clock(payload,"sourceAt",received)?;
            if payload.get("exchangeId").is_some() && self.mapping.get(&id(&payload["exchangeId"])? )!=Some(&m){
                return Err("DISCOVERY_FLOW_EXCHANGE_SCOPE".into());
            }
            if event=="market_settled"{self.settle(&m);}return Ok(());
        }
        if event=="gap" {
            for ex in payload["exchangeIds"].as_array().filter(|a|a.len()<=4096).ok_or("DISCOVERY_FLOW_GAP")? {
                if !self.mapping.contains_key(&id(ex)?){return Err("DISCOVERY_FLOW_EXCHANGE_SCOPE".into())}
            } return Ok(());
        }
        if event!="markets_batch" {return Err("DISCOVERY_FLOW_EVENT".into())}
        let rows=payload["markets"].as_array().filter(|a|a.len()<=1000).ok_or("DISCOVERY_FLOW_BATCH")?;
        let mut markets=BTreeSet::new();
        for row in rows {
            let market=self.market(row)?;
            if !markets.insert(market.clone()){return Err("DISCOVERY_FLOW_DUPLICATE_MARKET".into())}
            let b=&row["batch"];self.tournament(b)?;let d=&b["delivery"];
            let revision=d["revision"].as_u64().ok_or("DISCOVERY_FLOW_REVISION")?;
            let previous=d["previousRevision"].as_u64().ok_or("DISCOVERY_FLOW_REVISION")?;
            if d["model"]!="best-effort-authoritative-resync" || revision<=previous {
                return Err("DISCOVERY_FLOW_DELIVERY".into());
            }
            if b.get("resyncRequired").is_some_and(|v|!v.is_boolean()) {
                return Err("DISCOVERY_FLOW_RESYNC".into());
            }
            for key in ["books","bookDirty","marketSettled","trades"] {
                if b.get(key).is_some_and(|v|v.as_array().is_none_or(|a|a.len()>MAX_ARRAY)) {
                    return Err("DISCOVERY_FLOW_ARRAY".into());
                }
            }
            if let Some(settlements)=b["marketSettled"].as_array(){for row in settlements {
                if self.market(row)?!=market{return Err("DISCOVERY_FLOW_MARKET_SCOPE".into())}
                clock(row["at"].as_str().ok_or("DISCOVERY_FLOW_CLOCK")?)?
                    .checked_sub(received).filter(|n|*n<=0).ok_or("DISCOVERY_FLOW_FUTURE")?;
                self.settle(&market);
            }}
            if let Some(trades)=b["trades"].as_array(){for t in trades {
                if self.market(t)?!=market{return Err("DISCOVERY_FLOW_MARKET_SCOPE".into())}
                let exchange=id(&t["exchangeId"])?;
                if self.mapping.get(&exchange)!=Some(&market){return Err("DISCOVERY_FLOW_EXCHANGE_SCOPE".into())}
                let key=t["id"].as_str().filter(|s|!s.is_empty()&&s.len()<=128)
                    .ok_or("DISCOVERY_FLOW_PRINT_ID")?;
                let key=(exchange.clone(),key.to_owned());
                let executed=clock(t["executedAt"].as_str().ok_or("DISCOVERY_FLOW_CLOCK")?)?;
                if executed>received{return Err("DISCOVERY_FLOW_FUTURE".into())}
                let print=Print {market:market.clone(),exchange,executed,received,
                    sequence:t["sequence"].as_u64().filter(|n|*n>0).ok_or("DISCOVERY_FLOW_SEQUENCE")?,
                    price:official_tick_units(&t["price"])?,
                    quantity:t["quantity"].as_u64().filter(|n|*n>0&&*n<=9_007_199_254_740_991)
                        .ok_or("DISCOVERY_FLOW_QUANTITY")?};
                if self.settled.contains(&market){continue}
                if let Some(old)=self.seen.get(&key){
                    // Re-delivery clocks are not original trade facts, and
                    // must never replace the first observation's clock.
                    if old.market!=print.market || old.exchange!=print.exchange
                        || old.executed!=print.executed || old.sequence!=print.sequence
                        || old.price!=print.price || old.quantity!=print.quantity {
                        return Err("DISCOVERY_FLOW_PRINT_CONFLICT".into());
                    }continue;
                }
                if self.seen.len()>=MAX_REMEMBERED_PRINTS{return Err("DISCOVERY_FLOW_CAPACITY".into())}
                self.seen.insert(key,print);
            }}
        } Ok(())
    }
    /// Positive observed counts only, count descending then stable exchange ID.
    /// This deliberately does not call either observed volume or completeness
    /// a forecast of the next fill. It is safe only for READ nomination.
    pub fn ranked(&mut self,now:u64)->Result<Vec<(String,u64)>> {
        let ns=i64::try_from(now).ok().and_then(|n|n.checked_mul(1_000_000))
            .ok_or("DISCOVERY_FLOW_CLOCK")?;
        if self.last_received.is_some_and(|r|r/1_000_000>ns/1_000_000)
            || self.last_ranked.is_some_and(|old|now<old){
            return Err("DISCOVERY_FLOW_RANK_CLOCK".into());
        }
        self.last_ranked=Some(now);
        let end=ns.checked_add(999_999).ok_or("DISCOVERY_FLOW_CLOCK")?;
        let window=(FLOW_WINDOW_MS*1_000_000)as i64;
        let mut counts:BTreeMap<String,u64>=BTreeMap::new();
        for p in self.seen.values(){
            if p.executed<=end && p.received<=end && ns.saturating_sub(p.executed)<=window
                && ns.saturating_sub(p.received)<=window && !self.settled.contains(&p.market) {
                *counts.entry(p.exchange.clone()).or_default()+=1;
            }
        }
        let mut rows:Vec<_>=counts.into_iter().collect();
        rows.sort_by(|a,b|b.1.cmp(&a.1).then_with(||a.0.cmp(&b.0)));Ok(rows)
    }
    pub fn remembered_prints(&self)->usize {self.seen.len()}
}
