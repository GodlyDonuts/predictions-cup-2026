//! Whole-venue discovery; REST recovery remains bounded by the account queue.
use crate::{breadth::Study,feedback::Result};
use serde_json::{Value,json};
use std::collections::{BTreeMap,BTreeSet};
pub struct Universe{study:Study,allowed:BTreeSet<String>,excluded:BTreeSet<String>,maximum:usize,last_error:Option<String>,owned_cursor:usize}
impl Universe{
 pub fn new(exchanges:&BTreeMap<String,String>,positions:&Value,maximum:usize)->Result<Self>{
  if maximum==0||maximum>exchanges.len(){return Err("UNIVERSE_CAPACITY".into())}
  let mut excluded=BTreeSet::new();
  for row in positions["positions"].as_array().ok_or("UNIVERSE_POSITIONS")?{
   let quantity=row["quantity"].as_i64().ok_or("UNIVERSE_POSITION_QUANTITY")?;
   let id=row["exchangeId"].as_str().ok_or("UNIVERSE_POSITION_ID")?;
   if quantity!=0{excluded.insert(id.to_owned());}
  }
  let allowed=exchanges.keys().filter(|e|!excluded.contains(*e)).cloned().collect();
  Ok(Self{study:Study::default(),allowed,excluded,maximum,last_error:None,owned_cursor:0})
 }
 pub fn observe(&mut self,event:&str,payload:&Value,received:&str)->Result<()>{
  if let Err(e)=self.study.accept(&json!({"source":"official_sig_realtime","event":event,"payload":payload,"receivedAt":received})){
   // Discovery cannot stop reconciliation of actual owned orders. A failed
   // observation discards all candidate state; the financial parser stays strict.
   self.last_error=Some(e.to_string());self.study=Study::default();
  }Ok(())
 }
 pub fn pairs(&self,exchanges:&BTreeMap<String,String>)->Value{json!(self.allowed.iter().map(|e|json!({"exchangeId":e,"marketId":exchanges[e]})).collect::<Vec<_>>())}
 pub fn select(&mut self,required:&BTreeSet<String>)->Result<Vec<String>>{
  if required.iter().any(|e|!self.allowed.contains(e)){return Err("UNIVERSE_UNOWNED_SCOPE".into())}
  let report=self.study.report();let mut candidates=report["candidates"].as_array().ok_or("UNIVERSE_REPORT")?.clone();
  candidates.retain(|c|self.allowed.contains(c["exchangeId"].as_str().unwrap_or(""))&&c["bookSourceAgeMs"].as_i64().is_some_and(|a|(0..=10000).contains(&a))&&c["recentTrades"].as_u64().unwrap_or(0)>0);
  candidates.sort_by(|a,b|b["eligibleForRestReconciliation"].as_bool().cmp(&a["eligibleForRestReconciliation"].as_bool()).then_with(||b["recentTrades"].as_u64().cmp(&a["recentTrades"].as_u64())).then_with(||b["balancedTurnover"].as_f64().unwrap_or(0.0).total_cmp(&a["balancedTurnover"].as_f64().unwrap_or(0.0))).then_with(||a["exchangeId"].as_str().cmp(&b["exchangeId"].as_str())));
  let owned:Vec<_>=required.iter().cloned().collect();let mut out=Vec::new();
  let new_candidate=candidates.iter().any(|c|!required.contains(c["exchangeId"].as_str().unwrap_or("")));
  if !owned.is_empty(){let start=self.owned_cursor%owned.len();let budget=self.maximum-if new_candidate&&owned.len()>=self.maximum&&self.maximum>1{1}else{0};let take=owned.len().min(budget);for offset in 0..take{out.push(owned[(start+offset)%owned.len()].clone());}self.owned_cursor=(start+take)%owned.len();}
  for c in candidates{if out.len()>=self.maximum{break}let e=c["exchangeId"].as_str().ok_or("UNIVERSE_ID")?.to_owned();if !required.contains(&e)&&!out.contains(&e){out.push(e);}}
  Ok(out)
 }
 pub fn status(&self)->Value{let report=self.study.report();json!({"venueExchanges":self.allowed.len()+self.excluded.len(),"eligibleExchanges":self.allowed.len(),"foreignHeldExcluded":self.excluded.len(),"observedExchanges":report["marketsObserved"],"frames":report["frames"],"lastDiagnosticError":self.last_error,"maximumConcurrentMarkets":self.maximum,"selection":"owned obligations, then fresh recent market activity; all-market websocket discovery"})}
}
#[cfg(test)]mod tests{
 use super::*;
 fn exchanges()->BTreeMap<String,String>{[("2".into(),"1".into()),("4".into(),"3".into()),("6".into(),"5".into())].into()}
 fn frame(id:&str,exchange:&str,at:&str)->Value{json!({"markets":[{"marketId":id,"batch":{"delivery":{"model":"best-effort-authoritative-resync","revision":1,"previousRevision":0},"books":[{"exchangeId":exchange,"asOf":{"sequence":10,"at":at},"bids":[{"price":0.4,"quantity":100}],"asks":[{"price":0.6,"quantity":100}]}],"trades":[{"id":format!("trade-{id}"),"marketId":id,"exchangeId":exchange,"sequence":9,"executedAt":at,"price":0.4,"quantity":1}]}}]})}
 #[test]fn discovers_outside_original_three_and_excludes_foreign_inventory(){let mut u=Universe::new(&exchanges(),&json!({"positions":[{"exchangeId":"2","quantity":-5}]}),1).unwrap();let at="2026-10-15T00:00:00Z";u.observe("markets_batch",&frame("1","2",at),at).unwrap();u.observe("markets_batch",&frame("3","4",at),at).unwrap();assert_eq!(u.select(&BTreeSet::new()).unwrap(),vec!["4"]);assert_eq!(u.pairs(&exchanges()).as_array().unwrap().len(),2);}
 #[test]fn owned_work_takes_priority_over_discovery(){let mut u=Universe::new(&exchanges(),&json!({"positions":[]}),1).unwrap();let at="2026-10-15T00:00:00Z";u.observe("markets_batch",&frame("3","4",at),at).unwrap();assert_eq!(u.select(&["6".into()].into()).unwrap(),vec!["6"]);}
 #[test]fn reconnect_drops_candidate_until_new_market_evidence(){let mut u=Universe::new(&exchanges(),&json!({"positions":[]}),1).unwrap();let at="2026-10-15T00:00:00Z";u.observe("markets_batch",&frame("3","4",at),at).unwrap();u.observe("stream_reset",&json!({"localBoundary":true}),"2026-10-15T00:00:01Z").unwrap();assert!(u.select(&BTreeSet::new()).unwrap().is_empty());}
 #[test]fn failed_discovery_clears_candidates_but_preserves_owned_reconciliation(){let mut u=Universe::new(&exchanges(),&json!({"positions":[]}),1).unwrap();let at="2026-10-15T00:00:01Z";u.observe("markets_batch",&frame("3","4",at),at).unwrap();u.observe("markets_batch",&frame("3","4",at),"2026-10-15T00:00:00Z").unwrap();assert!(u.select(&BTreeSet::new()).unwrap().is_empty());assert_eq!(u.select(&["6".into()].into()).unwrap(),vec!["6"]);assert_eq!(u.status()["lastDiagnosticError"],"BREADTH_RECEIPT_ORDER");}
 #[test]fn many_owned_markets_rotate_without_an_unbounded_rest_packet(){let mut u=Universe::new(&exchanges(),&json!({"positions":[]}),1).unwrap();let required=["2".into(),"4".into(),"6".into()].into();let all:Vec<_>=(0..3).flat_map(|_|{let selected=u.select(&required).unwrap();assert_eq!(selected.len(),1);selected}).collect();assert_eq!(all,vec!["2","4","6"]);}
 #[test]fn inherited_portfolio_does_not_starve_new_market_discovery(){let mut u=Universe::new(&exchanges(),&json!({"positions":[]}),2).unwrap();let at="2026-10-15T00:00:00Z";u.observe("markets_batch",&frame("3","4",at),at).unwrap();let required=["2".into(),"6".into()].into();assert_eq!(u.select(&required).unwrap(),vec!["2","4"]);assert_eq!(u.select(&required).unwrap(),vec!["6","4"]);}
}
