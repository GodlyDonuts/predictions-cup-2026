//! Offline execution-eligibility candidate. Observing a fill is not admitting it
//! to the financial bank. This module creates no account or dispatch authority.
use chrono::{DateTime,FixedOffset,SecondsFormat};
use serde::{Serialize,Deserialize};
use sha2::{Digest,Sha256};
use std::collections::{BTreeMap,BTreeSet};

#[derive(Clone,Debug,Serialize,Deserialize,PartialEq,Eq)]
#[serde(deny_unknown_fields)]
pub struct Scope {pub account_tag:String,pub tournament_id:String,pub exchange_id:String,pub market_id:String}
#[derive(Clone,Debug,Serialize,Deserialize,PartialEq,Eq)]
#[serde(tag="kind",deny_unknown_fields)]
pub enum Source {Engine{epoch:String},Legacy{epoch:String}}
#[derive(Clone,Copy,Debug,Serialize,Deserialize,PartialEq,Eq)]
pub enum Side {Yes,No}
#[derive(Clone,Copy,Debug,Serialize,Deserialize,PartialEq,Eq)]
pub enum Action {Buy,Sell}
#[derive(Clone,Debug,Serialize,Deserialize,PartialEq,Eq)]
#[serde(deny_unknown_fields)]
pub struct Fill {pub order_id:String,pub trade_id:String,pub side:Side,pub action:Action,pub quantity:u64,pub price_units:i128,pub fee_units:i128,pub executed_at:String}
impl Fill {fn key(&self)->String{format!("{}:{}",self.order_id,self.trade_id)}}
#[derive(Clone,Debug,Serialize,Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Tape {
 pub scope:Scope,pub source:Source,pub order_id:String,pub receipt_sha256:String,
 pub request_started_at:String,pub received_at:String,pub complete:bool,
 pub has_more:bool,pub next_cursor:Option<String>,pub projected_through_sequence:Option<u64>,
 pub signed_total:i64,pub fills:Vec<Fill>,
}
#[derive(Clone,Debug,Serialize,Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Terminal {
 pub scope:Scope,pub source:Source,pub order_id:String,
 pub receipt_sha256:String,pub received_at:String,pub open:bool,pub filled_quantity:u64,
}
#[derive(Clone,Debug,Serialize,Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Binding {pub order_id:String,pub side:Side,pub action:Action,pub admitted_quantity:u64,pub terminal:Option<Terminal>}
/// These assertions require verification by the sole dispatcher. They are not
/// independently verified account facts and cannot authorize any API operation.
#[derive(Clone,Debug)]
pub struct ClosedRoster {
 pub bindings:Vec<Binding>,pub dispatcher_generation:String,
 pub submissions_frozen:bool,pub all_owned_orders_included:bool,
 pub unknown_attempts:u64,pub foreign_or_unbound_exposure:bool,
}
#[derive(Clone,Debug,Serialize,Deserialize,PartialEq,Eq)]
#[serde(deny_unknown_fields)]
pub struct BankAnchor {pub sequence:u64,pub hash:String}
#[derive(Clone,Debug,Serialize,Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Issue {pub code:String,pub receipt_sha256:String}
#[derive(Clone,Debug,Serialize,Deserialize)]
#[serde(deny_unknown_fields)]
pub struct State {
 pub schema_version:u32,pub scope:Scope,pub source:Source,pub bank_anchor:BankAnchor,
 pub admitted:BTreeMap<String,Fill>,pub observed:BTreeMap<String,Fill>,
 pub latest_tapes:BTreeMap<String,Tape>,pub receipt_references:BTreeSet<String>,
 pub conflicting_facts:Vec<Fill>,pub issues:Vec<Issue>,pub frontier:Option<String>,
}
#[derive(Debug)]
pub struct Plan {
 pub eligible_for_review:bool,pub execution_authority:bool,pub reasons:Vec<String>,
 pub staged_fills:Vec<Fill>,pub receipt_references:Vec<String>,
 prepared_against:String,closed_cohort_reviewed:bool,
}
fn clock(s:&str)->Result<DateTime<FixedOffset>,String>{DateTime::parse_from_rfc3339(s).map_err(|_|"CLOCK".into())}
fn hex(s:&str)->bool{s.len()==64&&s.bytes().all(|b|b.is_ascii_hexdigit())}
fn ident(s:&str)->bool{!s.is_empty()&&s.len()<=128&&!s.chars().any(char::is_control)}
fn numeric(s:&str)->bool{!s.is_empty()&&!s.starts_with('0')&&s.bytes().all(|b|b.is_ascii_digit())&&s.len()<=32}
fn normalized(mut f:Fill)->Result<Fill,String>{
 if !numeric(&f.order_id)||!numeric(&f.trade_id)||f.quantity==0||f.quantity>i32::MAX as u64||!(5000..=995000).contains(&f.price_units)||f.price_units%5000!=0||f.fee_units<0{return Err("FILL_FACTS".into())}
 f.executed_at=clock(&f.executed_at)?.to_rfc3339_opts(SecondsFormat::Nanos,true);Ok(f)
}
impl State {
 pub fn new(scope:Scope,source:Source,anchor:BankAnchor)->Result<Self,String>{
  if anchor.sequence!=0{return Err("NONEMPTY_BANK_REQUIRES_AUDITED_BOOTSTRAP".into())}
  let state=Self{schema_version:1,scope,source,bank_anchor:anchor,admitted:BTreeMap::new(),observed:BTreeMap::new(),latest_tapes:BTreeMap::new(),receipt_references:BTreeSet::new(),conflicting_facts:vec![],issues:vec![],frontier:None};state.audit()?;Ok(state)
 }
 fn problem(&mut self,code:&str,receipt:&str){self.issues.push(Issue{code:code.into(),receipt_sha256:receipt.into()});}
 /// Retains valid newly observed facts even when execution eligibility is lost.
 /// The caller must retain original private bytes BEFORE invoking this method.
 pub fn observe(&mut self,tape:Tape){
  let receipt=tape.receipt_sha256.clone();self.receipt_references.insert(receipt.clone());
  if self.audit().is_err(){self.problem("CHECKPOINT_STATE_INVALID",&receipt);return}
  let checked=(||->Result<Tape,String>{
   if tape.scope!=self.scope||tape.source!=self.source{return Err("SCOPE_OR_SOURCE_EPOCH".into())}
   if !hex(&receipt)||!numeric(&tape.order_id)||!tape.complete||tape.has_more||tape.next_cursor.is_some()||tape.fills.len()>10000{return Err("INCOMPLETE_OR_INVALID_TAPE".into())}
   let received=clock(&tape.received_at)?;if clock(&tape.request_started_at)?>received{return Err("TAPE_CLOCK".into())}
   let mut next=tape;let mut keys=BTreeSet::new();let mut total=0i64;let mut side=None;let mut action=None;
   for fact in &mut next.fills {
    *fact=normalized(fact.clone())?;
    if fact.order_id!=next.order_id||clock(&fact.executed_at)?>received||!keys.insert(fact.key())||side.is_some_and(|s|s!=fact.side)||action.is_some_and(|a|a!=fact.action){return Err("TAPE_FILL_SCOPE_OR_DUPLICATE".into())}
    side=Some(fact.side);action=Some(fact.action);let n=i64::try_from(fact.quantity).map_err(|_|"QUANTITY")?;total=total.checked_add(if fact.side==Side::No{-n}else{n}).ok_or("QUANTITY")?;
   }
   if total!=next.signed_total{return Err("TAPE_TOTAL".into())}Ok(next)
  })();
  let tape=match checked{Ok(t)=>t,Err(code)=>{self.problem(&code,&receipt);return}};
  let keys=tape.fills.iter().map(Fill::key).collect::<BTreeSet<_>>();
  if self.observed.values().any(|f|f.order_id==tape.order_id&&!keys.contains(&f.key())){self.problem("CUMULATIVE_TAPE_LOST_FILL",&receipt)}
  for fact in &tape.fills {
   if let Some(old)=self.observed.get(&fact.key()){
    if old!=fact{self.conflicting_facts.push(fact.clone());self.problem("FILL_ID_CONFLICT",&receipt)}
   }else{
    if self.frontier.as_ref().is_some_and(|at|clock(&fact.executed_at).unwrap()<=clock(at).unwrap()) {self.problem("UNSEEN_FILL_AT_OR_BEFORE_ADMITTED_FRONTIER",&receipt)}
    self.observed.insert(fact.key(),fact.clone());
   }
  }
  self.latest_tapes.insert(tape.order_id.clone(),tape);
 }
 pub fn plan(&self,roster:&ClosedRoster)->Plan{
  if self.audit().is_err(){return Plan{eligible_for_review:false,execution_authority:false,reasons:vec!["CHECKPOINT_STATE_INVALID".into()],staged_fills:self.observed.values().cloned().collect(),receipt_references:self.receipt_references.iter().cloned().collect(),prepared_against:self.fingerprint(),closed_cohort_reviewed:false}}
  let mut reasons=self.issues.iter().map(|i|i.code.clone()).collect::<Vec<_>>();
  if !roster.submissions_frozen||!roster.all_owned_orders_included||!ident(&roster.dispatcher_generation)||roster.unknown_attempts!=0||roster.foreign_or_unbound_exposure{reasons.push("DISPATCHER_ROSTER_NOT_FROZEN_OR_UNCERTAIN".into())}
  let mut orders=BTreeSet::new();
  for binding in &roster.bindings {
   if !numeric(&binding.order_id)||binding.admitted_quantity==0||!orders.insert(binding.order_id.clone()){reasons.push("ROSTER_BINDING".into());continue}
   let Some(tape)=self.latest_tapes.get(&binding.order_id)else{reasons.push("MISSING_OWNED_TAPE".into());continue};
   let Some(terminal)=&binding.terminal else{reasons.push("ACTIVE_ORDER_HAS_NO_CLOSED_LIFECYCLE_BARRIER".into());continue};
   let correct_clock=clock(&terminal.received_at).ok().zip(clock(&tape.request_started_at).ok()).is_some_and(|(end,start)|start>=end);
   if terminal.scope!=self.scope||terminal.source!=self.source||terminal.order_id!=binding.order_id||terminal.open||!hex(&terminal.receipt_sha256)||!correct_clock||terminal.filled_quantity!=tape.signed_total.unsigned_abs()||terminal.filled_quantity>binding.admitted_quantity||tape.fills.iter().any(|f|f.side!=binding.side||f.action!=binding.action){reasons.push("TERMINAL_TAPE_IDENTITY_TOTAL_OR_ORDER".into())}
  }
  if roster.bindings.is_empty()||roster.bindings.len()>4096||self.latest_tapes.keys().any(|id|!orders.contains(id)){reasons.push("ROSTER_INCOMPLETE".into())}
  let mut staged=self.observed.iter().filter(|(key,_)|!self.admitted.contains_key(*key)).map(|(_,f)|f.clone()).collect::<Vec<_>>();
  staged.sort_by(|a,b|clock(&a.executed_at).unwrap().cmp(&clock(&b.executed_at).unwrap()).then_with(||a.key().cmp(&b.key())));
  // Cross-order clock ties cannot be ordered from a legacy/engine ID guess.
  if staged.windows(2).any(|p|p[0].executed_at==p[1].executed_at&&(p[0].order_id!=p[1].order_id||p[0].price_units!=p[1].price_units)){reasons.push("AMBIGUOUS_EXECUTION_TIE".into())}
  if self.audit().is_err(){reasons.push("CHECKPOINT_STATE_INVALID".into())}
  reasons.sort();reasons.dedup();let reviewed=reasons.is_empty();Plan{eligible_for_review:reviewed,execution_authority:false,reasons,staged_fills:staged,receipt_references:self.receipt_references.iter().cloned().collect(),prepared_against:self.fingerprint(),closed_cohort_reviewed:reviewed}
 }
 /// Called only AFTER the caller verifies the corresponding immutable bank
 /// prefix actually admitted exactly these facts. It neither writes nor trades.
 pub fn record_admitted(&self,plan:&Plan,bank_anchor:BankAnchor)->Result<Self,String>{
  self.audit()?;
  if !plan.eligible_for_review||plan.execution_authority||!plan.reasons.is_empty()||!plan.closed_cohort_reviewed||plan.prepared_against!=self.fingerprint()||!self.issues.is_empty()||!hex(&bank_anchor.hash)||bank_anchor.sequence<self.bank_anchor.sequence||(!plan.staged_fills.is_empty()&&bank_anchor.sequence==self.bank_anchor.sequence){return Err("ADMISSION_REQUIRES_VERIFIED_BANK_PREFIX".into())}
  let mut next=self.clone();let expected=self.observed.iter().filter(|(key,_)|!self.admitted.contains_key(*key)).map(|(key,f)|(key.clone(),f.clone())).collect::<BTreeMap<_,_>>();
  let received=plan.staged_fills.iter().map(|f|(f.key(),f.clone())).collect::<BTreeMap<_,_>>();if expected!=received||received.len()!=plan.staged_fills.len(){return Err("ADMISSION_FACTS_CHANGED".into())}
  next.admitted.extend(received);next.bank_anchor=bank_anchor;next.frontier=next.admitted.values().map(|f|f.executed_at.clone()).max_by(|a,b|clock(a).unwrap().cmp(&clock(b).unwrap()));next.audit()?;Ok(next)
 }
 pub fn audit(&self)->Result<(),String>{
  let epoch=match &self.source{Source::Engine{epoch}|Source::Legacy{epoch}=>epoch};
  if self.schema_version!=1||!ident(&self.scope.account_tag)||!ident(&self.scope.tournament_id)||!numeric(&self.scope.exchange_id)||!numeric(&self.scope.market_id)||!ident(epoch)||!hex(&self.bank_anchor.hash)||self.observed.len()>200000{return Err("STATE_SCOPE_OR_BOUNDS".into())}
  for(key,f)in &self.observed{if normalized(f.clone())?!=*f||key!=&f.key(){return Err("STATE_FACTS".into())}}
  if self.admitted.iter().any(|(key,f)|self.observed.get(key)!=Some(f)){return Err("ADMITTED_FACTS_CHANGED".into())}
  let frontier=self.admitted.values().map(|f|f.executed_at.clone()).max_by(|a,b|clock(a).unwrap().cmp(&clock(b).unwrap()));if self.frontier!=frontier{return Err("FRONTIER_CHANGED".into())}Ok(())
 }
 pub fn checkpoint(&self)->Result<Vec<u8>,String>{self.audit()?;let bytes=serde_json::to_vec(self).map_err(|_|"CHECKPOINT_JSON")?;let sum=format!("{:x}",Sha256::digest(&bytes));serde_json::to_vec(&(sum,self)).map_err(|_|"CHECKPOINT_JSON".into())}
 fn fingerprint(&self)->String{format!("{:x}",Sha256::digest(serde_json::to_vec(self).expect("serializable staging state")))}
 pub fn restore(bytes:&[u8],expected_anchor:&BankAnchor)->Result<Self,String>{
  if bytes.len()>64*1024*1024{return Err("CHECKPOINT_BOUNDS".into())}let(sum,state):(String,Self)=serde_json::from_slice(bytes).map_err(|_|"CHECKPOINT_JSON")?;let body=serde_json::to_vec(&state).map_err(|_|"CHECKPOINT_JSON")?;
  if sum!=format!("{:x}",Sha256::digest(&body))||&state.bank_anchor!=expected_anchor{return Err("CHECKPOINT_OR_BANK_ANCHOR_CHANGED".into())}state.audit()?;Ok(state)
 }
}

