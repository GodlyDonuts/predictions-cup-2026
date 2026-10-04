//! Canonical contract-side projection, separate from REST fill-price conventions.
//! All clocks, original source identities and quantities are retained.
use crate::{publication::{MarketCapture,MarketEvent,QuoteTick},quote_replay::QuoteDeclaration,quote_markouts,types::{Book,Level}};
use serde_json::{json,Value};
use std::time::Instant;
const SCALE:i128=1_000_000;
pub fn no_quote(q:&QuoteTick)->QuoteTick{
 let mut no=q.clone();no.bid=q.ask.map(|ask|SCALE-ask);no.ask=q.bid.map(|bid|SCALE-bid);no
}
pub fn no_book(yes:&Book)->Result<Book,String>{
 if yes.instrument!=format!("{}:YES",yes.exchange_id)||yes.bids.is_empty()||yes.asks.is_empty(){return Err("CANONICAL_BOOK_IDENTITY_OR_DEPTH".into());}
 let validate=|levels:&[Level],ascending:bool|->Result<(),String>{let mut previous=None;
  for l in levels{if l.price<5000||l.price>995000||l.price%5000!=0||l.quantity<1||previous.is_some_and(|p|if ascending{l.price<=p}else{l.price>=p}){return Err("CANONICAL_BOOK_LADDER".into());}previous=Some(l.price);}Ok(())};
 validate(&yes.bids,false)?;validate(&yes.asks,true)?;if yes.bids[0].price>=yes.asks[0].price{return Err("CANONICAL_BOOK_CROSSED".into());}
 let mut no=yes.clone();no.instrument=format!("{}:NO",yes.exchange_id);
 no.bids=yes.asks.iter().map(|l|Level{price:SCALE-l.price,quantity:l.quantity}).collect();
 no.asks=yes.bids.iter().map(|l|Level{price:SCALE-l.price,quantity:l.quantity}).collect();Ok(no)
}
pub fn run(capture:&MarketCapture,d:&QuoteDeclaration,ticks:&[QuoteTick],workers:usize)->Result<Value,String>{
 if !(1..=32).contains(&workers){return Err("INVALID_REPLAY_WORKERS".into());}
 let started=Instant::now();let projected:Vec<_>=ticks.iter().map(no_quote).collect();
 let(mut book_events,mut minimum_no_ask_quantity,mut maximum_no_ask_quantity)=(0u64,None::<i64>,0i64);
 for e in &capture.events{if let MarketEvent::Book(b)=&e.event{let no=no_book(b)?;book_events+=1;let n=no.asks[0].quantity;minimum_no_ask_quantity=Some(minimum_no_ask_quantity.map_or(n,|old|old.min(n)));maximum_no_ask_quantity=maximum_no_ask_quantity.max(n);}}
 // Explicitly divide the requested compute budget between both side pools.
 let side_workers=(workers/2).max(1);
 let (yes,no)=if workers==1{(quote_markouts::run(capture,d,ticks,1),quote_markouts::run(capture,d,&projected,1))}
 else{rayon::join(||quote_markouts::run(capture,d,ticks,side_workers),||quote_markouts::run(capture,d,&projected,side_workers))};
 let yes=yes?;let no=no?;let candidates=yes["candidates"].as_u64().unwrap()+no["candidates"].as_u64().unwrap();
 let paths=yes["diagnosticPaths"].as_u64().unwrap()+no["diagnosticPaths"].as_u64().unwrap();
 Ok(json!({"schemaVersion":1,"mode":"canonical_two_side_quote_markouts_exploratory_v1","cohort":d.cohort,"yes":yes,"no":no,"candidates":candidates,"diagnosticPaths":paths,"workers":workers,"sideWorkers":side_workers,"replayMs":started.elapsed().as_secs_f64()*1000.0,
 "projection":{"noBid":"one_minus_yes_ask","noAsk":"one_minus_yes_bid","clocks":"exact_original_request_receipt_publication_and_sequence","depth":"not_available_in_quote_stream","priceBasis":"canonical_contract_side_not_rest_fill_basis","samples":"sides_cases_and_boundary_tracks_are_correlated","fees":0,"executionAuthority":false},"canonicalDepthChecks":{"sourceBookEvents":book_events,"noBooksDerivedAndValidated":book_events,"minimumTopNoAskShares":minimum_no_ask_quantity,"maximumTopNoAskShares":maximum_no_ask_quantity,"entryOrExitFillsSimulated":false},"networkRequests":0,"competitionOrders":0,"profitabilityEstablished":false}))
}
#[cfg(test)]
mod tests{
 use super::*;
 #[test]
 fn complement_swaps_ladders_preserves_quantity_and_exact_original_versions(){
  let yes=Book{market_id:"1".into(),instrument:"101:YES".into(),exchange_id:"101".into(),received_at:10001,request_started_at:9500,source_ns:10000000001,sequence:9007199254740991,
   bids:vec![Level{price:700000,quantity:333},Level{price:695000,quantity:222}],asks:vec![Level{price:710000,quantity:444},Level{price:715000,quantity:555}]};
  let no=no_book(&yes).unwrap();assert_eq!(no.instrument,"101:NO");assert_eq!(no.bids,vec![Level{price:290000,quantity:444},Level{price:285000,quantity:555}]);assert_eq!(no.asks,vec![Level{price:300000,quantity:333},Level{price:305000,quantity:222}]);assert!(no.same_version(&yes));assert_eq!(no.request_started_at,yes.request_started_at);assert_eq!(no.received_at,yes.received_at);
  let mut invalid=yes.clone();invalid.bids[0].quantity=0;assert!(no_book(&invalid).is_err());let mut invalid=yes;invalid.asks.reverse();assert!(no_book(&invalid).is_err());
 }
 #[test]
 fn quote_complement_is_involutive_and_never_invents_missing_prices(){
  let q=QuoteTick{market:"1".into(),at:100,request:80,received:99,bid:Some(700000),ask:Some(710000),sequence:9007199};
  let no=no_quote(&q);assert_eq!(no.bid,Some(290000));assert_eq!(no.ask,Some(300000));let restored=no_quote(&no);assert_eq!(restored.bid,q.bid);assert_eq!(restored.ask,q.ask);assert_eq!(restored.sequence,q.sequence);assert_eq!(restored.at,q.at);assert_eq!(restored.request,q.request);assert_eq!(restored.received,q.received);
  let missing=QuoteTick{bid:None,ask:None,..q};assert!(no_quote(&missing).bid.is_none());assert!(no_quote(&missing).ask.is_none());
 }
}
