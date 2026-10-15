//! Causal execution diagnostics and allocation ranking, never order authority.
//! Only actually committed native events and original durable ordinary attempts
//! belong here. A Session cursor proves a prefix exists, not membership of a
//! caller-supplied event: exact prepared bytes must also match the Session ref.
//! Cold restart deliberately loses confidence; rebuild by chronological replay,
//! never by deserializing caller-supplied fitted statistics.
use std::collections::{BTreeMap,BTreeSet,VecDeque};
use serde::{Serialize,Deserialize};
use serde_json::{Value,json};
use sha2::{Digest,Sha256};
use baal_transport_service::decimal_units;
use moloch_replay::session::Session;
use crate::dispatch::DispatchLog;
pub type Result<T>=std::result::Result<T,&'static str>;
pub const UNIT:i128=1_000_000;
const TICK:i128=5_000;
fn need(b:bool,e:&'static str)->Result<()>{if b{Ok(())}else{Err(e)}}
fn hash<T:Serialize>(v:&T)->Result<String>{Ok(format!("{:x}",Sha256::digest(serde_json::to_vec(v).map_err(|_|"EDGE_SERIALIZATION")?)))}
fn hex(s:&str)->bool{s.len()==64&&s.bytes().all(|b|b.is_ascii_digit()||(b'a'..=b'f').contains(&b))}
fn time(v:&Value)->Result<u64>{let n=chrono::DateTime::parse_from_rfc3339(v.as_str().ok_or("EDGE_CLOCK")?).map_err(|_|"EDGE_CLOCK")?.timestamp_millis();u64::try_from(n).map_err(|_|"EDGE_CLOCK")}
fn money(v:&Value)->Result<i128>{let n=decimal_units(v.as_str().map(str::to_owned).unwrap_or_else(||v.to_string()).as_str()).map_err(|_|"EDGE_MONEY")?;Ok(n)}
fn add(a:i128,b:i128)->Result<i128>{a.checked_add(b).ok_or("EDGE_OVERFLOW")}
fn mul(a:i128,b:i128)->Result<i128>{a.checked_mul(b).ok_or("EDGE_OVERFLOW")}

#[derive(Clone,Serialize,Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config{
 pub fill_window_ms:u64,pub markout_ms:u64,pub post_window_ms:u64,pub embargo_ms:u64,
 pub maximum_book_age_ms:u64,pub maximum_delivery_ms:u64,pub maximum_training_age_ms:u64,pub minimum_samples:usize,
 pub minimum_coverage_bps:u32,pub probability_haircut_bps:u32,pub uncertainty_per_share:i128,
 pub exit_fee_per_share:i128,pub fee_policy_source_sha256:String,
 pub maximum_keys:usize,pub maximum_pending:usize,pub retained_samples:usize,
}
impl Config{pub fn validate(&self)->Result<()>{
 need((1..=120_000).contains(&self.fill_window_ms)&&(1..=120_000).contains(&self.markout_ms)&&(1..=10_000).contains(&self.post_window_ms)&&self.embargo_ms<=120_000,"EDGE_WINDOWS")?;
 need(self.maximum_training_age_ms>=self.markout_ms&&self.maximum_training_age_ms<=7*86_400_000,"EDGE_TRAINING_AGE")?;
 need((1..=10_000).contains(&self.maximum_book_age_ms)&&(1..=20_000).contains(&self.maximum_delivery_ms),"EDGE_FRESHNESS")?;
 need(self.minimum_samples>=2&&self.minimum_samples<=self.retained_samples&&self.retained_samples<=256&&(1..=1024).contains(&self.maximum_keys)&&(1..=4096).contains(&self.maximum_pending),"EDGE_BOUNDS")?;
 need((1..=10_000).contains(&self.minimum_coverage_bps)&&self.probability_haircut_bps<=10_000&&self.uncertainty_per_share>=0&&self.uncertainty_per_share<=UNIT&&self.exit_fee_per_share>=0&&self.exit_fee_per_share<=UNIT&&hex(&self.fee_policy_source_sha256),"EDGE_COST_POLICY")
}}
#[derive(Clone,Debug,Serialize,PartialEq,Eq,PartialOrd,Ord)]
pub struct Key{pub exchange:String,pub market:String,pub side:String,pub action:String,pub quantity:u64,pub limit_units:i128,pub spread_ticks:i128,pub distance_ticks:i128}
#[derive(Clone)]struct Book{market:String,exchange:String,sequence:u128,source:u64,request:u64,received:u64,observed:u64,invalid:bool,bids:Vec<(i128,u64)>,asks:Vec<(i128,u64)>,sha:String}
#[derive(Clone)]struct Own{market:String,exchange:String,yes_bid:bool,price:i128,quantity:u64}
#[derive(Clone)]struct Coverage{from:u64,to:u64,sequence:u128}
#[derive(Clone)]struct Mark{value:Option<i128>,available:u64,reason:&'static str,source_sha:String}
#[derive(Clone)]struct Prob{value:Option<bool>,available:u64}
#[derive(Default)]struct Bucket{marks:VecDeque<Mark>,probability:VecDeque<Prob>}
struct PendingMark{target:u64,until:u64,executed:u64,quantity:u64,principal:i128,fee:i128,side:String,action:String}
struct Exposure{key:Key,attempt:u64,deadline:u64,quote_hash:String,filled:bool,first_fill:bool,filled_quantity:u64,principal:i128,fee:i128,first_executed:Option<u64>,fill_ids:BTreeSet<String>,mark:Option<PendingMark>,mark_done:bool,terminal:bool,probability_done:bool}

