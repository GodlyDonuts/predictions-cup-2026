use moloch_replay::{engine::Engine,session::Session};
use serde_json::{json,Value};
use sha2::{Digest,Sha256};
use std::{collections::BTreeSet,fs,path::PathBuf,sync::atomic::{AtomicU64,Ordering}};
const BASE:i64=1_792_054_800_000;
fn iso(ms:i64)->String{chrono::DateTime::from_timestamp_millis(BASE+ms).unwrap().to_rfc3339_opts(chrono::SecondsFormat::Millis,true)}
fn hash(v:&Value)->String{format!("{:x}",Sha256::digest(serde_json::to_vec(v).unwrap()))}
struct F{root:PathBuf,bank:Option<Session>,policy:Value,seed:Value,scope:Vec<String>}
impl Drop for F{fn drop(&mut self){self.bank.take();let _=fs::remove_dir_all(&self.root);}}
impl F{
 fn new(inherited:bool)->Self{
  static N:AtomicU64=AtomicU64::new(0);let root=std::env::temp_dir().join(format!("live-capacity-{}-{}-{}",std::process::id(),std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos(),N.fetch_add(1,Ordering::Relaxed)));fs::create_dir(&root).unwrap();
  let policy=json!({"initialCash":"40000.000000","maximumCommitted":"40000.000000","maximumMarketCost":"10000.000000","quantity":10000,"entryDepthBps":1000,"maximumMarkets":3,"entryStopAt":iso(3600000)});
  let seed=json!({"runId":"capacity-fixture","tournamentId":"cup","availableCash":"40000.000000","accountSnapshotHash":"a".repeat(64),"observedAt":iso(0)});
  let bank=Session::open_live(&root.join("bank"),&policy,&seed).unwrap();let scope=(100..128).chain(200..213).map(|n|n.to_string()).collect();let mut f=Self{root,bank:Some(bank),policy,seed,scope};
  if inherited{let rows:Vec<_>=(100..128).map(|n|json!({"exchangeId":n.to_string(),"marketId":n.to_string(),"quantity":1,"lotPrincipal":"0.200000","reportedCostBasis":"0.200000","reportedMinusLotPrincipal":"0.000000","basisClassification":"exact","lots":[{"lotId":(n+1000).to_string(),"quantity":1,"side":"YES","entryPrice":"0.200000","openedAt":iso(-1000)}]})).collect();
   f.put(&json!({"type":"actual_position_adoption","source":"official_sig_api_v1","authority":"human_moloch_inherited_management_20261015","complete":true,"receiptHash":"d".repeat(64),"tournamentId":"cup","receivedAt":iso(0),"processedAt":iso(0),"requestStartedAt":iso(0),"openingCost":"5.600000","effectiveMaximumMarkets":31,"positions":rows})).unwrap();}
  f
 }
 fn b(&self)->&Session{self.bank.as_ref().unwrap()}
 fn put(&mut self,e:&Value)->Result<bool,Box<dyn std::error::Error>>{let n=self.b().last_source_sequence("fixture")+1;self.bank.as_mut().unwrap().accept("fixture",n,e)}
 fn event(&self,at:i64)->Value{let old=self.b().engine().fresh_market_limit();let maximum=self.b().engine().policy()["maximumMarkets"].as_u64().unwrap();json!({"type":"live_market_capacity_v1","source":"moloch_live_dispatcher","authorizationId":"human_moloch_breadth_20261016","receivedAt":iso(at),"processedAt":iso(at),"bankScope":{"runId":"capacity-fixture","bankManifestHash":self.b().manifest_hash(),"tournamentId":"cup"},"bankHead":self.b().durable_head(),"expectedReportSha256":hash(&self.b().engine().report().unwrap()),"expectedPolicySha256":hash(self.b().engine().policy()),"expectedPreviousFreshMarketLimit":old,"expectedPreviousMaximumMarkets":maximum,"freshMarketLimit":12,"effectiveMaximumMarkets":maximum+12-old as u64,"scopeExchangeIds":self.scope,"controlSha256":"b".repeat(64),"expectedPreviousCapacitySha256":self.b().engine().live_market_capacity_record().map_or(Value::Null,|r|r["sha256"].clone())})}
 fn book(&mut self,ex:u64,at:i64){let ex=ex.to_string();self.put(&json!({"type":"book","receivedAt":iso(at),"book":{"marketId":ex,"exchangeId":ex,"tournamentId":"cup","sequence":(at+1).to_string(),"sourceAt":iso(at),"requestStartedAt":iso(at),"receivedAt":iso(at),"bids":[{"price":"0.400000","quantity":100}],"asks":[{"price":"0.450000","quantity":100}]}})).unwrap();}
 fn fresh_orders(&self)->BTreeSet<String>{self.b().engine().order_records().iter().filter(|q|q["retired"]==false&&q["action"]=="buy").filter_map(|q|q["exchangeId"].as_str().map(str::to_owned)).collect()}
}
#[test]fn inherited_three_to_twelve_is_durable_without_changing_any_money_or_existing_order(){
 let mut f=F::new(true);for i in 0..4{f.book(200+i,1000+i as i64*2500);}assert_eq!(f.fresh_orders().len(),3);assert_eq!(f.b().engine().fresh_market_limit(),3);assert_eq!(f.b().engine().policy()["maximumMarkets"],31);
 let policy=f.b().engine().policy().clone();let before=f.b().engine().report().unwrap();let orders=f.b().engine().order_records();let positions=f.b().engine().wallet.positions();let manifest=fs::read(f.root.join("bank/manifest.json")).unwrap();let e=f.event(9000);f.put(&e).unwrap();
 assert_eq!(f.b().engine().fresh_market_limit(),12);let mut expected=policy.clone();expected["maximumMarkets"]=json!(40);assert_eq!(f.b().engine().policy(),&expected);assert_eq!(f.b().engine().wallet.positions(),positions);assert_eq!(f.b().engine().order_records(),orders);
 let after=f.b().engine().report().unwrap();for key in ["cash","reservedCash","ownedCost","fees","realized","newTradingRealized","inheritedRealizedGross","openingInventoryCost"]{assert_eq!(after[key],before[key],"{key}");}assert_eq!(fs::read(f.root.join("bank/manifest.json")).unwrap(),manifest);assert!(f.b().engine().fills().is_empty());assert!(f.b().engine().live_order_bindings().is_empty());
 for i in 3..13{f.book(200+i,10000+(i-3)as i64*2500);}assert_eq!(f.fresh_orders().len(),12);assert!(!f.fresh_orders().contains("212"));assert_eq!(f.b().engine().policy()["maximumCommitted"],"40000.000000");assert_eq!(f.b().engine().policy()["maximumMarketCost"],"10000.000000");assert_eq!(f.b().engine().policy()["quantity"],10000);
 let report=f.b().engine().report().unwrap();let head=f.b().durable_head();f.bank.take();let b=Session::open_live(&f.root.join("bank"),&f.policy,&f.seed).unwrap();assert_eq!(b.engine().report().unwrap(),report);assert_eq!(b.durable_head(),head);assert_eq!(b.engine().fresh_market_limit(),12);
}
#[test]fn noninherited_live_capacity_changes_only_original_market_field(){let mut f=F::new(false);let old=f.b().engine().policy().clone();let e=f.event(0);f.put(&e).unwrap();assert_eq!(f.b().engine().fresh_market_limit(),12);let mut expected=old;expected["maximumMarkets"]=json!(12);assert_eq!(f.b().engine().policy(),&expected);assert_eq!(f.b().engine().wallet.cash,40_000_000_000);assert!(f.b().engine().order_records().is_empty());}
#[test]fn default_history_has_no_capacity_record_and_no_extra_planning(){let f=F::new(true);assert!(f.b().engine().live_market_capacity_record().is_none());assert!(f.b().engine().report().unwrap().get("liveMarketCapacity").is_none());assert_eq!(f.b().engine().fresh_market_limit(),3);}
#[test]fn changed_prefix_previous_scope_caps_or_authority_is_atomic_denial(){
 for case in ["run","manifest","tournament","head","report","policy","previous_fresh","previous_total","total","over12","decrease","scope_small","scope_duplicate","scope_foreign","control","authority","source","future","previous_record"]{
  let mut f=F::new(true);let mut e=f.event(0);match case{
   "run"=>e["bankScope"]["runId"]=json!("other"),"manifest"=>e["bankScope"]["bankManifestHash"]=json!("e".repeat(64)),"tournament"=>e["bankScope"]["tournamentId"]=json!("other"),"head"=>e["bankHead"]["sequence"]=json!(999),"report"=>e["expectedReportSha256"]=json!("e".repeat(64)),"policy"=>e["expectedPolicySha256"]=json!("e".repeat(64)),"previous_fresh"=>e["expectedPreviousFreshMarketLimit"]=json!(4),"previous_total"=>e["expectedPreviousMaximumMarkets"]=json!(32),"total"=>e["effectiveMaximumMarkets"]=json!(41),"over12"=>{e["freshMarketLimit"]=json!(13);e["effectiveMaximumMarkets"]=json!(41)},"decrease"=>{e["freshMarketLimit"]=json!(2);e["effectiveMaximumMarkets"]=json!(30)},"scope_small"=>e["scopeExchangeIds"]=json!(["100"]),"scope_duplicate"=>e["scopeExchangeIds"][1]=e["scopeExchangeIds"][0].clone(),"scope_foreign"=>{let ids:Vec<_>=(200..241).map(|n|n.to_string()).collect();e["scopeExchangeIds"]=json!(ids)},"control"=>e["controlSha256"]=json!("invalid"),"authority"=>e["authorizationId"]=json!("unapproved"),"source"=>e["source"]=json!("other"),"future"=>e["receivedAt"]=json!(iso(1)),_=>{e.as_object_mut().unwrap().remove("expectedPreviousCapacitySha256");}}
  let report=f.b().engine().report().unwrap();let head=f.b().durable_head();assert!(f.put(&e).is_err(),"{case}");assert_eq!(f.b().engine().report().unwrap(),report,"{case}");assert_eq!(f.b().durable_head(),head,"{case}");
 }
}
#[test]fn capacity_uses_current_daily_cutoff_not_original_manifest_policy(){
 let mut f=F::new(false);let day=86_400_000;let at=3_600_000;let end=at+day;
 let mandate=json!({"authorizationId":"capacity-test-original-daily-window","approvedAt":iso(0),"anchorMs":BASE+at,"campaignEndMs":BASE+at+2*day,"intervalMs":day,"maximumCommitted":"40000.000000","maximumMarketCost":"10000.000000","maximumQuantity":10000});
 f.put(&json!({"type":"live_entry_window","source":"moloch_live_dispatcher","receivedAt":iso(at),"processedAt":iso(at),"previousEntryStopAt":iso(at),"entryStopAt":iso(end),"mandate":mandate})).unwrap();
 let current=f.b().engine().policy().clone();assert_ne!(current["entryStopAt"],f.policy["entryStopAt"]);
 let mut stale=f.event(at);stale["expectedPolicySha256"]=json!(hash(&f.policy));assert!(f.put(&stale).is_err());
 let e=f.event(at);f.put(&e).unwrap();let mut expected=current;expected["maximumMarkets"]=json!(12);assert_eq!(f.b().engine().policy(),&expected);assert_eq!(f.b().engine().policy()["entryStopAt"],iso(end));
}
#[test]fn paper_and_standalone_seed_cannot_inherit_unproved_capacity(){let mut f=F::new(false);let e=f.event(0);let mut paper=Engine::new(&f.policy).unwrap();let before=paper.report().unwrap();assert!(paper.advance(&e).is_err());assert_eq!(paper.report().unwrap(),before);f.put(&e).unwrap();assert!(matches!(Engine::from_seed_state(&f.b().engine().report().unwrap()),Err("SEED_LIVE_MARKET_CAPACITY_REQUIRE_REPLAY")));}

