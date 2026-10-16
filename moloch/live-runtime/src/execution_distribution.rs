//! Prospective joint execution/value diagnostics. NEVER execution authority.
//! Labels come from exact committed Session inputs plus original owned-order
//! receipts, not fitted JSON. Cold restart starts a new observation cohort.
//! Money remains integer millionths; floating point is used only for explicitly
//! empirical descriptive statistics, never ledger/accounting/funding.
use crate::{dispatch::DispatchLog,feedback};
use baal_transport_service::{Receipt,decimal_units};
use moloch_replay::session::Session;
use serde::{Serialize,Deserialize};
use serde_json::{Value,json};
use sha2::{Digest,Sha256};
use std::collections::{BTreeMap,BTreeSet,VecDeque};
pub type Result<T>=std::result::Result<T,&'static str>;
const UNIT:i128=1_000_000;
fn need(ok:bool,e:&'static str)->Result<()>{if ok{Ok(())}else{Err(e)}}
fn digest<T:Serialize+?Sized>(v:&T)->Result<String>{Ok(format!("{:x}",Sha256::digest(serde_json::to_vec(v).map_err(|_|"DIST_JSON")?)))}
fn clock(v:&Value)->Result<u64>{chrono::DateTime::parse_from_rfc3339(v.as_str().ok_or("DIST_CLOCK")?).ok().and_then(|d|u64::try_from(d.timestamp_millis()).ok()).ok_or("DIST_CLOCK")}
fn amount(v:&Value)->Result<i128>{decimal_units(&v.as_str().map(str::to_owned).unwrap_or_else(||v.to_string())).map_err(|_|"DIST_MONEY")}
fn plus(a:i128,b:i128)->Result<i128>{a.checked_add(b).ok_or("DIST_RANGE")}
fn times(a:i128,b:u64)->Result<i128>{a.checked_mul(i128::from(b)).ok_or("DIST_RANGE")}
fn id(v:&Value)->Result<String>{v.as_str().map(str::to_owned).or_else(||v.as_u64().map(|n|n.to_string())).filter(|s|!s.is_empty()).ok_or("DIST_ID")}

#[derive(Clone,Serialize,Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config{
 pub markout_ms:u64,pub post_window_ms:u64,pub maximum_book_age_ms:u64,pub maximum_delivery_ms:u64,
 pub block_ms:u64,pub minimum_blocks:usize,pub minimum_completed:usize,pub retained_samples:usize,pub maximum_pending:usize,
 pub shrinkage_strength:u32,pub minimum_value_coverage_bps:u32,
 /// Explicit policy charges, separate from conditional adverse selection.
 pub uncertainty_floor_per_share:i128,pub tail_charge_per_filled_share:i128,
 pub write_charge_per_order:i128,pub capital_charge_per_million_unit_ms:i128,
 pub fee_policy_source_sha256:String,pub exit_fee_per_share:i128,
}
impl Config{pub fn validate(&self)->Result<()>{
 need((1..=120_000).contains(&self.markout_ms)&&(1..=20_000).contains(&self.post_window_ms)&&(1..=10_000).contains(&self.maximum_book_age_ms)&&(1..=20_000).contains(&self.maximum_delivery_ms),"DIST_WINDOWS")?;
 need(self.block_ms>=self.markout_ms&&self.block_ms<=86_400_000&&(2..=64).contains(&self.minimum_blocks)&&self.minimum_completed>=2&&self.minimum_completed<=self.retained_samples&&self.retained_samples<=1024&&(1..=4096).contains(&self.maximum_pending)&&self.shrinkage_strength>0&&self.shrinkage_strength<=1000&&(1..=10000).contains(&self.minimum_value_coverage_bps),"DIST_BOUNDS")?;
 need([self.uncertainty_floor_per_share,self.tail_charge_per_filled_share,self.write_charge_per_order,self.capital_charge_per_million_unit_ms,self.exit_fee_per_share].iter().all(|n|*n>=0&&*n<=UNIT)&&self.fee_policy_source_sha256.len()==64&&self.fee_policy_source_sha256.bytes().all(|b|b.is_ascii_hexdigit()),"DIST_CHARGES")
}}
/// Statistical pooling deliberately omits exact price, size, exchange and race.
/// Exact identities remain in every exposure's provenance. No queue-position
/// or signed-aggressor feature is invented from aggregate L2.
#[derive(Clone,Debug,Serialize,PartialEq,Eq,PartialOrd,Ord)]
pub struct PoolKey{pub policy_sha256:String,pub strategy_sha256:String,pub side:String,pub price_band:u8,pub spread_band:u8,pub distance_band:i8,pub quantity_depth_band:u8,pub quote_age_band:u8,pub lifetime_band:u8,pub utc_six_hour_block:u8}
#[derive(Clone)]struct Book{raw_sha:String,market:String,exchange:String,sequence:u128,source:u64,request:u64,received:u64,observed:u64,realtime:bool,invalid:bool,bids:Vec<(i128,u64)>,asks:Vec<(i128,u64)>}
#[derive(Clone)]struct Own{exchange:String,yes_bid:bool,price:i128,remaining:u64}
#[derive(Clone)]struct Coverage{from:u64,to:u64,sequence:u128,sha:String}
#[derive(Clone)]struct Fill{source_id:String,quantity:u64,price:i128,fee:i128,executed:u64,observed:u64,receipt_sha:String,mark:Option<Mark>}
#[derive(Clone)]struct Mark{gain:Option<i128>,reason:&'static str,available:u64,source_sha:String}
struct Exposure{requested:u64,key:Option<PoolKey>,feature_reason:Option<&'static str>,market:String,exchange:String,side:String,attempt:u64,created:u64,limit:i128,proof_sha:String,admitted:Option<u64>,ack_observed:Option<u64>,fills:Vec<Fill>,terminal:Option<Value>,confirmed:bool,sample:Option<Sample>,owned_capital_time:i128,owned_remaining:u64,capital_clock_unknown:bool}
#[derive(Clone)]struct Lot{quantity:u64,price:i128,entered:u64,owner:Option<u64>}
#[derive(Clone)]struct Sample{key:Option<PoolKey>,market:String,attempt:u64,completed:u64,requested:u64,filled:u64,gain:Option<i128>,reservation_capital_time:i128,status:String,proof_sha:String}
#[derive(Clone,Debug,Serialize)]pub struct OutcomeView{pub requested_quantity:u64,pub admitted_quantity:Option<u64>,pub observed_filled_quantity:u64,pub class:String,pub execution_confirmed:bool,pub conditional_gain_units:Option<i128>,pub value_censored_quantity:u64,pub first_fill_latency_ms:Option<u64>,pub maximum_receipt_delay_ms:Option<u64>,pub reservation_capital_time_unit_ms:Option<i128>,pub owned_capital_time_observed_unit_ms:i128,pub owned_capital_remaining_quantity:u64,pub capital_time_complete:bool,pub source_sha256:String}
#[derive(Debug,Serialize)]pub struct Estimate{
 pub pool_key:PoolKey,pub parent_completed:usize,pub market_completed:usize,pub distinct_markets:usize,pub time_blocks:usize,pub parent_counts:[usize;3],pub market_counts:[usize;3],
 pub posterior_zero_partial_full:[f64;3],pub expected_filled_fraction:Option<f64>,pub observed_joint_mean_per_requested_share:Option<f64>,pub shrunk_joint_mean_per_requested_share:Option<f64>,
 pub conditional_per_filled_share:Option<f64>,pub minimum_observed_per_requested_share:Option<f64>,pub empirical_block_standard_error:Option<f64>,pub uncertainty_charge_per_requested_share:Option<f64>,pub tail_charge_per_requested_share:Option<f64>,pub mean_write_charge_per_requested_share:Option<f64>,pub capital_time_charge_per_requested_share:Option<f64>,pub net_shadow_score_per_requested_share:Option<f64>,
 pub value_covered:usize,pub value_censored:usize,pub unresolved:usize,pub first_fill_latency_counts:[usize;4],pub terminal_status_counts:BTreeMap<String,usize>,pub capital_time_incomplete:usize,pub leave_market_out_joint_mean_range:Option<[f64;2]>,pub qualified_descriptive_sample:bool,
 pub confidence_kind:&'static str,pub cancellation_is_informative:bool,pub independent_censoring_assumed:bool,pub execution_authority:bool,
}
pub struct Shadow{config:Config,manifest:Value,run:Value,policy_sha:String,last:u64,disabled:Option<&'static str>,cursors:BTreeMap<String,(u64,String)>,books:BTreeMap<String,Book>,coverage:BTreeMap<String,Coverage>,own:BTreeMap<u64,Own>,inventory:BTreeMap<(String,String),VecDeque<Lot>>,seen_fills:BTreeSet<String>,exposures:BTreeMap<u64,Exposure>,retained:VecDeque<u64>,duplicates:u64,unattributed_fills:u64,excluded_carry:u64}
fn levels(raw:&Value,descending:bool)->Result<Vec<(i128,u64)>>{let mut out=Vec::new();let rows=raw.as_array().filter(|r|r.len()<=199).ok_or("DIST_DEPTH")?;for l in rows{let p=amount(&l["price"])?;let n=l["quantity"].as_u64().filter(|n|*n>0).ok_or("DIST_DEPTH_QUANTITY")?;need((5000..=995000).contains(&p)&&p%5000==0&&out.last().is_none_or(|(old,_)|if descending{*old>p}else{*old<p}),"DIST_DEPTH_PRICE")?;out.push((p,n));}Ok(out)}
fn book(raw:&Value,at:u64)->Result<Book>{let source=clock(&raw["sourceAt"])?;let request=clock(&raw["requestStartedAt"])?;let received=clock(&raw["receivedAt"])?;need(source<=received&&request<=received&&received<=at,"DIST_BOOK_CLOCK")?;let bids=levels(&raw["bids"],true)?;let asks=levels(&raw["asks"],false)?;need(bids.first().zip(asks.first()).is_none_or(|(b,a)|b.0<a.0),"DIST_CROSSED_BOOK")?;Ok(Book{raw_sha:digest(raw)?,market:id(&raw["marketId"])?,exchange:id(&raw["exchangeId"])?,sequence:raw["sequence"].as_str().ok_or("DIST_BOOK_SEQUENCE")?.parse().map_err(|_|"DIST_BOOK_SEQUENCE")?,source,request,received,observed:at,realtime:raw["transport"]=="official_sig_realtime",invalid:raw["invalid"]==true,bids,asks})}
fn own(q:&Value,remaining:u64)->Result<Own>{let no=q["side"]=="NO";let p=amount(&q["price"])?;Ok(Own{exchange:id(&q["exchangeId"])?,yes_bid:(q["action"]=="buy")!=no,price:if no{UNIT-p}else{p},remaining})}
fn native_fill_matches(a:&Value,b:&Value)->bool{a["counterfactual"]==false&&a["sourceId"]==b["fillId"]&&["orderId","actualOrderId","marketId","exchangeId","side","action","quantity","price","fee","executedAt"].iter().all(|k|a[*k]==b[*k])}
fn intent_tournament(log:&DispatchLog,qid:u64)->Result<&str>{let spec=&log.intents.get(&qid).ok_or("DIST_ORIGINAL_INTENT")?["spec"];let body=&spec["body"];need(spec["method"]=="POST"&&spec["endpoint"]=="/orders"&&spec["bodyJson"].as_str().is_some_and(|s|serde_json::from_str::<Value>(s).ok().as_ref()==Some(body)),"DIST_ORIGINAL_REQUEST_BYTES")?;body["tournamentId"].as_str().filter(|t|!t.is_empty()).ok_or("DIST_TOURNAMENT")}
impl Shadow{
 pub fn new(bank:&Session,config:Config)->Result<Self>{config.validate()?;let seed=bank.live_account_seed().ok_or("DIST_LIVE_BANK")?;let mut own_orders=BTreeMap::new();for q in bank.engine().order_records(){let qid=q["id"].as_u64().ok_or("DIST_QID")?;if bank.engine().live_order_bindings().contains_key(&qid){let n=q["quantity"].as_u64().ok_or("DIST_QUANTITY")?.checked_sub(q["filled"].as_u64().ok_or("DIST_QUANTITY")?).ok_or("DIST_QUANTITY")?;if n>0{own_orders.insert(qid,own(&q,n)?);}}}need(own_orders.len()<=4096,"DIST_OWN_BOUND")?;
  let mut inventory=BTreeMap::new();for(ex,p)in bank.engine().wallet.positions().as_object().ok_or("DIST_POSITIONS")?{for side in ["YES","NO"]{let mut lots=VecDeque::new();for l in p[side].as_array().ok_or("DIST_LOTS")?{lots.push_back(Lot{quantity:l["quantity"].as_u64().ok_or("DIST_QUANTITY")?,price:amount(&l["price"])?,entered:0,owner:None});}inventory.insert((ex.clone(),side.into()),lots);}}
  let last=bank.engine().report().map_err(|_|"DIST_REPORT")?["at"].as_str().filter(|s|!s.is_empty()).map(|s|clock(&json!(s))).transpose()?.unwrap_or(0);
  Ok(Self{config,manifest:bank.manifest_hash().clone(),run:seed["runId"].clone(),policy_sha:digest(bank.engine().policy())?,last,disabled:None,cursors:["moloch-live","moloch-package"].into_iter().map(|s|(s.into(),(bank.last_source_sequence(s),String::new()))).collect(),books:BTreeMap::new(),coverage:BTreeMap::new(),own:own_orders,inventory,seen_fills:bank.engine().fills().iter().map(|f|id(&f["sourceId"])).collect::<Result<_>>()?,exposures:BTreeMap::new(),retained:VecDeque::new(),duplicates:0,unattributed_fills:0,excluded_carry:0})
 }
 fn bind(&self,bank:&Session)->Result<()>{need(self.disabled.is_none()&&bank.manifest_hash()==&self.manifest&&bank.live_account_seed().is_some_and(|s|s["runId"]==self.run)&&digest(bank.engine().policy())?==self.policy_sha,"DIST_BANK_POLICY_OR_DISABLED")}
 fn fresh(&self,b:&Book,at:u64)->bool{!b.invalid&&[b.source,b.received,b.observed].iter().all(|t|*t<=at&&at-*t<=self.config.maximum_book_age_ms)&&(b.realtime||b.request<=at&&at-b.request<=self.config.maximum_book_age_ms)&&b.observed-b.received<=self.config.maximum_delivery_ms}
 fn external(&self,b:&Book)->Book{let mut out=b.clone();for q in self.own.values().filter(|q|q.exchange==b.exchange){let levels=if q.yes_bid{&mut out.bids}else{&mut out.asks};for(p,n)in levels.iter_mut(){if *p==q.price{*n=n.saturating_sub(q.remaining)}}levels.retain(|(_,n)|*n>0);}out}
 fn features(&self,q:&Value,n:u64,strategy:String,at:u64)->Result<PoolKey>{let ex=id(&q["exchangeId"])?;let b=self.books.get(&ex).ok_or("DIST_FEATURE_NO_BOOK")?;need(self.fresh(b,at)&&b.market==q["marketId"],"DIST_FEATURE_STALE_BOOK")?;let b=self.external(b);let(bid,ask,depth)=if q["side"]=="YES"{(b.bids.first(),b.asks.first(),&b.bids)}else{(b.asks.first(),b.bids.first(),&b.asks)};let(bp,bn)=*bid.ok_or("DIST_FEATURE_DEPTH")?;let(ap,_)=*ask.ok_or("DIST_FEATURE_DEPTH")?;let side=id(&q["side"])?;let(bp,ap)=if side=="NO"{(UNIT-bp,UNIT-ap)}else{(bp,ap)};let p=amount(&q["price"])?;need(p<ap&&p>=bp,"DIST_FEATURE_NOT_DISPLAYED_PASSIVE")?;let ratio=u128::from(n)*10000/u128::from(bn);let spread=(ap-bp)/5000;let distance=(p-bp)/5000;let age=at.checked_sub(q["createdAt"].as_u64().ok_or("DIST_CLOCK")?).ok_or("DIST_CLOCK")?;let lifetime=q["expiresAt"].as_u64().ok_or("DIST_CLOCK")?.checked_sub(at).ok_or("DIST_CLOCK")?;let _=depth;
  Ok(PoolKey{policy_sha256:self.policy_sha.clone(),strategy_sha256:strategy,side,price_band:u8::try_from((p/200000).min(4)).map_err(|_|"DIST_RANGE")?,spread_band:if spread<=1{0}else if spread<=2{1}else if spread<=5{2}else if spread<=10{3}else{4},distance_band:if distance==0{0}else if distance==1{1}else{2},quantity_depth_band:if ratio<=100{0}else if ratio<=1000{1}else if ratio<=10000{2}else{3},quote_age_band:if age<=3000{0}else if age<=10000{1}else if age<=60000{2}else{3},lifetime_band:if lifetime<=10000{0}else if lifetime<=60000{1}else{2},utc_six_hour_block:((at/21_600_000)%4)as u8})
 }
 fn verify_ack(&self,bank:&Session,log:&DispatchLog,qid:u64,at:u64)->Result<Value>{let tournament=intent_tournament(log,qid)?;let q=&log.intents.get(&qid).ok_or("DIST_ORIGINAL_INTENT")?["quote"];let r:Receipt=serde_json::from_value(log.acknowledgments.get(&qid).ok_or("DIST_ORIGINAL_ACK")?.clone()).map_err(|_|"DIST_ACK_RECEIPT")?;let stamp=crate::daily_windows::iso(at).map_err(|_|"DIST_CLOCK")?;let e=if let Some(recovery)=log.recovered.get(&qid){let actual=recovery["actualOrderId"].as_str().ok_or("DIST_RECOVERY_ID")?;let tape:Receipt=serde_json::from_value(recovery["fills"].clone()).map_err(|_|"DIST_RECOVERY_TAPE")?;feedback::recovered_acknowledgment(q,actual,tournament,&r,&tape,&stamp).map_err(|_|"DIST_RECOVERED_ACK")?}else{need(r.endpoint=="/orders","DIST_ACK_ENDPOINT")?;feedback::acknowledgment(q,&r,&stamp).map_err(|_|"DIST_ACK_BINDING")?};need(bank.engine().live_order_bindings().get(&qid).is_some_and(|id|e["actualOrderId"]==*id),"DIST_NATIVE_ACK_BINDING")?;Ok(e)}
 fn refresh_closed(&mut self,bank:&Session,log:&DispatchLog,at:u64)->Result<()>{
  let ids:Vec<_>=self.own.keys().filter(|id|log.closed.contains(id)).copied().collect();if ids.is_empty(){return Ok(())}
  // Build the native view ONCE. Cloning the complete order history per closed
  // qid made this prospective diagnostic quadratic on the physical send path.
  let orders=bank.engine().order_records();let indexed:BTreeMap<_,_>=orders.iter().filter_map(|q|q["id"].as_u64().map(|id|(id,q))).collect();
  for qid in ids{let e=self.verify_ack(bank,log,qid,at)?;let q=indexed.get(&qid).ok_or("DIST_QUOTE")?;need(q["retired"]==true&&q["quantity"]==e["quantity"],"DIST_CLOSED_NATIVE")?;self.own.remove(&qid);}Ok(())
 }
 /// Call immediately after durable original attempt, BEFORE HTTP. No carrier,
 /// package, hypothetical proposal or reconstructed retrospective trial.
 pub fn track_attempt(&mut self,bank:&Session,log:&DispatchLog,qid:u64,at:u64)->Result<bool>{self.bind(bank)?;need(at>=self.last&&log.attempted.contains(&qid)&&!log.acknowledgments.contains_key(&qid)&&!log.closed.contains(&qid),"DIST_ORIGINAL_ATTEMPT")?;if self.exposures.contains_key(&qid){return Ok(false)}let i=log.intents.get(&qid).ok_or("DIST_ORIGINAL_INTENT")?;if !i["fundamental"].is_null(){self.excluded_carry+=1;return Ok(false)}let q=bank.engine().order_records().into_iter().find(|q|q["id"]==qid).ok_or("DIST_QUOTE")?;need(q["retired"]==false&&q["filled"]==0&&i["quote"]==q&&!bank.engine().is_package_order(qid),"DIST_ORIGINAL_QUOTE")?;if q["action"]!="buy"||q["executionStyle"]=="inventoryAON"{return Ok(false)}
  let tournament=intent_tournament(log,qid)?;let spec:&Value=&i["spec"];let body=&spec["body"];let n=body["quantity"].as_u64().filter(|n|*n>0).ok_or("DIST_ORIGINAL_SIZE")?;let limit=amount(&q["price"])?;need(spec["method"]=="POST"&&spec["endpoint"]=="/orders"&&body["exchangeId"]==q["exchangeId"]&&bank.engine().live_package_books().iter().any(|b|b["exchangeId"]==q["exchangeId"]&&b["marketId"]==q["marketId"]&&b["tournamentId"]==tournament)&&clock(&body["expirationDate"])?==q["expiresAt"].as_u64().ok_or("DIST_CLOCK")?&&spec["bodyJson"].as_str().is_some_and(|s|serde_json::from_str::<Value>(s).ok().as_ref()==Some(body))&&body["action"]=="buy"&&body["side"]==if q["side"]=="NO"{"no"}else{"yes"}&&amount(&body["price"])?==limit&&q["quantity"]==n&&body["idempotencyKey"]==format!("{}:{qid}",self.run.as_str().ok_or("DIST_RUN")?),"DIST_ORIGINAL_REQUEST")?;
  self.refresh_closed(bank,log,at)?;need(self.exposures.len()<self.config.maximum_pending+self.config.retained_samples,"DIST_EXPOSURE_BOUND")?;let strategy=digest(&json!({"kind":"ordinary_passive","model":digest(&self.config)?,"maker":i["maker"].get("reviewedSettingsSha256"),"policy":self.policy_sha}))?;let features=self.features(&q,n,strategy,at);let(key,feature_reason)=match features{Ok(k)=>(Some(k),None),Err(e)if matches!(e,"DIST_FEATURE_NO_BOOK"|"DIST_FEATURE_STALE_BOOK"|"DIST_FEATURE_DEPTH"|"DIST_FEATURE_NOT_DISPLAYED_PASSIVE")=>(None,Some(e)),Err(e)=>return Err(e)};
  let market=id(&q["marketId"])?;let exchange=id(&q["exchangeId"])?;let side=id(&q["side"])?;let created=q["createdAt"].as_u64().filter(|t|*t<=at).ok_or("DIST_CLOCK")?;let proof_sha=digest(&json!({"head":bank.durable_head(),"dispatch":log.descriptor_identity().map_err(|_|"DIST_DISPATCH_DESCRIPTOR")?,"intent":i,"attemptClock":at,"features":key}))?;
  self.exposures.insert(qid,Exposure{requested:n,key,feature_reason,market,exchange,side,attempt:at,created,limit,proof_sha,admitted:None,ack_observed:None,fills:Vec::new(),terminal:None,confirmed:false,sample:None,owned_capital_time:0,owned_remaining:0,capital_clock_unknown:false});self.last=at;Ok(true)
 }
 pub fn observe_committed(&mut self,bank:&Session,source:&str,sequence:u64,e:&Value)->Result<()>{let r=self.observe_inner(bank,source,sequence,e);if let Err(error)=r{self.disabled=Some(error)}r}
 fn observe_inner(&mut self,bank:&Session,source:&str,sequence:u64,e:&Value)->Result<()>{self.bind(bank)?;let mut original=e.clone();original.as_object_mut().ok_or("DIST_EVENT")?.remove("sourceRef");let sha=digest(&original)?;need(sequence>0&&bank.committed_source_hash(source,sequence)==Some(sha.as_str())&&e.get("sourceRef").is_none_or(|r|*r==json!({"id":source,"sequence":sequence,"hash":sha})),"DIST_SOURCE_MEMBERSHIP")?;
  if let Some((prior,old))=self.cursors.get(source){if sequence==*prior&&old==&sha{self.duplicates+=1;return Ok(())}need(sequence==prior+1,"DIST_SOURCE_ORDER")?;}else{need(sequence==1&&bank.last_source_sequence(source)==1&&self.cursors.len()<16,"DIST_NEW_SOURCE_BASELINE")?;}
  let at=clock(e.get("processedAt").unwrap_or(&e["receivedAt"]))?;need(at>=self.last,"DIST_OBSERVATION_CLOCK")?;
  match e["type"].as_str(){Some("book")=>{let b=book(&e["book"],at)?;if let Some(old)=self.books.get(&b.exchange){need(b.market==old.market&&b.sequence>=old.sequence&&(b.sequence!=old.sequence||b.source>=old.source),"DIST_BOOK_ROLLBACK")?;}need(self.books.contains_key(&b.exchange)||self.books.len()<1024,"DIST_BOOK_BOUND")?;self.price_marks(&b,at)?;self.books.insert(b.exchange.clone(),b);},Some("coverage")=>{let ex=id(&e["exchangeId"])?;let from=clock(&e["from"])?;let to=clock(&e["to"])?;let seq=e["projectedThroughSequence"].as_str().ok_or("DIST_COVERAGE_SEQUENCE")?.parse().map_err(|_|"DIST_COVERAGE_SEQUENCE")?;need(e["complete"]==true&&from<=to&&to<=at,"DIST_COVERAGE")?;let next=if let Some(old)=self.coverage.get(&ex){if from<=old.to&&to>=old.from&&seq>=old.sequence{Coverage{from:from.min(old.from),to:to.max(old.to),sequence:seq,sha:digest(&json!([old.sha,sha]))?}}else{Coverage{from,to,sequence:seq,sha:sha.clone()}}}else{Coverage{from,to,sequence:seq,sha:sha.clone()}};self.coverage.insert(ex,next);},Some("gap")=>{let ex=id(&e["exchangeId"])?;self.books.remove(&ex);self.coverage.remove(&ex);for p in self.exposures.values_mut().filter(|p|p.exchange==ex){for f in &mut p.fills{if f.mark.is_none(){f.mark=Some(Mark{gain:None,reason:"source_gap",available:at,source_sha:sha.clone()});}}}},Some("actual_order_ack")=>{let qid=e["orderId"].as_u64().ok_or("DIST_QID")?;let n=e["quantity"].as_u64().ok_or("DIST_QUANTITY")?;self.own.insert(qid,own(e,n)?);if let Some(p)=self.exposures.get_mut(&qid){need(n<=p.requested&&n>0,"DIST_ACK_SIZE")?;p.admitted=Some(n);p.ack_observed=Some(at);}},Some("live_package_ack")=>{for a in e["acknowledgments"].as_array().ok_or("DIST_PACKAGE_ACKS")?{let qid=a["orderId"].as_u64().ok_or("DIST_QID")?;self.own.insert(qid,own(a,a["quantity"].as_u64().ok_or("DIST_QUANTITY")?)?);}},Some("actual_fill")=>self.actual_fill(bank,e,at)?,Some("actual_order_terminal")=>{let qid=e["orderId"].as_u64().ok_or("DIST_QID")?;need(e["source"]=="official_sig_api_v1"&&e["fillsComplete"]==true,"DIST_TERMINAL")?;self.own.remove(&qid);if let Some(p)=self.exposures.get_mut(&qid){p.terminal=Some(original.clone());}},_=>{}}
  self.last=at;self.expire_marks(at)?;self.refresh_values()?;self.cursors.insert(source.into(),(sequence,sha));Ok(())
 }
 fn actual_fill(&mut self,bank:&Session,e:&Value,at:u64)->Result<()>{need(e["source"]=="official_sig_api_v1"&&bank.engine().fills().iter().any(|f|native_fill_matches(f,e)),"DIST_ACTUAL_FILL_MEMBERSHIP")?;let qid=e["orderId"].as_u64().ok_or("DIST_QID")?;let fill_id=id(&e["fillId"])?;let n=e["quantity"].as_u64().filter(|n|*n>0).ok_or("DIST_QUANTITY")?;let price=amount(&e["price"])?;let fee=amount(&e["fee"])?;let executed=clock(&e["executedAt"])?;need(executed<=at&&fee>=0,"DIST_FILL_CLOCK_OR_FEE")?;
  // Duplicate native receipts do not consume FIFO twice or create trials.
  if self.seen_fills.contains(&fill_id){self.duplicates+=1;return Ok(())}need(self.seen_fills.len()<65_536,"DIST_FILL_ID_BOUND")?;self.seen_fills.insert(fill_id.clone());
  if let Some(o)=self.own.get_mut(&qid){o.remaining=o.remaining.checked_sub(n).ok_or("DIST_OWN_FILL_QUANTITY")?;}
  if let Some(p)=self.exposures.get_mut(&qid){need(e["exchangeId"]==p.exchange&&e["side"]==p.side&&e["action"]=="buy"&&executed>=p.attempt&&p.fills.len()<256&&p.fills.iter().map(|f|f.quantity).sum::<u64>().checked_add(n).is_some_and(|total|total<=p.requested),"DIST_FILL_SCOPE")?;let target=executed.checked_add(self.config.markout_ms).ok_or("DIST_RANGE")?;let mark=(at>=target).then(||Mark{gain:None,reason:"fill_observed_after_target",available:at,source_sha:e["receiptHash"].as_str().unwrap_or("").into()});p.fills.push(Fill{source_id:fill_id,quantity:n,price,fee,executed,observed:at,receipt_sha:id(&e["receiptHash"])?,mark});}else{self.unattributed_fills+=1;}
  let ex=id(&e["exchangeId"])?;let side=id(&e["side"])?;let buying=e["action"]=="buy";let consumed=if buying{if side=="YES"{"NO"}else{"YES"}}else{side.as_str()};let key=(ex.clone(),consumed.into());let mut left=n;let mut closed=Vec::new();if let Some(lots)=self.inventory.get_mut(&key){while left>0&&!lots.is_empty(){let l=lots.front_mut().unwrap();let take=left.min(l.quantity);closed.push((l.owner,take,l.price,l.entered));l.quantity-=take;left-=take;if l.quantity==0{lots.pop_front();}}}
  for(owner,n,cost,start)in closed{if let Some(owner)=owner{if let Some(p)=self.exposures.get_mut(&owner){p.owned_remaining=p.owned_remaining.checked_sub(n).ok_or("DIST_FIFO_QUANTITY")?;if let Some(dt)=executed.checked_sub(start){p.owned_capital_time=plus(p.owned_capital_time,times(times(cost,n)?,dt)?)?;}else{p.capital_clock_unknown=true;}}}}
  if buying&&left>0{let owner=self.exposures.contains_key(&qid).then_some(qid);self.inventory.entry((ex,side)).or_default().push_back(Lot{quantity:left,price,entered:executed,owner});if let Some(p)=self.exposures.get_mut(&qid){p.owned_remaining=p.owned_remaining.checked_add(left).ok_or("DIST_RANGE")?;}}else if !buying{need(left==0,"DIST_FIFO_UNOBSERVED_INVENTORY")?;}self.prune_completed_outside_retention();Ok(())
 }
 fn price_marks(&mut self,raw:&Book,at:u64)->Result<()>{let b=self.external(raw);let fresh=self.fresh(raw,at);let coverage=self.coverage.get(&b.exchange);let mut order_depth:BTreeMap<String,Option<i128>>=BTreeMap::new();
  // Price EACH simultaneous lot against the aggregate same-cohort filled
  // quantity at this horizon. Never reuse a shallow first level per fill.
  for side in ["YES","NO"]{let total=self.exposures.values().filter(|p|p.exchange==b.exchange&&p.side==side).flat_map(|p|&p.fills).filter(|f|f.mark.is_none()&&b.source>=f.executed.saturating_add(self.config.markout_ms)).try_fold(0u64,|a,g|a.checked_add(g.quantity).ok_or("DIST_RANGE"))?;if total==0{continue}let rows=if side=="YES"{&b.bids}else{&b.asks};let mut left=total;let mut value=0i128;for(price,n)in rows{let take=left.min(*n);value=plus(value,times(if side=="YES"{*price}else{UNIT-*price},take)?)?;left-=take;if left==0{break}}order_depth.insert(side.into(),(left==0).then_some(value.div_euclid(i128::from(total))));}
  for p in self.exposures.values_mut().filter(|p|p.exchange==b.exchange){for f in p.fills.iter_mut().filter(|f|f.mark.is_none()){
   let target=f.executed.checked_add(self.config.markout_ms).ok_or("DIST_RANGE")?;if b.source<target{continue}let until=target.checked_add(self.config.post_window_ms).ok_or("DIST_RANGE")?;
   let reason=if b.source>until{"first_book_after_window"}else if !fresh||at>until.saturating_add(self.config.maximum_delivery_ms){"stale_first_book"}else if !coverage.is_some_and(|c|c.from<=f.executed&&c.to>=b.source&&c.sequence>=b.sequence){"missing_trade_continuity"}else{"priced"};
   let per=order_depth.get(&p.side).copied().flatten();let gain=if reason=="priced"{per.map(|value|times(value-f.price-self.config.exit_fee_per_share,f.quantity).and_then(|n|n.checked_sub(f.fee).ok_or("DIST_RANGE"))).transpose()?}else{None};
   f.mark=Some(Mark{gain,reason:if reason=="priced"&&gain.is_none(){"insufficient_aggregate_depth"}else{reason},available:at,source_sha:digest(&json!({"book":raw.raw_sha,"coverage":coverage.map(|c|&c.sha),"ownAdjustedBids":b.bids,"ownAdjustedAsks":b.asks,"value":gain}))?});
  }}Ok(())
 }
 fn expire_marks(&mut self,at:u64)->Result<()>{for p in self.exposures.values_mut(){for f in &mut p.fills{if f.mark.is_none()&&at>f.executed.saturating_add(self.config.markout_ms).saturating_add(self.config.post_window_ms).saturating_add(self.config.maximum_delivery_ms){f.mark=Some(Mark{gain:None,reason:"no_first_book_in_window",available:at,source_sha:f.receipt_sha.clone()});}}}Ok(())}
 fn refresh_values(&mut self)->Result<()>{for p in self.exposures.values_mut(){if let Some(s)=&mut p.sample{if s.filled==0{s.gain=Some(0)}else if p.fills.iter().all(|f|f.mark.is_some()){s.gain=p.fills.iter().try_fold(Some(0i128),|a,f|match(a,f.mark.as_ref().and_then(|m|m.gain)){(Some(a),Some(b))=>plus(a,b).map(Some),_=>Ok(None)})?;}}}Ok(())}
 /// Sole worker calls AFTER native terminal acceptance AND original log.closed
 /// append, using the SAME original GET/order + complete later GET/fills it
 /// already read. This causes NO HTTP. No ACK is treated as a fill/close.
 pub fn confirm_terminal(&mut self,bank:&Session,log:&DispatchLog,qid:u64,order:&Receipt,tape:&Receipt,at:u64)->Result<bool>{self.bind(bank)?;need(at>=self.last,"DIST_OBSERVATION_CLOCK")?;if !self.exposures.contains_key(&qid){return Ok(false)}if self.exposures[&qid].confirmed{return Ok(false)}need(log.closed.contains(&qid)&&log.attempted.contains(&qid),"DIST_ORIGINAL_CLOSED_REQUIRED")?;let ack=self.verify_ack(bank,log,qid,at)?;let actual=id(&ack["actualOrderId"])?;let q=bank.engine().order_records().into_iter().find(|q|q["id"]==qid).ok_or("DIST_QUOTE")?;need(q["retired"]==true&&order.endpoint==format!("/orders/{actual}")&&tape.endpoint==format!("/orders/{actual}/fills?limit=200"),"DIST_ORIGINAL_CLOSE_ENDPOINT")?;
  let tournament=intent_tournament(log,qid)?;let p=self.exposures.get_mut(&qid).unwrap();let witness=p.terminal.as_ref().ok_or("DIST_COMMITTED_TERMINAL_REQUIRED")?;let stamp=witness["processedAt"].as_str().or_else(||witness["receivedAt"].as_str()).ok_or("DIST_CLOCK")?;let terminal=feedback::terminal(&q,&actual,tournament,order,tape,stamp).map_err(|_|"DIST_TERMINAL_RECEIPTS")?.ok_or("DIST_ORDER_STILL_OPEN")?;need(&terminal==witness,"DIST_TERMINAL_RECEIPT_MEMBERSHIP")?;let rows=feedback::fills(&q,&actual,tournament,tape,stamp).map_err(|_|"DIST_FULL_TAPE")?;
  need(rows.len()==p.fills.len()&&rows.iter().all(|e|p.fills.iter().any(|f|e["fillId"]==f.source_id&&e["quantity"]==f.quantity&&amount(&e["price"]).ok()==Some(f.price)&&amount(&e["fee"]).ok()==Some(f.fee)&&clock(&e["executedAt"]).ok()==Some(f.executed)))&&rows.iter().all(|e|bank.engine().fills().iter().any(|f|native_fill_matches(f,e))),"DIST_TAPE_OBSERVED_FILL_SET")?;
  let filled=p.fills.iter().try_fold(0u64,|a,f|a.checked_add(f.quantity).ok_or("DIST_RANGE"))?;need(q["filled"]==filled,"DIST_NATIVE_FILL_TOTAL")?;
  let end=clock(&json!(&tape.received_at))?;let mut timeline=p.fills.iter().map(|f|(f.executed,f.quantity)).collect::<Vec<_>>();if let Some(admitted)=p.admitted{if admitted<p.requested{timeline.push((p.ack_observed.ok_or("DIST_ACK_CLOCK")?,p.requested-admitted));}}timeline.sort();let mut remaining=p.requested;let mut start=p.created;let mut reservation=0i128;for(executed,n)in timeline{let duration=executed.checked_sub(start).ok_or("DIST_CAPITAL_CLOCK")?;reservation=plus(reservation,times(times(p.limit,remaining)?,duration)?)?;remaining=remaining.checked_sub(n).ok_or("DIST_RANGE")?;start=executed;}reservation=plus(reservation,times(times(p.limit,remaining)?,end.checked_sub(start).ok_or("DIST_CAPITAL_CLOCK")?)?)?;
  p.confirmed=true;p.sample=Some(Sample{key:p.key.clone(),market:p.market.clone(),attempt:p.attempt,completed:at,requested:p.requested,filled,gain:if filled==0{Some(0)}else{None},reservation_capital_time:reservation,status:id(&terminal["status"])?,proof_sha:digest(&json!({"attempt":p.proof_sha,"order":feedback::digest(order).map_err(|_|"DIST_RECEIPT_HASH")?,"tape":feedback::digest(tape).map_err(|_|"DIST_RECEIPT_HASH")?,"terminal":witness,"head":bank.durable_head()}))?});self.retained.push_back(qid);
  while self.retained.len()>self.config.retained_samples{let old=self.retained.pop_front().unwrap();if self.exposures.get(&old).is_some_and(|p|p.owned_remaining==0){self.exposures.remove(&old);}}
  self.last=at;self.refresh_values()?;Ok(true)
 }
 /// Keep owned/unknown exposure evidence; reclaim aged completed cohorts only after actual FIFO closure.
 fn prune_completed_outside_retention(&mut self){let retained:BTreeSet<_>=self.retained.iter().copied().collect();self.exposures.retain(|qid,p|!p.confirmed||p.owned_remaining>0||retained.contains(qid));}
 pub fn outcome(&self,qid:u64,at:u64)->Result<OutcomeView>{let p=self.exposures.get(&qid).ok_or("DIST_EXPOSURE")?;need(at>=self.last,"DIST_PAST_QUERY")?;let filled=p.fills.iter().map(|f|f.quantity).sum::<u64>();let sample=p.sample.as_ref();let mut owned=p.owned_capital_time;for lots in self.inventory.values(){for l in lots.iter().filter(|l|l.owner==Some(qid)){owned=plus(owned,times(times(l.price,l.quantity)?,at.checked_sub(l.entered).ok_or("DIST_CAPITAL_CLOCK")?)?)?;}}
  Ok(OutcomeView{requested_quantity:p.requested,admitted_quantity:p.admitted,observed_filled_quantity:filled,class:if !p.confirmed{"unresolved"}else if filled==0{"confirmed_zero"}else if filled==p.requested{"confirmed_full"}else{"confirmed_partial"}.into(),execution_confirmed:p.confirmed,conditional_gain_units:sample.and_then(|s|s.gain),value_censored_quantity:p.fills.iter().filter(|f|f.mark.as_ref().is_some_and(|m|m.gain.is_none())).map(|f|f.quantity).sum(),first_fill_latency_ms:p.fills.iter().map(|f|f.executed-p.attempt).min(),maximum_receipt_delay_ms:p.fills.iter().map(|f|f.observed-f.executed).max(),reservation_capital_time_unit_ms:sample.map(|s|s.reservation_capital_time),owned_capital_time_observed_unit_ms:owned,owned_capital_remaining_quantity:p.owned_remaining,capital_time_complete:p.confirmed&&p.owned_remaining==0&&!p.capital_clock_unknown,source_sha256:sample.map(|s|s.proof_sha.clone()).unwrap_or_else(||p.proof_sha.clone())})
 }
 pub fn pool_key(&self,qid:u64)->Option<&PoolKey>{self.exposures.get(&qid).and_then(|p|p.key.as_ref())}
 /// Read-only prospective query of THIS configured, original-source observer.
 /// This neither creates a trial nor transfers ordinary short-horizon labels to
 /// a terminal-carry or bounded-aggressive strategy. An unobserved strategy has
 /// no fitted evidence. Requery at actual submission if age/lifetime changes.
 pub fn estimate_action(&self,bank:&Session,scope:&baal_transport_service::Scope,action:&Value,strategy_sha:&str,created_at:u64,expiry:u64,horizon:u64,at:u64)->Result<Estimate>{
  self.bind(bank)?;self.bind_estimate(at)?;
  need(horizon==self.config.markout_ms,"DIST_QUERY_HORIZON_MISMATCH")?;
  need(strategy_sha.len()==64&&strategy_sha.bytes().all(|b|b.is_ascii_hexdigit())&&self.exposures.values().any(|p|p.key.as_ref().is_some_and(|k|k.strategy_sha256==strategy_sha)),"DIST_QUERY_STRATEGY_UNOBSERVED")?;
  let ex=id(&action["exchangeId"])?;let market=id(&action["marketId"])?;let n=action["quantity"].as_u64().filter(|n|*n>0).ok_or("DIST_QUERY_QUANTITY")?;
  need(action["tournamentId"]==scope.tournament_id&&scope.exchanges.get(&ex)==Some(&market)&&action["action"]=="buy"&&matches!(action["side"].as_str(),Some("yes"|"no")),"DIST_QUERY_SCOPE")?;
  let p=amount(&action["limitPrice"])?;need((5000..=995000).contains(&p)&&p%5000==0&&n<=bank.engine().policy()["quantity"].as_u64().ok_or("DIST_QUERY_POLICY")?,"DIST_QUERY_PRICE_SIZE")?;
  need(created_at<=at&&at<expiry&&expiry-at<=bank.engine().policy()["lifetimeMs"].as_u64().ok_or("DIST_QUERY_POLICY")?,"DIST_QUERY_CLOCK")?;
  let raw=bank.engine().live_package_books().into_iter().find(|r|r["exchangeId"]==ex&&r["marketId"]==market&&r["tournamentId"]==scope.tournament_id).ok_or("DIST_QUERY_NATIVE_BOOK")?;
  need(self.books.get(&ex).is_some_and(|b|digest(&raw).ok().as_deref()==Some(b.raw_sha.as_str())),"DIST_QUERY_OBSERVER_NOT_CURRENT")?;
  let report=bank.engine().report().map_err(|_|"DIST_REPORT")?;let report_at=clock(&report["at"])?;need(self.last>=report_at,"DIST_QUERY_OBSERVER_NOT_CURRENT")?;
  let q=json!({"exchangeId":ex,"marketId":market,"side":if action["side"]=="no"{"NO"}else{"YES"},"price":moloch_replay::money(p),"createdAt":created_at,"expiresAt":expiry});
  let key=self.features(&q,n,strategy_sha.to_owned(),at)?;self.estimate(&key,&market,at)
 }
 pub fn estimate(&self,key:&PoolKey,market:&str,at:u64)->Result<Estimate>{self.bind_estimate(at)?;let parent:Vec<_>=self.exposures.values().filter_map(|p|p.sample.as_ref()).filter(|s|s.key.as_ref()==Some(key)).collect();let local:Vec<_>=parent.iter().copied().filter(|s|s.market==market).collect();let counts=|rows:&[&Sample]|{let mut n=[0usize;3];for s in rows{n[if s.filled==0{0}else if s.filled==s.requested{2}else{1}]+=1;}n};let pc=counts(&parent);let lc=counts(&local);let prior:Vec<_>=pc.iter().map(|n|(*n as f64+1.)/(parent.len()as f64+3.)).collect();let strength=f64::from(self.config.shrinkage_strength);let probs=std::array::from_fn(|i|(lc[i]as f64+strength*prior[i])/(local.len()as f64+strength));
  let mean=|rows:&[&Sample],bucket:Option<usize>|->Option<f64>{let values:Vec<_>=rows.iter().filter(|s|bucket.is_none_or(|b|b==if s.filled==0{0}else if s.filled==s.requested{2}else{1})).filter_map(|s|s.gain.map(|g|g as f64/s.requested as f64)).collect();(!values.is_empty()).then(||values.iter().sum::<f64>()/values.len()as f64)};
  let mut joint=Some(0.);for b in 0..3{let pm=if b==0{Some(0.)}else{mean(&parent,Some(b))};let lm=mean(&local,Some(b));let valid=local.iter().filter(|s|s.gain.is_some()&&(if s.filled==0{0}else if s.filled==s.requested{2}else{1})==b).count()as f64;let m=match(pm,lm){(Some(p),Some(l))=>Some((valid*l+strength*p)/(valid+strength)),(p,None)=>p,(None,Some(l))=>Some(l)};joint=joint.zip(m).map(|(sum,m)|sum+probs[b]*m);}
  let valued:Vec<_>=parent.iter().copied().filter(|s|s.gain.is_some()).collect();let mut blocks=BTreeMap::<u64,Vec<f64>>::new();for s in &valued{blocks.entry(s.attempt/self.config.block_ms).or_default().push(s.gain.unwrap()as f64/s.requested as f64);}let bm:Vec<_>=blocks.values().map(|v|v.iter().sum::<f64>()/v.len()as f64).collect();let se=if bm.len()>1{let m=bm.iter().sum::<f64>()/bm.len()as f64;Some((bm.iter().map(|x|(x-m).powi(2)).sum::<f64>()/((bm.len()-1)*bm.len())as f64).sqrt())}else{None};let uncertainty=se.map(|s|(2.*s).max(self.config.uncertainty_floor_per_share as f64));
  let filled_fraction=(!parent.is_empty()).then(||parent.iter().map(|s|s.filled as f64/s.requested as f64).sum::<f64>()/parent.len()as f64);let tail=filled_fraction.map(|f|f*self.config.tail_charge_per_filled_share as f64);let write=(!parent.is_empty()).then(||parent.iter().map(|s|self.config.write_charge_per_order as f64/s.requested as f64).sum::<f64>()/parent.len()as f64);let mut incomplete=0;let mut capital=0f64;for s in &parent{let p=self.exposures.values().find(|p|p.sample.as_ref().is_some_and(|x|std::ptr::eq(x,*s))).ok_or("DIST_SAMPLE")?;if p.owned_remaining!=0||p.capital_clock_unknown{incomplete+=1;}capital+=plus(s.reservation_capital_time,p.owned_capital_time)? as f64/s.requested as f64*self.config.capital_charge_per_million_unit_ms as f64/1_000_000.;}let capital=(!parent.is_empty()&&incomplete==0).then(||capital/parent.len()as f64);
  let markets:BTreeSet<_>=parent.iter().map(|s|s.market.as_str()).collect();let leave:Vec<_>=markets.iter().filter_map(|m|{let rows:Vec<_>=parent.iter().copied().filter(|s|s.market!=*m).collect();mean(&rows,None)}).collect();let range=(leave.len()>1).then(||[leave.iter().copied().fold(f64::INFINITY,f64::min),leave.iter().copied().fold(f64::NEG_INFINITY,f64::max)]);let unresolved=self.exposures.values().filter(|p|p.key.as_ref()==Some(key)&&!p.confirmed).count();let qualified=unresolved==0&&parent.len()>=self.config.minimum_completed&&bm.len()>=self.config.minimum_blocks&&valued.len()*10000>=parent.len()*self.config.minimum_value_coverage_bps as usize;
  let mut latency=[0usize;4];let mut statuses=BTreeMap::<String,usize>::new();for p in self.exposures.values().filter(|p|p.key.as_ref()==Some(key)&&p.confirmed){if let Some(s)=&p.sample{*statuses.entry(s.status.clone()).or_default()+=1;}if let Some(first)=p.fills.iter().map(|f|f.executed-p.attempt).min(){latency[if first<=1000{0}else if first<=10000{1}else if first<=60000{2}else{3}]+=1;}}let total_filled=valued.iter().map(|s|s.filled).sum::<u64>();let conditional=(total_filled>0).then(||valued.iter().map(|s|s.gain.unwrap()as f64).sum::<f64>()/total_filled as f64);let minimum=valued.iter().map(|s|s.gain.unwrap()as f64/s.requested as f64).reduce(f64::min);let net=if qualified{joint.zip(uncertainty).zip(tail).zip(write).zip(capital).map(|((((g,u),t),w),c)|g-u-t-w-c)}else{None};
  Ok(Estimate{pool_key:key.clone(),parent_completed:parent.len(),market_completed:local.len(),distinct_markets:markets.len(),time_blocks:bm.len(),parent_counts:pc,market_counts:lc,posterior_zero_partial_full:probs,expected_filled_fraction:filled_fraction,observed_joint_mean_per_requested_share:mean(&parent,None),shrunk_joint_mean_per_requested_share:joint,conditional_per_filled_share:conditional,minimum_observed_per_requested_share:minimum,empirical_block_standard_error:se,uncertainty_charge_per_requested_share:uncertainty,tail_charge_per_requested_share:tail,mean_write_charge_per_requested_share:write,capital_time_charge_per_requested_share:capital,net_shadow_score_per_requested_share:net,value_covered:valued.len(),value_censored:parent.len()-valued.len(),unresolved,first_fill_latency_counts:latency,terminal_status_counts:statuses,capital_time_incomplete:incomplete,leave_market_out_joint_mean_range:range,qualified_descriptive_sample:qualified,confidence_kind:"empirical_block_standard_error_not_a_confidence_guarantee",cancellation_is_informative:true,independent_censoring_assumed:false,execution_authority:false})
 }
 fn bind_estimate(&self,at:u64)->Result<()>{need(self.disabled.is_none()&&at>=self.last,"DIST_DISABLED_OR_PAST_QUERY")}
 pub fn diagnostic(&self)->Value{let mut reasons=BTreeMap::<String,u64>::new();let mut labels=Sha256::new();for p in self.exposures.values(){if let Some(r)=p.feature_reason{*reasons.entry(r.into()).or_default()+=1;}for f in &p.fills{labels.update(f.receipt_sha.as_bytes());if let Some(m)=&f.mark{*reasons.entry(m.reason.into()).or_default()+=1;labels.update(m.source_sha.as_bytes());labels.update(m.available.to_le_bytes());}}}json!({"version":1,"executionAuthority":false,"disabled":self.disabled,"lastObservedAtMs":self.last,"bankManifestHash":self.manifest,"policySha256":self.policy_sha,"tracked":self.exposures.len(),"confirmed":self.exposures.values().filter(|p|p.confirmed).count(),"unresolved":self.exposures.values().filter(|p|!p.confirmed).count(),"excludedFundamentalCarry":self.excluded_carry,"duplicateEvents":self.duplicates,"unattributedFills":self.unattributed_fills,"knownOwnLiquidityOrders":self.own.len(),"censorAndFeatureReasons":reasons,"sourceCursors":self.cursors,"labelSourceDigest":format!("{:x}",labels.finalize()),"conditionalValueKind":"hypothetical_later_executable_depth_not_realized_profit","capitalTime":"original_reservation_plus_actual_FIFO_owned_cost_until_observed_exit","restart":"cold_cohort_no_deserialized_fits"})}
}

thread_local!{static OBSERVER:std::cell::RefCell<Option<Shadow>>=const{std::cell::RefCell::new(None)};}
pub fn initialize_observer(bank:&Session,config:Option<Config>)->Result<()>{let model=config.map(|c|Shadow::new(bank,c)).transpose()?;OBSERVER.with(|s|*s.borrow_mut()=model);Ok(())}
pub fn observe_runtime_committed(bank:&Session,source:&str,sequence:u64,event:&Value){OBSERVER.with(|s|{if let Some(m)=s.borrow_mut().as_mut(){if m.disabled.is_none(){let _=m.observe_committed(bank,source,sequence,event);}}});}
pub fn track_runtime_attempt(bank:&Session,log:&DispatchLog,qid:u64,at:u64){OBSERVER.with(|s|{if let Some(m)=s.borrow_mut().as_mut(){if m.disabled.is_none(){if let Err(e)=m.track_attempt(bank,log,qid,at){m.disabled=Some(e);}}}});}
pub fn confirm_runtime_terminal(bank:&Session,log:&DispatchLog,qid:u64,order:&Receipt,tape:&Receipt,at:u64){OBSERVER.with(|s|{if let Some(m)=s.borrow_mut().as_mut(){if m.disabled.is_none(){if let Err(e)=m.confirm_terminal(bank,log,qid,order,tape,at){m.disabled=Some(e);}}}});}
pub fn observer_diagnostic()->Option<Value>{OBSERVER.with(|s|s.borrow().as_ref().map(Shadow::diagnostic))}
/// A query into actual observer labels; no caller may supply fitted means,
/// counts, losses or prior outcomes. At cold start this deliberately fails.
pub fn estimate_runtime_attempt(qid:u64,at:u64)->Result<Estimate>{OBSERVER.with(|s|{let s=s.borrow();let m=s.as_ref().ok_or("DIST_RUNTIME_UNAVAILABLE")?;let p=m.exposures.get(&qid).ok_or("DIST_EXPOSURE")?;m.estimate(p.key.as_ref().ok_or("DIST_UNQUALIFIED_FEATURES")?,&p.market,at)})}
/// No observer initialization, fit import, account reads or lifecycle mutations.
pub fn estimate_runtime_action(bank:&Session,scope:&baal_transport_service::Scope,action:&Value,strategy_sha:&str,created_at:u64,expiry:u64,horizon:u64,at:u64)->Result<Estimate>{OBSERVER.with(|s|s.borrow().as_ref().ok_or("DIST_RUNTIME_UNAVAILABLE")?.estimate_action(bank,scope,action,strategy_sha,created_at,expiry,horizon,at))}

/// A separate, prospective data cohort. It supplies no estimates, price labels,
/// probability qualification or execution capability to the ordinary model.
#[derive(Clone,Serialize,Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AcquisitionConfig {
 pub cohort_id:String,pub maximum_pending:usize,pub retained_completed:usize,
}
impl AcquisitionConfig {
 pub fn validate(&self)->Result<()> {
  need(!self.cohort_id.is_empty()&&self.cohort_id.len()<=96&&self.cohort_id.bytes().all(|b|b.is_ascii_alphanumeric()||b"-_".contains(&b))
   &&(1..=256).contains(&self.maximum_pending)&&(1..=1024).contains(&self.retained_completed),"ACQ_CONFIG")
 }
}
#[derive(Clone)]
struct AcquisitionFill {facts:Value,first_observation:Value,quantity:u64,executed:u64,observed:u64}
struct AcquisitionExposure {
 evidence:Value,quote:Value,intent_sha:String,attempt_sha:String,requested:u64,native_reserved:u64,
 admitted:Option<u64>,ack_receipt:Option<String>,ack_observed:Option<u64>,actual_id:Option<String>,
 created:u64,attempt:u64,expires:u64,limit:i128,capital_at:u64,reserved:i128,reservation_time:i128,
 owned_closed_time:i128,owned_remaining:u64,fills:BTreeMap<(String,String),AcquisitionFill>,
 terminal:Option<Value>,confirmed:bool,aborted:bool,close_proof:Option<String>,
}
impl AcquisitionExposure {
 fn integrate(&mut self,at:u64)->Result<()> {
  self.reservation_time=plus(self.reservation_time,times(self.reserved,at.checked_sub(self.capital_at).ok_or("ACQ_CAPITAL_CLOCK")?)?)?;
  self.capital_at=at;Ok(())
 }
}
#[derive(Clone)]
struct AcquisitionLot {quantity:u64,price:i128,observed:u64,owner:Option<u64>}
/// Exact quantities and clock differences, rather than a fitted fill model.
/// Principal-time fields are decimal INTEGER strings in millionths * ms.
#[derive(Serialize)]
#[serde(rename_all="camelCase")]
pub struct AcquisitionOutcome {
 pub qid:u64,pub class:String,pub requested_quantity:u64,pub native_reserved_quantity:u64,
 pub admitted_quantity:Option<u64>,pub observed_filled_quantity:u64,pub filled_fraction:Option<[u64;2]>,
 pub first_execution_latency_ms:Option<u64>,pub first_observation_latency_ms:Option<u64>,pub maximum_fill_observation_delay_ms:Option<u64>,
 pub attempt_at_ms:u64,pub quote_created_at_ms:u64,pub original_expiry_ms:u64,pub observed_through_ms:u64,
 pub reservation_principal_time_unit_ms:String,pub owned_principal_time_observed_unit_ms:String,pub owned_remaining_quantity:u64,
 pub reservation_released:bool,pub acquisition_confirmed:bool,pub terminal_status:Option<String>,
 pub cancellation_is_informative:bool,pub original_evidence:Value,pub close_proof_sha256:Option<String>,
 pub fill_observations:Vec<Value>,
 pub execution_authority:bool,
}
/// Cannot deserialize a fit, restore an unresolved order as zero, or change the
/// Session. Each restart names a new prefix/cohort and leaves prior orders unscored.
pub struct AcquisitionShadow {
 config:AcquisitionConfig,scope:baal_transport_service::Scope,manifest:Value,run:String,cohort_sha:String,
 initial_head:Value,started:u64,last:u64,disabled:Option<&'static str>,dispatch_identity:Value,
 cursors:BTreeMap<String,(u64,String)>,pre_cohort:BTreeSet<u64>,exposures:BTreeMap<u64,AcquisitionExposure>,
 retained:VecDeque<u64>,inventory:BTreeMap<(String,String),VecDeque<AcquisitionLot>>,
 seen_fills:BTreeMap<(String,String),String>,duplicates:u64,completed:[u64;3],excluded_ordinary:u64,
}
fn acquisition_identity(log:&DispatchLog)->Result<Value> {
 let d=log.descriptor_identity().map_err(|_|"ACQ_ORIGINAL_DISPATCH")?;Ok(json!({"device":d["device"],"inode":d["inode"]}))
}
fn acquisition_fill_facts(e:&Value)->Result<Value> {
 Ok(json!({"orderId":e["orderId"],"actualOrderId":id(&e["actualOrderId"])?,"fillId":id(&e["fillId"])?,
  "marketId":id(&e["marketId"])?,"exchangeId":id(&e["exchangeId"])?,"side":e["side"],"action":e["action"],
  "quantity":e["quantity"],"priceUnits":amount(&e["price"])?,"feeUnits":amount(&e["fee"])?,"executedAt":e["executedAt"]}))
}
fn acquisition_book_evidence(raw:Option<&Value>,side:&Value,limit:i128,at:u64)->Result<Value> {
 let Some(raw)=raw else{return Ok(json!({"state":"unavailable"}))};let b=book(raw,at)?;let no=side=="NO";
 let bid=b.bids.first().map(|(p,n)|json!({"priceUnits":p,"quantity":n}));let ask=b.asks.first().map(|(p,n)|json!({"priceUnits":p,"quantity":n}));
 let mut through=0u64;for(p,n)in if no{&b.bids}else{&b.asks}{if (if no{UNIT-*p}else{*p})>limit{break}through=through.checked_add(*n).ok_or("ACQ_DEPTH_RANGE")?;}
 Ok(json!({"state":"recorded_native_book","sha256":b.raw_sha,"sequence":b.sequence.to_string(),"sourceAt":raw["sourceAt"],"requestStartedAt":raw["requestStartedAt"],"receivedAt":raw["receivedAt"],"transport":raw["transport"],"invalid":b.invalid,"yesBestBid":bid,"yesBestAsk":ask,"buyDepthThroughUnchangedLimit":through,"includesOwnLiquidity":true}))
}
impl AcquisitionShadow {
 pub fn new(bank:&Session,log:&DispatchLog,scope:&baal_transport_service::Scope,config:AcquisitionConfig,started:u64)->Result<Self> {
  config.validate()?;need(!scope.tournament_id.is_empty()&&!scope.exchanges.is_empty()&&scope.exchanges.len()<=4096,"ACQ_SCOPE")?;
  let seed=bank.live_account_seed().ok_or("ACQ_LIVE_BANK")?;let run=id(&seed["runId"])?;
  let at=bank.engine().report().map_err(|_|"ACQ_REPORT")?["at"].clone();need(at.as_str().is_none_or(|s|s.is_empty())||clock(&at)?<=started,"ACQ_START_CLOCK")?;
  let pre_cohort=log.attempted.iter().filter(|id|!log.closed.contains(id)).copied().collect::<BTreeSet<_>>();need(pre_cohort.len()<=4096,"ACQ_PRE_COHORT_BOUND")?;
  let mut inventory=BTreeMap::new();let positions=bank.engine().wallet.positions();
  for(ex,p)in positions.as_object().ok_or("ACQ_POSITIONS")? {for side in ["YES","NO"] {
   let mut lots=VecDeque::new();for l in p[side].as_array().ok_or("ACQ_LOTS")? {lots.push_back(AcquisitionLot{quantity:l["quantity"].as_u64().ok_or("ACQ_QUANTITY")?,price:amount(&l["price"])?,observed:started,owner:None});}
   inventory.insert((ex.clone(),side.into()),lots);
  }}
  let mut seen_fills=BTreeMap::new();need(bank.engine().fills().len()<=65_536,"ACQ_FILL_BOUND")?;
  for f in bank.engine().fills(){let mut e=f.clone();e["fillId"]=f["sourceId"].clone();let key=(id(&e["actualOrderId"])?,id(&e["fillId"])?);seen_fills.insert(key,digest(&acquisition_fill_facts(&e)?)?);}
  let initial_head=bank.durable_head();let cohort_sha=digest(&json!({"config":config,"scope":scope,"manifest":bank.manifest_hash(),"run":run,"head":initial_head,"startedAtMs":started}))?;
  Ok(Self{config,scope:scope.clone(),manifest:bank.manifest_hash().clone(),run,cohort_sha,initial_head,started,last:started,disabled:None,dispatch_identity:acquisition_identity(log)?,
   cursors:["moloch-live","moloch-package"].into_iter().map(|s|(s.into(),(bank.last_source_sequence(s),String::new()))).collect(),pre_cohort,exposures:BTreeMap::new(),retained:VecDeque::new(),inventory,seen_fills,duplicates:0,completed:[0;3],excluded_ordinary:0})
 }
 fn bind(&self,bank:&Session)->Result<()> {need(self.disabled.is_none()&&bank.manifest_hash()==&self.manifest&&bank.live_account_seed().is_some_and(|s|s["runId"]==self.run),"ACQ_BANK_OR_DISABLED")}
 fn log(&self,log:&DispatchLog)->Result<()> {need(acquisition_identity(log)?==self.dispatch_identity,"ACQ_ORIGINAL_DISPATCH_CHANGED")}
 /// Immediately after the original durable attempt, before its one SDK call.
 pub fn track_attempt(&mut self,bank:&Session,log:&DispatchLog,qid:u64,at:u64)->Result<bool> {
  self.bind(bank)?;self.log(log)?;need(at>=self.last&&at>=self.started,"ACQ_ATTEMPT_CLOCK")?;
  if self.pre_cohort.contains(&qid)||self.exposures.contains_key(&qid){return Ok(false)}
  let i=log.intents.get(&qid).ok_or("ACQ_ORIGINAL_INTENT")?;if i["fundamental"].is_null(){self.excluded_ordinary+=1;return Ok(false)}
  need(log.attempted.contains(&qid)&&!log.acknowledgments.contains_key(&qid)&&!log.closed.contains(&qid)&&!log.aborted_records().contains_key(&qid),"ACQ_ORIGINAL_ATTEMPT")?;
  let attempt=log.original_attempt(qid).ok_or("ACQ_ORIGINAL_ATTEMPT")?;need(clock(&attempt["at"])?==at&&attempt.get("captureAtMs").is_none_or(|n|n.as_u64()==Some(at)),"ACQ_ORIGINAL_ATTEMPT_CLOCK")?;
  let q=bank.engine().order_records().into_iter().find(|q|q["id"]==qid).ok_or("ACQ_NATIVE_QUOTE")?;
  if q["action"]!="buy"||bank.engine().is_package_order(qid){return Ok(false)}
  let tag=&i["fundamental"];need(tag["version"]==1&&tag["runId"]==self.run&&tag["bankManifestHash"]==self.manifest&&tag["qid"]==qid
   &&tag["bindingSha256"].as_str().is_some_and(|s|s.len()==64&&s.bytes().all(|b|b.is_ascii_hexdigit()))&&q["retired"]==false&&q["filled"]==0&&i["quote"]==q,"ACQ_ORIGINAL_TAG_OR_QUOTE")?;
  let tournament=intent_tournament(log,qid)?;let spec=&i["spec"];let body=&spec["body"];let n=body["quantity"].as_u64().filter(|n|*n>0).ok_or("ACQ_REQUEST_QUANTITY")?;
  let native=q["quantity"].as_u64().ok_or("ACQ_NATIVE_QUANTITY")?;let ex=id(&q["exchangeId"])?;let market=id(&q["marketId"])?;let limit=amount(&q["price"])?;
  need(tournament==self.scope.tournament_id&&self.scope.exchanges.get(&ex)==Some(&market)&&body["exchangeId"]==q["exchangeId"]&&body["action"]=="buy"
   &&body["side"]==if q["side"]=="NO"{"no"}else{"yes"}&&n<=native&&amount(&body["price"])?==limit&&clock(&body["expirationDate"])?==q["expiresAt"].as_u64().ok_or("ACQ_EXPIRY")?
   &&body["idempotencyKey"]==format!("{}:{qid}",self.run),"ACQ_EXACT_REQUEST_SCOPE")?;
  let created=q["createdAt"].as_u64().filter(|t|*t<=at).ok_or("ACQ_CREATED_CLOCK")?;let expires=q["expiresAt"].as_u64().filter(|t|*t>at).ok_or("ACQ_EXPIRY")?;
  let raw=bank.engine().live_package_books().into_iter().find(|b|b["exchangeId"]==ex);
  if let Some(b)=&raw{need(b["marketId"]==market&&b["tournamentId"]==tournament,"ACQ_BOOK_SCOPE")?;}
  let mode=q["reviewedFundamentalEntry"].get("executionMode").and_then(Value::as_str).unwrap_or("joinBid");need(matches!(mode,"joinBid"|"improveOneTick"|"boundedLimitMayRest"),"ACQ_EXECUTION_MODE")?;
  self.prune();need(self.exposures.values().filter(|p|!p.confirmed&&!p.aborted).count()<self.config.maximum_pending&&self.exposures.len()<self.config.maximum_pending+self.config.retained_completed,"ACQ_EXPOSURE_BOUND")?;
  let descriptor=log.descriptor_identity().map_err(|_|"ACQ_ORIGINAL_DISPATCH")?;let evidence=json!({"nativeHead":bank.durable_head(),"originalIntentSha256":digest(i)?,"originalAttemptSha256":digest(attempt)?,"originalSpecSha256":digest(spec)?,"originalQuoteSha256":digest(&q)?,"fundamentalTag":tag,"tournamentId":tournament,"marketId":market,"exchangeId":ex,"side":q["side"],"executionMode":mode,"executionModeOmitted":q["reviewedFundamentalEntry"].get("executionMode").is_none(),"nativeReviewMetadata":q["reviewedFundamentalEntry"],"policySha256":digest(bank.engine().policy())?,"book":acquisition_book_evidence(raw.as_ref(),&q["side"],limit,at)?,"dispatch":descriptor,"reviewApprovalIndependentlyVerified":false});
  let reserved=times(limit,native)?;let reservation_time=times(reserved,at-created)?;
  self.exposures.insert(qid,AcquisitionExposure{evidence,quote:q,intent_sha:digest(i)?,attempt_sha:digest(attempt)?,requested:n,native_reserved:native,admitted:None,ack_receipt:None,ack_observed:None,actual_id:None,created,attempt:at,expires,limit,capital_at:at,reserved,reservation_time,owned_closed_time:0,owned_remaining:0,fills:BTreeMap::new(),terminal:None,confirmed:false,aborted:false,close_proof:None});self.last=at;Ok(true)
 }
 fn reservation(&mut self,bank:&Session,qid:u64,at:u64)->Result<()> {
  let q=bank.engine().order_records().into_iter().find(|q|q["id"]==qid).ok_or("ACQ_NATIVE_QUOTE")?;let p=self.exposures.get_mut(&qid).ok_or("ACQ_EXPOSURE")?;p.integrate(at)?;
  let n=q["quantity"].as_u64().ok_or("ACQ_QUANTITY")?.checked_sub(q["filled"].as_u64().ok_or("ACQ_QUANTITY")?).ok_or("ACQ_QUANTITY")?;
  p.reserved=if q["retired"]==true{0}else{times(p.limit,n)?};Ok(())
 }
 pub fn observe_committed(&mut self,bank:&Session,source:&str,sequence:u64,e:&Value)->Result<()> {
  let result=self.observe_inner(bank,source,sequence,e);if let Err(error)=result{self.disabled=Some(error)}result
 }
 fn observe_inner(&mut self,bank:&Session,source:&str,sequence:u64,e:&Value)->Result<()> {
  self.bind(bank)?;let mut original=e.clone();original.as_object_mut().ok_or("ACQ_EVENT")?.remove("sourceRef");let sha=digest(&original)?;
  need(sequence>0&&bank.committed_source_hash(source,sequence)==Some(sha.as_str())&&e.get("sourceRef").is_none_or(|r|*r==json!({"id":source,"sequence":sequence,"hash":sha})),"ACQ_SOURCE_MEMBERSHIP")?;
  if let Some((n,h))=self.cursors.get(source){if sequence==*n&&h==&sha{self.duplicates+=1;return Ok(())}need(sequence==n+1,"ACQ_SOURCE_ORDER")?;}else{need(sequence==1&&self.cursors.len()<16,"ACQ_SOURCE_BASELINE")?;}
  let at=clock(e.get("processedAt").unwrap_or(&e["receivedAt"]))?;need(at>=self.last,"ACQ_OBSERVATION_CLOCK")?;
  match e["type"].as_str() {
   Some("actual_order_ack")=>{let qid=e["orderId"].as_u64().ok_or("ACQ_QID")?;if let Some(p)=self.exposures.get_mut(&qid){let n=e["quantity"].as_u64().filter(|n|*n>0&&*n<=p.requested).ok_or("ACQ_ACK_QUANTITY")?;
    need(bank.engine().live_order_bindings().get(&qid).is_some_and(|id|e["actualOrderId"]==*id)&&e["exchangeId"]==p.quote["exchangeId"]&&e["action"]=="buy","ACQ_ACK_BINDING")?;
    let actual=id(&e["actualOrderId"])?;let receipt=id(&e["receiptHash"])?;if let Some(old)=&p.actual_id{need(old==&actual&&p.admitted==Some(n)&&p.ack_receipt.as_ref()==Some(&receipt),"ACQ_ACK_CHANGED")?;}else{p.actual_id=Some(actual);p.admitted=Some(n);p.ack_receipt=Some(receipt);p.ack_observed=Some(at);p.evidence["ackObservation"]=json!({"event":original,"source":{"id":source,"sequence":sequence,"sha256":sha}});}self.reservation(bank,qid,at)?;}}
   Some("actual_fill")=>self.fill(bank,e,at,&json!({"id":source,"sequence":sequence,"sha256":sha}))?,
   Some("actual_order_terminal")=>{let qid=e["orderId"].as_u64().ok_or("ACQ_QID")?;if let Some(p)=self.exposures.get_mut(&qid){if let Some(old)=&p.terminal{let mut a=old.clone();let mut b=e.clone();for k in ["sourceRef","processedAt"]{a.as_object_mut().unwrap().remove(k);b.as_object_mut().unwrap().remove(k);}need(a==b,"ACQ_TERMINAL_CHANGED")?;}else{let mut terminal=e.clone();terminal.as_object_mut().unwrap().remove("sourceRef");p.terminal=Some(terminal);}self.reservation(bank,qid,at)?;}},
   Some("unsubmitted_intent_retired")=>{let qid=e["orderId"].as_u64().ok_or("ACQ_QID")?;if self.exposures.contains_key(&qid){self.reservation(bank,qid,at)?;}},
   Some("actual_position_adoption"|"actual_collateral_settlement")=>return Err("ACQ_UNSUPPORTED_INVENTORY_TRANSITION"),
   _=>{},
  }
  self.cursors.insert(source.into(),(sequence,sha));self.last=at;self.prune();Ok(())
 }
 fn fill(&mut self,bank:&Session,e:&Value,at:u64,source:&Value)->Result<()> {
  need(bank.engine().fills().iter().any(|f|native_fill_matches(f,e)),"ACQ_NATIVE_FILL_MEMBERSHIP")?;
  let facts=acquisition_fill_facts(e)?;let key=(id(&e["actualOrderId"])?,id(&e["fillId"])?);let sha=digest(&facts)?;
  if let Some(old)=self.seen_fills.get(&key){need(old==&sha,"ACQ_FILL_CONFLICT")?;self.duplicates+=1;return Ok(())}
  need(self.seen_fills.len()<65_536,"ACQ_FILL_BOUND")?;
  let qid=e["orderId"].as_u64().ok_or("ACQ_QID")?;let n=e["quantity"].as_u64().filter(|n|*n>0).ok_or("ACQ_QUANTITY")?;let price=amount(&e["price"])?;let executed=clock(&e["executedAt"])?;need(executed<=clock(&e["receivedAt"])?&&clock(&e["receivedAt"])?<=at,"ACQ_FILL_CLOCK")?;
  if let Some(p)=self.exposures.get_mut(&qid){need(p.actual_id.as_ref()==Some(&key.0)&&e["side"]==p.quote["side"]&&e["exchangeId"]==p.quote["exchangeId"]&&e["action"]=="buy"&&executed>=p.attempt&&p.fills.len()<256
   &&p.fills.values().map(|f|f.quantity).sum::<u64>().checked_add(n).is_some_and(|s|s<=p.admitted.unwrap_or(0)),"ACQ_FILL_BINDING")?;
   need(!p.confirmed,"ACQ_LATE_FILL_AFTER_CONFIRMED_TAPE")?;p.fills.insert(key.clone(),AcquisitionFill{facts,first_observation:json!({"receivedAt":e["receivedAt"],"processedAt":e.get("processedAt").unwrap_or(&e["receivedAt"]),"receiptSha256":e["receiptHash"],"source":source,"tiedFillAccounting":e["tiedFillAccounting"]}),quantity:n,executed,observed:at});self.reservation(bank,qid,at)?;
  }
  let ex=id(&e["exchangeId"])?;let side=id(&e["side"])?;let buy=e["action"]=="buy";need(matches!(side.as_str(),"YES"|"NO")&&matches!(e["action"].as_str(),Some("buy"|"sell")),"ACQ_FILL_ACTION")?;
  let consumed=if buy{if side=="YES"{"NO"}else{"YES"}}else{side.as_str()};let mut left=n;let mut closed=Vec::new();
  if let Some(lots)=self.inventory.get_mut(&(ex.clone(),consumed.into())){while left>0 {let Some(l)=lots.front_mut()else{break};let take=left.min(l.quantity);closed.push((l.owner,take,l.price,l.observed));left-=take;l.quantity-=take;if l.quantity==0{lots.pop_front();}}}
  for(owner,n,cost,entered)in closed{if let Some(owner)=owner{let p=self.exposures.get_mut(&owner).ok_or("ACQ_OWNED_EVIDENCE_DROPPED")?;p.owned_remaining=p.owned_remaining.checked_sub(n).ok_or("ACQ_FIFO_QUANTITY")?;p.owned_closed_time=plus(p.owned_closed_time,times(times(cost,n)?,at.checked_sub(entered).ok_or("ACQ_OWNED_CLOCK")?)?)?;}}
  if buy&&left>0{let owner=self.exposures.contains_key(&qid).then_some(qid);self.inventory.entry((ex.clone(),side.clone())).or_default().push_back(AcquisitionLot{quantity:left,price,observed:at,owner});if let Some(p)=self.exposures.get_mut(&qid){p.owned_remaining=p.owned_remaining.checked_add(left).ok_or("ACQ_RANGE")?;}}else if !buy{need(left==0,"ACQ_FIFO_MISSING")?;}
  for(s,native)in [("YES",moloch_replay::Side::Yes),("NO",moloch_replay::Side::No)]{let tracked=self.inventory.get(&(ex.clone(),s.into())).map(|ls|ls.iter().map(|l|l.quantity).sum::<u64>()).unwrap_or(0);need(tracked==bank.engine().wallet.quantity(&ex,native),"ACQ_NATIVE_FIFO_QUANTITY")?;}
  self.seen_fills.insert(key,sha);Ok(())
 }
 /// SAME original receipts already applied by the worker; never fetches a tape.
 /// Durable log.closed, exact committed terminal and the entire fill set agree.
 pub fn confirm_terminal(&mut self,bank:&Session,log:&DispatchLog,qid:u64,order:&Receipt,tape:&Receipt,at:u64)->Result<bool> {
  self.bind(bank)?;self.log(log)?;need(at>=self.last,"ACQ_CONFIRM_CLOCK")?;let Some(p)=self.exposures.get(&qid)else{return Ok(false)};if p.confirmed{return Ok(false)}
  need(log.closed.contains(&qid)&&log.attempted.contains(&qid)&&log.intents.get(&qid).is_some_and(|i|digest(i).ok().as_ref()==Some(&p.intent_sha))&&log.original_attempt(qid).is_some_and(|a|digest(a).ok().as_ref()==Some(&p.attempt_sha)),"ACQ_ORIGINAL_CLOSED")?;
  let ack:Receipt=serde_json::from_value(log.acknowledgments.get(&qid).ok_or("ACQ_ORIGINAL_ACK")?.clone()).map_err(|_|"ACQ_ACK_RECEIPT")?;let tid=intent_tournament(log,qid)?;let stamp=crate::daily_windows::iso(at).map_err(|_|"ACQ_CLOCK")?;
  need(p.attempt<=clock(&json!(ack.request_started_at))?&&clock(&json!(ack.request_started_at))?<=clock(&json!(ack.received_at))?&&clock(&json!(ack.received_at))?<=at,"ACQ_ORIGINAL_ACK_CLOCK")?;
  let acknowledged=if let Some(recovery)=log.recovered.get(&qid){let t:Receipt=serde_json::from_value(recovery["fills"].clone()).map_err(|_|"ACQ_RECOVERED_TAPE")?;need(t.endpoint==format!("/orders/{}/fills?limit=200",id(&recovery["actualOrderId"])?),"ACQ_RECOVERED_TAPE_ENDPOINT")?;feedback::recovered_acknowledgment(&p.quote,id(&recovery["actualOrderId"])?.as_str(),tid,&ack,&t,&stamp).map_err(|_|"ACQ_ORIGINAL_ACK")?}else{need(ack.endpoint=="/orders","ACQ_ACK_ENDPOINT")?;feedback::acknowledgment(&p.quote,&ack,&stamp).map_err(|_|"ACQ_ORIGINAL_ACK")?};
  let actual=id(&acknowledged["actualOrderId"])?;need(p.actual_id.as_ref()==Some(&actual)&&p.admitted==acknowledged["quantity"].as_u64()&&p.ack_receipt.as_ref()==Some(&feedback::digest(&ack).map_err(|_|"ACQ_RECEIPT_HASH")?)
   &&bank.engine().live_order_bindings().get(&qid)==Some(&actual)&&order.endpoint==format!("/orders/{actual}")&&tape.endpoint==format!("/orders/{actual}/fills?limit=200"),"ACQ_ORIGINAL_ACK_BINDING")?;
  for r in [order,tape]{let start=clock(&json!(r.request_started_at))?;let end=clock(&json!(r.received_at))?;need(r.status==200&&p.attempt<=start&&start<=end&&end<=at,"ACQ_ORIGINAL_READ_CLOCK")?;}
  let q=bank.engine().order_records().into_iter().find(|q|q["id"]==qid).ok_or("ACQ_NATIVE_QUOTE")?;let witness=p.terminal.as_ref().ok_or("ACQ_COMMITTED_TERMINAL_REQUIRED")?;
  let processed=witness["processedAt"].as_str().or_else(||witness["receivedAt"].as_str()).ok_or("ACQ_CLOCK")?;
  let terminal=feedback::terminal(&q,&actual,tid,order,tape,processed).map_err(|_|"ACQ_TERMINAL_RECEIPTS")?.ok_or("ACQ_ORDER_OPEN")?;need(&terminal==witness&&q["retired"]==true,"ACQ_TERMINAL_MEMBERSHIP")?;
  let rows=feedback::fills(&q,&actual,tid,tape,processed).map_err(|_|"ACQ_COMPLETE_TAPE")?;
  need(rows.len()==p.fills.len()&&rows.iter().all(|e|acquisition_fill_facts(e).ok().is_some_and(|facts|p.fills.get(&(actual.clone(),id(&e["fillId"]).unwrap_or_default())).is_some_and(|f|f.facts==facts))&&bank.engine().fills().iter().any(|f|native_fill_matches(f,e))),"ACQ_TAPE_FILL_SET")?;
  let filled=p.fills.values().map(|f|f.quantity).sum::<u64>();need(q["filled"]==filled&&terminal["filledQuantity"]==filled,"ACQ_NATIVE_FILL_TOTAL")?;
  let proof=digest(&json!({"intent":p.intent_sha,"attempt":p.attempt_sha,"orderReceipt":feedback::digest(order).map_err(|_|"ACQ_RECEIPT_HASH")?,"fillReceipt":feedback::digest(tape).map_err(|_|"ACQ_RECEIPT_HASH")?,"terminal":witness,"nativeHead":bank.durable_head(),"dispatch":log.descriptor_identity().map_err(|_|"ACQ_ORIGINAL_DISPATCH")?}))?;
  let class=if filled==0{0}else if filled==p.requested{2}else{1};let p=self.exposures.get_mut(&qid).unwrap();p.integrate(at)?;need(p.reserved==0,"ACQ_RESERVATION_NOT_RELEASED")?;p.confirmed=true;p.close_proof=Some(proof);p.evidence["cancelAttemptedAtConfirmation"]=json!(log.cancel_attempted(qid));p.evidence["cancelAcknowledgedAtConfirmation"]=json!(log.cancel_acknowledged.contains(&qid));self.completed[class]+=1;self.retained.push_back(qid);self.last=at;self.prune();Ok(true)
 }
 /// A positive SDK proof is separate from terminal-zero. It grants no retry and
 /// is never inferred from an error string, expiry, or an unverified JSON row.
 pub fn refresh_aborts(&mut self,bank:&Session,log:&DispatchLog,at:u64)->Result<()> {
  self.bind(bank)?;self.log(log)?;need(at>=self.last,"ACQ_OBSERVATION_CLOCK")?;
  let ids=self.exposures.iter().filter(|(id,p)|!p.aborted&&log.proved_no_network(**id)).map(|(id,_)|*id).collect::<Vec<_>>();
  for id in ids {let p=&self.exposures[&id];need(p.fills.is_empty()&&p.admitted.is_none()&&!log.acknowledgments.contains_key(&id)&&!bank.engine().live_order_bindings().contains_key(&id)&&log.aborted_records().contains_key(&id),"ACQ_ABORT_CONTRADICTION")?;self.reservation(bank,id,at)?;self.exposures.get_mut(&id).unwrap().aborted=true;self.retained.push_back(id);}
  self.last=at;self.prune();Ok(())
 }
 fn prune(&mut self) {while self.retained.len()>self.config.retained_completed{let id=self.retained.pop_front().unwrap();if self.exposures.get(&id).is_some_and(|p|p.owned_remaining==0&&p.reserved==0){self.exposures.remove(&id);}}
  let keep=self.retained.iter().copied().collect::<BTreeSet<_>>();self.exposures.retain(|id,p|(!p.confirmed&&!p.aborted)||p.owned_remaining>0||p.reserved>0||keep.contains(id));
 }
 pub fn outcome(&self,qid:u64,at:u64)->Result<AcquisitionOutcome> {
  need(at>=self.last,"ACQ_QUERY_CLOCK")?;let p=self.exposures.get(&qid).ok_or("ACQ_EXPOSURE")?;let filled=p.fills.values().map(|f|f.quantity).sum::<u64>();let valid=self.disabled.is_none();
  let reservation=plus(p.reservation_time,times(p.reserved,at.checked_sub(p.capital_at).ok_or("ACQ_CAPITAL_CLOCK")?)?)?;let mut owned=p.owned_closed_time;
  for l in self.inventory.values().flat_map(|ls|ls.iter()).filter(|l|l.owner==Some(qid)){owned=plus(owned,times(times(l.price,l.quantity)?,at.checked_sub(l.observed).ok_or("ACQ_OWNED_CLOCK")?)?)?;}
  Ok(AcquisitionOutcome{qid,class:if !valid{"observer_unavailable"}else if p.aborted{"proved_no_network"}else if !p.confirmed{if p.admitted.is_some(){"acknowledged_pending"}else{"unknown_or_pending_ack"}}else if filled==0{"confirmed_zero"}else if filled==p.requested{"confirmed_full"}else{"confirmed_partial"}.into(),requested_quantity:p.requested,native_reserved_quantity:p.native_reserved,admitted_quantity:p.admitted,observed_filled_quantity:filled,filled_fraction:(p.confirmed&&valid).then_some([filled,p.requested]),first_execution_latency_ms:p.fills.values().map(|f|f.executed-p.attempt).min(),first_observation_latency_ms:p.fills.values().map(|f|f.observed-p.attempt).min(),maximum_fill_observation_delay_ms:p.fills.values().map(|f|f.observed-f.executed).max(),attempt_at_ms:p.attempt,quote_created_at_ms:p.created,original_expiry_ms:p.expires,observed_through_ms:at,reservation_principal_time_unit_ms:reservation.to_string(),owned_principal_time_observed_unit_ms:owned.to_string(),owned_remaining_quantity:p.owned_remaining,reservation_released:p.reserved==0,acquisition_confirmed:p.confirmed&&valid,terminal_status:p.terminal.as_ref().and_then(|e|e["status"].as_str()).map(str::to_owned),cancellation_is_informative:true,original_evidence:p.evidence.clone(),close_proof_sha256:p.close_proof.clone(),fill_observations:p.fills.values().map(|f|json!({"facts":f.facts,"firstObservation":f.first_observation})).collect(),execution_authority:false})
 }
 pub fn diagnostic(&self,at:u64)->Result<Value> {
  need(at>=self.last,"ACQ_QUERY_CLOCK")?;let rows=self.exposures.keys().map(|qid|self.outcome(*qid,at)).collect::<Result<Vec<_>>>()?;
  Ok(json!({"version":1,"kind":"fundamental_acquisition_only","cohortId":self.config.cohort_id,"cohortSha256":self.cohort_sha,"startedAtMs":self.started,"initialNativeHead":self.initial_head,"bankManifestHash":self.manifest,"scopeSha256":digest(&self.scope)?,"disabled":self.disabled,"preCohortPendingUnscored":self.pre_cohort,"rows":rows,"completedZeroPartialFull":self.completed,"completedCountsUsable":self.disabled.is_none(),"principalTimeExcludesFees":true,"excludedOrdinary":self.excluded_ordinary,"duplicateEvents":self.duplicates,"sourceCursors":self.cursors,"cancellationIsInformative":true,"independentCensoringAssumed":false,"reservationClock":"native_committed_observation_steps_from_original_quote_creation","ownedCapitalClock":"native_committed_observation_FIFO_until_actual_close_or_truncated_at_query","restart":"new_named_cohort_prior_pending_unscored","priceValueLabelsCollected":false,"fittedModel":false,"executionAuthority":false}))
 }
}
thread_local!{static ACQUISITION_OBSERVER:std::cell::RefCell<Option<AcquisitionShadow>>=const{std::cell::RefCell::new(None)};}
pub fn initialize_acquisition_observer(bank:&Session,log:&DispatchLog,scope:&baal_transport_service::Scope,config:Option<AcquisitionConfig>,started:u64)->Result<()> {
 ACQUISITION_OBSERVER.with(|s|*s.borrow_mut()=None);let observer=config.map(|c|AcquisitionShadow::new(bank,log,scope,c,started)).transpose()?;ACQUISITION_OBSERVER.with(|s|*s.borrow_mut()=observer);Ok(())
}
fn acquisition_runtime(f:impl FnOnce(&mut AcquisitionShadow)->Result<()>) {ACQUISITION_OBSERVER.with(|s|{if let Some(m)=s.borrow_mut().as_mut(){if m.disabled.is_none(){if let Err(e)=f(m){m.disabled=Some(e);}}}});}
pub fn observe_acquisition_committed(bank:&Session,source:&str,sequence:u64,event:&Value){acquisition_runtime(|m|m.observe_committed(bank,source,sequence,event));}
pub fn track_acquisition_attempt(bank:&Session,log:&DispatchLog,qid:u64,at:u64){acquisition_runtime(|m|m.track_attempt(bank,log,qid,at).map(|_|()));}
pub fn confirm_acquisition_terminal(bank:&Session,log:&DispatchLog,qid:u64,order:&Receipt,tape:&Receipt,at:u64){acquisition_runtime(|m|m.confirm_terminal(bank,log,qid,order,tape,at).map(|_|()));}
pub fn acquisition_observer_diagnostic(bank:&Session,log:&DispatchLog,at:u64)->Option<Value> {
 ACQUISITION_OBSERVER.with(|s|{let mut s=s.borrow_mut();let m=s.as_mut()?;if m.disabled.is_none(){if let Err(e)=m.refresh_aborts(bank,log,at){m.disabled=Some(e);}}
  Some(m.diagnostic(at).unwrap_or_else(|e|json!({"state":"unavailable","reason":e,"executionAuthority":false})))})
}

/// Read-only identity of the actual configured observer, never imported fits.
/// The caller must still use estimate_runtime_action for current source proof.
pub fn runtime_configuration(bank:&Session)->Result<(Config,String)>{OBSERVER.with(|slot|{
 let observer=slot.borrow();let model=observer.as_ref().ok_or("DIST_RUNTIME_UNAVAILABLE")?;
 model.bind(bank)?;Ok((model.config.clone(),digest(&model.config)?))
})}
