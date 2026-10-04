//! Exploratory price diagnostics, deliberately separate from executable wallets.
//! Requests, availability, missing observations and source boundaries stay causal.
use crate::{money::{amount,TICK},publication::{MarketCapture,MarketEvent,QuoteTick},quote_replay::{QuoteDeclaration,QuoteParameters,StudyFeatures},evaluation::Scenario};
use rayon::prelude::*;
use serde::Serialize;
use serde_json::{json,Value};
use std::{collections::{BTreeMap,HashMap},time::Instant};
const WINDOW:i64=120_000;
#[derive(Clone)]
struct Spec{strategy:String,p:QuoteParameters,scenario:Scenario,corrected:bool,key:usize,book_boundary_guard:bool}
#[derive(Clone,Serialize)]
#[serde(rename_all="camelCase")]
struct Observation{published_at:i64,request_started_at:i64,received_at:i64,sequence:usize,bid:i128,ask:i128}
impl Observation{fn from(q:&QuoteTick)->Option<Self>{Some(Self{published_at:q.at,request_started_at:q.request,received_at:q.received,sequence:q.sequence,bid:q.bid?,ask:q.ask?})}}
#[derive(Serialize)]
#[serde(rename_all="camelCase")]
struct Episode{
 market:String,signal:Observation,reference_bid_micros:i128,status:String,
 entry:Option<Observation>,outcome:Option<Observation>,target_at:Option<i64>,
 midpoint_move_twice_micros:Option<i128>,spread_cost_twice_micros:Option<i128>,
 slippage_cost_micros:i128,quoted_net_markout_micros:Option<i128>,latency_ask_move_micros:Option<i128>,
 entry_price_rejections:u64,
}
impl Episode{
 fn new(m:&str,q:&QuoteTick,reference:i128,slippage:i128)->Self{Self{market:m.into(),signal:Observation::from(q).unwrap(),reference_bid_micros:reference,status:"pending".into(),entry:None,outcome:None,target_at:None,midpoint_move_twice_micros:None,spread_cost_twice_micros:None,slippage_cost_micros:slippage,quoted_net_markout_micros:None,latency_ask_move_micros:None,entry_price_rejections:0}}
 fn deadline(&self,s:&Spec)->i64{self.target_at.unwrap_or(self.signal.published_at+s.scenario.latency_ms)+WINDOW}
 fn advance(&mut self,q:&QuoteTick,s:&Spec)->Result<bool,String>{
  if q.at>self.deadline(s){self.status=if self.entry.is_some(){"outcome_not_observed"}else{"entry_not_observed"}.into();return Ok(true);}
  let Some(o)=Observation::from(q)else{self.status="missing_or_invalid_quote".into();return Ok(true);};
  if self.entry.is_none(){
   if o.request_started_at<self.signal.published_at+s.scenario.latency_ms{return Ok(false);}
   // Preserve the signal's ask limit instead of choosing a favourable future ask.
   if o.ask>self.signal.ask{self.entry_price_rejections+=1;return Ok(false);}
   self.latency_ask_move_micros=Some(o.ask-self.signal.ask);
   self.target_at=Some(o.published_at+s.p.horizon_ms);self.entry=Some(o);return Ok(false);
  }
  if o.request_started_at<self.target_at.unwrap(){return Ok(false);}
  let e=self.entry.as_ref().unwrap();
  // Twice-midpoints keep arithmetic exact even for half-micro prices.
  let move_twice=o.bid+o.ask-e.bid-e.ask;
  let spread_twice=e.ask-e.bid+o.ask-o.bid;
  let net=o.bid-e.ask-self.slippage_cost_micros;
  if move_twice-spread_twice-2*self.slippage_cost_micros!=2*net{return Err("MARKOUT_ECONOMICS_MISMATCH".into());}
  self.midpoint_move_twice_micros=Some(move_twice);self.spread_cost_twice_micros=Some(spread_twice);
  self.quoted_net_markout_micros=Some(net);self.outcome=Some(o);self.status="observed".into();Ok(true)
 }
}
#[derive(Default)]
struct State{pending:Option<Episode>,episodes:Vec<Episode>,next_signal:i64,reasons:BTreeMap<String,u64>}
impl State{
 fn finish(&mut self,status:&str){if let Some(mut e)=self.pending.take(){e.status=status.into();self.episodes.push(e);}}
 fn count(&mut self,reason:&str){*self.reasons.entry(reason.into()).or_default()+=1;}
}
struct MarketResult{states:Vec<State>,falling_rejections:u64}
fn market_run(m:&str,ticks:&[QuoteTick],boundaries:&[i64],through:i64,specs:&[Spec],features:&[(QuoteParameters,bool)])->Result<MarketResult,String>{
 let mut states:Vec<State>=specs.iter().map(|_|State::default()).collect();
 let mut history=StudyFeatures::default();let mut boundary_index=0;let mut barrier=i64::MIN;let mut last_receipt:Option<i64>=None;let mut falling_rejections=0;
 for q in ticks{
  // Boundary evidence has priority over quotes at the same publication clock.
  while boundary_index<boundaries.len()&&boundaries[boundary_index]<=q.at{
   barrier=boundaries[boundary_index];boundary_index+=1;
   for (state,spec) in states.iter_mut().zip(specs){if spec.book_boundary_guard{state.finish("source_boundary");}}
  }
  if last_receipt.is_some_and(|r|q.received-r>WINDOW){for s in &mut states{s.finish("observation_gap");}}
  last_receipt=Some(q.received);
  for (state,spec) in states.iter_mut().zip(specs){
   if spec.book_boundary_guard&&q.request<=barrier{continue;}
   if let Some(e)=state.pending.as_mut(){if e.advance(q,spec)?{let e=state.pending.take().unwrap();state.episodes.push(e);}}
  }
  history.observe(m,q.at,q.received,q.bid);
  // Inclusive availability at this quote, not a fictitious book request.
  let references=features.iter().map(|(p,v2)|history.signal(m,q.at,q.at+1,p,*v2)).collect::<Result<Vec<_>,_>>()?;
  falling_rejections+=references.iter().filter(|(_,r)|*r=="recoveryBidFalling").count() as u64;
  for (state,spec) in states.iter_mut().zip(specs){
   if spec.book_boundary_guard&&q.request<=barrier{state.count("request_before_boundary");continue;}
   let(reference,reason)=references[spec.key];state.count(reason);
   if state.pending.is_some()||q.at<state.next_signal{continue;}
   let(Some(reference),Some(ask))=(reference,q.ask)else{continue;};
   if reference-ask-2*TICK*spec.scenario.slippage_ticks<amount(&spec.p.minimum_edge)?{state.count("edge_below_threshold");continue;}
   state.count("eligible_signal");
   state.next_signal=q.at+spec.scenario.latency_ms+spec.p.horizon_ms+2*WINDOW;
   state.pending=Some(Episode::new(m,q,reference,2*TICK*spec.scenario.slippage_ticks));
  }
 }
 if boundaries[boundary_index..].iter().any(|at|*at<=through){for (state,spec) in states.iter_mut().zip(specs){if spec.book_boundary_guard{state.finish("source_boundary");}}}
 for (state,spec) in states.iter_mut().zip(specs){if let Some(e)=state.pending.as_ref(){let status=if through<=e.deadline(spec){"right_censored"}else if e.entry.is_some(){"outcome_not_observed"}else{"entry_not_observed"};state.finish(status);}}
 Ok(MarketResult{states,falling_rejections})
}
fn statistics(episodes:&[Episode])->Value{
 let mut statuses=BTreeMap::new();let mut values=Vec::new();let mut days=BTreeMap::<String,u64>::new();let mut markets=BTreeMap::<String,u64>::new();let(mut midpoint,mut spread,mut slip,mut latency)=(0i128,0i128,0i128,0i128);
 for e in episodes{*statuses.entry(e.status.clone()).or_insert(0u64)+=1;
  *days.entry(crate::engine::iso(e.signal.published_at)[..10].to_owned()).or_default()+=1;
  *markets.entry(e.market.clone()).or_default()+=1;
  if let Some(v)=e.quoted_net_markout_micros{values.push(v);midpoint+=e.midpoint_move_twice_micros.unwrap();spread+=e.spread_cost_twice_micros.unwrap();slip+=e.slippage_cost_micros;latency+=e.latency_ask_move_micros.unwrap();}
 }
 values.sort();let n=values.len();let mean=|total:i128|if n==0{Value::Null}else{json!(total as f64/n as f64/1_000_000.0)};
 let quantile=|p:f64|if n==0{Value::Null}else{json!(values[((n-1)as f64*p).round()as usize]as f64/1_000_000.0)};
 json!({"episodes":episodes.len(),"observed":n,"statusCounts":statuses,"signalDays":days,"signalMarkets":markets,
 "quotedNetMarkout":{"mean":mean(values.iter().sum()),"median":quantile(0.5),"p10":quantile(0.1),"p90":quantile(0.9),"positive":values.iter().filter(|v|**v>0).count(),"negative":values.iter().filter(|v|**v<0).count(),"flat":values.iter().filter(|v|**v==0).count()},
 "meanMidpointMove":if n==0{Value::Null}else{json!(midpoint as f64/n as f64/2_000_000.0)},"meanSpreadCost":if n==0{Value::Null}else{json!(spread as f64/n as f64/2_000_000.0)},"meanSlippageCost":mean(slip),"meanLatencyAskMove":mean(latency),"profitabilityEstablished":false})
}
pub fn run(capture:&MarketCapture,d:&QuoteDeclaration,ticks:&[QuoteTick],workers:usize)->Result<Value,String>{
 let started=Instant::now();if !(1..=32).contains(&workers){return Err("INVALID_REPLAY_WORKERS".into());}
 let mut feature_keys=HashMap::new();let mut features=Vec::new();let mut specs=Vec::new();
 for s in &d.strategies{for corrected in [false,true]{if corrected&&s.parameters.recovery_ticks==0{continue;}
  let p=&s.parameters;let key=(p.lookback_ms,p.minimum_minutes,p.minimum_span_ms,p.recovery_ticks,corrected);
  let i=*feature_keys.entry(key).or_insert_with(||{let i=features.len();features.push((p.clone(),corrected));i});
  for scenario in &d.scenarios{for book_boundary_guard in [false,true]{specs.push(Spec{strategy:s.id.clone(),p:p.clone(),scenario:scenario.clone(),corrected,key:i,book_boundary_guard});}}
 }}
 let mut groups=BTreeMap::<String,Vec<QuoteTick>>::new();for q in ticks{groups.entry(q.market.clone()).or_default().push(q.clone());}
 let mut global_boundaries=Vec::new();let mut local_boundaries=HashMap::<String,Vec<i64>>::new();
 for e in &capture.events{if let MarketEvent::Boundary(m)=&e.event{if let Some(m)=m{local_boundaries.entry(m.clone()).or_default().push(e.at);}else{global_boundaries.push(e.at);}}}
 let through=capture.meta["through"].as_i64().ok_or("MARKOUT_THROUGH_MISSING")?;
 let groups:Vec<_>=groups.into_iter().map(|(m,mut q)|{q.sort_by_key(|q|(q.at,q.sequence));let mut b=global_boundaries.clone();if let Some(local)=local_boundaries.get(&m){b.extend(local);}b.sort();(m,q,b)}).collect();
 let pool=rayon::ThreadPoolBuilder::new().num_threads(workers).build().map_err(|_|"REPLAY_POOL_FAILED")?;
 let results=pool.install(||groups.par_iter().map(|(m,q,b)|market_run(m,q,b,through,&specs,&features)).collect::<Result<Vec<_>,_>>())?;
 let falling_rejections:u64=results.iter().map(|r|r.falling_rejections).sum();
 let mut totals:Vec<State>=specs.iter().map(|_|State::default()).collect();
 for r in results{for(total,state)in totals.iter_mut().zip(r.states){total.episodes.extend(state.episodes);for(reason,n)in state.reasons{*total.reasons.entry(reason).or_default()+=n;}}}
 let(mut cards,mut sensitivity)=(Vec::new(),Vec::new());
 for (s,mut state) in specs.iter().zip(totals){state.episodes.sort_by_key(|e|(e.signal.published_at,e.market.clone()));
  let card=json!({"strategy":s.strategy,"parameters":s.p,"scenario":s.scenario,"featurePolicy":if s.corrected{"prior_distinct_minute_nonfalling_v2"}else{"original_self_comparison_v1"},"boundaryPolicy":if s.book_boundary_guard{"book_feed_boundary_censoring_sensitivity"}else{"independent_quote_stream"},"summary":statistics(&state.episodes),"signalDiagnostics":state.reasons,"episodes":state.episodes});
  if s.book_boundary_guard{sensitivity.push(card);}else{cards.push(card);}
 }
 Ok(json!({"schemaVersion":2,"mode":"chronological_quote_markouts_exploratory_v2","cohort":d.cohort,"candidates":cards.len(),"diagnosticPaths":specs.len(),"cards":cards,"bookBoundarySensitivityCards":sensitivity,"markets":groups.len(),"quoteRows":ticks.len(),"distinctFeaturePasses":features.len(),"fallingReboundFeatureRejections":falling_rejections,"workers":workers,"replayMs":started.elapsed().as_secs_f64()*1000.0,"fees":0,"networkRequests":0,"competitionOrders":0,"profitabilityEstablished":false,
 "interpretation":{"dataUse":"reused_exploratory_development_data","execution":"indicative_quotes_only_no_depth_or_account_eligibility","exit":"first_independent_request_at_fixed_horizon_no_take_profit_or_stop_loss","entry":"first_independent_request_inside_120s_and_signal_ask_limit","nonOverlap":"per_market_case_signal_plus_latency_plus_horizon_plus_240s","outcomeLatenessMs":WINDOW,"maximumObservationGapMs":WINDOW,"quoteAvailability":"publication_inclusive","boundaryPolicy":"independent_verified_quote_stream_not_censored_by_other_feed_restarts","boundarySensitivity":"separate_cards_cancel_on_book_feed_boundary_before_same_clock_quote","historyAcrossBoundary":"independent_quote_history_retained","depthFraction":"declared_but_not_applied_without_quote_size","samples":"cases_scenarios_and_boundary_tracks_share_correlated_opportunities_not_independent_trades","selection":"no_fit_or_winner_selection","censoring":"all_started_episodes_retained_with_missing_or_right_censored_status"}}))
}
#[cfg(test)]
mod tests{
 use super::*;
 fn spec()->Spec{Spec{strategy:"test".into(),p:QuoteParameters{lookback_ms:3600000,minimum_minutes:30,minimum_span_ms:1800000,minimum_edge:"0.020000".into(),recovery_ticks:0,horizon_ms:1800000,take_profit:"0.010000".into()},scenario:Scenario{id:"base".into(),latency_ms:3000,slippage_ticks:1,depth_fraction_bps:100},corrected:false,key:0,book_boundary_guard:false}}
 fn tick(at:i64,request:i64,bid:i128,ask:i128)->QuoteTick{QuoteTick{market:"1".into(),at,request,received:at,bid:Some(bid),ask:Some(ask),sequence:1}}
 #[test]
 fn independent_requests_limit_and_fixed_horizon_reconcile_exact_costs(){
  let s=spec();let q=tick(0,-100,800000,805000);let mut e=Episode::new("1",&q,900000,10000);
  assert!(!e.advance(&tick(4000,2000,800000,805000),&s).unwrap());assert!(e.entry.is_none());
  assert!(!e.advance(&tick(5000,4000,805000,810000),&s).unwrap());assert_eq!(e.entry_price_rejections,1);
  assert!(!e.advance(&tick(6000,5000,800000,805000),&s).unwrap());assert_eq!(e.target_at,Some(1806000));
  assert!(!e.advance(&tick(1807000,1805000,900000,905000),&s).unwrap());assert!(e.outcome.is_none());
  assert!(e.advance(&tick(1808000,1807000,820000,825000),&s).unwrap());
  assert_eq!(e.quoted_net_markout_micros,Some(5000));assert_eq!(e.midpoint_move_twice_micros,Some(40000));assert_eq!(e.spread_cost_twice_micros,Some(10000));
 }
 #[test]
 fn missing_quotes_and_elapsed_deadlines_remain_in_denominator(){
  let s=spec();let q=tick(0,-100,800000,805000);let mut e=Episode::new("1",&q,900000,10000);let mut missing=tick(4000,3500,800000,805000);missing.bid=None;
  assert!(e.advance(&missing,&s).unwrap());assert_eq!(e.status,"missing_or_invalid_quote");assert!(e.quoted_net_markout_micros.is_none());
  let mut e=Episode::new("1",&q,900000,10000);assert!(e.advance(&tick(123001,123000,800000,805000),&s).unwrap());assert_eq!(e.status,"entry_not_observed");
 }
 #[test]
 fn episodes_are_nonoverlapping_and_boundary_has_same_clock_priority(){
  let s=spec();let f=vec![(s.p.clone(),false)];let mut q:Vec<_>=(0..40).map(|i|tick(i*60000,i*60000-100,900000,905000)).collect();
  q.extend((40..77).map(|i|{let bid=if i<=41||i>=75{800000}else{900000};tick(i*60000,i*60000-100,bid,bid+5000)}));
  let r=market_run("1",&q,&[],77*60000,&[s.clone()],&f).unwrap();assert_eq!(r.states[0].episodes.len(),2);assert_eq!(r.states[0].episodes[0].status,"observed");assert_eq!(r.states[0].episodes[1].status,"right_censored");
  let mut guarded=s.clone();guarded.book_boundary_guard=true;
  let r=market_run("1",&q,&[41*60000],77*60000,&[s,guarded],&f).unwrap();assert_eq!(r.states[0].episodes[0].status,"observed");assert_eq!(r.states[1].episodes[0].status,"source_boundary");assert_eq!(r.states[1].reasons["request_before_boundary"],1);
 }
}