#[cfg(test)]mod tests{
 use super::*;
 fn scope()->Scope{Scope{account_tag:"fixture".into(),tournament_id:"cup".into(),exchange_id:"994".into(),market_id:"305".into()}}
 fn source()->Source{Source::Engine{epoch:"verified-fixture-engine-epoch".into()}}
 fn anchor(n:u64)->BankAnchor{BankAnchor{sequence:n,hash:"a".repeat(64)}}
 fn state()->State{State::new(scope(),source(),anchor(0)).unwrap()}
 fn fill(order:&str,trade:&str,second:u8,side:Side,price:i128)->Fill{Fill{order_id:order.into(),trade_id:trade.into(),side,action:Action::Buy,quantity:1,price_units:price,fee_units:0,executed_at:format!("2026-10-15T00:00:{second:02}Z")}}
 fn tape(order:&str,fills:Vec<Fill>)->Tape{let total=fills.iter().map(|f|if f.side==Side::No{-(f.quantity as i64)}else{f.quantity as i64}).sum();Tape{scope:scope(),source:source(),order_id:order.into(),receipt_sha256:"b".repeat(64),request_started_at:"2026-10-15T00:00:10Z".into(),received_at:"2026-10-15T00:00:11Z".into(),complete:true,has_more:false,next_cursor:None,projected_through_sequence:Some(50),signed_total:total,fills}}
 fn binding(order:&str,side:Side,n:u64)->Binding{Binding{order_id:order.into(),side,action:Action::Buy,admitted_quantity:n,terminal:Some(Terminal{scope:scope(),source:source(),order_id:order.into(),receipt_sha256:"c".repeat(64),received_at:"2026-10-15T00:00:09Z".into(),open:false,filled_quantity:n})}}
 fn roster(bindings:Vec<Binding>)->ClosedRoster{ClosedRoster{bindings,dispatcher_generation:"fixture-generation".into(),submissions_frozen:true,all_owned_orders_included:true,unknown_attempts:0,foreign_or_unbound_exposure:false}}
 #[test]fn closed_roster_sorts_all_orders_but_never_confers_execution_authority(){let mut s=state();s.observe(tape("201",vec![fill("201","302",2,Side::Yes,400000),fill("201","301",1,Side::Yes,200000)]));s.observe(tape("202",vec![fill("202","303",3,Side::No,500000)]));let p=s.plan(&roster(vec![binding("201",Side::Yes,2),binding("202",Side::No,1)]));assert!(p.eligible_for_review);assert!(!p.execution_authority);assert_eq!(p.staged_fills.iter().map(|f|f.trade_id.as_str()).collect::<Vec<_>>(),vec!["301","302","303"]);assert!(s.admitted.is_empty());}
 #[test]fn cumulative_tape_duplicates_do_not_double_count_and_missing_old_fill_blocks(){let mut s=state();let t=tape("201",vec![fill("201","301",1,Side::Yes,200000)]);s.observe(t.clone());s.observe(t);assert_eq!(s.observed.len(),1);s.observe(tape("201",vec![]));assert!(s.plan(&roster(vec![binding("201",Side::Yes,1)])).reasons.contains(&"CUMULATIVE_TAPE_LOST_FILL".into()));assert_eq!(s.observed.len(),1);}
 #[test]fn delayed_older_fill_is_retained_and_blocks_after_checkpoint_restore(){let mut s=state();s.observe(tape("202",vec![fill("202","303",3,Side::No,500000)]));let p=s.plan(&roster(vec![binding("202",Side::No,1)]));let committed=s.record_admitted(&p,anchor(1)).unwrap();let mut restored=State::restore(&committed.checkpoint().unwrap(),&anchor(1)).unwrap();restored.observe(tape("201",vec![fill("201","301",1,Side::Yes,200000)]));let p=restored.plan(&roster(vec![binding("201",Side::Yes,1),binding("202",Side::No,1)]));assert!(!p.eligible_for_review);assert!(p.reasons.contains(&"UNSEEN_FILL_AT_OR_BEFORE_ADMITTED_FRONTIER".into()));assert_eq!(p.staged_fills.len(),1);assert_eq!(restored.admitted.len(),1);assert_eq!(restored.observed.len(),2);assert!(restored.record_admitted(&p,anchor(2)).is_err());}
 #[test]fn coverage_checkpoint_does_not_make_active_orders_terminal(){let mut s=state();s.observe(tape("201",vec![fill("201","301",1,Side::Yes,200000)]));let mut b=binding("201",Side::Yes,1);b.terminal=None;let p=s.plan(&roster(vec![b]));assert!(!p.eligible_for_review);assert!(p.reasons.contains(&"ACTIVE_ORDER_HAS_NO_CLOSED_LIFECYCLE_BARRIER".into()));}
 #[test]fn namespace_and_scope_mismatch_cannot_merge_identical_trade_ids(){for case in 0..3{let mut s=state();let mut t=tape("201",vec![fill("201","301",1,Side::Yes,200000)]);match case{0=>t.source=Source::Legacy{epoch:"legacy".into()},1=>t.scope.tournament_id="other".into(),_=>t.source=Source::Engine{epoch:"different".into()}}s.observe(t);assert!(s.observed.is_empty());assert!(!s.issues.is_empty());assert_eq!(s.receipt_references.len(),1);}}
 #[test]fn shared_trade_id_across_distinct_owned_orders_is_not_a_duplicate(){let mut s=state();s.observe(tape("201",vec![fill("201","301",1,Side::Yes,200000)]));s.observe(tape("202",vec![fill("202","301",2,Side::No,500000)]));assert_eq!(s.observed.len(),2);assert!(s.plan(&roster(vec![binding("201",Side::Yes,1),binding("202",Side::No,1)])).eligible_for_review);}
 #[test]fn conflicting_fact_and_cross_order_clock_tie_stage_for_reconciliation(){let mut s=state();s.observe(tape("201",vec![fill("201","301",1,Side::Yes,200000)]));s.observe(tape("202",vec![fill("202","302",1,Side::No,500000)]));assert!(s.plan(&roster(vec![binding("201",Side::Yes,1),binding("202",Side::No,1)])).reasons.contains(&"AMBIGUOUS_EXECUTION_TIE".into()));s.observe(tape("201",vec![fill("201","301",1,Side::Yes,250000)]));assert_eq!(s.conflicting_facts.len(),1);assert_eq!(s.observed["201:301"].price_units,200000);}
 #[test]fn paging_terminal_ordering_and_unresolved_attempts_fail_closed(){for case in 0..6{let mut s=state();let mut t=tape("201",vec![fill("201","301",1,Side::Yes,200000)]);let mut r=roster(vec![binding("201",Side::Yes,1)]);match case{0=>t.has_more=true,1=>r.bindings[0].terminal.as_mut().unwrap().received_at="2026-10-15T00:00:12Z".into(),2=>r.unknown_attempts=1,3=>r.submissions_frozen=false,4=>r.bindings[0].terminal.as_mut().unwrap().order_id="999".into(),_=>r.bindings[0].terminal.as_mut().unwrap().scope.tournament_id="foreign".into()}s.observe(t);assert!(!s.plan(&r).eligible_for_review);}}
 #[test]fn checkpoint_digest_anchor_frontier_and_candidate_changes_are_rejected(){assert!(State::new(scope(),source(),anchor(1)).is_err());let mut s=state();s.observe(tape("201",vec![fill("201","301",1,Side::Yes,200000)]));let mut p=s.plan(&roster(vec![binding("201",Side::Yes,1)]));p.staged_fills[0].price_units=250000;assert!(s.record_admitted(&p,anchor(1)).is_err());let bytes=s.checkpoint().unwrap();assert!(State::restore(&bytes,&anchor(1)).is_err());let mut v:serde_json::Value=serde_json::from_slice(&bytes).unwrap();v[1]["frontier"]=serde_json::json!("2026-10-15T00:00:01Z");assert!(State::restore(&serde_json::to_vec(&v).unwrap(),&anchor(0)).is_err());}
}
