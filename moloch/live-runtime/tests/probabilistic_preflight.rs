#![cfg(unix)]
//! Actual Session/opaque stage, scripted original reads only; never account HTTP.
use moloch_live_runtime::{feedback::Result,package_preflight,package_worker as base,probabilistic_portfolio_worker as worker,probabilistic_portfolio as prob,package_authority::{Prepared,PackageSnapshotSet},package_dispatch::PackageDispatchLog,dispatch::DispatchLog,package_not_sent_evidence::VerifiedNotSent};
use baal_transport_service::{Scope,Receipt,Endpoint,Permit,multi_leg,multi_leg_portfolio as wire,admission::{BindingLookup,ImmutableBinding}};
use moloch_replay::session::Session;
use std::path::{Path,PathBuf};
use serde_json::{Value,json};
#[path="support/probabilistic_fixture.rs"]mod fixture;
use fixture::{F,NOW,T,iso};
struct Reads{scope:Scope,clock:u64,step:u64,facts:PackageSnapshotSet,ordinary:DispatchLog,activation:moloch_live_runtime::authority::Activation,dir:PathBuf,reads:Vec<Receipt>,fail_at:usize,bad_at:usize,bad:&'static str,prepared:bool}
impl base::Backend for Reads{
 fn now_ms(&self)->u64{self.clock}fn scope(&self)->&Scope{&self.scope}fn account_scope(&self)->&str{"fixture-no-account-client"}fn evidence_directory(&self)->&Path{&self.dir}
 fn quiet(&self)->Result<()>{self.ordinary.require_resolved_attempts()}
 fn lookup(&self,intent:&str)->Result<BindingLookup>{Ok(BindingLookup{lookup_schema_version:1,intent:intent.into(),binding:None,unknown_binding:None,global_unknown_count:0,has_unknown:false})}
 fn prepare(&mut self,_:&Session,_:&moloch_live_runtime::package_admission::Admission,_:Permit,_:PackageSnapshotSet)->Result<()>{panic!("legacy preparation forbidden")}
 fn clear_ready(&mut self){}
 fn send(&mut self,_:multi_leg::Basket,_:Permit)->baal_transport_service::Result<multi_leg::Outcome>{panic!("no physical legacy method")}
 fn take_not_sent(&mut self)->Result<VerifiedNotSent>{panic!("no SDK method")}
 fn retained_not_sent(&mut self,_:&ImmutableBinding,_:&str,_:u64)->Result<VerifiedNotSent>{panic!("no SDK method")}
 fn bind_owned(&mut self,_:&str){panic!("no actual order")}
 fn read(&mut self,e:Endpoint)->Result<Receipt>{
  let mut r=match e{Endpoint::Tournament=>self.facts.account.clone(),Endpoint::Account=>self.facts.cash_balance.clone(),Endpoint::Positions=>self.facts.positions.clone(),Endpoint::Orders{status}=>{assert_eq!(status,"open");self.facts.open_orders.clone()},Endpoint::Market{market_id}=>self.component(&market_id).market.clone(),Endpoint::Nodes{market_id}=>self.component(&market_id).contract.clone(),Endpoint::OrderBook{market_id,depth}=>{assert_eq!(depth,20);self.component(&market_id).book.clone()},_=>panic!("unexpected endpoint")};
  r.request_started_at=iso(self.clock);self.clock+=self.step;r.received_at=iso(self.clock);
  let n=self.reads.len()+1;if n==self.bad_at{match self.bad{"endpoint"=>r.endpoint.push_str("&unverified=1"),"future"=>r.received_at=iso(self.clock+1),"coverage"=>r.data["coverage"]["complete"]=json!(false),_=>panic!("unknown mutation")}}
  self.reads.push(r.clone());if n==self.fail_at{Err("original fixture transport failure".into())}else{Ok(r)}
 }
}
impl Reads{fn component(&self,market:&str)->&moloch_live_runtime::package_authority::ComponentSnapshots{let(ex,_)=self.scope.exchanges.iter().find(|(_,m)|m.as_str()==market).unwrap();&self.facts.components[ex]}}
impl worker::Backend for Reads{
 fn ordinary_dispatch(&self)->&DispatchLog{&self.ordinary}
 fn prepare_probabilistic(&mut self,bank:&Session,p:&prob::Plan,r:&prob::Reservation,b:&wire::Basket,permit:Permit,facts:PackageSnapshotSet)->Result<()>{
  let _actual=Prepared::new_probabilistic(bank,&self.activation,&self.scope,p,r,b,permit,facts,self.clock,self.activation.entry_until_ms)?;self.prepared=true;
  // Deliberate fixture stop AFTER genuine authority construction but BEFORE a
  // durable physical attempt; coordinator must fence/release without any SDK.
  Err("fixture stops after genuine authority verification".into())
 }
 fn send_portfolio(&mut self,_:wire::Basket,_:Permit)->baal_transport_service::Result<wire::Outcome>{panic!("no physical SDK method")}
}
fn setup()->(F,Reads,PackageDispatchLog,worker::Staged){
 let mut f=F::with_tournament(T);fixture::qualify(&mut f);fixture::held(&mut f,0,5,0.8);
 let ordinary=DispatchLog::open(&f.root.join("preflight-original-ordinary")).unwrap();let surface=f.bound(NOW).unwrap();let semantic=fixture::semantics(&f,&surface);let policy=fixture::policy(&f);let plan=prob::optimize(&f.bank,&ordinary,&f.scope,&surface,&semantic,&policy,NOW).unwrap().unwrap();
 assert!(plan.legs().iter().all(|l|l.exchange_id!="101"),"held fixture must remain absent from wire");
 let activation=moloch_live_runtime::authority::Activation{run_id:"surface-fixture".into(),entry_until_ms:NOW+600000,maximum_committed:"100.000000".into(),maximum_market_cost:"100.000000".into(),maximum_quantity:100,tournament_id:T.into(),tournament_slug:"cup".into(),binary_sha256:"a".repeat(64)};
 let dir=f.root.join("preflight-retained-evidence");std::fs::create_dir_all(&dir).unwrap();let io=Reads{scope:f.scope.clone(),clock:NOW,step:10,facts:fixture::facts(&f,NOW),ordinary,activation,dir,reads:vec![],fail_at:0,bad_at:0,bad:"",prepared:false};
 let mut log=PackageDispatchLog::open(&f.root.join("preflight-original-package")).unwrap();let staged=worker::stage(&mut f.bank,&mut log,&io,plan,&surface,&semantic,&policy,&io.activation).unwrap();(f,io,log,staged)
}
#[test]fn unchanged_held_component_gets_original_market_node_and_book_without_a_fake_wire_leg(){
 let(f,mut io,log,staged)=setup();let head=f.bank.durable_head();let report=f.bank.engine().report().unwrap();let before=log.verified_record(staged.package_id()).unwrap();let mut observed=vec![];
 let facts=package_preflight::collect_probabilistic_staged(&mut io,&f.scope,&staged,|r|{observed.push(serde_json::to_value(r)?);Ok(())}).unwrap();
 assert_eq!(staged.reservation().event()["qualification"]["contracts"].as_array().unwrap().len(),5);assert_eq!(facts.components.len(),5);assert_eq!(io.reads.len(),19);assert_eq!(observed.len(),19);assert!(staged.basket().legs.iter().all(|l|l.exchange_id!="101"));
 assert_eq!(io.reads[4].endpoint,format!("/markets/201?tournamentId={T}"));assert_eq!(observed[5],serde_json::to_value(&facts.components["101"].contract).unwrap());assert_eq!(observed[6],serde_json::to_value(&facts.components["101"].book).unwrap());assert_eq!(observed,io.reads.iter().map(|r|serde_json::to_value(r).unwrap()).collect::<Vec<Value>>());
 assert_eq!(f.bank.engine().wallet.quantity("101",moloch_replay::Side::Yes),5);assert_eq!(f.bank.durable_head(),head);assert_eq!(f.bank.engine().report().unwrap(),report);assert_eq!(log.verified_record(staged.package_id()).unwrap(),before);
}
#[test]fn exact_qualification_scope_including_nonwire_mapping_is_checked_before_any_read(){
 for case in ["missing_held","remapped_held","tournament","backend_scope"]{let(f,mut io,_,staged)=setup();let mut s=f.scope.clone();match case{"missing_held"=>{s.exchanges.remove("101");io.scope=s.clone();},"remapped_held"=>{s.exchanges.insert("101".into(),"999".into());io.scope=s.clone();},"tournament"=>{s.tournament_id="other".into();io.scope=s.clone();},_=>{s.exchanges.remove("101");}}assert!(package_preflight::collect_probabilistic_staged(&mut io,&s,&staged,|_|Ok(())).is_err(),"{case}");assert!(io.reads.is_empty(),"{case}");}
}
#[test]fn callback_stop_is_honored_at_every_global_and_nonwire_or_wire_component_read(){
 let(f,mut io,_,staged)=setup();for cut in 1..=19{io.clock=NOW;io.reads.clear();let mut calls=0;let err=package_preflight::collect_probabilistic_staged(&mut io,&f.scope,&staged,|_|{calls+=1;if calls==cut{Err("original control/source stop".into())}else{Ok(())}}).err().unwrap();assert_eq!(err.to_string(),"original control/source stop");assert_eq!(calls,cut);assert_eq!(io.reads.len(),cut);}
}
#[test]fn read_failure_has_no_fabricated_callback_and_bad_original_receipts_stop_following_reads(){
 for case in ["transport","endpoint","future","coverage"]{let(f,mut io,_,staged)=setup();let cut=if case=="coverage"{4}else{6};if case=="transport"{io.fail_at=cut;}else{io.bad_at=cut;io.bad=case;}let mut calls=0;assert!(package_preflight::collect_probabilistic_staged(&mut io,&f.scope,&staged,|_|{calls+=1;Ok(())}).is_err(),"{case}");assert_eq!(io.reads.len(),cut);assert_eq!(calls,cut-usize::from(case=="transport"));}
}
#[test]fn complete_qualification_never_renews_original_book_source_clock(){
 let(f,mut io,_,staged)=setup();io.step=800;let mut calls=0;assert!(package_preflight::collect_probabilistic_staged(&mut io,&f.scope,&staged,|_|{calls+=1;Ok(())}).is_err());assert_eq!(calls,13);assert!(io.clock-NOW>10000&&io.clock-NOW<20000);assert_eq!(io.reads[12].data["contexts"][0]["orderbook"]["exchanges"][0]["asOf"]["at"],iso(NOW));assert!(f.bank.engine().fills().is_empty());
}
#[test]fn genuine_stage_full_preflight_reaches_actual_authority_but_missing_or_changed_held_facts_do_not(){
 for case in ["complete","missing_held","changed_held"]{let(mut f,mut io,mut log,staged)=setup();let mut facts=package_preflight::collect_probabilistic_staged(&mut io,&f.scope,&staged,|_|Ok(())).unwrap();match case{"missing_held"=>{facts.components.remove("101");},"changed_held"=>facts.components.get_mut("101").unwrap().contract.data["root"]["contract_id"]=json!("999"),_=>{}}
  io.clock=NOW+3000;let cash=f.bank.engine().wallet.cash;let result=worker::submit_once(&mut f.bank,&mut log,&mut io,staged,facts).unwrap();assert_eq!(result,worker::SendResult::NeverAttempted);assert_eq!(io.prepared,case=="complete","{case}");assert_eq!(f.bank.engine().wallet.cash,cash);assert_eq!(f.bank.engine().wallet.quantity("101",moloch_replay::Side::Yes),5);assert!(f.bank.engine().fills().is_empty());assert_eq!(f.bank.engine().package_records()[0]["phase"],"no_execution");
 }
}
