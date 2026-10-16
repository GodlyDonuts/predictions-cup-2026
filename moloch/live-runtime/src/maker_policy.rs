//! Runtime entry selection, not order authority. Only the sole worker's opaque
//! live observer can supply empirical evidence. Exploration is explicitly
//! unqualified and consumes a small reviewed capital/loss budget.
use crate::{dispatch::DispatchLog,execution_edge as edge,execution_distribution as distribution,feedback::Result};
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
 /// Separately reviewed opt-in. Omission retains legacy review bytes/hashes.
 #[serde(default,skip_serializing_if="Option::is_none")]
 pub joint_execution:Option<JointExecution>,
 pub authorization_id:String,pub reviewed_at_ms:u64,pub review_source_sha256:String,
 pub bank_manifest_hash:String,pub run_id:String,pub baseline_native_head:Value,
 pub baseline_report_sha256:String,pub baseline_new_trading_realized:String,
 pub exploration_maximum_order_notional:String,pub exploration_maximum_deployed_notional:String,
 pub exploration_maximum_quantity:u64,pub additional_realized_loss_stop:String,
 pub expected_inventory_age_ms:u64,pub additional_portfolio_charge:String,
}
/// Production-profile empirical thresholds, not calibrated-probability claims.
#[derive(Clone,Serialize,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct JointExecution{
 pub version:u32,pub distribution_config_sha256:String,
 pub minimum_completed:usize,pub minimum_time_blocks:usize,pub minimum_per_class:usize,
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
  if let Some(j)=&settings.joint_execution{
   need(j.version==1&&hex(&j.distribution_config_sha256)&&(100..=1024).contains(&j.minimum_completed)&&(20..=64).contains(&j.minimum_time_blocks)&&j.minimum_per_class>0&&j.minimum_per_class<=j.minimum_completed/3,"MAKER_JOINT_PRODUCTION_PROFILE")?;
  }
  decimal_units(&settings.baseline_new_trading_realized)?;
  let sha=hash(&settings)?;let out=Self{settings,sha};out.bind(bank)?;Ok(out)
 }
 pub fn sha256(&self)->&str{&self.sha}
 /// Validate static model configuration before any account client/connect.
 /// Missing data after startup is an entry defer, never an inventory STOP.
 pub fn validate_model(&self,config:&edge::Config)->Result<()>{config.validate()?;need(config.markout_ms==self.settings.expected_inventory_age_ms,"MAKER_MODEL_HORIZON")}
 fn bind(&self,bank:&Session)->Result<()>{
  let s=&self.settings;let head=bank.durable_head();let report=bank.engine().report()?;
  need(bank.engine().is_live()&&bank.manifest_hash()==&json!(s.bank_manifest_hash)&&bank.live_account_seed().is_some_and(|seed|seed["runId"]==s.run_id),"MAKER_BANK_BINDING")?;
  need(s.baseline_native_head["manifestHash"]==*bank.manifest_hash()&&s.baseline_native_head["sequence"].as_u64().is_some_and(|n|head["sequence"].as_u64().is_some_and(|m|m>=n))&&s.baseline_native_head["previous"].as_str().is_some_and(hex),"MAKER_BASELINE_HEAD")?;
  let at=clock(&report["at"])?;need(at>=s.reviewed_at_ms,"MAKER_BEFORE_REVIEW")?;
  if head==s.baseline_native_head{need(hash(&report)?==s.baseline_report_sha256&&new_realized(&report)?==decimal_units(&s.baseline_new_trading_realized)?,"MAKER_BASELINE_REPORT")?;}
  need(bank.engine().policy()["maximumInventoryAgeMs"].as_u64()==Some(s.expected_inventory_age_ms)&&bank.engine().policy()["inventoryAgeClock"]=="execution","MAKER_INVENTORY_HORIZON")?;
  need(bank.engine().policy()["feeBps"]==0&&cash(&bank.engine().policy()["feePerShare"])?==0,"MAKER_UNSUPPORTED_FEE_POLICY")
 }
}
fn new_realized(report:&Value)->Result<i128>{if report.get("inheritedInventory").is_some(){cash(&report["newTradingRealized"])}else{cash(&report["realized"])}}
#[derive(Clone,Copy,Debug,PartialEq,Eq,Serialize)]
#[serde(rename_all="snake_case")]
pub enum Class{OwnedExit,CoveredHedge,Qualified,Exploration,Deferred}
/// No Deserialize/public constructor: quote, bank, dispatch and model evidence
/// cannot be assembled from arbitrary diagnostics by the caller.
pub struct Decision{class:Class,reason:String,proof:Value,quote:Value,ledger_sha:String,head:Value,dispatch:Value,settings_sha:String,run:String,tournament:String,expires:u64,model:Option<edge::RuntimeMakerEvaluation>,joint:Option<JointEvidence>}
impl Decision{
 pub fn class(&self)->Class{self.class}pub fn reason(&self)->&str{&self.reason}pub fn proof(&self)->&Value{&self.proof}
 pub fn review_sha256(&self)->&str{&self.settings_sha}
 pub fn allowed(&self)->bool{self.class!=Class::Deferred}
 pub fn candidate(&self)->Option<&edge::Candidate>{if self.class==Class::Qualified&&self.joint.is_none(){self.model.as_ref().and_then(|m|m.evaluation().candidate.as_ref())}else{None}}
 pub fn priority(&self)->u8{match self.class{Class::OwnedExit|Class::CoveredHedge=>0,Class::Qualified=>1,Class::Exploration=>2,Class::Deferred=>3}}
 pub fn validate_frozen(&self,quote:&Value,ledger:&Value,qid:u64,now:u64)->Result<()>{need(self.allowed()&&now<self.expires&&self.quote==*quote&&self.quote["id"].as_u64()==Some(qid)&&hash(ledger)?==self.ledger_sha,"MAKER_FROZEN_READY_CHANGED")}
 pub fn validate_current(&self,bank:&Session,review:&Reviewed,log:&DispatchLog,now:u64)->Result<()>{
  need(now<self.expires,"MAKER_DECISION_EXPIRED")?;
  need(self.allowed()&&bank.durable_head()==self.head&&dispatch(log)==self.dispatch&&review.sha==self.settings_sha,"MAKER_DECISION_CHANGED")?;review.bind(bank)?;log.require_resolved_attempts()?;
  let id=self.quote["id"].as_u64().ok_or("MAKER_QID")?;need(bank.engine().order_records().iter().any(|q|q==&self.quote)&&log.never_attempted(id),"MAKER_QUOTE_CHANGED")?;
  if let Some(m)=&self.model{m.validate_current(bank,id,now)?;}if let Some(j)=&self.joint{j.validate(bank,review,now)?;}Ok(())
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
impl Decision{
 fn rank_economics(&self)->Result<(i128,i128,u64,u64)>{
  if let Some(j)=&self.joint{let(gain,gross,horizon)=j.economics.ok_or("MAKER_RANK_CANDIDATE")?;return Ok((gain,gross,horizon,self.expires))}
  let c=self.candidate().ok_or("MAKER_RANK_CANDIDATE")?;Ok((c.net_gain_units,c.gross_funding_units,c.holding_horizon_ms,c.valid_until_ms))
 }
}
fn dispatch(log:&DispatchLog)->Value{log.proof(0)}
/// Selection only over opaque decisions, in the caller's existing fair order.
/// Owned management precedes qualified gain / (gross capital * horizon), then
/// exploration. Equal exact ratios retain input order. Expired/deferred entries
/// are omitted. This grants no risk budget or physical authority; re-evaluate
/// the selected quote after real preflight and retain its final physical token.
pub fn rank(decisions:&[&Decision],now:u64)->Result<Vec<usize>>{
 need(decisions.len()<=4096,"MAKER_RANK_BOUND")?;
 if let Some(first)=decisions.first(){need(decisions.iter().all(|d|d.head==first.head&&d.dispatch==first.dispatch&&d.settings_sha==first.settings_sha&&d.run==first.run&&d.tournament==first.tournament),"MAKER_RANK_CONTEXT")?;}
 let mut ordered:Vec<usize>=Vec::new();
 for(index,d)in decisions.iter().enumerate(){
  if !d.allowed()||now>=d.expires{continue}
  if d.class==Class::Qualified{let(gain,gross,horizon,until)=d.rank_economics()?;need(gain>0&&gross>0&&horizon>0&&now<until,"MAKER_RANK_ECONOMICS")?;mul(gross,horizon as i128)?;}
  let mut position=ordered.len();for(i,old_index)in ordered.iter().enumerate(){let old=decisions[*old_index];let before=if d.priority()!=old.priority(){d.priority()<old.priority()}else if d.class==Class::Qualified{
   let(gain,gross,horizon,_)=d.rank_economics()?;let(old_gain,old_gross,old_horizon,_)=old.rank_economics()?;
   mul(gain,mul(old_gross,old_horizon as i128)?)?>mul(old_gain,mul(gross,horizon as i128)?)?
  }else{false};if before{position=i;break}}
  ordered.insert(position,index);
 }Ok(ordered)
}
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
 need(q["expiresAt"].as_u64().ok_or("MAKER_QUOTE_STATE")?>now,"MAKER_QUOTE_EXPIRED")?;
 need(!bank.engine().is_package_order(qid)&&q["retired"]==false&&q["filled"]==0&&q.get("cancelEffectiveAt").is_none_or(Value::is_null)&&log.never_attempted(qid)&&q["expiresAt"].as_u64().is_some_and(|t|t>now),"MAKER_QUOTE_STATE")?;
 need(scope.exchanges.get(q["exchangeId"].as_str().ok_or("MAKER_SCOPE")?).is_some_and(|m|q["marketId"]==*m),"MAKER_SCOPE")?;
 let mut d=Decision{class:Class::Deferred,reason:"not_evaluated".into(),proof:json!({}),ledger_sha:hash(&bank.engine().report()?)?,head:bank.durable_head(),dispatch:dispatch(log),settings_sha:review.sha.clone(),run:review.settings.run_id.clone(),tournament:scope.tournament_id.clone(),expires:q["expiresAt"].as_u64().unwrap(),quote:q.clone(),model:None,joint:None};
 if q["action"]=="sell"{need(remaining(&q)?<=bank.engine().wallet.quantity(q["exchangeId"].as_str().unwrap(),if q["side"]=="YES"{Side::Yes}else{Side::No}),"MAKER_EXIT_UNCOVERED")?;d.class=Class::OwnedExit;d.reason="owned_exit_keeps_original_authority".into();}
 else{need(q["action"]=="buy","MAKER_ACTION")?;if covered_hedge(bank,log,&q)?{d.class=Class::CoveredHedge;d.reason="fully_covered_fifo_nonnegative_redemption".into();}
 else{
 let s=&review.settings;let gross=mul(cash(&q["price"])?,remaining(&q)? as i128)?;let horizon=edge::CapitalHorizon{milliseconds:s.expected_inventory_age_ms,source_sha256:hash(bank.engine().policy())?};let mut costs=edge::ExecutionCosts{inventory_and_partial_risk_units:0,source_sha256:review.sha.clone(),horizon};
 if let Some(j)=&s.joint_execution{joint_entry(bank,scope,log,review,&q,qid,now,j,&costs,&mut d)?;}else{
 match edge::evaluate_runtime_maker(bank,qid,Some(&costs),now){Err(e)=>{d.reason=format!("observer_unavailable:{e}");},Ok(initial)=>{
  if initial.config().markout_ms!=s.expected_inventory_age_ms{d.reason="model_horizon_mismatch".into();d.model=Some(initial);}else{let estimate=&initial.evaluation().estimate;let veto=estimate.valid_marks>=initial.config().minimum_samples&&estimate.valid_marks*10000>=estimate.matured_marks*initial.config().minimum_coverage_bps as usize&&estimate.stressed_conditional_per_share.is_some_and(|n|n<=0);
  d.expires=d.expires.min(initial.valid_until_ms());
  if veto{d.reason="covered_empirical_nonpositive_markout_veto".into();d.model=Some(initial);}
  else if estimate.available{
   costs.inventory_and_partial_risk_units=add(partial_risk_charge(gross,estimate.conservative_fill_probability_bps.ok_or("MAKER_PROBABILITY")?)?,decimal_units(&s.additional_portfolio_charge)?)?;
   let scored=edge::evaluate_runtime_maker(bank,qid,Some(&costs),now)?;if scored.evaluation().candidate.as_ref().is_some_and(|c|c.net_gain_units>0&&now<c.valid_until_ms){d.class=Class::Qualified;d.reason="empirical_gain_after_conservative_partial_and_portfolio_charge".into();}else{d.reason="qualified_evidence_nonpositive_after_risk_charge".into();}d.model=Some(scored);
  }else{let deployed=deployed_new_capital(bank,log)?;let realized=new_realized(&bank.engine().report()?)?;let baseline=decimal_units(&s.baseline_new_trading_realized)?;let loss=baseline.checked_sub(realized).ok_or("MAKER_OVERFLOW")?.max(0);
   d.reason=if remaining(&q)?>s.exploration_maximum_quantity||gross>decimal_units(&s.exploration_maximum_order_notional)?{"exploration_order_cap"}else if add(deployed,gross)?>decimal_units(&s.exploration_maximum_deployed_notional)?{"exploration_deployed_cap"}else if loss>=decimal_units(&s.additional_realized_loss_stop)?{"exploration_realized_loss_cap"}else{d.class=Class::Exploration;"bounded_unqualified_exploration_not_positive_ev"}.into();d.model=Some(initial);
  }
 }}}
 }
 }}
 d.proof=json!({"class":d.class,"reason":d.reason,"qid":qid,"nativeHead":d.head,"dispatchHead":d.dispatch,"reviewedSettingsSha256":d.settings_sha,"expiresAtMs":d.expires,"statisticalConfidenceClaimed":false,"executionAuthority":false,"baselineHistoricalAncestryProven":false,"realizedLossStopIsHardLossBound":false,"maximumOutstandingExploratoryDownside":review.settings.exploration_maximum_deployed_notional,"partialRiskChargeKind":"explicit_full_loss_non_full_trial_policy_not_calibrated_estimate","model":d.model.as_ref().map(|m|m.evaluation())});
 if review.settings.joint_execution.is_some(){
  d.proof["qualificationModel"]=json!("joint_zero_partial_full_v1");d.proof["legacyModelRole"]=json!("current_quote_source_eligibility_and_comparative_diagnostic_only");
  d.proof["partialRiskChargeKind"]=json!("joint_observed_outcomes_no_extra_full_loss_nonfull_charge");
  d.proof["jointExecution"]=serde_json::to_value(d.joint.as_ref().map(|j|json!({"observerSourceSha256":j.observer_sha,"configSha256":j.config_sha,"strategySha256":j.strategy,"estimate":j.estimate,"netGainUnits":j.economics.map(|x|x.0),"grossFundingUnits":j.economics.map(|x|x.1),"reviewedRankingHorizonMs":j.economics.map(|x|x.2),"rankingHorizonIsNotCashReleaseGuarantee":true,"additionalPortfolioCharge":review.settings.additional_portfolio_charge,"descriptiveNotCalibrated":true})))?;
 }Ok(d)
}

// Private evidence is constructed only from the actual thread-local observer.
// No Deserialize or caller-provided fitted means/counts can mint a Decision.
struct JointEvidence{observer_sha:String,config_sha:String,strategy:String,scope:Scope,action:Value,created:u64,expiry:u64,horizon:u64,estimate:Option<distribution::Estimate>,economics:Option<(i128,i128,u64)>}
fn joint_strategy(bank:&Session,config_sha:&str,review_sha:&str)->Result<String>{hash(&json!({"kind":"ordinary_passive","model":config_sha,"maker":review_sha,"policy":hash(bank.engine().policy())?}))}
impl JointEvidence{
 fn validate(&self,bank:&Session,review:&Reviewed,now:u64)->Result<()>{
  need(distribution::observer_diagnostic().as_ref().map(hash).transpose()?.as_deref()==Some(self.observer_sha.as_str()),"MAKER_JOINT_SOURCE_CHANGED")?;
  let(_,sha)=distribution::runtime_configuration(bank)?;need(sha==self.config_sha&&self.strategy==joint_strategy(bank,&sha,&review.sha)?,"MAKER_JOINT_CONFIG_CHANGED")?;
  let current=distribution::estimate_runtime_action(bank,&self.scope,&self.action,&self.strategy,self.created,self.expiry,self.horizon,now);
  match(&self.estimate,current){(Some(old),Ok(new))=>need(hash(old)?==hash(&new)?,"MAKER_JOINT_ESTIMATE_CHANGED"),(None,Err("DIST_QUERY_STRATEGY_UNOBSERVED"))=>Ok(()),_=>Err("MAKER_JOINT_OBSERVER_CHANGED".into())}
 }
}
fn joint_covered(e:&distribution::Estimate,j:&JointExecution,c:&distribution::Config)->bool{
 e.qualified_descriptive_sample&&e.parent_completed>=j.minimum_completed.max(c.minimum_completed)
  &&e.time_blocks>=j.minimum_time_blocks.max(c.minimum_blocks)&&e.parent_counts.iter().all(|n|*n>=j.minimum_per_class)
  &&e.parent_counts.iter().sum::<usize>()==e.parent_completed&&e.value_covered==e.parent_completed&&e.value_censored==0
  &&e.unresolved==0&&e.capital_time_incomplete==0&&e.capital_time_charge_per_requested_share.is_some()
  &&e.empirical_block_standard_error.is_some_and(|x|x.is_finite()&&x>=0.)
  &&[e.uncertainty_charge_per_requested_share,e.tail_charge_per_requested_share,e.mean_write_charge_per_requested_share,e.capital_time_charge_per_requested_share].iter().all(|x|x.is_some_and(|n|n.is_finite()&&n>=0.))
}
/// Floor the positive empirical score PER requested share before multiplying.
/// Joint score already charges write, tail, uncertainty and actual capital time.
fn joint_net(score:f64,n:u64,portfolio:i128)->Result<i128>{
 need(score.is_finite()&&score<=UNIT as f64&&n>0&&portfolio>=0,"MAKER_JOINT_SCORE_RANGE")?;
 need(score>=-(UNIT as f64)*10.,"MAKER_JOINT_SCORE_RANGE")?;
 add(mul(score.floor()as i128,n as i128)?,portfolio.checked_neg().ok_or("MAKER_OVERFLOW")?)
}
fn bounded_exploration(bank:&Session,log:&DispatchLog,review:&Reviewed,q:&Value,d:&mut Decision)->Result<()>{
 let s=&review.settings;let gross=mul(cash(&q["price"])?,remaining(q)? as i128)?;let deployed=deployed_new_capital(bank,log)?;
 let loss=decimal_units(&s.baseline_new_trading_realized)?.checked_sub(new_realized(&bank.engine().report()?)?).ok_or("MAKER_OVERFLOW")?.max(0);
 d.reason=if remaining(q)?>s.exploration_maximum_quantity||gross>decimal_units(&s.exploration_maximum_order_notional)?{"exploration_order_cap"}else if add(deployed,gross)?>decimal_units(&s.exploration_maximum_deployed_notional)?{"exploration_deployed_cap"}else if loss>=decimal_units(&s.additional_realized_loss_stop)?{"exploration_realized_loss_cap"}else{d.class=Class::Exploration;"bounded_unqualified_joint_exploration_not_positive_ev"}.into();Ok(())
}
fn joint_entry(bank:&Session,scope:&Scope,log:&DispatchLog,review:&Reviewed,q:&Value,qid:u64,now:u64,j:&JointExecution,costs:&edge::ExecutionCosts,d:&mut Decision)->Result<()>{
 // Legacy positive/negative scores NEVER decide qualification in this mode.
 let initial=match edge::evaluate_runtime_maker(bank,qid,Some(costs),now){Ok(v)=>v,Err(e)=>{d.reason=format!("current_source_unavailable:{e}");return Ok(())}};
 d.expires=d.expires.min(initial.valid_until_ms());d.model=Some(initial);
 let(config,sha)=match distribution::runtime_configuration(bank){Ok(v)=>v,Err(e)=>{d.reason=format!("joint_observer_unavailable:{e}");return Ok(())}};
 if sha!=j.distribution_config_sha256||config.markout_ms!=review.settings.expected_inventory_age_ms||config.retained_samples<j.minimum_completed{d.reason="joint_config_or_horizon_mismatch".into();return Ok(())}
 let strategy=joint_strategy(bank,&sha,&review.sha)?;let action=json!({"tournamentId":scope.tournament_id,"marketId":q["marketId"],"exchangeId":q["exchangeId"],"side":q["side"].as_str().ok_or("MAKER_SIDE")?.to_lowercase(),"action":"buy","quantity":q["quantity"],"limitPrice":q["price"]});
 let created=q["createdAt"].as_u64().ok_or("MAKER_CLOCK")?;let expiry=q["expiresAt"].as_u64().ok_or("MAKER_CLOCK")?;
 let observer_sha=hash(&distribution::observer_diagnostic().ok_or("MAKER_JOINT_OBSERVER_REQUIRED")?)?;
 let mut evidence=JointEvidence{observer_sha,config_sha:sha,strategy,scope:scope.clone(),action,created,expiry,horizon:review.settings.expected_inventory_age_ms,estimate:None,economics:None};
 match distribution::estimate_runtime_action(bank,scope,&evidence.action,&evidence.strategy,created,expiry,evidence.horizon,now){
  Err("DIST_QUERY_STRATEGY_UNOBSERVED")=>bounded_exploration(bank,log,review,q,d)?,
  Err(e)=>{d.reason=format!("joint_features_unavailable:{e}");return Ok(())},
  Ok(e)=>{
   if joint_covered(&e,j,&config){
    if let Some(score)=e.net_shadow_score_per_requested_share.filter(|s|s.is_finite()){
     let n=remaining(q)?;let net=if score>0.{joint_net(score,n,decimal_units(&review.settings.additional_portfolio_charge)?)?}else{0};
     if net>0{d.class=Class::Qualified;d.reason="joint_requested_value_positive_after_all_declared_charges".into();evidence.economics=Some((net,mul(cash(&q["price"])?,n as i128)?,review.settings.expected_inventory_age_ms));}
     else{d.reason="covered_joint_nonpositive_after_all_declared_charges".into();}
    }else{d.reason="joint_score_unavailable_not_zero".into();}
   }else{bounded_exploration(bank,log,review,q,d)?;}
   evidence.estimate=Some(e);
  }
 }
 d.joint=Some(evidence);Ok(())
}

#[cfg(test)]mod joint_arithmetic_tests{
 use super::*;
 #[test]fn requested_share_score_does_not_charge_unfilled_principal_again(){
  // A JOINT score of9,000.9 micro-units/requested share already incorporates
  // zero/partial/full outcomes and declared uncertainty/tail/write/capital costs.
  assert_eq!(joint_net(9000.9,2,1).unwrap(),17999);
  assert!(partial_risk_charge(800000,5000).unwrap()>joint_net(9000.9,2,1).unwrap());
  assert_eq!(joint_net(0.,2,0).unwrap(),0);assert_eq!(joint_net(1.,2,3).unwrap(),-1);
  assert!(joint_net(f64::NAN,2,0).is_err());assert!(joint_net(f64::INFINITY,2,0).is_err());assert!(joint_net(1_000_001.,2,0).is_err());
 }
 fn config()->distribution::Config{distribution::Config{markout_ms:60000,post_window_ms:1000,maximum_book_age_ms:10000,maximum_delivery_ms:1000,block_ms:60000,minimum_blocks:20,minimum_completed:100,retained_samples:128,maximum_pending:128,shrinkage_strength:4,minimum_value_coverage_bps:10000,uncertainty_floor_per_share:1000,tail_charge_per_filled_share:500,write_charge_per_order:100,capital_charge_per_million_unit_ms:1,fee_policy_source_sha256:"a".repeat(64),exit_fee_per_share:0}}
 // Consumer-boundary fixture only, never a mintable live estimate/token.
 fn summary()->distribution::Estimate{distribution::Estimate{
  pool_key:distribution::PoolKey{policy_sha256:"a".repeat(64),strategy_sha256:"b".repeat(64),side:"YES".into(),price_band:2,spread_band:2,distance_band:0,quantity_depth_band:0,quote_age_band:0,lifetime_band:2,utc_six_hour_block:0},parent_completed:100,market_completed:20,distinct_markets:5,time_blocks:20,parent_counts:[34,33,33],market_counts:[7,7,6],posterior_zero_partial_full:[0.34,0.33,0.33],expected_filled_fraction:Some(0.495),observed_joint_mean_per_requested_share:Some(10000.),shrunk_joint_mean_per_requested_share:Some(10000.),conditional_per_filled_share:Some(20000.),minimum_observed_per_requested_share:Some(-1000.),empirical_block_standard_error:Some(1000.),uncertainty_charge_per_requested_share:Some(2000.),tail_charge_per_requested_share:Some(100.),mean_write_charge_per_requested_share:Some(100.),capital_time_charge_per_requested_share:Some(100.),net_shadow_score_per_requested_share:Some(7700.),value_covered:100,value_censored:0,unresolved:0,first_fill_latency_counts:[1,2,3,4],terminal_status_counts:Default::default(),capital_time_incomplete:0,leave_market_out_joint_mean_range:Some([9000.,11000.]),qualified_descriptive_sample:true,confidence_kind:"fixture_only",cancellation_is_informative:true,independent_censoring_assumed:false,execution_authority:false}}
 #[test]fn production_joint_coverage_rejects_censoring_open_capital_missing_classes_and_correlated_single_block(){
  let j=JointExecution{version:1,distribution_config_sha256:"c".repeat(64),minimum_completed:100,minimum_time_blocks:20,minimum_per_class:1};let c=config();assert!(joint_covered(&summary(),&j,&c));
  for case in 0..9{let mut e=summary();match case{0=>e.value_censored=1,1=>e.value_covered=99,2=>e.unresolved=1,3=>e.capital_time_incomplete=1,4=>e.time_blocks=1,5=>e.parent_counts=[50,0,50],6=>e.capital_time_charge_per_requested_share=None,7=>e.empirical_block_standard_error=Some(f64::NAN),_=>e.parent_completed=99};assert!(!joint_covered(&e,&j,&c),"case {case}");}
 }

}