fn full_scope_event(f:&F,at:i64)->Value{
 let mut e=f.event(at);let old=f.b().engine().fresh_market_limit();let total=f.b().engine().policy()["maximumMarkets"].as_u64().unwrap() as usize;
 e["authorizationId"]=json!("human_moloch_full_universe_20261021");e["freshMarketLimit"]=json!(old+f.scope.len()-total);e["effectiveMaximumMarkets"]=json!(f.scope.len());e
}
#[test]fn full_scope_releases_thirteenth_market_and_replays_original_twelve_market_history(){
 let mut f=F::new(true);let old=f.event(0);f.put(&old).unwrap();
 for i in 0..13{f.book(200+i,1000+i as i64*2500);}assert_eq!(f.fresh_orders().len(),12);assert!(!f.fresh_orders().contains("212"));
 let before=f.b().engine().report().unwrap();let orders=f.b().engine().order_records();let policy=f.b().engine().policy().clone();
 let e=full_scope_event(&f,32000);f.put(&e).unwrap();let after=f.b().engine().report().unwrap();
 for key in ["cash","reservedCash","ownedCost","fees","realized","newTradingRealized","inheritedRealizedGross","openingInventoryCost"]{assert_eq!(after[key],before[key],"{key}");}
 assert_eq!(f.b().engine().order_records(),orders);let mut expected=policy;expected["maximumMarkets"]=json!(41);assert_eq!(f.b().engine().policy(),&expected);assert_eq!(f.b().engine().fresh_market_limit(),13);
 f.book(212,33500);assert!(f.fresh_orders().contains("212"));assert_eq!(f.fresh_orders().len(),13);
 let report=f.b().engine().report().unwrap();let head=f.b().durable_head();f.bank.take();let restored=Session::open_live(&f.root.join("bank"),&f.policy,&f.seed).unwrap();assert_eq!(restored.engine().report().unwrap(),report);assert_eq!(restored.durable_head(),head);assert_eq!(restored.engine().fresh_market_limit(),13);
}
#[test]fn full_scope_without_inherited_inventory_uses_exact_scope_size(){let mut f=F::new(false);let e=full_scope_event(&f,0);f.put(&e).unwrap();assert_eq!(f.b().engine().fresh_market_limit(),41);assert_eq!(f.b().engine().policy()["maximumMarkets"],41);assert_eq!(f.b().engine().wallet.cash,40_000_000_000);assert!(f.b().engine().order_records().is_empty());}
#[test]fn full_scope_cannot_set_arbitrary_quota_or_rewrite_previous_capacity(){
 for case in ["partial","larger","previous_record","previous_limit","foreign","duplicate","authority"]{
  let mut f=F::new(true);let original=f.event(0);f.put(&original).unwrap();let mut e=full_scope_event(&f,1);
  match case{"partial"=>{e["freshMarketLimit"]=json!(12);e["effectiveMaximumMarkets"]=json!(40)},"larger"=>{e["freshMarketLimit"]=json!(14);e["effectiveMaximumMarkets"]=json!(42)},"previous_record"=>e["expectedPreviousCapacitySha256"]=json!("c".repeat(64)),"previous_limit"=>e["expectedPreviousFreshMarketLimit"]=json!(11),"foreign"=>e["scopeExchangeIds"]=json!((200..241).map(|n|n.to_string()).collect::<Vec<_>>()),"duplicate"=>e["scopeExchangeIds"][1]=e["scopeExchangeIds"][0].clone(),_=>e["authorizationId"]=json!("unapproved")}
  let report=f.b().engine().report().unwrap();let head=f.b().durable_head();assert!(f.put(&e).is_err(),"{case}");assert_eq!(f.b().engine().report().unwrap(),report,"{case}");assert_eq!(f.b().durable_head(),head,"{case}");
 }
}
