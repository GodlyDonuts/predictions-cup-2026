//! Startup-only immutable lookup and validation of an already admitted, closed ACK.
//! This never accepts an event, changes a ledger or substitutes for receipt reconstruction.
use crate::dispatch::DispatchLog;
use moloch_replay::session::Session;
use serde_json::Value;
use std::collections::BTreeMap;
pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
fn ns(v:&Value)->Result<i64>{Ok(chrono::DateTime::parse_from_rfc3339(v.as_str().ok_or("TIMESTAMP")?)
    .map_err(|_|"TIMESTAMP")?.timestamp_nanos_opt().ok_or("TIMESTAMP_RANGE")?)}
/// Rebuilt after every actual native acceptance; no mutable/live quote cache.
pub struct Index { head:Value, quotes:BTreeMap<u64,Value>, last_ns:Option<i64> }
impl Index {
    pub fn new(bank:&Session)->Result<Self>{
        let mut quotes=BTreeMap::new();
        for q in bank.engine().order_records(){let id=q["id"].as_u64().ok_or("LIVE_QUOTE_ID")?;
            if quotes.insert(id,q).is_some(){return Err("LIVE_STARTUP_DUPLICATE_QUOTE".into())}}
        let at=bank.engine().report()?["at"].clone();
        Ok(Self{head:bank.durable_head(),quotes,last_ns:if at.is_null(){None}else{Some(ns(&at)?)}})
    }
    fn current(&self,bank:&Session)->Result<()>{if self.head!=bank.durable_head(){return Err("LIVE_STARTUP_ACK_INDEX_CHANGED".into())}Ok(())}
    pub fn quote<'a>(&'a self,bank:&Session,id:u64)->Result<&'a Value>{self.current(bank)?;self.quotes.get(&id).ok_or("LIVE_QUOTE_MISSING".into())}
    /// Same closed ordinary exception used by fundamental startup restoration.
    /// It is not, on its own, sufficient to skip native ACK validation.
    pub fn closed_legacy(&self,bank:&Session,log:&DispatchLog,qid:u64,event:&Value)->Result<bool>{
        let q=self.quote(bank,qid)?;
        Ok(log.closed.contains(&qid)&&log.intents.get(&qid).is_some_and(|i|i.get("fundamental").is_none_or(Value::is_null))
            &&event["type"]=="actual_order_ack"&&event["orderId"].as_u64()==Some(qid)
            &&bank.engine().live_order_bindings().get(&qid).is_some_and(|id|event["actualOrderId"]==id.as_str())&&q["retired"]==true)
    }
    /// Only an already replayed, retired, ordinary binding can use this path.
    /// All native existing-binding ACK checks remain exact, including ns clocks.
    /// No discarded future reconcile/audit is needed: the original Session prefix
    /// was already replayed/audited and this function publishes no new event/state.
    pub fn validate_closed_duplicate(&self,bank:&Session,log:&DispatchLog,qid:u64,event:&Value)->Result<bool>{
        let q=self.quote(bank,qid)?;
        if !log.closed.contains(&qid)||q["retired"]!=true||!bank.engine().live_order_bindings().contains_key(&qid)
            ||bank.engine().is_package_order(qid)
            ||log.intents.get(&qid).is_none_or(|i|i.get("fundamental").is_some_and(|v|!v.is_null()))
            ||["reviewedFundamentalEntry","reviewedInventoryExit"].iter().any(|k|q.get(*k).is_some_and(|v|!v.is_null())) {return Ok(false)}
        if !bank.engine().is_live()||event["type"]!="actual_order_ack"||event["source"]!="official_sig_api_v1" {return Err("ACTUAL_EVENT_SOURCE".into())}
        if event["receiptHash"].as_str().is_none_or(|s|s.len()!=64||!s.bytes().all(|b|b.is_ascii_hexdigit())){return Err("ACTUAL_RECEIPT_HASH".into())}
        let actual=event["actualOrderId"].as_str().ok_or("STRING")?;
        if actual.is_empty()||actual.starts_with('0')||!actual.bytes().all(|b|b.is_ascii_digit()){return Err("SCOPE_ID".into())}
        if event["orderId"].as_u64()!=Some(qid){return Err("ACTUAL_INTENT_ID".into())}
        let n=event["quantity"].as_u64().filter(|n|*n>0&&*n<=q["quantity"].as_u64().unwrap_or(0)).ok_or("ACTUAL_ACK_QUANTITY")?;
        for field in ["exchangeId","marketId","price","side","action"] {if event[field]!=q[field]{return Err("ACTUAL_ACK_INTENT_MISMATCH".into())}}
        if bank.engine().live_order_bindings().get(&qid).map(String::as_str)!=Some(actual)||Some(n)!=q["quantity"].as_u64(){return Err("ACTUAL_ORDER_BINDING_CHANGED".into())}
        let received=ns(&event["receivedAt"])?;let processed=ns(event.get("processedAt").unwrap_or(&event["receivedAt"]))?;
        if processed<received||self.last_ns.is_some_and(|last|processed<last){return Err("NONCAUSAL_EVENT".into())}
        if event.get("deferPlanningForPackage").is_some(){return Err("PACKAGE_PLANNING_FLAG_EVENT".into())}
        Ok(true)
    }
}