#[derive(Debug,Serialize)]
pub struct Estimate{
 pub available:bool,pub reason:String,pub valid_marks:usize,pub matured_marks:usize,
 pub complete_exposures:usize,pub censored_exposures:usize,pub unresolved_mature_exposures:usize,
 pub mean_conditional_per_share:Option<i128>,pub worst_conditional_per_share:Option<i128>,
 pub stressed_conditional_per_share:Option<i128>,pub fill_probability_bps:Option<u32>,
 pub conservative_fill_probability_bps:Option<u32>,pub last_label_available_at_ms:Option<u64>,
 pub statistical_confidence_claimed:bool,
}
#[derive(Default,Serialize)]pub struct Diagnostics{pub attempted:u64,pub overlapping_attempts:u64,pub unattributed_fills:u64,pub ignored_nonentry:u64,pub duplicate_events:u64,pub censored_late_fills:u64,pub completed_marks:u64,pub unavailable_marks:u64}
pub struct Model{config:Config,manifest:Value,run:Value,policy_sha:String,tournament:Option<String>,last:u64,disabled:Option<&'static str>,cursors:BTreeMap<String,(u64,String)>,books:BTreeMap<String,Book>,coverage:BTreeMap<String,Coverage>,own:BTreeMap<u64,Own>,buckets:BTreeMap<Key,Bucket>,cluster_until:BTreeMap<String,u64>,pending:BTreeMap<u64,Exposure>,pub diagnostics:Diagnostics}
fn levels(v:&Value,descending:bool)->Result<Vec<(i128,u64)>>{
 let rows=v.as_array().ok_or("EDGE_LEVELS")?;need(rows.len()<=64,"EDGE_DEPTH_BOUND")?;let mut out=Vec::with_capacity(rows.len());
 for l in rows{let p=money(&l["price"])?;let q=l["quantity"].as_u64().filter(|n|*n>0).ok_or("EDGE_LEVEL_QUANTITY")?;need((0..=UNIT).contains(&p)&&out.last().is_none_or(|(old,_)|if descending{*old>p}else{*old<p}),"EDGE_LEVEL_ORDER")?;out.push((p,q));}Ok(out)
}
fn book(v:&Value,at:u64)->Result<Book>{
 let source=time(&v["sourceAt"])?;let request=time(&v["requestStartedAt"])?;let received=time(&v["receivedAt"])?;
 need(source<=received&&request<=received&&received<=at,"EDGE_BOOK_CLOCK")?;
 let bids=levels(&v["bids"],true)?;let asks=levels(&v["asks"],false)?;
 need(bids.first().zip(asks.first()).is_none_or(|(b,a)|b.0<a.0),"EDGE_CROSSED_BOOK")?;
 Ok(Book{market:v["marketId"].as_str().ok_or("EDGE_MARKET")?.into(),exchange:v["exchangeId"].as_str().ok_or("EDGE_EXCHANGE")?.into(),sequence:v["sequence"].as_str().ok_or("EDGE_SEQUENCE")?.parse().map_err(|_|"EDGE_SEQUENCE")?,source,request,received,observed:at,invalid:v["invalid"]==true,bids,asks,sha:hash(v)?})
}
fn depth(b:&Book,side:&str,buy:bool,quantity:u64)->Result<Option<i128>>{
 let rows=match(side,buy){("YES",false)|("NO",true)=>&b.bids,("YES",true)|("NO",false)=>&b.asks,_=>return Err("EDGE_SIDE")};
 let mut n=quantity;let mut total=0;
 for (p,q) in rows{let take=n.min(*q);let price=if side=="NO"{UNIT-*p}else{*p};total=add(total,mul(price,take as i128)?)?;n-=take;if n==0{return Ok(Some(total))}}Ok(None)
}
// One observer for the sole serialized worker thread, shared by ordinary and
// package source paths. No caller can deserialize a model or obtain financial
// authority from this slot. Cold initialization starts at the current bank.
thread_local!{static RUNTIME_OBSERVER:std::cell::RefCell<Option<Model>>=const{std::cell::RefCell::new(None)};}
pub fn clear_observer(){RUNTIME_OBSERVER.with(|slot|*slot.borrow_mut()=None);}
pub fn initialize_observer(bank:&Session,config:Option<Config>)->Result<()>{clear_observer();let model=config.map(|c|Model::new(bank,c)).transpose()?;RUNTIME_OBSERVER.with(|slot|*slot.borrow_mut()=model);Ok(())}
/// Call only AFTER actual successful Session acceptance, with EXACT prepared
/// event bytes. This never propagates model errors into official accounting.
pub fn observe_runtime_committed(bank:&Session,source:&str,sequence:u64,event:&Value){RUNTIME_OBSERVER.with(|slot|{if let Some(model)=slot.borrow_mut().as_mut(){if model.disabled.is_none(){let _=model.observe_committed(bank,source,sequence,event);}}});}
/// The original durable ordinary attempt must already exist. Unsupported or
/// malformed model input withholds confidence; it cannot undo a durable attempt.
pub fn track_runtime_attempt(bank:&Session,log:&DispatchLog,qid:u64,now:u64){RUNTIME_OBSERVER.with(|slot|{if let Some(model)=slot.borrow_mut().as_mut(){if model.disabled.is_none(){if let Err(error)=model.track_attempt(bank,log,qid,now){model.disabled=Some(error);}}}});}
pub fn observer_diagnostic()->Option<Value>{RUNTIME_OBSERVER.with(|slot|slot.borrow().as_ref().map(Model::diagnostic_json))}
impl Model{
 pub fn new(bank:&Session,config:Config)->Result<Self>{config.validate()?;let seed=bank.live_account_seed().ok_or("EDGE_LIVE_BANK_REQUIRED")?;
 let mut own=BTreeMap::new();for q in bank.engine().order_records(){let id=q["id"].as_u64().ok_or("EDGE_QID")?;if bank.engine().live_order_bindings().contains_key(&id){let quantity=q["quantity"].as_u64().ok_or("EDGE_QUANTITY")?.checked_sub(q["filled"].as_u64().ok_or("EDGE_QUANTITY")?).ok_or("EDGE_QUANTITY")?;if quantity>0{own.insert(id,Self::own_order(&q,quantity)?);}}}need(own.len()<=4096,"EDGE_OWN_ORDER_BOUND")?;
 let cursors=["moloch-live","moloch-package"].into_iter().map(|id|(id.into(),(bank.last_source_sequence(id),String::new()))).collect();
 let last=bank.engine().report().map_err(|_|"EDGE_BANK_REPORT")?["at"].as_str().filter(|s|!s.is_empty()).map(|s|time(&json!(s))).transpose()?.unwrap_or(0);
 Ok(Self{config,manifest:bank.manifest_hash().clone(),run:seed["runId"].clone(),policy_sha:hash(bank.engine().policy())?,tournament:None,last,disabled:None,cursors,books:BTreeMap::new(),coverage:BTreeMap::new(),own,buckets:BTreeMap::new(),cluster_until:BTreeMap::new(),pending:BTreeMap::new(),diagnostics:Diagnostics::default()})}
 fn bind(&self,bank:&Session)->Result<()>{need(bank.manifest_hash()==&self.manifest&&bank.live_account_seed().is_some_and(|s|s["runId"]==self.run)&&hash(bank.engine().policy())?==self.policy_sha,"EDGE_BANK_OR_POLICY_CHANGED")}
 fn fresh(&self,b:&Book,now:u64)->bool{!b.invalid&&[b.source,b.request,b.received,b.observed].iter().all(|t|*t<=now&&now-*t<=self.config.maximum_book_age_ms)&&b.observed-b.received<=self.config.maximum_delivery_ms}
 fn continuous(&self,exchange:&str,from:u64,b:&Book)->bool{self.coverage.get(exchange).is_some_and(|c|c.from<=from&&c.to>=b.source&&c.sequence>=b.sequence)}
 // Never price a hypothetical close against known own resting liquidity.
 // Historical bound orders without an observed complete terminal are removed
 // conservatively too; unknown closed-state can reduce coverage, never alpha.
 fn own_order(q:&Value,quantity:u64)->Result<Own>{let no=q["side"]=="NO";let price=money(&q["price"])?;Ok(Own{market:q["marketId"].as_str().ok_or("EDGE_MARKET")?.into(),exchange:q["exchangeId"].as_str().ok_or("EDGE_EXCHANGE")?.into(),yes_bid:(q["action"]=="buy")!=no,price:if no{UNIT-price}else{price},quantity})}
 fn external_depth(&self,_bank:&Session,b:&Book)->Result<Book>{let mut external=b.clone();for q in self.own.values(){if q.market!=b.market||q.exchange!=b.exchange{continue}let rows=if q.yes_bid{&mut external.bids}else{&mut external.asks};for(l,n)in rows.iter_mut(){if *l==q.price{*n=n.saturating_sub(q.quantity)}}rows.retain(|(_,n)|*n>0);}Ok(external)}
 fn key(&self,bank:&Session,q:&Value,at:u64)->Result<Key>{
 let exchange=q["exchangeId"].as_str().ok_or("EDGE_EXCHANGE")?;let raw=self.books.get(exchange).ok_or("EDGE_NO_BOOK")?;let external=self.external_depth(bank,raw)?;let b=&external;
 need(self.fresh(b,at)&&self.continuous(exchange,b.source,b),"EDGE_CURRENT_COVERAGE_OR_FRESHNESS")?;
 let market=q["marketId"].as_str().ok_or("EDGE_MARKET")?;need(market==b.market,"EDGE_MARKET_SCOPE")?;
 let side=q["side"].as_str().filter(|s|matches!(*s,"YES"|"NO")).ok_or("EDGE_SIDE")?;need(q["action"]=="buy"&&q["executionStyle"]!="inventoryAON","EDGE_ENTRY_ONLY")?;
 let price=money(&q["price"])?;let quantity=q["quantity"].as_u64().filter(|n|*n>0).ok_or("EDGE_QUANTITY")?;
 let bid=b.bids.first().ok_or("EDGE_NO_DEPTH")?.0;let ask=b.asks.first().ok_or("EDGE_NO_DEPTH")?.0;
 let (own_bid,own_ask)=if side=="YES"{(bid,ask)}else{(UNIT-ask,UNIT-bid)};
 need(price<own_ask&&price>=own_bid,"EDGE_NOT_DISPLAYED_PASSIVE")?;
 need(depth(b,side,false,quantity)?.is_some(),"EDGE_NO_FULL_EXIT_DEPTH")?;
 Ok(Key{exchange:exchange.into(),market:market.into(),side:side.into(),action:"buy".into(),quantity,limit_units:price,spread_ticks:(own_ask-own_bid)/TICK,distance_ticks:(price-own_bid)/TICK})
 }
 /// Immediately after the original durable ordinary attempt, BEFORE SDK send.
 /// Captures features without labels. Planned/unattempted/package/exit orders
 /// cannot enter this ordinary passive-entry training denominator.
 pub fn track_attempt(&mut self,bank:&Session,log:&DispatchLog,qid:u64,now:u64)->Result<bool>{
 self.bind(bank)?;need(self.disabled.is_none()&&now>=self.last,"EDGE_DISABLED_OR_PAST_QUERY")?;
 need(log.attempted.contains(&qid)&&!log.acknowledgments.contains_key(&qid)&&!log.closed.contains(&qid),"EDGE_ORIGINAL_ATTEMPT_REQUIRED")?;
 if self.pending.contains_key(&qid){return Ok(false)}
 let q=bank.engine().order_records().into_iter().find(|q|q["id"]==qid).ok_or("EDGE_QUOTE")?;
 need(q["retired"]==false&&q["filled"]==0&&log.intents.get(&qid).is_some_and(|i|i["quote"]==q),"EDGE_ORIGINAL_QUOTE")?;
 need(!bank.engine().is_package_order(qid),"EDGE_PACKAGE_NOT_ORDINARY")?;
 if q["action"]!="buy"||q["executionStyle"]=="inventoryAON"{self.diagnostics.ignored_nonentry+=1;return Ok(false)}
 let key=self.key(bank,&q,now)?;need(self.pending.len()<self.config.maximum_pending,"EDGE_PENDING_BOUND")?;
 if !self.buckets.contains_key(&key){need(self.buckets.len()<self.config.maximum_keys,"EDGE_KEY_BOUND")?;self.buckets.insert(key.clone(),Bucket::default());}
 if self.cluster_until.get(&key.exchange).is_some_and(|end|now<*end){self.diagnostics.overlapping_attempts+=1;return Ok(false)}
 let deadline=now.checked_add(self.config.fill_window_ms).ok_or("EDGE_CLOCK_OVERFLOW")?;
 let cluster_end=deadline.checked_add(self.config.markout_ms).and_then(|n|n.checked_add(self.config.post_window_ms)).and_then(|n|n.checked_add(self.config.embargo_ms)).ok_or("EDGE_CLOCK_OVERFLOW")?;self.cluster_until.insert(key.exchange.clone(),cluster_end);
 self.pending.insert(qid,Exposure{key,attempt:now,deadline,quote_hash:hash(&q)?,filled:false,first_fill:false,filled_quantity:0,principal:0,fee:0,first_executed:None,fill_ids:BTreeSet::new(),mark:None,mark_done:false,terminal:false,probability_done:false});self.diagnostics.attempted+=1;self.last=now;Ok(true)
 }
 /// Feed the ACTUAL successfully committed prepared events, in original bank
 /// order, including processedAt. Do not reconstruct old events from latest
 /// prices or public prints. An error disables MODEL confidence only; it must
 /// never reject or drop an official accounting event from the financial bank.
 pub fn observe_committed(&mut self,bank:&Session,source:&str,sequence:u64,event:&Value)->Result<()>{
 let result=self.observe_inner(bank,source,sequence,event);if let Err(e)=result{self.disabled=Some(e)}result
 }
 fn observe_inner(&mut self,bank:&Session,source:&str,sequence:u64,e:&Value)->Result<()>{
 self.bind(bank)?;need(self.disabled.is_none()&&!source.is_empty()&&source.len()<=128&&sequence>0&&sequence<=bank.last_source_sequence(source),"EDGE_COMMITTED_SOURCE")?;
 let mut original=e.clone();original.as_object_mut().ok_or("EDGE_EVENT")?.remove("sourceRef");let digest=hash(&original)?;
 need(bank.committed_source_hash(source,sequence)==Some(digest.as_str()),"EDGE_SOURCE_BYTES_NOT_COMMITTED")?;
 if let Some((prior,h))=self.cursors.get(source){if sequence==*prior&&h==&digest{self.diagnostics.duplicate_events+=1;return Ok(())}need(sequence==prior.checked_add(1).ok_or("EDGE_CURSOR_OVERFLOW")?,"EDGE_SOURCE_GAP_OR_CONFLICT")?;}else{need(self.cursors.len()<8&&sequence==1&&bank.last_source_sequence(source)==1,"EDGE_NEW_SOURCE_BASELINE_REQUIRED")?;}
 let at=time(e.get("processedAt").unwrap_or(&e["receivedAt"]))?;need(at>=self.last,"EDGE_OBSERVATION_ROLLBACK")?;
 if let Some(t)=e.get("tournamentId").and_then(Value::as_str).or_else(||e["book"]["tournamentId"].as_str()){
  if let Some(old)=&self.tournament{need(old==t,"EDGE_TOURNAMENT_CHANGED")?;}else{self.tournament=Some(t.into());}
 }
 match e["type"].as_str(){
 Some("book")=>{let raw=&e["book"];let exchange=raw["exchangeId"].as_str().ok_or("EDGE_EXCHANGE")?.to_owned();let b=book(raw,at)?;
  need(self.books.contains_key(&exchange)||self.books.len()<self.config.maximum_keys,"EDGE_BOOK_BOUND")?;
  if let Some(old)=self.books.get(&exchange){need(b.market==old.market&&b.sequence>=old.sequence&&!(b.sequence==old.sequence&&b.source<old.source),"EDGE_BOOK_ROLLBACK")?;}
  self.mark_book(bank,&exchange,&b,at)?;self.books.insert(exchange,b);
 },
 Some("coverage")=>{let exchange=e["exchangeId"].as_str().ok_or("EDGE_EXCHANGE")?.to_owned();let from=time(&e["from"])?;let to=time(&e["to"])?;let seq=e["projectedThroughSequence"].as_str().ok_or("EDGE_COVERAGE_SEQUENCE")?.parse().map_err(|_|"EDGE_COVERAGE_SEQUENCE")?;
  need(e["complete"]==true&&from<=to&&to<=at&&(self.coverage.contains_key(&exchange)||self.coverage.len()<self.config.maximum_keys),"EDGE_INCOMPLETE_COVERAGE")?;
  let next=if let Some(old)=self.coverage.get(&exchange){if from<=old.to&&to>=old.from&&seq>=old.sequence{Coverage{from:from.min(old.from),to:to.max(old.to),sequence:seq}}else{Coverage{from,to,sequence:seq}}}else{Coverage{from,to,sequence:seq}};self.coverage.insert(exchange,next);
 },
 Some("gap")=>{let exchange=e["exchangeId"].as_str().ok_or("EDGE_EXCHANGE")?;self.coverage.remove(exchange);self.books.remove(exchange);let ids:Vec<_>=self.pending.iter().filter(|(_,p)|p.key.exchange==exchange&&p.mark.is_some()).map(|(id,_)|*id).collect();for id in ids{self.finish_mark(id,None,at,"gap",hash(e)?)?;}},
 Some("actual_order_ack")=>self.observe_ack(e)?,
 // The exact durable parent envelope above proves these rows were validated
 // together by the native bank. ACKs affect only conservative own liquidity;
 // actual independently committed fill events remain the only fill labels.
 Some("live_package_ack")=>{let rows=e["acknowledgments"].as_array().filter(|r|(1..=6).contains(&r.len())).ok_or("EDGE_PACKAGE_ACK_ROWS")?;for row in rows{need(row["type"]=="actual_order_ack"&&row["source"]=="official_sig_api_v1","EDGE_PACKAGE_ACK_ROW")?;self.observe_ack(row)?;}},
 Some("actual_fill")=>self.actual_fill(bank,e,at)?,
 Some("actual_order_terminal")=>{need(e["source"]=="official_sig_api_v1"&&e["fillsComplete"]==true,"EDGE_TERMINAL_COMPLETE")?;let id=e["orderId"].as_u64().ok_or("EDGE_QID")?;self.own.remove(&id);
  if let Some(p)=self.pending.get_mut(&id){let q=bank.engine().order_records().into_iter().find(|q|q["id"]==id).ok_or("EDGE_QUOTE")?;need(q["retired"]==true&&q["filled"]==e["filledQuantity"],"EDGE_TERMINAL_NATIVE")?;p.terminal=true;}
 },_=>{}
 }
 self.last=at;self.cursors.insert(source.into(),(sequence,digest));self.expire(at)?;Ok(())
 }
 fn observe_ack(&mut self,e:&Value)->Result<()>{let id=e["orderId"].as_u64().ok_or("EDGE_QID")?;let quantity=e["quantity"].as_u64().ok_or("EDGE_QUANTITY")?;need(self.own.contains_key(&id)||self.own.len()<4096,"EDGE_OWN_ORDER_BOUND")?;self.own.entry(id).or_insert(Self::own_order(e,quantity)?);Ok(())}
 fn actual_fill(&mut self,bank:&Session,e:&Value,at:u64)->Result<()>{
 need(e["source"]=="official_sig_api_v1"&&e["receiptHash"].as_str().is_some_and(hex),"EDGE_OFFICIAL_FILL")?;
 need(bank.engine().fills().iter().any(|f|f["counterfactual"]==false&&f["sourceId"]==e["fillId"]&&f["orderId"]==e["orderId"]&&f["actualOrderId"]==e["actualOrderId"]&&f["price"]==e["price"]&&f["fee"]==e["fee"]&&f["quantity"]==e["quantity"]&&f["executedAt"]==e["executedAt"]),"EDGE_FILL_NOT_IN_BANK")?;
 let id=e["orderId"].as_u64().ok_or("EDGE_QID")?;let Some(p)=self.pending.get_mut(&id)else{self.diagnostics.unattributed_fills+=1;return Ok(())};
 let executed=time(&e["executedAt"])?;need(executed>=p.attempt&&executed<=at&&e["exchangeId"]==p.key.exchange&&e["marketId"]==p.key.market&&e["side"]==p.key.side&&e["action"]==p.key.action,"EDGE_FILL_FEATURE_CLOCK_OR_SCOPE")?;
 if executed>p.deadline{return Ok(())}
 let fill_id=e["fillId"].as_str().ok_or("EDGE_FILL_ID")?;
 if p.fill_ids.contains(fill_id){return Ok(())}need(p.fill_ids.len()<128,"EDGE_FILL_ROWS_BOUND")?;p.fill_ids.insert(fill_id.into());
 let quantity=e["quantity"].as_u64().filter(|n|*n>0).ok_or("EDGE_FILL_QUANTITY")?;let fee=money(&e["fee"])?;need(fee>=0,"EDGE_FEE_UNKNOWN")?;
 p.filled_quantity=p.filled_quantity.checked_add(quantity).ok_or("EDGE_OVERFLOW")?;need(p.filled_quantity<=p.key.quantity,"EDGE_FILL_QUANTITY_CHANGED")?;
 p.principal=add(p.principal,mul(money(&e["price"])?,quantity as i128)?)?;p.fee=add(p.fee,fee)?;p.first_executed=Some(p.first_executed.map_or(executed,|old|old.min(executed)));
 if p.filled_quantity<p.key.quantity{return Ok(())}p.filled=true;if p.first_fill{return Ok(())}p.first_fill=true;
 let target=executed.checked_add(self.config.markout_ms).ok_or("EDGE_CLOCK_OVERFLOW")?;let until=target.checked_add(self.config.post_window_ms).ok_or("EDGE_CLOCK_OVERFLOW")?;
 // Normalize to original full limit funding for allocation: never assume a
 // future quote repeats historical price improvement on its actual fills.
 p.mark=Some(PendingMark{target,until,executed:p.first_executed.unwrap(),quantity:p.key.quantity,principal:mul(p.key.limit_units,p.key.quantity as i128)?.max(p.principal),fee:p.fee,side:p.key.side.clone(),action:p.key.action.clone()});
 if at>=target{self.diagnostics.censored_late_fills+=1;self.finish_mark(id,None,at,"fill_observed_after_target",hash(e)?)?;}Ok(())
 }
 fn mark_book(&mut self,bank:&Session,exchange:&str,b:&Book,at:u64)->Result<()>{
 let adjusted=self.external_depth(bank,b)?;let b=&adjusted;
 let ids:Vec<_>=self.pending.iter().filter(|(_,p)|p.key.exchange==exchange&&p.mark.as_ref().is_some_and(|m|b.source>=m.target)).map(|(id,_)|*id).collect();
 for id in ids{let p=&self.pending[&id];let m=p.mark.as_ref().unwrap();let reason=if b.source>m.until{"book_after_window"}else if !self.fresh(b,at)||at>m.until.saturating_add(self.config.maximum_delivery_ms){"stale_or_late_book"}else if !self.continuous(exchange,m.executed,b){"missing_trade_continuity"}else{"priced"};
  let value=if reason=="priced"{match depth(b,&m.side,m.action=="sell",m.quantity)?{Some(v)=>{let gross=if m.action=="buy"{v.checked_sub(m.principal)}else{m.principal.checked_sub(v)}.ok_or("EDGE_OVERFLOW")?;Some(gross.checked_sub(m.fee).ok_or("EDGE_OVERFLOW")?.div_euclid(m.quantity as i128).checked_sub(self.config.exit_fee_per_share).ok_or("EDGE_OVERFLOW")?)},None=>None}}else{None};
  self.finish_mark(id,value,at,if reason=="priced"&&value.is_none(){"insufficient_full_depth"}else{reason},b.sha.clone())?;
 }Ok(())
 }
 fn finish_mark(&mut self,id:u64,value:Option<i128>,at:u64,reason:&'static str,source_sha:String)->Result<()>{let p=self.pending.get_mut(&id).ok_or("EDGE_PENDING")?;p.mark=None;p.mark_done=true;let b=self.buckets.get_mut(&p.key).ok_or("EDGE_BUCKET")?;b.marks.push_back(Mark{value,available:at,reason,source_sha});while b.marks.len()>self.config.retained_samples{b.marks.pop_front();}if value.is_some(){self.diagnostics.completed_marks+=1}else{self.diagnostics.unavailable_marks+=1}Ok(())}
 fn expire(&mut self,at:u64)->Result<()>{
 let expired:Vec<_>=self.pending.iter().filter(|(_,p)|p.mark.as_ref().is_some_and(|m|at>m.until.saturating_add(self.config.maximum_delivery_ms))).map(|(id,_)|*id).collect();for id in expired{self.finish_mark(id,None,at,"no_book_in_window",String::new())?;}
 let ready:Vec<_>=self.pending.iter().filter(|(_,p)|p.terminal&&!p.probability_done).map(|(id,_)|*id).collect();
 for id in ready{let p=self.pending.get_mut(&id).unwrap();let value=if p.filled{Some(true)}else if at>=p.deadline{Some(false)}else{None};let b=self.buckets.get_mut(&p.key).unwrap();b.probability.push_back(Prob{value,available:at});while b.probability.len()>self.config.retained_samples{b.probability.pop_front();}p.probability_done=true;}
 self.pending.retain(|_,p|!(p.probability_done&&(p.mark_done||!p.first_fill)));Ok(())
 }
 pub fn estimate(&self,key:&Key,now:u64)->Estimate{
 let bucket=self.buckets.get(key);let since=now.saturating_sub(self.config.maximum_training_age_ms);let values:Vec<_>=bucket.map(|b|b.marks.iter().filter(|m|m.available>=since).filter_map(|m|m.value).collect()).unwrap_or_default();let marks=bucket.map_or(0,|b|b.marks.iter().filter(|m|m.available>=since).count());let probs:Vec<_>=bucket.map(|b|b.probability.iter().filter(|p|p.available>=since).filter_map(|p|p.value).collect()).unwrap_or_default();let censored=bucket.map_or(0,|b|b.probability.iter().filter(|p|p.available>=since&&p.value.is_none()).count());let open=self.pending.values().filter(|p|p.key==*key&&now>=p.deadline&&!p.probability_done).count();
 let mean=if values.is_empty(){None}else{values.iter().try_fold(0i128,|a,v|a.checked_add(*v)).map(|s|s/values.len() as i128)};let worst=values.iter().min().copied();let stressed=worst.and_then(|w|w.checked_sub(self.config.uncertainty_per_share));let prob=if probs.is_empty(){None}else{Some((probs.iter().filter(|p|**p).count()*10_000/probs.len()) as u32)};
 let reason=if self.disabled.is_some(){"model_disabled"}else if now<self.last{"future_state_not_queryable"}else if !values.is_empty()&&(mean.is_none()||stressed.is_none()){"arithmetic_unavailable"}else if values.len()<self.config.minimum_samples||probs.len()<self.config.minimum_samples{"insufficient_independent_samples"}else if values.len()*10_000<marks*self.config.minimum_coverage_bps as usize||probs.len()*10_000<(probs.len()+censored)*self.config.minimum_coverage_bps as usize||open>0{"incomplete_or_open_coverage"}else{"empirical_coverage_qualified"};
 Estimate{available:reason=="empirical_coverage_qualified",reason:reason.into(),valid_marks:values.len(),matured_marks:marks,complete_exposures:probs.len(),censored_exposures:censored,unresolved_mature_exposures:open,mean_conditional_per_share:mean,worst_conditional_per_share:worst,stressed_conditional_per_share:stressed,fill_probability_bps:prob,conservative_fill_probability_bps:prob.map(|p|p.saturating_sub(self.config.probability_haircut_bps)),last_label_available_at_ms:bucket.and_then(|b|b.marks.back().map(|m|m.available)),statistical_confidence_claimed:false}
 }
 /// Returns a planning score only. Existing physical authority, cash, book,
 /// unknown/STOP, portfolio risk and durable intent checks remain mandatory.
 pub fn maker_candidate(&self,bank:&Session,qid:u64,costs:Option<&ExecutionCosts>,now:u64)->Result<MakerEvaluation>{
 self.bind(bank)?;let q=bank.engine().order_records().into_iter().find(|q|q["id"]==qid).ok_or("EDGE_QUOTE")?;need(q["retired"]==false&&q["filled"]==0&&q["expiresAt"].as_u64().is_some_and(|end|now<end),"EDGE_QUOTE_NO_LONGER_ELIGIBLE")?;
 let key=self.key(bank,&q,now)?;let estimate=self.estimate(&key,now);let costs=costs.ok_or("EDGE_INVENTORY_COST_OR_TURNOVER_UNKNOWN")?;need(costs.inventory_and_partial_risk_units>=0&&costs.horizon.milliseconds>0&&hex(&costs.source_sha256)&&hex(&costs.horizon.source_sha256),"EDGE_COST_OR_TURNOVER_SOURCE")?;let cost=costs.inventory_and_partial_risk_units;
 let gain=if estimate.available{let conditional=estimate.stressed_conditional_per_share.ok_or("EDGE_NO_MARK")?;let p=estimate.conservative_fill_probability_bps.ok_or("EDGE_NO_EXPOSURE")?;Some((mul(mul(conditional,key.quantity as i128)?,p as i128)?/10_000).checked_sub(cost).ok_or("EDGE_OVERFLOW")?)}else{None};
 let gross=mul(key.limit_units,key.quantity as i128)?;let candidate=gain.filter(|n|*n>0).map(|n|Candidate{id:format!("ordinary:{qid}"),basis:Basis::OwnFillConditionalEstimate,net_gain_units:n,gross_funding_units:gross,capital_locked_units:gross,holding_horizon_ms:costs.horizon.milliseconds,valid_until_ms:[self.books[&key.exchange].source,self.books[&key.exchange].request,self.books[&key.exchange].received,self.books[&key.exchange].observed].into_iter().min().unwrap().saturating_add(self.config.maximum_book_age_ms).min(q["expiresAt"].as_u64().unwrap()),risk_groups:vec![key.exchange.clone()],source_sha256:self.policy_sha.clone(),horizon_source_sha256:costs.horizon.source_sha256.clone(),write_admissions:1});
 Ok(MakerEvaluation{key,estimate,estimated_net_gain_units:gain,candidate})
 }
 pub fn diagnostic_json(&self)->Value{let reasons=self.buckets.values().flat_map(|b|b.marks.iter()).fold(BTreeMap::<String,u64>::new(),|mut map,m|{*map.entry(m.reason.into()).or_default()+=1;map});let mut labels=Sha256::new();for m in self.buckets.values().flat_map(|b|b.marks.iter()){labels.update(m.source_sha.as_bytes());}let mut attempts=Sha256::new();for p in self.pending.values(){attempts.update(p.quote_hash.as_bytes());}json!({"disabled":self.disabled,"lastObservedAtMs":self.last,"committedSourceSequences":self.cursors.iter().map(|(source,(sequence,_))|(source.clone(),json!(sequence))).collect::<BTreeMap<_,_>>(),"keys":self.buckets.len(),"pending":self.pending.len(),"diagnostics":self.diagnostics,"markReasons":reasons,"labelSourceDigest":format!("{:x}",labels.finalize()),"activeAttemptQuoteDigest":format!("{:x}",attempts.finalize()),"confidenceKind":"empirical_worst_sample_stress_not_statistical","executionAuthority":false})}

}
#[derive(Clone,Debug,Serialize)]pub struct CapitalHorizon{pub milliseconds:u64,pub source_sha256:String}
#[derive(Clone,Debug,Serialize)]pub struct ExecutionCosts{pub inventory_and_partial_risk_units:i128,pub source_sha256:String,pub horizon:CapitalHorizon}
#[derive(Debug,Serialize)]pub struct MakerEvaluation{pub key:Key,pub estimate:Estimate,pub estimated_net_gain_units:Option<i128>,pub candidate:Option<Candidate>}
#[derive(Clone,Debug,Serialize,PartialEq,Eq)]pub enum Basis{OwnFillConditionalEstimate,StatewiseGuaranteedIncrementalGain,ExecutableCashGain,DownsideFloorUtility,LockedVsLiquidationBenchmark}
/// Numeric/provenance input from an independently checked native/graph plan.
/// A hash alone is not a certificate. Floor improvement and liquidation-relative
/// risk utility are deliberately excluded from profit/alpha allocation below.
#[derive(Clone,Debug,Serialize)]pub struct Candidate{pub id:String,pub basis:Basis,pub net_gain_units:i128,pub gross_funding_units:i128,pub capital_locked_units:i128,pub holding_horizon_ms:u64,pub valid_until_ms:u64,pub risk_groups:Vec<String>,pub source_sha256:String,pub horizon_source_sha256:String,pub write_admissions:u32}
#[derive(Serialize)]pub struct Allocation{pub selected:Vec<Candidate>,pub deferred:BTreeMap<String,String>,pub cash_committed:i128,pub capital_locked:i128,pub execution_authority:bool}
/// Exact cross-multiplied gain / (gross capital * holding time), with cash,
/// capital lock, write budget and overlapping risk-group exclusions. Scores are
/// comparative estimates, not realized returns or permission to send an order.
pub fn allocate(mut candidates:Vec<Candidate>,now:u64,cash:i128,capital_limit:i128,writes:u32)->Result<Allocation>{
 need(candidates.len()<=256&&cash>=0&&capital_limit>=0,"EDGE_ALLOCATION_BOUNDS")?;let mut deferred=BTreeMap::new();let mut valid=Vec::new();let mut ids=std::collections::BTreeSet::new();
 for c in candidates.drain(..){need(!c.id.is_empty()&&c.id.len()<=256&&ids.insert(c.id.clone()),"EDGE_CANDIDATE_ID")?;
  let reason=if matches!(c.basis,Basis::DownsideFloorUtility|Basis::LockedVsLiquidationBenchmark|Basis::ExecutableCashGain){Some("risk_utility_not_incremental_alpha")}else if c.net_gain_units<=0{Some("nonpositive_edge")}else if c.valid_until_ms<=now{Some("expired")}else if c.gross_funding_units<=0||c.capital_locked_units<0||c.holding_horizon_ms==0||c.holding_horizon_ms>366*86_400_000||c.write_admissions==0||c.risk_groups.is_empty()||c.risk_groups.len()>8||c.risk_groups.iter().any(|s|s.is_empty()||s.len()>128)||!hex(&c.source_sha256)||!hex(&c.horizon_source_sha256){Some("missing_economic_or_source_fields")}else{None};
  if let Some(reason)=reason{deferred.insert(c.id,reason.into());}else{mul(c.gross_funding_units,c.holding_horizon_ms as i128)?;valid.push(c)}
 }
 // Checked insertion sort avoids a fallible/overflow-prone sort comparator.
 let mut ordered:Vec<Candidate>=Vec::new();for c in valid{let denom=mul(c.gross_funding_units,c.holding_horizon_ms as i128)?;let mut position=ordered.len();for(i,old)in ordered.iter().enumerate(){let lhs=mul(c.net_gain_units,mul(old.gross_funding_units,old.holding_horizon_ms as i128)?)?;let rhs=mul(old.net_gain_units,denom)?;if lhs>rhs||(lhs==rhs&&c.id<old.id){position=i;break}}ordered.insert(position,c);}
 let mut result=Allocation{selected:vec![],deferred,cash_committed:0,capital_locked:0,execution_authority:false};let mut groups=std::collections::BTreeSet::new();let mut used=0u32;
 for c in ordered{let reason=if c.risk_groups.iter().any(|g|groups.contains(g)){Some("overlapping_risk_group")}else if add(result.cash_committed,c.gross_funding_units)?>cash{Some("gross_cash_budget")}else if add(result.capital_locked,c.capital_locked_units)?>capital_limit{Some("capital_lock_budget")}else if used.checked_add(c.write_admissions).is_none_or(|n|n>writes){Some("shared_write_budget")}else{None};
  if let Some(r)=reason{result.deferred.insert(c.id,r.into());}else{result.cash_committed=add(result.cash_committed,c.gross_funding_units)?;result.capital_locked=add(result.capital_locked,c.capital_locked_units)?;used+=c.write_admissions;groups.extend(c.risk_groups.iter().cloned());result.selected.push(c);}
 }Ok(result)
}
