//! Fixed contrasting mechanisms, independent of the frozen dip wallets.
//! No fitting, forecast calibration, account access or synthetic execution.
use crate::{publication::QuoteTick,quote_replay::{QuoteDeclaration,QuoteParameters},quote_markouts::{Episode,PricePath,statistics},quote_sides::no_quote,quote_endpoints,evaluation::Scenario};
use rayon::prelude::*;
use serde_json::{json,Value};
use std::{collections::{BTreeMap,VecDeque},time::Instant};
const LOOKBACK:i64=900_000;
const MINIMUM_SPAN:i64=840_000;
const MAXIMUM_GAP:i64=120_000;
const CHANGE_TWICE_MICROS:i128=40_000;
const HORIZONS:[i64;3]=[300_000,1_800_000,7_200_000];
#[derive(Clone)]
struct Spec{continuation:bool,horizon:i64,scenario:Scenario}
#[derive(Default)]
struct State{pending:Option<PricePath>,episodes:Vec<Episode>,features:Vec<Value>,next_signal:i64,reasons:BTreeMap<String,u64>}
impl State{
 fn finish(&mut self,status:Option<&str>){if let Some(p)=self.pending.take(){self.episodes.push(p.finish(status));}}
 fn count(&mut self,reason:&str){*self.reasons.entry(reason.into()).or_default()+=1;}
}
fn parameters(horizon:i64)->QuoteParameters{
 QuoteParameters{lookback_ms:LOOKBACK,minimum_minutes:10,minimum_span_ms:MINIMUM_SPAN,minimum_edge:"0.020000".into(),recovery_ticks:0,horizon_ms:horizon,take_profit:"0.010000".into()}
}
fn market_run(market:&str,ticks:&[QuoteTick],through:i64,specs:&[Spec],signal_after:Option<i64>)->Result<Vec<State>,String>{
 let mut states:Vec<State>=specs.iter().map(|_|State::default()).collect();
 let mut history=VecDeque::<&QuoteTick>::new();let mut previous:Option<&QuoteTick>=None;
 for q in ticks{
  if previous.is_some_and(|p|q.at-p.at>MAXIMUM_GAP||q.received-p.received>MAXIMUM_GAP){
   history.clear();for s in &mut states{s.finish(Some("observation_gap"));s.count("history_gap_reset");}
  }
  previous=Some(q);
  // Progress each path exactly once; missing quotes/deadlines retain its record.
  for s in &mut states{if let Some(p)=s.pending.as_mut(){if p.advance(q)?{s.finish(None);}}}
  let(Some(bid),Some(ask))=(q.bid,q.ask)else{history.clear();for s in &mut states{s.count("missing_quote");}continue;};
  history.push_back(q);while history.front().is_some_and(|p|p.at<q.at-LOOKBACK){history.pop_front();}
  let lag=history.front().unwrap();
  let minutes=history.iter().map(|q|q.at/60_000).collect::<std::collections::BTreeSet<_>>().len();
  if q.at-lag.at<MINIMUM_SPAN||minutes<10{for s in &mut states{s.count("insufficient_continuous_history");}continue;}
  let change=bid+ask-lag.bid.unwrap()-lag.ask.unwrap();
  for(s,spec)in states.iter_mut().zip(specs){
   s.count("window_ready");
   if signal_after.is_some_and(|cutoff|q.at<=cutoff||q.request<=cutoff){s.count("pre_cutoff");continue;}
   if s.pending.is_some()||q.at<s.next_signal{continue;}
   let triggered=if spec.continuation{change>=CHANGE_TWICE_MICROS}else{change<=-CHANGE_TWICE_MICROS};
   if !triggered{s.count("move_below_threshold");continue;}
   let(bid_move,ask_move)=(bid-lag.bid.unwrap(),ask-lag.ask.unwrap());
   let corroborated=if spec.continuation{bid_move>=CHANGE_TWICE_MICROS/2&&ask_move>=CHANGE_TWICE_MICROS/2}
    else{bid_move<=-CHANGE_TWICE_MICROS/2&&ask_move<=-CHANGE_TWICE_MICROS/2};
   if !corroborated{s.count("one_sided_quote_move");continue;}
   s.count("eligible_signal");
   s.next_signal=q.at+spec.scenario.latency_ms+spec.horizon+2*MAXIMUM_GAP;
   s.features.push(json!({"market":market,"signalSequence":q.sequence,"signalPublishedAt":q.at,"lagSequence":lag.sequence,
    "lagPublishedAt":lag.at,"lagRequestStartedAt":lag.request,"lagReceivedAt":lag.received,
    "lagBidMicros":lag.bid,"lagAskMicros":lag.ask,"midpointChangeTwiceMicros":change,
    "windowSamples":history.len(),"windowDistinctMinutes":minutes}));
   s.pending=Some(PricePath::new(market,q,lag.bid.unwrap(),&parameters(spec.horizon),&spec.scenario));
  }
 }
 for s in &mut states{if let Some(p)=s.pending.as_ref(){s.finish(Some(p.final_status(through)));}}
 Ok(states)
}
pub fn run(d:&QuoteDeclaration,ticks:&[QuoteTick],through:i64,workers:usize,signal_after:Option<i64>)->Result<Value,String>{
 if !(1..=32).contains(&workers){return Err("INVALID_REPLAY_WORKERS".into());}
 if signal_after.is_some_and(|cutoff|cutoff<0||cutoff>9_007_199_254_740_991){return Err("SIGNAL_CUTOFF_INVALID".into());}
 let started=Instant::now();let mut specs=Vec::new();
 for continuation in [true,false]{for horizon in HORIZONS{for scenario in &d.scenarios{
  specs.push(Spec{continuation,horizon,scenario:scenario.clone()});
 }}}
 let mut grouped=BTreeMap::<(String,String),Vec<QuoteTick>>::new();
 for q in ticks{grouped.entry(("YES".into(),q.market.clone())).or_default().push(q.clone());
  grouped.entry(("NO".into(),q.market.clone())).or_default().push(no_quote(q));}
 let groups:Vec<_>=grouped.into_iter().map(|(key,mut rows)|{rows.sort_by_key(|q|(q.at,q.sequence));(key,rows)}).collect();
 let pool=rayon::ThreadPoolBuilder::new().num_threads(workers).build().map_err(|_|"REPLAY_POOL_FAILED")?;
 let results=pool.install(||groups.par_iter().map(|((side,market),rows)|Ok((side.clone(),market_run(market,rows,through,&specs,signal_after)?))).collect::<Result<Vec<_>,String>>())?;
 let mut totals:BTreeMap<String,Vec<State>>=["YES","NO"].into_iter().map(|side|(side.into(),specs.iter().map(|_|State::default()).collect())).collect();
 for(side,states)in results{for(total,state)in totals.get_mut(&side).unwrap().iter_mut().zip(states){
  total.episodes.extend(state.episodes);total.features.extend(state.features);
  for(reason,count)in state.reasons{*total.reasons.entry(reason).or_default()+=count;}
 }}
 let mut sides=serde_json::Map::new();
 for(side,totals)in totals{
  let cards:Vec<_>=specs.iter().zip(totals).map(|(spec,state)|{
   let family=if spec.continuation{"continuation"}else{"reversal"};
   json!({"strategy":format!("trend15m_{}_h{}m",family,spec.horizon/60_000),
    "parameters":{"lookbackMs":LOOKBACK,"minimumSpanMs":MINIMUM_SPAN,"minimumDistinctMinutes":10,
      "minimumMidpointMove":"0.020000","minimumBidAndAskMove":"0.020000","horizonMs":spec.horizon,"fees":0},
    "scenario":spec.scenario,"featurePolicy":format!("continuous_midpoint_{}_v1",family),
    "summary":statistics(&state.episodes),"signalDiagnostics":state.reasons,
    "episodes":state.episodes,"causalSignalFeatures":state.features})
  }).collect();
  let strict=json!({"cards":cards});let no;
  let original=if side=="YES"{ticks}else{no=ticks.iter().map(no_quote).collect::<Vec<_>>();&no};
  let endpoints=quote_endpoints::run(original,&strict,through)?;
  sides.insert(side.to_lowercase(),json!({"strict":strict,"endpointSensitivity":endpoints}));
 }
 Ok(json!({"schemaVersion":1,"mode":"fixed_continuation_reversal_quote_study_v1","cohort":d.cohort,
  "sides":sides,"candidates":specs.len()*2,"quoteRows":ticks.len(),"markets":groups.len()/2,
  "workers":workers,"replayMs":started.elapsed().as_secs_f64()*1000.0,
  "signalAfter":signal_after,
  "fees":0,"networkRequests":0,"competitionOrders":0,"profitabilityEstablished":false,
  "interpretation":{"selection":"fixed_mechanisms_no_fit_or_best_case_promotion",
   "feature":"current_and_lag_midpoints_from_observed_continuous14_to15_minute_window",
   "corroboration":"bid_and_ask_must_both_move_same_direction_by_at_least0.020_no_spread_only_signal",
   "historyGaps":"publication_or_receipt_gap_over120s_clears_feature_history",
   "entry":"same_original_later_request_signal_ask_limit_and120s_deadline",
   "outcome":"same_original_later_request_fixed_horizon_exact_spread_slippage",
   "bookFeedBoundaries":"independent_quote_stream_not_depth_or_book_eligibility",
   "execution":"indicative_quote_prices_no_depth_queue_or_account_eligibility",
   "dataUse":"exploratory_earlier_data_future_validation_required",
   "cutoff":"optional_strict_publication_and_request_cutoff_caller_must_prove_prior_frozen_declaration",
   "samples":"contracts_cases_and_scenarios_correlated_not_independent_trades"}}))
}
