//! Runtime entry selection, not order authority. Only the sole worker's opaque
//! live observer can supply empirical evidence. Exploration is explicitly
//! unqualified and consumes a small reviewed capital/loss budget.
use crate::{dispatch::DispatchLog,execution_edge as edge,feedback::Result};
use baal_transport_service::{decimal_units,Permit,RequestSpec,Scope};
use moloch_replay::{session::Session,Side};
use serde::{Deserialize,Serialize};
use serde_json::{json,Value};
use sha2::{Digest,Sha256};
const UNIT:i128=1_000_000;
fn need(ok:bool,s:&'static str)->Result<()>{if ok{Ok(())}else{Err(s.into())}}
fn hash<T:Serialize>(v:&T)->Result<String>{Ok(format!("{:x}",Sha256::digest(serde_json::to_vec(v)?)))}
fn hex(s:&str)->bool{s.len()==64&&s.bytes().all(|b|b.is_ascii_digit()||(b'a'..=b'f').contains(&b))}
fn cash(v:&Value)->Result<i128>{Ok(decimal_units(v.as_str().ok_or("MAKER_MONEY")?)?)}
fn clock(v:&Value)->Result<u64>{Ok(u64::try_from(chrono::DateTime::parse_from_rfc3339(v.as_str().ok_or("MAKER_CLOCK")?)?.timestamp_millis())?)}
fn add(a:i128,b:i128)->Result<i128>{Ok(a.checked_add(b).ok_or("MAKER_OVERFLOW")?)}
fn mul(a:i128,b:i128)->Result<i128>{Ok(a.checked_mul(b).ok_or("MAKER_OVERFLOW")?)}
#[derive(Clone,Serialize,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct Settings{
 pub authorization_id:String,pub reviewed_at_ms:u64,pub review_source_sha256:String,
 pub bank_manifest_hash:String,pub run_id:String,pub baseline_native_head:Value,
 pub baseline_report_sha256:String,pub baseline_new_trading_realized:String,
 pub exploration_maximum_order_notional:String,pub exploration_maximum_deployed_notional:String,
 pub exploration_maximum_quantity:u64,pub additional_realized_loss_stop:String,
 pub expected_inventory_age_ms:u64,pub additional_portfolio_charge:String,
}
/// Trusted operator-selected review artifact, not a fitted model or proof of
/// historical report ancestry. The immutable control hash must bind Settings.
pub struct Reviewed{settings:Settings,sha:String}
impl Reviewed{
 pub fn load(settings:Settings,bank:&Session)->Result<Self>{
  need(!settings.authorization_id.is_empty()&&settings.authorization_id.len()<=128&&hex(&settings.review_source_sha256)&&hex(&settings.bank_manifest_hash)&&hex(&settings.baseline_report_sha256),"MAKER_REVIEW")?;
  need(settings.reviewed_at_ms>0&&settings.expected_inventory_age_ms>0&&settings.expected_inventory_age_ms<=120_000&&settings.exploration_maximum_quantity>0&&settings.exploration_maximum_quantity<=10000,"MAKER_POLICY_BOUNDS")?;
  let order=decimal_units(&settings.exploration_maximum_order_notional)?;let total=decimal_units(&settings.exploration_maximum_deployed_notional)?;let loss=decimal_units(&settings.additional_realized_loss_stop)?;
  need(order>0&&order<=20*UNIT&&total>=order&&total<=100*UNIT&&loss>0&&loss<=20*UNIT&&decimal_units(&settings.additional_portfolio_charge)?>=0,"MAKER_EXPLORATION_BOUNDS")?;
  decimal_units(&settings.baseline_new_trading_realized)?;
  let sha=hash(&settings)?;let out=Self{settings,sha};out.bind(bank)?;Ok(out)
 }
 pub fn sha256(&self)->&str{&self.sha}
 fn bind(&self,bank:&Session)->Result<()>{
  let s=&self.settings;let head=bank.durable_head();let report=bank.engine().report()?;
  need(bank.engine().is_live()&&bank.manifest_hash()==&json!(s.bank_manifest_hash)&&bank.live_account_seed().is_some_and(|seed|seed["runId"]==s.run_id),"MAKER_BANK_BINDING")?;
  need(s.baseline_native_head["manifestHash"]==*bank.manifest_hash()&&s.baseline_native_head["sequence"].as_u64().is_some_and(|n|head["sequence"].as_u64().is_some_and(|m|m>=n))&&s.baseline_native_head["previous"].as_str().is_some_and(hex),"MAKER_BASELINE_HEAD")?;
  let at=clock(&report["at"])?;need(at>=s.reviewed_at_ms,"MAKER_BEFORE_REVIEW")?;
  if head==s.baseline_native_head{need(hash(&report)?==s.baseline_report_sha256&&new_realized(&report)?==decimal_units(&s.baseline_new_trading_realized)?,"MAKER_BASELINE_REPORT")?;}
  need(bank.engine().policy()["maximumInventoryAgeMs"].as_u64()==Some(s.expected_inventory_age_ms)&&bank.engine().policy()["inventoryAgeClock"]=="execution","MAKER_INVENTORY_HORIZON")
 }
}
fn new_realized(report:&Value)->Result<i128>{if report.get("inheritedInventory").is_some(){cash(&report["newTradingRealized"])}else{cash(&report["realized"])}}
#[derive(Clone,Copy,Debug,PartialEq,Eq,Serialize)]
#[serde(rename_all="snake_case")]
pub enum Class{OwnedExit,CoveredHedge,Qualified,Exploration,Deferred}
/// No Deserialize/public constructor: quote, bank, dispatch and model evidence
/// cannot be assembled from arbitrary diagnostics by the caller.
pub struct Decision{class:Class,reason:String,proof:Value,quote:Value,ledger_sha:String,head:Value,dispatch:Value,settings_sha:String,run:String,tournament:String,expires:u64,model:Option<edge::RuntimeMakerEvaluation>}
impl Decision{
 pub fn class(&self)->Class{self.class}pub fn reason(&self)->&str{&self.reason}pub fn proof(&self)->&Value{&self.proof}
 pub fn allowed(&self)->bool{self.class!=Class::Deferred}
 pub fn candidate(&self)->Option<&edge::Candidate>{if self.class==Class::Qualified{self.model.as_ref().and_then(|m|m.evaluation().candidate.as_ref())}else{None}}
 pub fn priority(&self)->u8{match self.class{Class::OwnedExit|Class::CoveredHedge=>0,Class::Qualified=>1,Class::Exploration=>2,Class::Deferred=>3}}
 pub fn validate_frozen(&self,quote:&Value,ledger:&Value,qid:u64,now:u64)->Result<()>{need(self.allowed()&&now<self.expires&&self.quote==*quote&&self.quote["id"].as_u64()==Some(qid)&&hash(ledger)?==self.ledger_sha,"MAKER_FROZEN_READY_CHANGED")}
 pub fn validate_current(&self,bank:&Session,review:&Reviewed,log:&DispatchLog,now:u64)->Result<()>{
  need(self.allowed()&&now<self.expires&&bank.durable_head()==self.head&&dispatch(log)==self.dispatch&&review.sha==self.settings_sha,"MAKER_DECISION_CHANGED")?;review.bind(bank)?;log.require_resolved_attempts()?;
  let id=self.quote["id"].as_u64().ok_or("MAKER_QID")?;need(bank.engine().order_records().iter().any(|q|q==&self.quote)&&log.never_attempted(id),"MAKER_QUOTE_CHANGED")?;
  if let Some(m)=&self.model{m.validate_current(bank,id,now)?;}Ok(())
 }
 /// Additional physical-boundary guard only; ordinary ProductionAuthority
 /// still validates fresh account/contract/book, controls, owner and STOP.
 pub fn authorize(&self,scope:&Scope,spec:&RequestSpec,permit:&Permit,now:u64)->Result<()>{
  need(self.allowed()&&now<self.expires&&permit.run_id==self.run&&permit.intent_id==self.quote["id"].to_string()&&permit.owner=="Codex"&&now<permit.expires_at_ms,"MAKER_PHYSICAL_BINDING")?;
  let b=spec.body.as_ref().ok_or("MAKER_REQUEST")?;let n=self.quote["quantity"].as_u64().ok_or("MAKER_QUANTITY")?;let p=cash(&self.quote["price"])?;
  need(spec.method=="POST"&&spec.endpoint=="/orders"&&spec.body_json.as_ref().is_some_and(|raw|serde_json::from_str::<Value>(raw).ok().as_ref()==Some(b))&&b.as_object().is_some_and(|o|o.len()==8),"MAKER_REQUEST")?;
  need(scope.tournament_id==self.tournament&&scope.exchanges.get(self.quote["exchangeId"].as_str().ok_or("MAKER_SCOPE")?).is_some_and(|m|self.quote["marketId"]==*m)&&b["tournamentId"]==self.tournament&&b["exchangeId"]==self.quote["exchangeId"]&&b["side"]==self.quote["side"].as_str().ok_or("MAKER_SIDE")?.to_lowercase()&&b["action"]==self.quote["action"]&&b["quantity"].as_u64()==Some(n)&&baal_transport_service::prices::official_tick_units(&b["price"])?==p&&clock(&b["expirationDate"])?==self.quote["expiresAt"].as_u64().ok_or("MAKER_EXPIRY")?&&b["idempotencyKey"]==format!("{}:{}",self.run,self.quote["id"])&&n<=permit.maximum_quantity as u64&&mul(p,n as i128)?<=decimal_units(&permit.maximum_cost)?,"MAKER_REQUEST_BINDING")?;Ok(())
 }
}
fn dispatch(log:&DispatchLog)->Value{log.proof(0)}
fn remaining(q:&Value)->Result<u64>{Ok(q["quantity"].as_u64().ok_or("MAKER_QUANTITY")?.checked_sub(q["filled"].as_u64().ok_or("MAKER_QUANTITY")?).ok_or("MAKER_QUANTITY")?)}
/// Public diagnostics use actual native lots/attempts. Absent timestamps are
/// excluded only after reconstructing original adoption FIFO consumption.
pub fn deployed_new_capital(bank:&Session,log:&DispatchLog)->Result<i128>{
 log.require_resolved_attempts()?;let report=bank.engine().report()?;let positions=bank.engine().wallet.positions();let mut inherited=std::collections::BTreeMap::<(String,String),std::collections::VecDeque<(u64,i128)>>::new();
 if let Some(a)=report.get("inheritedInventory"){
  need(a["basis"]=="verified_open_lot_principal_fifo"&&a["receiptHash"].as_str().is_some_and(hex),"MAKER_ADOPTION_PROOF")?;
  for row in a["sourcePositions"].as_array().ok_or("MAKER_ADOPTION_POSITIONS")?{let ex=row["exchangeId"].as_str().ok_or("MAKER_ADOPTION_SCOPE")?;let signed=row["quantity"].as_i64().ok_or("MAKER_ADOPTION_SIDE")?;let side=if signed<0{"NO"}else{"YES"};let mut lots=std::collections::VecDeque::new();for l in row["lots"].as_array().ok_or("MAKER_ADOPTION_LOTS")?{lots.push_back((l["quantity"].as_u64().ok_or("MAKER_ADOPTION_LOTS")?,cash(&l["entryPrice"])?));}need(inherited.insert((ex.into(),side.into()),lots).is_none(),"MAKER_ADOPTION_DUPLICATE")?;}
  for f in bank.engine().fills(){let side=f["side"].as_str().ok_or("MAKER_FILL_SIDE")?;let closed=if f["action"]=="buy"{if side=="YES"{"NO"}else{"YES"}}else{side};if let Some(lots)=inherited.get_mut(&(f["exchangeId"].as_str().ok_or("MAKER_FILL_SCOPE")?.into(),closed.into())){let mut n=f["quantity"].as_u64().ok_or("MAKER_FILL_QUANTITY")?;while n>0&&!lots.is_empty(){let first=lots.front_mut().unwrap();let take=n.min(first.0);n-=take;first.0-=take;if first.0==0{lots.pop_front();}}}}
 }
 let mut total=0;for(ex,sides)in positions.as_object().ok_or("MAKER_POSITIONS")?{for side in ["YES","NO"]{let mut original=inherited.remove(&(ex.clone(),side.into())).unwrap_or_default();for lot in sides[side].as_array().ok_or("MAKER_LOTS")?{let n=lot["quantity"].as_u64().ok_or("MAKER_LOT_QUANTITY")?;let p=cash(&lot["price"])?;if let Some(at)=lot.get("executedAt"){clock(at)?;total=add(total,mul(n as i128,p)?)?;}else{need(original.pop_front()==Some((n,p)),"MAKER_UNATTRIBUTED_LOT")?;}}need(original.is_empty(),"MAKER_ADOPTION_REMAINDER")?;}}
 let records=bank.engine().order_records();for id in &log.attempted{if log.closed.contains(id){continue}let q=records.iter().find(|q|q["id"].as_u64()==Some(*id)).ok_or("MAKER_PENDING_NATIVE_QUOTE")?;if q["action"]=="buy"{total=add(total,mul(remaining(q)? as i128,cash(&q["price"])?)?)?;}}Ok(total)
}
fn covered_hedge(bank:&Session,log:&DispatchLog,q:&Value)->Result<bool>{
 let ex=q["exchangeId"].as_str().ok_or("MAKER_SCOPE")?;let opposite=if q["side"]=="YES"{Side::No}else{Side::Yes};let n=remaining(q)?;if n==0||bank.engine().wallet.quantity(ex,opposite)<n{return Ok(false)}
 let records=bank.engine().order_records();if records.iter().any(|other|other["id"]!=q["id"]&&other["exchangeId"]==ex&&(other["retired"]==false||other["id"].as_u64().is_some_and(|id|log.attempted.contains(&id)&&!log.closed.contains(&id)))){return Ok(false)}
 let positions=bank.engine().wallet.positions();let side=if opposite==Side::Yes{"YES"}else{"NO"};let mut left=n;let mut principal=0;for l in positions[ex][side].as_array().ok_or("MAKER_HEDGE_LOTS")?{let take=left.min(l["quantity"].as_u64().ok_or("MAKER_HEDGE_LOTS")?);principal=add(principal,mul(take as i128,cash(&l["price"])?)?)?;left-=take;if left==0{break}}
 Ok(left==0&&add(principal,mul(n as i128,cash(&q["price"])?)?)?<=mul(n as i128,UNIT)?)
}
/// Conservative explicit policy charge, not a calibrated partial-fill model.
pub fn partial_risk_charge(gross:i128,p:u32)->Result<i128>{need(gross>=0&&p<=10000,"MAKER_RISK_BOUND")?;Ok(add(mul(gross,(10000-p)as i128)?,9999)?/10000)}
pub fn decide(bank:&Session,scope:&Scope,log:&DispatchLog,review:&Reviewed,qid:u64,now:u64)->Result<Decision>{
 review.bind(bank)?;log.require_resolved_attempts()?;let q=bank.engine().order_records().into_iter().find(|q|q["id"].as_u64()==Some(qid)).ok_or("MAKER_QUOTE")?;
 need(!bank.engine().is_package_order(qid)&&q["retired"]==false&&q["filled"]==0&&q.get("cancelEffectiveAt").is_none_or(Value::is_null)&&log.never_attempted(qid)&&q["expiresAt"].as_u64().is_some_and(|t|t>now),"MAKER_QUOTE_STATE")?;
 need(scope.exchanges.get(q["exchangeId"].as_str().ok_or("MAKER_SCOPE")?).is_some_and(|m|q["marketId"]==*m),"MAKER_SCOPE")?;
 let mut d=Decision{class:Class::Deferred,reason:"not_evaluated".into(),proof:json!({}),ledger_sha:hash(&bank.engine().report()?)?,head:bank.durable_head(),dispatch:dispatch(log),settings_sha:review.sha.clone(),run:review.settings.run_id.clone(),tournament:scope.tournament_id.clone(),expires:q["expiresAt"].as_u64().unwrap(),quote:q.clone(),model:None};
 if q["action"]=="sell"{need(remaining(&q)?<=bank.engine().wallet.quantity(q["exchangeId"].as_str().unwrap(),if q["side"]=="YES"{Side::Yes}else{Side::No}),"MAKER_EXIT_UNCOVERED")?;d.class=Class::OwnedExit;d.reason="owned_exit_keeps_original_authority".into();}
 else{need(q["action"]=="buy","MAKER_ACTION")?;if covered_hedge(bank,log,&q)?{d.class=Class::CoveredHedge;d.reason="fully_covered_fifo_nonnegative_redemption".into();}
 else{
 let s=&review.settings;let gross=mul(cash(&q["price"])?,remaining(&q)? as i128)?;let horizon=edge::CapitalHorizon{milliseconds:s.expected_inventory_age_ms,source_sha256:hash(bank.engine().policy())?};let mut costs=edge::ExecutionCosts{inventory_and_partial_risk_units:0,source_sha256:review.sha.clone(),horizon};
 match edge::evaluate_runtime_maker(bank,qid,Some(&costs),now){Err(e)=>{d.reason=format!("observer_unavailable:{e}");},Ok(initial)=>{
  need(initial.config().markout_ms==s.expected_inventory_age_ms,"MAKER_MODEL_HORIZON")?;let estimate=&initial.evaluation().estimate;let veto=estimate.valid_marks>=initial.config().minimum_samples&&estimate.valid_marks*10000>=estimate.matured_marks*initial.config().minimum_coverage_bps as usize&&estimate.stressed_conditional_per_share.is_some_and(|n|n<=0);
  d.expires=d.expires.min(initial.valid_until_ms());
  if veto{d.reason="covered_empirical_nonpositive_markout_veto".into();d.model=Some(initial);}
  else if estimate.available{
   costs.inventory_and_partial_risk_units=add(partial_risk_charge(gross,estimate.conservative_fill_probability_bps.ok_or("MAKER_PROBABILITY")?)?,decimal_units(&s.additional_portfolio_charge)?)?;
   let scored=edge::evaluate_runtime_maker(bank,qid,Some(&costs),now)?;if scored.evaluation().candidate.as_ref().is_some_and(|c|c.net_gain_units>0&&now<c.valid_until_ms){d.class=Class::Qualified;d.reason="empirical_gain_after_conservative_partial_and_portfolio_charge".into();}else{d.reason="qualified_evidence_nonpositive_after_risk_charge".into();}d.model=Some(scored);
  }else{let deployed=deployed_new_capital(bank,log)?;let realized=new_realized(&bank.engine().report()?)?;let baseline=decimal_units(&s.baseline_new_trading_realized)?;let loss=baseline.checked_sub(realized).ok_or("MAKER_OVERFLOW")?.max(0);
   d.reason=if remaining(&q)?>s.exploration_maximum_quantity||gross>decimal_units(&s.exploration_maximum_order_notional)?{"exploration_order_cap"}else if add(deployed,gross)?>decimal_units(&s.exploration_maximum_deployed_notional)?{"exploration_deployed_cap"}else if loss>=decimal_units(&s.additional_realized_loss_stop)?{"exploration_realized_loss_cap"}else{d.class=Class::Exploration;"bounded_unqualified_exploration_not_positive_ev"}.into();d.model=Some(initial);
  }
 }}
 }}
 d.proof=json!({"class":d.class,"reason":d.reason,"qid":qid,"nativeHead":d.head,"dispatchHead":d.dispatch,"reviewedSettingsSha256":d.settings_sha,"expiresAtMs":d.expires,"statisticalConfidenceClaimed":false,"executionAuthority":false,"baselineHistoricalAncestryProven":false,"realizedLossStopIsHardLossBound":false,"maximumOutstandingExploratoryDownside":review.settings.exploration_maximum_deployed_notional,"partialRiskChargeKind":"explicit_full_loss_non_full_trial_policy_not_calibrated_estimate","model":d.model.as_ref().map(|m|m.evaluation())});Ok(d)
}


