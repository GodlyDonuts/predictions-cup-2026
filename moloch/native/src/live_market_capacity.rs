//! Human-directed ordinary funded breadth, independent of cash/rate authority.
//! Control/scope hashes are provenance; the sole runtime verifies the original
//! approved Control. A capacity event never submits, retires or releases orders.
use super::*;
fn sha(v:&Value)->Result<&str>{v.as_str().filter(|s|s.len()==64&&s.bytes().all(|b|b.is_ascii_digit()||(b'a'..=b'f').contains(&b))).ok_or("LIVE_CAPACITY_HASH")}
impl Engine{
 pub fn fresh_market_limit(&self)->usize{self.inherited.as_ref().map_or(self.policy.markets,|i|i.fresh_market_limit)}
 pub fn live_market_capacity_record(&self)->Option<Value>{self.live_market_capacity.clone()}
 pub(super) fn apply_live_market_capacity(&mut self,e:&Value,at:i64)->Result<()>{
  if !self.live_mode||e["source"]!="moloch_live_dispatcher"||e["authorizationId"]!="human_moloch_breadth_20261016"{return Err("LIVE_CAPACITY_AUTHORITY")}
  if ms(&e["processedAt"])?!=at||ms(&e["receivedAt"])? >at{return Err("LIVE_CAPACITY_CLOCK")}
  let scope=&e["bankScope"];let fields=scope.as_object().ok_or("LIVE_CAPACITY_SCOPE")?;
  let tournament=scope["tournamentId"].as_str().filter(|s|!s.is_empty()).ok_or("LIVE_CAPACITY_SCOPE")?;
  if fields.len()!=3||scope["runId"].as_str().is_none_or(|s|s.is_empty()||s.len()>128)||sha(&scope["bankManifestHash"]).is_err(){return Err("LIVE_CAPACITY_SCOPE")}
  sha(&e["controlSha256"])?;
  if sha(&e["expectedPolicySha256"])?!=hash(&self.policy.raw){return Err("LIVE_CAPACITY_POLICY_CHANGED")}
  let previous=self.fresh_market_limit();let maximum=self.policy.markets;
  if e["expectedPreviousFreshMarketLimit"].as_u64()!=Some(previous as u64)||e["expectedPreviousMaximumMarkets"].as_u64()!=Some(maximum as u64){return Err("LIVE_CAPACITY_PREVIOUS")}
  let fresh=e["freshMarketLimit"].as_u64().filter(|n|*n>previous as u64&&*n<=12).ok_or("LIVE_CAPACITY_LIMIT")? as usize;
  let effective=maximum.checked_add(fresh-previous).filter(|n|*n<=4096).ok_or("LIVE_CAPACITY_LIMIT")?;
  if e["effectiveMaximumMarkets"].as_u64()!=Some(effective as u64){return Err("LIVE_CAPACITY_TOTAL")}
  let ids=e["scopeExchangeIds"].as_array().filter(|a|a.len()>=effective&&a.len()<=4096).ok_or("LIVE_CAPACITY_SCOPE_CAPACITY")?;
  let mut allowed=BTreeSet::new();let mut last:Option<String>=None;
  for raw in ids{let ex=id(raw)?;if raw.as_str()!=Some(ex.as_str())||last.as_ref().is_some_and(|x|x>=&ex){return Err("LIVE_CAPACITY_SCOPE_IDS")};last=Some(ex.clone());allowed.insert(ex);}
  if self.books.values().any(|b|b.tournament!=tournament||!allowed.contains(&b.exchange))||self.orders.iter().any(|q|q.tournament!=tournament||!allowed.contains(&q.exchange))||self.wallet.lots.iter().any(|((ex,_),lots)|!lots.is_empty()&&!allowed.contains(ex)){return Err("LIVE_CAPACITY_SCOPE_CHANGED")}
  let prior=self.live_market_capacity.as_ref().map_or(Value::Null,|r|r["sha256"].clone());
  if e.get("expectedPreviousCapacitySha256")!=Some(&prior){return Err("LIVE_CAPACITY_PREVIOUS_RECORD")}
  if self.live_market_capacity.as_ref().is_some_and(|r|r["bankScope"]!=*scope){return Err("LIVE_CAPACITY_SCOPE_CHANGED")}
  let mut raw=self.policy.raw.clone();raw["maximumMarkets"]=json!(effective);let policy=Policy::new(&raw)?;
  let revision=self.live_market_capacity.as_ref().map_or(Some(1),|r|r["revision"].as_u64().and_then(|n|n.checked_add(1))).ok_or("LIVE_CAPACITY_REVISION")?;
  let mut record=json!({"version":1,"revision":revision,"bankScope":scope,"freshMarketLimit":fresh,"effectiveMaximumMarkets":effective,"previousFreshMarketLimit":previous,"previousMaximumMarkets":maximum,"scopeExchangeIds":ids,"controlSha256":e["controlSha256"],"authorizationId":e["authorizationId"],"previousPolicySha256":e["expectedPolicySha256"],"previousCapacitySha256":prior,"installedAtMs":at});record["sha256"]=json!(hash(&record));
  // Exactly one current policy field changes; existing lots, funded reserves,
  // write pacing, entry windows, caps and original physical history are intact.
  self.policy=policy;if let Some(i)=self.inherited.as_mut(){i.fresh_market_limit=fresh;}self.live_market_capacity=Some(record);Ok(())
 }
}
