use moloch_live_runtime::relationship_census::{self,Availability,Census,CensusCache,CensusReport,SourceEvidence,Surface};
use moloch_live_runtime::relationship_strategy as strategy;
use strategy::*;
use baal_transport_service::{Receipt,Scope,relationships};
use moloch_relationship_arbitrage::{Book,Level,Side,Source,UNIT};
use serde_json::json;
use sha2::{Digest,Sha256};
use std::{collections::{BTreeMap,BTreeSet},path::PathBuf};

const T:&str="bda92870-621e-47b0-bc3c-3602c5c26f55";
const R:&str="00000000-0000-4000-8000-000000000124";
const AT:&str="2026-10-15T09:01:00.000Z";
fn now()->u64{chrono::DateTime::parse_from_rfc3339(AT).unwrap().timestamp_millis() as u64}
fn receipt(endpoint:String,data:serde_json::Value)->Receipt{Receipt{endpoint,request_started_at:AT.into(),received_at:AT.into(),status:200,data}}
fn level(p:i128)->Vec<Level>{vec![Level{price:p,quantity:100}]}
fn surface()->Surface{Surface{availability:Availability::NotRequested,row_count:None,source:None,error_code:None,http_status:None,retry_after_ms:None}}
struct Fixture{census:Census,canonical:Receipt,certs:Vec<SettlementCertificate>,trusted:BTreeSet<String>,rules:Vec<Receipt>,books:BTreeMap<String,Book>,policy:Policy,capital:Capital}
impl Fixture{
 fn analyze(&self,positions:&[OwnedPosition])->Analysis{analyze(&self.census,&self.canonical,&Certificates{values:&self.certs,trusted_sha256:&self.trusted,rule_receipts:&self.rules},&self.books,positions,&self.capital,&self.policy)}
 fn approve(&mut self){self.trusted=self.certs.iter().map(|c|certificate_sha256(c).unwrap()).collect();}
}
fn fixture(n:usize,exhaustive:bool)->Fixture{
 let scope=Scope{tournament_id:T.into(),tournament_slug:"midterm-elections".into(),exchanges:(0..n).map(|i|((101+i).to_string(),(201+i).to_string())).collect()};
 let nodes:Vec<_>=(0..n).map(|i|json!({"exchangeId":(101+i).to_string(),"marketId":(201+i).to_string(),"marketTitle":"not a proof","outcome":"YES","currentPrice":null,"role":"member","contractId":format!("contract-{}",101+i)})).collect();
 let canonical=receipt(format!("/relationships?tournamentId={T}&depth=4"),json!({"data":[{"id":R,"type":"mutually_exclusive","source":"admin_tagged","status":"active","label":null,"reason":null,"isExhaustive":exhaustive,"expression":null,"nodes":nodes,"direction":{"kind":"symmetric","fromExchangeIds":[],"toExchangeIds":[],"description":"price metadata"},"constraint":{"description":"not a settlement certificate","priceRule":"sum <= 1","boundSemantics":"price","evaluationEndpoint":"/relationships/constraints"},"createdAt":AT,"updatedAt":AT,"version":2}],"pagination":{"limit":200,"hasMore":false,"nextCursor":null}}));
 let list=relationships::parse_list(&canonical.data,&scope).unwrap();
 let rules:Vec<_>=(0..n).map(|i|receipt(format!("/markets/{}/nodes",201+i),json!({"market_id":(201+i).to_string(),"root":{"node_type":"contract","id":format!("contract-{}",101+i)}}))).collect();
 let cert=SettlementCertificate{version:1,tournament_id:T.into(),relationship_id:R.into(),relationship_version:2,canonical_relationship_sha256:canonical_relationship_sha256(&list.data[0]).unwrap(),relationship_receipt_sha256:receipt_sha256(&canonical).unwrap(),issued_at_ms:now(),expires_at_ms:now()+60_000,verifier_id:"fixture-only-reviewed-rules".into(),logic:CertifiedLogic::MutuallyExclusive{exhaustive},contracts:(0..n).map(|i|ContractCertificate{exchange_id:(101+i).to_string(),market_id:(201+i).to_string(),contract_id:format!("contract-{}",101+i),canonical_outcome:"YES".into(),rule_receipt_sha256:receipt_sha256(&rules[i]).unwrap(),rule_endpoint:rules[i].endpoint.clone(),rule_data_sha256:None,ordinary_yes_payout:"1".into(),ordinary_no_payout:"0".into(),ordinary_no_yes_payout:"0".into(),ordinary_no_no_payout:"1".into(),terminal_rules_complete:true,exceptions:vec![]}).collect()};
 let scope_sha256=format!("{:x}",Sha256::digest(serde_json::to_vec(&scope).unwrap()));
 let census=Census{report:CensusReport{version:1,started_at_ms:now(),completed_at_ms:now(),tournament_id:T.into(),scope_sha256,relationships:Surface{availability:Availability::Available,row_count:Some(1),source:Some(SourceEvidence{receipt_sha256:receipt_sha256(&canonical).unwrap(),private_file:"fixture-private.json".into(),request_started_at:AT.into(),received_at:AT.into(),status:200}),error_code:None,http_status:Some(200),retry_after_ms:None},constraints:surface(),collateral:surface(),account:surface(),graphs:BTreeMap::new(),active_relationship_count:1,exhaustive_relationship_count:usize::from(exhaustive),known_floor_component_count:0,unknown_floor_component_count:0,feed_complete:false,transport_guard_observed:false,execution_authority:false},cache:CensusCache{scope,relationships:Some(list),constraints:None,collateral:None,buying_power:None,graphs:BTreeMap::new(),private_evidence_directory:PathBuf::from("fixture-unused")}};
 let books=(0..n).map(|i|{let e=(101+i).to_string();(e.clone(),Book{exchange_id:e,source:Source{sha256:"b".repeat(64),received_ms:now()},observed_ms:now(),yes_asks:level(if n==3{300_000}else{600_000}),no_asks:level(if n==3{800_000}else if i==0{450_000}else{505_000}),yes_bids:level(290_000),no_bids:level(400_000)})}).collect();
 let policy=Policy{now_ms:now(),max_relationship_age_ms:30_000,max_rule_age_ms:3_600_000,max_book_age_ms:3000,max_book_skew_ms:1000,entry_fee_per_share_per_leg:0,buffer_per_share:5000,minimum_surplus:1,maximum_quantity:100,quantities:vec![10],maximum_retained_candidates:16};
 let capital=Capital{gross_account_cash:100*UNIT,attributed_bank_cash:20*UNIT,reserved_cash:0,maximum_committed_remaining:20*UNIT,package_cost_cap:20*UNIT,orphan_cash_loss_cap:20*UNIT};
 let mut f=Fixture{census,canonical,certs:vec![cert],trusted:BTreeSet::new(),rules,books,policy,capital};f.approve();f
}
use moloch_live_runtime::{authority,feedback,daily_windows,package_admission};
use moloch_live_runtime::package_authority::{Prepared,PackageSnapshotSet,ComponentSnapshots};
use baal_transport_service::{Permit,RequestSpec,multi_leg};
use moloch_replay::session::Session;
fn stamp(at:u64)->String{daily_windows::iso(at).unwrap()}
fn sha<T:serde::Serialize>(v:&T)->String{format!("{:x}",Sha256::digest(serde_json::to_vec(v).unwrap()))}
struct F{bank:Session,path:PathBuf,scope:Scope,a:authority::Activation,admission:package_admission::Admission,permit:Permit,spec:RequestSpec,facts:PackageSnapshotSet,at:u64}
impl Drop for F{fn drop(&mut self){let _=std::fs::remove_dir_all(&self.path);}}
fn production_authority(f:&F)->authority::ProductionAuthority{
 use moloch_replay::session::ExclusiveLock;
 let binary=f.path.join("test-binary");let control=f.path.join("test-control");
 std::fs::write(&binary,b"checked-package-test-binary").unwrap();std::fs::write(&control,b"unchanged-control").unwrap();
 let mut a=f.a.clone();a.binary_sha256=format!("{:x}",Sha256::digest(std::fs::read(&binary).unwrap()));
 let lease=std::sync::Arc::new(ExclusiveLock::acquire(&f.path.join("physical-owner.lock")).unwrap());
 let auth=authority::ProductionAuthority::new(a,f.scope.clone(),vec![f.path.join("STOP")],control,binary,lease).unwrap();
 auth.bind_stream(std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),f.at+60_000);auth
}
fn refresh_physical_facts(f:&mut F,at:u64){
 f.at=at;
 for r in [&mut f.facts.account,&mut f.facts.cash_balance,&mut f.facts.positions,&mut f.facts.open_orders]{r.request_started_at=stamp(at);r.received_at=stamp(at);}
 for c in f.facts.components.values_mut(){for r in [&mut c.market,&mut c.contract,&mut c.book]{r.request_started_at=stamp(at);r.received_at=stamp(at);}for b in c.book.data["contexts"][0]["orderbook"]["exchanges"].as_array_mut().unwrap(){b["asOf"]["at"]=json!(stamp(at));b["asOf"]["sequence"]=json!(2);}}
}
#[test]fn paced_preflight_can_refresh_complete_v2_rules_and_books_after_initial_ten_seconds(){
 let mut f=F::new();assert_eq!(f.admission.expires_at_ms(),now()+30_000);
 let original=f.facts.components["101"].contract.clone();
 refresh_physical_facts(&mut f,now()+15_000);assert_ne!(sha(&original),sha(&f.facts.components["101"].contract));assert_eq!(original.data,f.facts.components["101"].contract.data);
 f.ready().unwrap().authorize(&f.spec,&f.permit,f.at,&BTreeSet::new(),f.a.entry_until_ms).unwrap();assert!(f.bank.engine().fills().is_empty());
}
#[test]fn longer_intent_does_not_authorize_old_financial_books_or_stale_account_facts(){
 let mut f=F::new();f.at=now()+15_000;assert!(f.ready().is_err());
 refresh_physical_facts(&mut f,now()+15_000);f.facts.components.get_mut("101").unwrap().book.data["contexts"][0]["orderbook"]["exchanges"][0]["asOf"]["at"]=json!(AT);assert!(f.ready().is_err());
}
#[test]fn refreshed_v2_rule_cannot_change_any_reviewed_body_field_or_endpoint(){
 for changed in ["rule","endpoint"]{let mut f=F::new();refresh_physical_facts(&mut f,now()+15_000);let r=&mut f.facts.components.get_mut("101").unwrap().contract;if changed=="rule"{r.data["resolutionException"]=json!("changed")}else{r.endpoint.push_str("&other=1")};assert!(f.ready().is_err());}
}
#[test]fn v1_physical_rule_receipt_stays_exact_despite_new_intent_lifetime(){
 let mut f=F::make_version(false,1);f.admission.validate_physical_rule("101",&f.facts.components["101"].contract).unwrap();refresh_physical_facts(&mut f,now()+15_000);assert!(f.admission.validate_physical_rule("101",&f.facts.components["101"].contract).is_err());
}
#[test]fn current_books_cannot_extend_expired_relationship_bounded_intent(){
 let mut f=F::new();let expiry=f.admission.expires_at_ms();refresh_physical_facts(&mut f,expiry);assert!(f.ready().is_err());
}
#[test]fn production_boundary_authorizes_only_staged_package_and_clears_it(){
 use baal_transport_service::Authority;
 let f=F::new();let a=production_authority(&f);let log=moloch_live_runtime::dispatch::DispatchLog::open(&f.path.join("ordinary-dispatch.ndjson")).unwrap();
 assert!(a.authorize(&f.scope,&f.spec,&f.permit,true,f.at).is_err());
 a.prepare_package(&f.bank,&f.admission,f.permit.clone(),f.facts.clone(),&log,f.at).unwrap();
 a.authorize(&f.scope,&f.spec,&f.permit,false,f.at).unwrap();a.authorize(&f.scope,&f.spec,&f.permit,true,f.at).unwrap();
 a.clear_ready();assert!(a.authorize(&f.scope,&f.spec,&f.permit,true,f.at).is_err());
}
#[test]fn production_package_boundary_preserves_stop_binary_control_and_stream_fences(){
 use baal_transport_service::Authority;
 for case in ["stop","binary","control","stream"]{
  let f=F::new();let a=production_authority(&f);let log=moloch_live_runtime::dispatch::DispatchLog::open(&f.path.join("ordinary-dispatch.ndjson")).unwrap();
  a.prepare_package(&f.bank,&f.admission,f.permit.clone(),f.facts.clone(),&log,f.at).unwrap();
  match case{"stop"=>std::fs::write(f.path.join("STOP"),b"stop").unwrap(),"binary"=>std::fs::write(f.path.join("test-binary"),b"changed").unwrap(),"control"=>std::fs::write(f.path.join("test-control"),b"changed").unwrap(),_=>a.bind_stream(std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true)),f.at+60_000)}
  assert!(a.authorize(&f.scope,&f.spec,&f.permit,true,f.at).is_err(),"{case}");
 }
}
impl F{
 fn make(attempt:bool)->Self{Self::make_version(attempt,2)}
 fn make_version(attempt:bool,version:u32)->Self{let mut f=fixture(2,false);f.certs[0].version=version;f.certs[0].canonical_relationship_sha256=if version==2{relationship_semantics_sha256(&f.census.cache.relationships.as_ref().unwrap().data[0]).unwrap()}else{canonical_relationship_sha256(&f.census.cache.relationships.as_ref().unwrap().data[0]).unwrap()};for(i,r)in f.rules.iter_mut().enumerate(){r.endpoint=if version==2{format!("/markets/{}/nodes?tournamentId={T}",201+i)}else{format!("/markets/{}/nodes",201+i)};r.data["contexts"]=json!([{"type":"tournament","tournament":{"id":T}}]);f.certs[0].contracts[i].rule_endpoint=r.endpoint.clone();f.certs[0].contracts[i].rule_receipt_sha256=receipt_sha256(r).unwrap();f.certs[0].contracts[i].rule_data_sha256=if version==2{Some(rule_data_sha256(r).unwrap())}else{None};}f.rules.push(f.canonical.clone());f.approve();
 let path=std::env::temp_dir().join(format!("package-authority-{}-{}",std::process::id(),std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));let end=now()+60000;
 let policy=json!({"initialCash":"1000.000000","maximumCommitted":"1000.000000","maximumMarketCost":"100.000000","quantity":100,"maximumMarkets":3,"minimumSpreadTicks":200,"entryStopAt":stamp(end)});let seed=json!({"runId":"fixture","availableCash":"1000.000000","accountSnapshotHash":"a".repeat(64),"observedAt":AT});let mut bank=Session::open_live(&path,&policy,&seed).unwrap();let scope=f.census.cache.scope.clone();let mut components=BTreeMap::new();
 for(index,(ex,b))in f.books.iter().enumerate(){let m=&scope.exchanges[ex];let bids:Vec<_>=b.no_asks.iter().map(|l|json!({"price":moloch_replay::money(UNIT-l.price),"quantity":l.quantity})).collect();let asks:Vec<_>=b.yes_asks.iter().map(|l|json!({"price":moloch_replay::money(l.price),"quantity":l.quantity})).collect();let raw=json!({"tournamentId":T,"marketId":m,"exchangeId":ex,"sequence":"1","requestStartedAt":AT,"sourceAt":AT,"receivedAt":AT,"bids":bids,"asks":asks});bank.accept("book",index as u64+1,&json!({"type":"book","receivedAt":AT,"book":raw})).unwrap();
 let bids:Vec<_>=b.no_asks.iter().map(|l|json!({"price":serde_json::from_str::<serde_json::Value>(&moloch_replay::money(UNIT-l.price)).unwrap(),"quantity":l.quantity})).collect();let asks:Vec<_>=b.yes_asks.iter().map(|l|json!({"price":serde_json::from_str::<serde_json::Value>(&moloch_replay::money(l.price)).unwrap(),"quantity":l.quantity})).collect();let c=ComponentSnapshots{market:receipt(format!("/markets/{m}?tournamentId={T}"),json!({"id":m,"status":"open","isComposite":false,"isMultiOutcome":false,"contexts":[{"type":"tournament","tournament":{"id":T},"status":"open"}]})),contract:f.rules[index].clone(),book:receipt(format!("/markets/{m}/orderbook?tournamentId={T}&depth=20"),json!({"contexts":[{"type":"tournament","tournament":{"id":T},"orderbook":{"exchanges":[{"exchangeId":ex,"asOf":{"sequence":1,"at":AT},"bids":bids,"asks":asks}]}}]}))};components.insert(ex.clone(),c);}
 let a=authority::Activation{run_id:"fixture".into(),entry_until_ms:end,maximum_committed:"1000.000000".into(),maximum_market_cost:"100.000000".into(),maximum_quantity:100,tournament_id:T.into(),tournament_slug:scope.tournament_slug.clone(),binary_sha256:"b".repeat(64)};
 let admission=package_admission::admit(&bank,&a,&f.census,&f.canonical,&Certificates{values:&f.certs,trusted_sha256:&f.trusted,rule_receipts:&f.rules},1000*UNIT,&f.policy,end).unwrap().unwrap();let permit=Permit{run_id:a.run_id.clone(),intent_id:admission.package_id().into(),owner:"Codex".into(),expires_at_ms:admission.expires_at_ms(),maximum_quantity:100,maximum_cost:admission.basket().maximum_cost.clone()};let spec=multi_leg::request_spec(admission.basket(),&scope,&permit).unwrap();bank.accept("package",1,admission.reserve_event()).unwrap();
 let facts=PackageSnapshotSet{account:receipt(format!("/tournaments/{}",scope.tournament_slug),json!({"id":T,"slug":scope.tournament_slug,"status":"active","endDate":stamp(end+1000)})),cash_balance:receipt(format!("/account?tournamentId={T}"),json!({"balance":1000,"availableAdvance":0,"buyingPower":1000})),positions:receipt(format!("/tournaments/{}/portfolio/positions",scope.tournament_slug),json!({"positions":[]})),open_orders:receipt(format!("/orders?status=open&limit=200&tournamentId={T}"),json!({"data":[],"pagination":{"hasMore":false},"coverage":{"complete":true}})),components};
 let mut out=Self{bank,path,scope,a,admission,permit,spec,facts,at:now()+3000};if attempt{out.attempt(sha(&out.spec));}out
 }
 fn new()->Self{Self::make(true)}
 fn attempt(&mut self,h:String){let mut e=self.admission.reserve_event().clone();e["type"]=json!("live_package_attempt");e["receivedAt"]=json!(stamp(self.at));e["processedAt"]=e["receivedAt"].clone();e["requestHash"]=json!(h);e["idempotencyKey"]=json!(self.admission.basket().idempotency_key);e["bookHashes"]=self.bank.engine().live_package_books().iter().map(|b|(b["exchangeId"].as_str().unwrap().to_owned(),json!(sha(b)))).collect::<serde_json::Map<_,_>>().into();self.bank.accept("package",2,&e).unwrap();}
 fn ready(&self)->feedback::Result<Prepared>{Prepared::new(&self.bank,&self.a,&self.scope,&self.admission,self.permit.clone(),self.facts.clone(),self.at,self.a.entry_until_ms)}
}
#[test]fn real_admission_native_attempt_and_full_crossing_no_depth_authorize(){let f=F::new();let p=f.ready().unwrap();p.authorize(&f.spec,&f.permit,f.at,&BTreeSet::new(),f.a.entry_until_ms).unwrap();assert_eq!(f.spec.body.as_ref().unwrap()["executionMode"],"allOrNone");assert_eq!(f.spec.body.as_ref().unwrap()["legs"][0]["side"],"no");assert!(f.bank.engine().fills().is_empty());}
#[test]fn reserved_unknown_or_acknowledged_package_cannot_create_fresh_authority(){let f=F::make(false);assert!(f.ready().is_err());let mut f=F::new();let mut e=f.admission.reserve_event().clone();e["type"]=json!("live_package_unknown");e["receivedAt"]=json!(stamp(f.at));e["processedAt"]=e["receivedAt"].clone();e["requestHash"]=json!(sha(&f.spec));f.bank.accept("package",3,&e).unwrap();assert!(f.ready().is_err());}
#[test]fn native_attempt_hash_must_bind_exact_sdk_request(){let mut f=F::make(false);f.attempt("f".repeat(64));assert!(f.ready().is_err());}
#[test]fn permit_payload_and_wire_bytes_cannot_change_while_waiting(){let f=F::new();let p=f.ready().unwrap();for case in ["body","bytes","permit"]{let mut spec=f.spec.clone();let mut permit=f.permit.clone();match case{"body"=>spec.body.as_mut().unwrap()["legs"][0]["quantity"]=json!(11),"bytes"=>spec.body_json=Some("{}".into()),_=>permit.maximum_cost="100.000000".into()};assert!(p.authorize(&spec,&permit,f.at,&BTreeSet::new(),f.a.entry_until_ms).is_err());}}
#[test]fn expiry_entry_window_change_and_regressed_clock_deny_at_physical_boundary(){let f=F::new();let p=f.ready().unwrap();for(at,end)in [(f.admission.expires_at_ms(),f.a.entry_until_ms),(f.at,f.a.entry_until_ms+1),(f.at-1,f.a.entry_until_ms)]{assert!(p.authorize(&f.spec,&f.permit,at,&BTreeSet::new(),end).is_err());}}
#[test]fn every_receipt_clock_and_request_start_freshness_is_required(){for case in ["account","cash","positions","orders","market","contract","book","unordered"]{let mut f=F::new();let c=f.facts.components.get_mut("101").unwrap();let r=match case{"account"=>&mut f.facts.account,"cash"=>&mut f.facts.cash_balance,"positions"=>&mut f.facts.positions,"orders"=>&mut f.facts.open_orders,"market"=>&mut c.market,"contract"=>&mut c.contract,_=>&mut c.book};r.request_started_at=if case=="unordered"{stamp(f.at+1)}else{stamp(now()-20001)};assert!(f.ready().is_err(),"{case}");}}
#[test]fn signed_no_foreign_component_inventory_and_duplicate_zero_rows_deny(){for rows in [json!([{"exchangeId":"101","marketId":"201","quantity":-1}]),json!([{"exchangeId":"101","marketId":"201","quantity":0},{"exchangeId":"101","marketId":"201","quantity":0}])]{let mut f=F::new();f.facts.positions.data["positions"]=rows;assert!(f.ready().is_err());}}
#[test]fn global_foreign_order_rejected_even_outside_package_scope(){let mut f=F::new();f.facts.open_orders.data["data"]=json!([{"id":99,"exchangeId":"103","tournamentId":T}]);let p=f.ready().unwrap();assert!(p.authorize(&f.spec,&f.permit,f.at,&BTreeSet::new(),f.a.entry_until_ms).is_err());p.authorize(&f.spec,&f.permit,f.at,&["99".into()].into(),f.a.entry_until_ms).unwrap();}
#[test]fn same_component_order_always_blocks_even_if_owned(){let mut f=F::new();f.facts.open_orders.data["data"]=json!([{"id":99,"exchangeId":"101","tournamentId":T}]);assert!(f.ready().is_err());}
#[test]fn cash_rounding_credit_and_unknown_balance_cannot_fund_package(){for data in [json!({"balance":999,"availableAdvance":1,"buyingPower":1000}),json!({"balance":1000.005,"availableAdvance":0,"buyingPower":1000.005}),json!({"balance":null,"availableAdvance":1,"buyingPower":null})]{let mut f=F::new();f.facts.cash_balance.data=data;assert!(f.ready().is_err());}}
#[test]fn full_fixed_limit_depth_is_required_no_side_complements_bid_correctly(){for case in ["price","depth","asof","tick"]{let mut f=F::new();let b=&mut f.facts.components.get_mut("101").unwrap().book.data["contexts"][0]["orderbook"]["exchanges"][0];match case{"price"=>b["bids"][0]["price"]=json!(0.54),"depth"=>b["bids"][0]["quantity"]=json!(9),"asof"=>b["asOf"]["at"]=json!(stamp(now()-10001)),_=>b["bids"][0]["price"]=json!(0.551)};assert!(f.ready().is_err(),"{case}");}}
#[test]fn changed_opaque_rule_body_or_contract_identity_denies(){for case in ["opaque","identity","clock"]{let mut f=F::new();let c=&mut f.facts.components.get_mut("101").unwrap().contract;match case{"opaque"=>c.data["newRefundCondition"]=json!("changed"),"identity"=>c.data["root"]["id"]=json!("other"),_=>{c.received_at=stamp(now()+1);c.request_started_at=stamp(now()+1);}};assert!(f.ready().is_err(),"{case}");}}
#[test]fn inactive_context_unsupported_market_and_ended_tournament_deny(){for case in ["inactive","composite","multi","ended","context"]{let mut f=F::new();let c=f.facts.components.get_mut("101").unwrap();match case{"inactive"=>c.market.data["contexts"][0]["status"]=json!("closed"),"composite"=>c.market.data["isComposite"]=json!(true),"multi"=>c.market.data["isMultiOutcome"]=json!(true),"ended"=>f.facts.account.data["endDate"]=json!(stamp(f.at-1)),_=>c.market.data["contexts"][0]["tournament"]["id"]=json!("other")};assert!(f.ready().is_err(),"{case}");}}
#[test]fn incomplete_open_orders_and_wrong_cash_endpoint_deny(){for case in ["page","coverage","cash","position"]{let mut f=F::new();match case{"page"=>f.facts.open_orders.data["pagination"]["hasMore"]=json!(true),"coverage"=>f.facts.open_orders.data["coverage"]["complete"]=json!(false),"cash"=>f.facts.cash_balance.endpoint="/account?tournamentId=wrong".into(),_=>f.facts.positions.endpoint="/tournaments/other/portfolio/positions".into()};assert!(f.ready().is_err(),"{case}");}}
#[test]fn wrong_activation_scope_caps_and_permit_owner_cannot_prepare(){for case in ["run","scope","caps","owner","quantity"]{let mut f=F::new();match case{"run"=>f.a.run_id="other".into(),"scope"=>f.scope.tournament_id="other".into(),"caps"=>f.a.maximum_committed="1001.000000".into(),"owner"=>f.permit.owner="other".into(),_=>f.permit.maximum_quantity=1};assert!(f.ready().is_err(),"{case}");}}
