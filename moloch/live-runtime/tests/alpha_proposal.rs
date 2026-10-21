use moloch_live_runtime::alpha_proposal::*;
use baal_transport_service::Scope;
use moloch_replay::{session::Session,Side};
use serde_json::{json,Value};
use std::{path::PathBuf,sync::atomic::{AtomicU64,Ordering}};
const NOW:u64=1_792_022_400_000;
fn iso(t:u64)->String{chrono::DateTime::from_timestamp_millis(t as i64).unwrap().to_rfc3339_opts(chrono::SecondsFormat::Millis,true)}
struct F{root:PathBuf,bank:Option<Session>,scope:Scope}
impl Drop for F{fn drop(&mut self){self.bank.take();let _=std::fs::remove_dir_all(&self.root);}}
impl F{
 fn new(changes:Value)->Self{static NEXT:AtomicU64=AtomicU64::new(0);let root=std::env::temp_dir().join(format!("alpha-proposal-{}-{}-{}",std::process::id(),std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos(),NEXT.fetch_add(1,Ordering::Relaxed)));
 let mut policy=json!({"initialCash":"100.000000","quantity":100,"maximumCommitted":"100.000000","maximumMarketCost":"100.000000","requireCurrentFlow":true,"entryStopAt":iso(NOW+600000)});for(k,v)in changes.as_object().unwrap(){policy[k]=v.clone();}
 let seed=json!({"runId":"alpha-fixture","availableCash":"100.000000","accountSnapshotHash":"a".repeat(64),"observedAt":iso(NOW-100000)});let bank=Session::open_live(&root,&policy,&seed).unwrap();let scope=Scope{tournament_id:"cup".into(),tournament_slug:"cup".into(),exchanges:[("101".into(),"201".into())].into()};let mut f=Self{root,bank:Some(bank),scope};f.book(NOW,1);f}
 fn b(&self)->&Session{self.bank.as_ref().unwrap()}
 fn book(&mut self,at:u64,seq:u64){self.bank.as_mut().unwrap().accept("books",seq,&json!({"type":"book","receivedAt":iso(at),"book":{"marketId":"201","exchangeId":"101","tournamentId":"cup","sequence":seq.to_string(),"sourceAt":iso(at),"requestStartedAt":iso(at),"receivedAt":iso(at),"bids":[{"price":"0.400000","quantity":4},{"price":"0.395000","quantity":20}],"asks":[{"price":"0.420000","quantity":2},{"price":"0.430000","quantity":3},{"price":"0.450000","quantity":20}]}})).unwrap();}
 fn r(&self)->ReviewBounds{ReviewBounds{exchange_id:"101".into(),market_id:"201".into(),side:Side::Yes,review_sha256:"a".repeat(64),model_sha256:"b".repeat(64),source_sha256:"c".repeat(64),reviewed_at_ms:NOW-100,valid_until_ms:NOW+60000,horizon_ms:60000,expected_payout:600000,acquisition_ceiling:440000,maximum_principal:100_000_000,maximum_quantity:5,modes:vec![Mode::JoinBid,Mode::ImproveOneTick,Mode::BoundedLimitMayRest]}}
}
#[test]fn three_modes_use_actual_full_depth_without_claiming_fill_probability(){let f=F::new(json!({}));let head=f.b().durable_head();let report=f.b().engine().report().unwrap();let x=compare(f.b(),&f.scope,&f.r(),NOW).unwrap();assert_eq!(x.candidates.len(),3);let a=&x.candidates;assert_eq!((a[0].limit_price(),a[1].limit_price(),a[2].limit_price()),(400000,405000,430000));assert_eq!(a[0].immediate_acquisition_quantity(),0);assert_eq!(a[1].immediate_acquisition_quantity(),0);assert_eq!(a[2].immediate_acquisition_quantity(),5);assert_eq!(a[2].visible_acquisition_notional(),2*420000+3*430000);assert_eq!(a[2].gross_funding(),5*430000);assert_eq!(a[2].visible_liquidation_notional(),4*400000+395000);assert_eq!(a[2].conditional_forecast_surplus(),5*170000);assert!(a[2].provenance()["executionProbability"].is_null());assert_eq!(f.b().durable_head(),head);assert_eq!(f.b().engine().report().unwrap(),report);assert!(f.b().engine().fills().is_empty());}
#[test]fn cached_comparison_preserves_original_report_digest_and_refreshes_after_real_book(){
 use sha2::{Digest,Sha256};
 let mut f=F::new(json!({}));let mut previous=None;
 for (at,seq) in [(NOW,1),(NOW+1,2)]{
  if seq>1{f.book(at,seq);}
  let head=f.b().durable_head();let raw=std::fs::read(f.root.join("events.jsonl")).unwrap();
  let expected=format!("{:x}",Sha256::digest(serde_json::to_vec(&f.b().engine().report().unwrap()).unwrap()));
  if let Some(old)=previous{assert_ne!(old,expected);}
  for _ in 0..3{
   let x=compare(f.b(),&f.scope,&f.r(),at).unwrap();assert_eq!(x.candidates.len(),3);
   for candidate in x.candidates{assert_eq!(candidate.provenance()["reportSha256"],expected);assert_eq!(candidate.provenance()["bankHead"],head);}
  }
  assert_eq!(f.b().durable_head(),head);assert_eq!(std::fs::read(f.root.join("events.jsonl")).unwrap(),raw);previous=Some(expected);
 }
}
#[test]fn reviewed_ceiling_never_uses_deeper_expensive_asks(){let f=F::new(json!({}));let mut r=f.r();r.maximum_quantity=100;let c=compare(f.b(),&f.scope,&r,NOW).unwrap();let a=&c.candidates[2];assert_eq!(a.quantity(),5);assert_eq!(a.limit_price(),430000);}
#[test]fn no_side_uses_complemented_depth_in_correct_order(){let f=F::new(json!({}));let mut r=f.r();r.side=Side::No;r.expected_payout=800000;r.acquisition_ceiling=610000;let x=compare(f.b(),&f.scope,&r,NOW).unwrap();assert_eq!(x.candidates[0].limit_price(),580000);assert_eq!(x.candidates[1].limit_price(),585000);assert_eq!(x.candidates[2].limit_price(),605000);assert_eq!(x.candidates[2].visible_acquisition_notional(),4*600000+605000);assert_eq!(x.candidates[2].action()["side"],"no");}
#[test]fn funding_charges_entire_limit_not_weighted_visible_cost(){let f=F::new(json!({}));let mut r=f.r();r.maximum_principal=2_100_000;let x=compare(f.b(),&f.scope,&r,NOW).unwrap();let a=&x.candidates[2];assert_eq!(a.quantity(),4);assert_eq!(a.gross_funding(),1_720_000);assert!(a.gross_funding()<=r.maximum_principal);}
#[test]fn bounded_marginal_limit_funds_the_largest_actual_ask_prefix(){
 let f=F::new(json!({}));let head=f.b().durable_head();let report=f.b().engine().report().unwrap();
 for(budget,n,price)in [(2_160_000,5,430_000),(2_150_000,5,430_000),(2_149_999,4,430_000),(2_100_000,4,430_000),(840_000,2,420_000),(430_000,1,420_000)]{
  let mut r=f.r();r.maximum_principal=budget;r.modes=vec![Mode::BoundedLimitMayRest];let x=compare(f.b(),&f.scope,&r,NOW).unwrap();let a=&x.candidates[0];assert_eq!((a.quantity(),a.limit_price()),(n,price));assert_eq!(a.gross_funding(),n as i128*price);assert!(a.gross_funding()<=budget);assert_eq!(a.immediate_acquisition_quantity(),n);assert!(a.provenance()["executionProbability"].is_null());
 }
 assert_eq!(f.b().durable_head(),head);assert_eq!(f.b().engine().report().unwrap(),report);assert!(f.b().engine().fills().is_empty());
}
#[test]fn bounded_prefix_never_substitutes_weighted_cost_or_a_deeper_unaffordable_limit(){
 let f=F::new(json!({}));let mut r=f.r();r.modes=vec![Mode::BoundedLimitMayRest];r.maximum_principal=2_140_000;
 let x=compare(f.b(),&f.scope,&r,NOW).unwrap();assert_eq!(x.candidates[0].quantity(),4,"five visible shares cost2.13, but their full .43 limit requires2.15");
 r.maximum_principal=2_160_000;r.maximum_quantity=100;r.acquisition_ceiling=450_000;let x=compare(f.b(),&f.scope,&r,NOW).unwrap();assert_eq!((x.candidates[0].quantity(),x.candidates[0].limit_price()),(5,430_000),"the .45 level cannot fund five shares; keep the feasible .43 prefix");
 r.acquisition_ceiling=425_000;let x=compare(f.b(),&f.scope,&r,NOW).unwrap();assert_eq!((x.candidates[0].quantity(),x.candidates[0].limit_price()),(2,420_000),"original ceiling excludes the .43 and .45 asks");
 r.maximum_principal=419_999;let x=compare(f.b(),&f.scope,&r,NOW).unwrap();assert!(x.candidates.is_empty());assert_eq!(x.deferred[0].reason,"insufficient_fully_funded_capital");
}
#[test]fn bounded_marginal_sizing_preserves_complemented_no_side_full_limit_funding(){
 let f=F::new(json!({}));for(budget,n,price)in [(3_025_000,5,605_000),(3_024_999,4,600_000)]{let mut r=f.r();r.side=Side::No;r.expected_payout=800_000;r.acquisition_ceiling=610_000;r.maximum_principal=budget;r.modes=vec![Mode::BoundedLimitMayRest];let x=compare(f.b(),&f.scope,&r,NOW).unwrap();let a=&x.candidates[0];assert_eq!((a.quantity(),a.limit_price()),(n,price));assert_eq!(a.gross_funding(),n as i128*price);assert!(a.gross_funding()<=budget);assert_eq!(a.action()["side"],"no");}
}
#[test]fn bounded_prefix_keeps_native_review_and_liquidation_quantity_caps(){
 for(native,review,ceiling,expected,price)in [(2,100,450_000,2,420_000),(100,1,450_000,1,420_000),(100,100,450_000,24,450_000),(100,100,440_000,5,430_000)]{let f=F::new(json!({"quantity":native}));let mut r=f.r();r.maximum_quantity=review;r.acquisition_ceiling=ceiling;r.modes=vec![Mode::BoundedLimitMayRest];let x=compare(f.b(),&f.scope,&r,NOW).unwrap();assert_eq!((x.candidates[0].quantity(),x.candidates[0].limit_price()),(expected,price),"equal quantity keeps the earlier, lower marginal limit");}
}
#[test]fn review_and_native_quantity_caps_are_both_enforced(){let f=F::new(json!({"quantity":2}));let mut r=f.r();r.maximum_quantity=1;assert!(compare(f.b(),&f.scope,&r,NOW).unwrap().candidates.iter().all(|c|c.quantity()==1));r.maximum_quantity=10;assert!(compare(f.b(),&f.scope,&r,NOW).unwrap().candidates.iter().all(|c|c.quantity()<=2));}
#[test]fn current_actual_cash_total_and_per_market_caps_bound_candidates(){for changes in [json!({"initialCash":"0.500000"}),json!({"maximumCommitted":"0.500000"}),json!({"maximumMarketCost":"0.500000"})]{let f=F::new(changes);let x=compare(f.b(),&f.scope,&f.r(),NOW).unwrap();assert!(!x.candidates.is_empty());assert!(x.candidates.iter().all(|c|c.gross_funding()<=500000));}}
#[test]fn stale_future_and_gapped_books_cannot_be_candidates(){let mut f=F::new(json!({}));assert!(compare(f.b(),&f.scope,&f.r(),NOW+10001).unwrap().candidates.is_empty());f.book(NOW+1000,2);assert!(compare(f.b(),&f.scope,&f.r(),NOW).unwrap().candidates.is_empty());f.bank.as_mut().unwrap().accept("gap",1,&json!({"type":"gap","receivedAt":iso(NOW+1001),"exchangeIds":["101"],"reason":"fixture"})).unwrap();assert!(compare(f.b(),&f.scope,&f.r(),NOW+1001).unwrap().candidates.is_empty());}
#[test]fn expired_or_not_yet_reviewed_is_explicitly_deferred(){let f=F::new(json!({}));let mut r=f.r();r.valid_until_ms=NOW;assert_eq!(compare(f.b(),&f.scope,&r,NOW).unwrap().deferred[0].reason,"review_not_current");r.valid_until_ms=NOW+10000;r.reviewed_at_ms=NOW+1;assert!(compare(f.b(),&f.scope,&r,NOW).unwrap().candidates.is_empty());}
#[test]fn modes_are_reviewed_and_duplicate_modes_rejected(){let f=F::new(json!({}));let mut r=f.r();r.modes=vec![Mode::JoinBid];let x=compare(f.b(),&f.scope,&r,NOW).unwrap();assert_eq!(x.candidates.len(),1);r.modes.push(Mode::JoinBid);assert!(compare(f.b(),&f.scope,&r,NOW).is_err());}
#[test]fn foreign_scope_bad_hash_and_unsupported_fees_cannot_generate_action(){let f=F::new(json!({}));let mut r=f.r();r.market_id="999".into();assert!(compare(f.b(),&f.scope,&r,NOW).is_err());r=f.r();r.review_sha256="fabricated".into();assert!(compare(f.b(),&f.scope,&r,NOW).is_err());let f=F::new(json!({"feeBps":1}));assert_eq!(compare(f.b(),&f.scope,&f.r(),NOW).unwrap().deferred[0].reason,"unsupported_nonzero_fees");}
#[test]fn no_positive_value_or_executable_ask_is_not_invented(){let f=F::new(json!({}));let mut r=f.r();r.expected_payout=400000;assert!(compare(f.b(),&f.scope,&r,NOW).unwrap().candidates.is_empty());r=f.r();r.acquisition_ceiling=410000;let x=compare(f.b(),&f.scope,&r,NOW).unwrap();assert!(x.deferred.iter().any(|d|d.reason=="no_ask_depth_within_ceiling"));}
#[test]fn exact_source_and_head_are_immutable_provenance_not_dispatch_authority(){let mut f=F::new(json!({}));let x=compare(f.b(),&f.scope,&f.r(),NOW).unwrap();let p=x.candidates[0].provenance().clone();assert_eq!(p["bankHead"],f.b().durable_head());assert_eq!(x.candidates[0].valid_until_ms(),NOW+10000);f.book(NOW+1,2);assert_ne!(p["bankHead"],f.b().durable_head());assert_eq!(x.candidates[0].provenance(),&p);}
#[test]fn existing_actual_native_reservations_reduce_available_capital(){let f=F::new(json!({"requireCurrentFlow":false,"quantity":1000,"maximumCommitted":"1.000000"}));let report=f.b().engine().report().unwrap();let reserved=baal_transport_service::decimal_units(report["reservedCash"].as_str().unwrap()).unwrap();assert!(reserved>0);let x=compare(f.b(),&f.scope,&f.r(),NOW).unwrap();for c in x.candidates{assert!(c.gross_funding()+reserved<=1_000_000);}}
#[test]fn one_tick_spread_defers_improvement_instead_of_crossing(){let mut f=F::new(json!({}));let mut raw=f.b().engine().live_package_books()[0].clone();raw["sequence"]=json!("2");raw["asks"][0]["price"]=json!("0.405000");f.bank.as_mut().unwrap().accept("books",2,&json!({"type":"book","receivedAt":iso(NOW),"book":raw})).unwrap();let x=compare(f.b(),&f.scope,&f.r(),NOW).unwrap();assert!(x.deferred.iter().any(|d|d.mode==Mode::ImproveOneTick&&d.reason=="no_inside_tick"));assert!(!x.candidates.iter().any(|c|c.mode()==Mode::ImproveOneTick));}
