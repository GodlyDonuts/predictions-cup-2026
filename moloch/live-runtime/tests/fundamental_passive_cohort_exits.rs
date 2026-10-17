#![cfg(unix)]
// Local original-shaped fixture helpers; no shared fixture or existing test edits.
// Use the genuine exported capability type accepted by ProductionAuthority.
use moloch_live_runtime::{daily_windows, dispatch, fundamental_policy};
use fundamental_policy::*;
use baal_transport_service::{RequestSpec,Scope,Permit};
use moloch_replay::session::{ExclusiveLock,Session};
use serde_json::{Value,json};
use sha2::{Digest,Sha256};
use std::{collections::BTreeSet,path::PathBuf,sync::Arc};
const NOW:u64=1_792_108_800_000;
const ORIGINALS:[&[u8];3]=[b"synthetic forecast fixture, not a live publisher",b"synthetic official ballot fixture",br#"{"market_id":"201","root":{"node_type":"contract","node_id":"301","contract_id":"301","contract_type":"Election Outcome","contract_details":{"contractType":"Election Outcome","resolutionType":"Party Winner","winnerName":"Republican Party","electionDate":"2026-11-03","officeLevel":"Federal","usState":"Synthetic","raceId":1,"stageId":2,"raceName":"Synthetic fixture","raceStage":"General"},"settled_with":null,"settlement_date":null,"title":"Synthetic Republican Party Winner fixture"}}"#];
fn sha(b:&[u8])->String{format!("{:x}",Sha256::digest(b))}
fn iso(at:u64)->String{daily_windows::iso(at).unwrap()}
fn input(manifest:&str,at:u64)->ReviewInput{
    ReviewInput{version:1,reviewer_id:"test-operator".into(),assumption_label:"Synthetic fixture: model probability and Republican-party mapping assumed; not a guaranteed payoff".into(),
        run_id:"fundamental-fixture".into(),bank_manifest_sha256:manifest.into(),tournament_id:"cup".into(),market_id:"201".into(),exchange_id:"101".into(),contract_id:"301".into(),side:"NO".into(),official_contract_root_sha256:official_contract_root_sha256(&serde_json::from_slice(ORIGINALS[2]).unwrap()).unwrap(),existing_origin:None,passive_value_exit:None,same_side_top_ups:None,execution_policy:None,valuation_objective:None,
        ordinary_probability:"0.950000".into(),exceptional_probability_allowance:"0.010000".into(),minimum_exceptional_payout:"0.000000".into(),margin_per_share:"0.010000".into(),maximum_principal:"4.000000".into(),maximum_quantity:5,
        reviewed_at_ms:at,expires_at_ms:at+120000,hold_until_ms:NOW+600000,payout_mapping_is_operator_assumption:true,revoke_existing:false,
        sources:["forecast","official_ballot","official_contract_node"].iter().enumerate().map(|(i,k)|SourceRef{kind:k.to_string(),url:format!("https://fixture.invalid/{k}"),sha256:sha(ORIGINALS[i]),published_at_ms:at-100,observed_at_ms:at-50,maximum_age_ms:120000}).collect()}
}
// New runtime cohorts explicitly review deadline CASH, independently of p.
// Synthetic model artifacts are generated only in these offline fixtures.
fn deadline_bytes(r:&ReviewInput)->Vec<u8>{
    let Some(ValuationObjective::DeadlineLiquidation{deadline_ms,expected_cash_per_share,model_sha256,..})=&r.valuation_objective else{panic!("deadline fixture")};
    serde_json::to_vec(&DeadlineLiquidationEvidence{version:1,kind:"reviewed_deadline_liquidation_cash_value".into(),tournament_id:r.tournament_id.clone(),market_id:r.market_id.clone(),exchange_id:r.exchange_id.clone(),contract_id:r.contract_id.clone(),side:r.side.clone(),deadline_ms:*deadline_ms,expected_cash_per_share:expected_cash_per_share.clone(),model_sha256:model_sha256.clone(),model:json!({"method":"synthetic independently reviewed cash estimate","notCalibrated":true}),assumption_label:"Synthetic deadline cash fixture; no forecast calibration or fill guarantee".into()}).unwrap()
}
fn set_deadline_cash(r:&mut ReviewInput,cash:&str){
    r.version=2;let model=sha(&serde_json::to_vec(&json!({"method":"synthetic independently reviewed cash estimate","notCalibrated":true})).unwrap());
    r.valuation_objective=Some(ValuationObjective::DeadlineLiquidation{deadline_ms:r.hold_until_ms,expected_cash_per_share:cash.into(),source_sha256:String::new(),model_sha256:model});
    let hash=sha(&deadline_bytes(r));if let Some(ValuationObjective::DeadlineLiquidation{source_sha256,..})=&mut r.valuation_objective{*source_sha256=hash.clone();}
    r.sources.retain(|s|s.kind!="deadline_liquidation_model"&&s.kind!="official_settlement_timing");r.sources.push(SourceRef{kind:"deadline_liquidation_model".into(),url:"https://fixture.invalid/independent-deadline-model".into(),sha256:hash,published_at_ms:r.reviewed_at_ms-100,observed_at_ms:r.reviewed_at_ms-50,maximum_age_ms:120000});
}
fn timing_bytes(r:&ReviewInput)->Vec<u8>{
    let Some(ValuationObjective::TerminalSettlement{scheduled_settlement_at_ms,..})=&r.valuation_objective else{panic!("terminal fixture")};
    serde_json::to_vec(&json!({"tournamentId":r.tournament_id,"marketId":r.market_id,"contractId":r.contract_id,"scheduledSettlementAt":iso(*scheduled_settlement_at_ms)})).unwrap()
}
fn set_terminal(r:&mut ReviewInput){
    r.version=2;r.sources.retain(|s|s.kind!="deadline_liquidation_model"&&s.kind!="official_settlement_timing");
    r.valuation_objective=Some(ValuationObjective::TerminalSettlement{timing_source_sha256:String::new(),timing_json_pointer:"/scheduledSettlementAt".into(),scheduled_settlement_at_ms:NOW+300000,delay_policy:TerminalDelayPolicy::HoldForActualSettlement});
    let hash=sha(&timing_bytes(r));if let Some(ValuationObjective::TerminalSettlement{timing_source_sha256,..})=&mut r.valuation_objective{*timing_source_sha256=hash.clone();}
    r.sources.push(SourceRef{kind:"official_settlement_timing".into(),url:"https://fixture.invalid/official-settlement-timing".into(),sha256:hash,published_at_ms:r.reviewed_at_ms-100,observed_at_ms:r.reviewed_at_ms-50,maximum_age_ms:120000});
}
fn horizon_input(manifest:&str,at:u64)->ReviewInput{let mut r=input(manifest,at);set_deadline_cash(&mut r,"0.940500");r}
fn originals_for(r:&ReviewInput)->Vec<Vec<u8>>{
    let mut originals=ORIGINALS.iter().map(|b|b.to_vec()).collect::<Vec<_>>();
    if matches!(&r.valuation_objective,Some(ValuationObjective::DeadlineLiquidation{..})){originals.push(deadline_bytes(r));}
    if r.sources.iter().any(|s|s.kind=="official_settlement_timing"){originals.push(timing_bytes(r));}originals
}
fn approve(r:ReviewInput,trust:&mut BTreeSet<String>,now:u64)->Reviewed{
    trust.insert(review_sha256(&r).unwrap());let originals=originals_for(&r);
    let hashes=originals.iter().map(|b|sha(b)).collect::<Vec<_>>();
    let sources=originals.iter().enumerate().map(|(i,b)|SourceBytes{sha256:&hashes[i],bytes:b}).collect::<Vec<_>>();
    Reviewed::verify(r,&sources,trust,now).unwrap()
}
struct F{root:PathBuf,bank:Session,log:dispatch::DispatchLog,owner:Arc<ExclusiveLock>,store:Option<Cohorts>,scope:Scope,trust:BTreeSet<String>,seq:u64,book_version:u128,review:ReviewInput}
impl Drop for F{fn drop(&mut self){self.store.take();let _=std::fs::remove_dir_all(&self.root);}}
impl F{
    fn new()->Self{
        Self::setup(true,"cup")
    }
    fn setup(install:bool,tournament:&str)->Self{
        Self::setup_quantity(install,tournament,2)
    }
    fn setup_quantity(install:bool,tournament:&str,quantity:u64)->Self{
        static NEXT:std::sync::atomic::AtomicU64=std::sync::atomic::AtomicU64::new(0);
        let root=std::env::temp_dir().join(format!("fundamental-fixture-{}-{}-{}",std::process::id(),std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos(),NEXT.fetch_add(1,std::sync::atomic::Ordering::Relaxed)));
        std::fs::create_dir(&root).unwrap();let owner=Arc::new(ExclusiveLock::acquire(&root.join("owner.lock")).unwrap());
        let policy=json!({"initialCash":"1000.000000","maximumCommitted":"1000.000000","maximumMarketCost":"100.000000","maximumMarkets":3,"quantity":quantity,"minimumSpreadTicks":2,"entryStopAt":iso(NOW+3600000),"inventoryExitMode":"fixedGain","maximumInventoryAgeMs":60000,"inventoryMinimumGain":"0.005000","inventoryAgeClock":"execution"});
        let seed=json!({"runId":"fundamental-fixture","availableCash":"1000.000000","accountSnapshotHash":"a".repeat(64),"observedAt":iso(NOW-10000)});
        let bank=Session::open_live(&root.join("bank"),&policy,&seed).unwrap();let log=dispatch::DispatchLog::open(&root.join("dispatch.ndjson")).unwrap();
        let mut review=horizon_input(bank.manifest_hash().as_str().unwrap(),NOW);review.tournament_id=tournament.into();set_deadline_cash(&mut review,"0.940500");let scope=Scope{tournament_id:tournament.into(),tournament_slug:"cup".into(),exchanges:[("101".into(),"201".into())].into()};
        let mut f=Self{root,bank,log,owner,store:None,scope,trust:BTreeSet::new(),seq:0,book_version:0,review};
        f.book(NOW-5000,"0.100000","0.120000");f.book(NOW-4000,"0.100000","0.120000");
        f.store=Some(Cohorts::initialize(&f.root,&f.bank,&f.log,f.owner.clone()).unwrap());if install{f.install(f.review.clone(),NOW);}f
    }
    fn install(&mut self,r:ReviewInput,at:u64){let reviewed=approve(r,&mut self.trust,at);self.store.as_mut().unwrap().install(reviewed,&self.bank,&self.log,at).unwrap();}
    // The native getter omits gap-invalidated books, but native rollback
    // checks retain their original versions. Remember the venue watermark
    // before invalidation, independently of the consecutive Session cursor.
    fn remember_book_version(&mut self){if let Some(b)=self.bank.engine().live_package_books().iter().find(|b|b["exchangeId"]=="101"){self.book_version=self.book_version.max(b["sequence"].as_str().unwrap().parse::<u128>().unwrap());}}
    fn book(&mut self,at:u64,bid:&str,ask:&str){self.remember_book_version();self.seq+=1;self.book_version=self.book_version.checked_add(1).unwrap();self.bank.accept("fixture-books",self.seq,&json!({"type":"book","receivedAt":iso(at),"book":{"marketId":"201","exchangeId":"101","tournamentId":self.scope.tournament_id,"sequence":self.book_version.to_string(),"sourceAt":iso(at),"requestStartedAt":iso(at),"receivedAt":iso(at),"bids":[{"price":bid,"quantity":100}],"asks":[{"price":ask,"quantity":100}]}})).unwrap();}
    fn entry(&self)->Value{self.bank.engine().order_records().into_iter().find(|q|q["side"]=="NO"&&q["action"]=="buy"&&q["retired"]==false).unwrap()}
    fn spec(&self,q:&Value)->RequestSpec{RequestSpec{method:"POST".into(),endpoint:"/orders".into(),body:Some(json!({"idempotencyKey":format!("fundamental-fixture:{}",q["id"].as_u64().unwrap()),"tournamentId":self.scope.tournament_id,"exchangeId":"101","side":if q["side"]=="NO"{"no"}else{"yes"},"action":q["action"],"quantity":q["quantity"],"price":serde_json::from_str::<Value>(q["price"].as_str().unwrap()).unwrap(),"expirationDate":iso(q["expiresAt"].as_u64().unwrap())})),body_json:None}}
    fn bind_entry(&mut self)->(Value,RequestSpec){let q=self.entry();let spec=self.spec(&q);assert!(matches!(self.store.as_mut().unwrap().bind(&self.bank,&self.log,&self.scope,q["id"].as_u64().unwrap(),&spec,&self.trust,NOW).unwrap(),Decision::Entry{..}));(q,spec)}
    fn slice_spec(&mut self)->(Value,RequestSpec){let q=self.entry();let mut r=self.review.clone();r.reviewed_at_ms=NOW+1;r.maximum_quantity=1;r.maximum_principal=q["price"].as_str().unwrap().into();self.install(r,NOW+1);let mut spec=self.spec(&q);spec.body.as_mut().unwrap()["quantity"]=json!(1);(q,spec)}
    fn log_intent(&mut self,q:&Value,spec:&RequestSpec){let qid=q["id"].as_u64().unwrap();self.log.append(json!({"kind":"intent","qid":qid,"quote":q,"spec":spec,"fundamental":self.store.as_ref().unwrap().durable_tag(qid).unwrap()})).unwrap();self.log.append(json!({"kind":"attempt","qid":qid})).unwrap();}
    fn event(&mut self,e:Value){self.remember_book_version();let n=self.bank.last_source_sequence("fixture-official")+1;self.bank.accept("fixture-official",n,&e).unwrap();self.remember_book_version();}
    fn base(q:&Value,kind:&str,at:u64,actual:&str)->Value{json!({"type":kind,"source":"official_sig_api_v1","receiptHash":"b".repeat(64),"orderId":q["id"],"actualOrderId":actual,"exchangeId":q["exchangeId"],"marketId":q["marketId"],"side":q["side"],"action":q["action"],"quantity":q["quantity"],"price":q["price"],"receivedAt":iso(at)})}
    fn fill_entry(&mut self,partial:bool)->u64{
        let(q,spec)=self.bind_entry();let id=q["id"].as_u64().unwrap();self.log_intent(&q,&spec);
        self.log.append(json!({"kind":"ack","qid":id,"receipt":{"orderId":"201"}})).unwrap();self.event(Self::base(&q,"actual_order_ack",NOW+1,"201"));
        let mut fill=Self::base(&q,"actual_fill",NOW+3,"201");fill["quantity"]=json!(if partial{1}else{2});fill["fillId"]=json!("401");fill["executedAt"]=json!(iso(NOW+2));fill["fee"]=json!("0.000000");self.event(fill);
        if !partial{let mut terminal=Self::base(&q,"actual_order_terminal",NOW+4,"201");terminal["status"]=json!("filled");terminal["fillsComplete"]=json!(true);terminal["filledQuantity"]=json!(2);self.event(terminal);self.log.append(json!({"kind":"closed","qid":id})).unwrap();}id
    }
    fn age_exit(&mut self,at:u64,bid:&str,ask:&str)->(Value,u64){
        self.event(json!({"type":"gap","exchangeId":"101","receivedAt":iso(at-1)}));
        for q in self.bank.engine().order_records(){let id=q["id"].as_u64().unwrap();if q["retired"]==false&&self.log.never_attempted(id){self.event(json!({"type":"unsubmitted_intent_retired","source":"moloch_live_dispatcher","orderId":id,"confirmedNetworkAttempts":0,"receiptHash":"c".repeat(64),"receivedAt":iso(at-1)}));}}
        self.book(at,bid,ask);
        // A genuine cancellation consumes a native planning write and its
        // latency/write-gap. Advance real clock ticks; do not erase actual
        // partial BUY liability or shorten the financial policy to force a
        // desired fixture. A fresh book remains within its original10s TTL.
        for step in 0..=2{let now=at+step*3001;if step>0{self.event(json!({"type":"tick","receivedAt":iso(now)}));}
            if let Some(q)=self.bank.engine().order_records().into_iter().find(|q|q["retired"]==false&&q["action"]=="sell"&&q["inventoryExitReason"]=="maximumAge"){return(q,now)}
        }panic!("no native age exit after actual retirement and bounded planning ticks: {:?}",self.bank.engine().order_records())
    }
    fn assess(&self,q:&Value,at:u64)->Decision{self.store.as_ref().unwrap().assess(&self.bank,&self.log,&self.scope,q["id"].as_u64().unwrap(),&self.trust,at).unwrap()}
    fn reopen(&mut self){self.store.take();self.store=Some(Cohorts::open(&self.root,&self.bank,&self.log,self.owner.clone()).unwrap());}
    fn authority(&self)->ReviewAuthority{let path=self.root.join("review-authority.json");let bytes=serde_json::to_vec(&json!({"version":1,"trustedReviewSha256":self.trust})).unwrap();std::fs::write(&path,&bytes).unwrap();ReviewAuthority::load(&path,&sha(&bytes)).unwrap()}
    fn permit(&self,q:&Value,at:u64)->Permit{Permit{run_id:"fundamental-fixture".into(),intent_id:q["id"].as_u64().unwrap().to_string(),owner:"Codex".into(),expires_at_ms:at+10000,maximum_quantity:q["quantity"].as_u64().unwrap() as u32,maximum_cost:moloch_replay::money(baal_transport_service::decimal_units(q["price"].as_str().unwrap()).unwrap()*q["quantity"].as_u64().unwrap() as i128)}}
    fn retire_drafts(&mut self,at:u64){for q in self.bank.engine().order_records(){let id=q["id"].as_u64().unwrap();if q["retired"]==false&&self.log.never_attempted(id){self.event(json!({"type":"unsubmitted_intent_retired","source":"moloch_live_dispatcher","orderId":id,"confirmedNetworkAttempts":0,"receiptHash":"c".repeat(64),"receivedAt":iso(at),"deferPlanningForPackage":true}));}}}
}
fn top_up_close(f:&mut F,q:&Value,at:u64,actual:&str,filled:u64,close:bool){
    use baal_transport_service::Receipt;use moloch_live_runtime::feedback;
    let id=q["id"].as_u64().unwrap();let side=if q["side"]=="NO"{"no"}else{"yes"};let signed=if side=="no"{-(filled as i64)}else{filled as i64};
    if !f.log.intents.contains_key(&id){let spec=f.spec(q);f.log_intent(q,&spec);}
    let price:Value=serde_json::from_str(q["price"].as_str().unwrap()).unwrap();
    let ack=Receipt{endpoint:"/orders".into(),status:200,request_started_at:iso(at),received_at:iso(at+1),data:json!({"orderId":actual,"exchangeId":"101","side":side,"action":q["action"],"quantity":q["quantity"],"price":price})};
    let e=feedback::acknowledgment(q,&ack,&iso(at+1)).unwrap();f.store.as_ref().unwrap().validate_acknowledgment(id,&e).unwrap();f.log.append(json!({"kind":"ack","qid":id,"receipt":ack})).unwrap();f.event(e);
    let order=Receipt{endpoint:format!("/orders/{actual}"),status:200,request_started_at:iso(at+4),received_at:iso(at+5),data:json!({"id":actual,"exchangeId":"101","tournamentId":f.scope.tournament_id,"side":side,"action":q["action"],"priceLimit":price,"quantity":q["quantity"],"quantityFilled":filled,"open":!close,"terminalReasonCode":"Canceled"})};
    let rows=if filled==0{json!([])}else{json!([{"id":actual.parse::<u64>().unwrap()*10+1,"price":price,"quantity":signed,"side":side,"filledAt":iso(at+3)}])};
    let tape=Receipt{endpoint:format!("/orders/{actual}/fills?limit=200"),status:200,request_started_at:iso(at+6),received_at:iso(at+7),data:json!({"orderId":actual,"exchangeId":"101","tournamentId":f.scope.tournament_id,"coverage":{"complete":true,"projectedThroughSequence":1000},"pagination":{"hasMore":false,"nextCursor":null},"totalQuantityFilled":signed,"data":rows})};
    for e in feedback::fills(q,actual,&f.scope.tournament_id,&tape,&iso(at+8)).unwrap(){f.event(e);}
    if close{let terminal=feedback::terminal(q,actual,&f.scope.tournament_id,&order,&tape,&iso(at+8)).unwrap().unwrap();f.event(terminal);f.log.append(json!({"kind":"closed","qid":id,"orderReceiptHash":feedback::digest(&order).unwrap(),"fillReceiptHash":feedback::digest(&tape).unwrap()})).unwrap();}
}
fn install_cohort_planning(f:&mut F,at:u64){
    let report=f.bank.engine().report().unwrap();let e=json!({"type":"managed_inventory_planning_v1","source":"moloch_live_dispatcher","receivedAt":iso(at),"processedAt":iso(at),"bankScope":{"runId":"fundamental-fixture","bankManifestHash":f.bank.manifest_hash(),"tournamentId":f.scope.tournament_id},"bankHead":f.bank.durable_head(),"expectedReportSha256":sha(&serde_json::to_vec(&report).unwrap()),"managedExchangeIds":["101"],"approvedReviewSetSha256":"b".repeat(64),"generationSha256":"0".repeat(64),"controlSha256":"c".repeat(64),"expectedPreviousModeSha256":null});f.event(e);
}

fn transport(f:&F)->baal_transport_service::Transport{use baal_transport_service::*;Transport::new_for_mock(Config{queue_dir:f.root.join("cohort-queue"),account_scope:"synthetic-passive-cohort-account".into(),scope:f.scope.clone(),maximum_pages:2,writes_enabled:false},Box::new(DisabledCredentials),Box::new(SystemClock),Box::new(ReadOnlyAuthority),"http://127.0.0.1:9/api/v1".into()).unwrap()}
fn ready(close:bool,filled:u64)->(F,Value){let mut f=F::setup(false,"11111111-1111-1111-1111-111111111111");let mut r=f.review.clone();set_terminal(&mut r);f.review=r.clone();f.install(r,NOW);let(q,_)=f.bind_entry();top_up_close(&mut f,&q,NOW+1,"901",filled,close);install_cohort_planning(&mut f,NOW+9000);f.retire_drafts(NOW+9001);let mut r=f.review.clone();r.reviewed_at_ms=NOW+10000;r.passive_value_exit=Some(PassiveValueExit{version:1,maximum_quantity:2});f.install(r.clone(),NOW+10000);f.review=r;f.book(NOW+12000,"0.300000","0.320000");(f,q)}
fn plan(f:&F,t:&baal_transport_service::Transport,at:u64)->Result<Option<ExistingExit>,Box<dyn std::error::Error>>{f.store.as_ref().unwrap().prepare_existing_exit(&f.bank,&f.log,t,&f.scope,"101",&f.trust,at)}
fn reserve(f:&mut F,t:&baal_transport_service::Transport,at:u64)->Value{let p=plan(f,t,at).unwrap().unwrap();f.event(p.event().clone());f.bank.engine().order_records().into_iter().find(|q|q["retired"]==false&&q["reviewedInventoryExit"]["executionMode"]=="passiveValueLimit").unwrap()}
#[test]fn explicit_v2_permission_is_not_an_entry_or_inventory_adoption(){let mut f=F::setup(true,"11111111-1111-1111-1111-111111111111");let mut r=f.review.clone();r.passive_value_exit=Some(PassiveValueExit{version:1,maximum_quantity:2});let originals=originals_for(&r);let hash=review_sha256(&r).unwrap();let checked=verify_originals(r.clone(),&originals).unwrap();assert!(checked.supports_new_cohort());f.trust.insert(hash);f.store.as_mut().unwrap().install(checked,&f.bank,&f.log,NOW+1).unwrap();let t=transport(&f);let before=f.bank.durable_head();assert!(plan(&f,&t,NOW+1).unwrap().is_none());assert!(f.store.as_ref().unwrap().existing_exit_hint(&f.bank,&f.log,&f.scope,"101",&f.trust,NOW+1).unwrap().is_none());assert_eq!(f.bank.durable_head(),before);assert_eq!(f.bank.engine().wallet.quantity("101",moloch_replay::Side::No),0);assert!(f.bank.engine().fills().is_empty());}
#[test]fn genuine_closed_owned_cohort_mints_same_value_limit_and_original_physical_token(){let(mut f,buy)=ready(true,2);let t=transport(&f);let before=f.bank.engine().report().unwrap();let hint=f.store.as_ref().unwrap().existing_exit_hint(&f.bank,&f.log,&f.scope,"101",&f.trust,NOW+15000).unwrap().unwrap();assert_eq!(hint.reason,"reviewed_passive_value_exit");let p=plan(&f,&t,NOW+15000).unwrap().unwrap();assert_eq!(p.event()["type"],"reviewed_cohort_exit_reserved_v1");assert_eq!(p.event()["cohortOrigin"]["entries"][0]["qid"],buy["id"]);assert_eq!(p.event()["cohortOrigin"]["entries"][0]["actualOrderId"],"901");assert!(p.event().get("adoptionRef").is_none());assert!(p.event().get("adoptionReceiptHash").is_none());assert_eq!(p.event()["expectedPayout"],"0.940500");assert_eq!(p.event()["limitPrice"],"0.945000");let q=reserve(&mut f,&t,NOW+15000);assert_eq!(q["reviewedInventoryExit"]["originKind"],"original_entry_cohort");assert_eq!(q["reviewedInventoryExit"]["minimumNetValue"],"0.940500");let at=q["activationAt"].as_u64().unwrap();let spec=f.spec(&q);assert!(matches!(f.store.as_mut().unwrap().bind(&f.bank,&f.log,&f.scope,q["id"].as_u64().unwrap(),&spec,&f.trust,at).unwrap(),Decision::Exit{..}));let auth=f.authority();let permit=f.permit(&q,at);let token=f.store.as_ref().unwrap().physical_decision(&f.bank,&f.log,&f.scope,q["id"].as_u64().unwrap(),&spec,&permit,&auth,at).unwrap();token.authorize(&f.scope,&spec,&permit,at).unwrap();for key in ["cash","ownedCost","realized","positions","fills"]{assert_eq!(f.bank.engine().report().unwrap()[key],before[key]);}assert_eq!(f.bank.engine().wallet.quantity("101",moloch_replay::Side::No),2);}
#[test]fn partial_buy_requires_original_terminal_and_partial_sell_retains_fifo_across_recovery(){let(mut blocked,_)=ready(false,1);let t=transport(&blocked);let head=blocked.bank.durable_head();let p=plan(&blocked,&t,NOW+15000);assert!(p.is_err()||p.unwrap().is_none());assert_eq!(blocked.bank.durable_head(),head);blocked.reopen();assert_eq!(blocked.bank.engine().wallet.quantity("101",moloch_replay::Side::No),1);drop(t);let(mut f,_)=ready(true,1);let t=transport(&f);let q=reserve(&mut f,&t,NOW+15000);assert_eq!(q["quantity"],1,"only actual filled and terminal-reconciled BUY units owned");}
#[test]fn actual_partial_sell_unknown_remainder_terminal_and_replay_do_not_double_allocate(){let(mut f,_)=ready(true,2);let t=transport(&f);let q=reserve(&mut f,&t,NOW+15000);let id=q["id"].as_u64().unwrap();let at=q["activationAt"].as_u64().unwrap();let spec=f.spec(&q);f.store.as_mut().unwrap().bind(&f.bank,&f.log,&f.scope,id,&spec,&f.trust,at).unwrap();top_up_close(&mut f,&q,at+1,"902",1,false);assert_eq!(f.bank.engine().wallet.quantity("101",moloch_replay::Side::No),1);assert_eq!(f.bank.engine().wallet.market_cost("101"),880000);assert_eq!(f.bank.engine().report().unwrap()["newTradingRealized"],"0.065000");let current=f.bank.engine().report().unwrap();assert!(plan(&f,&t,at+10).is_err());f.reopen();assert_eq!(f.bank.engine().report().unwrap(),current);assert!(plan(&f,&t,at+10).is_err());let side="no";let price:Value=serde_json::from_str(q["price"].as_str().unwrap()).unwrap();let order=baal_transport_service::Receipt{endpoint:"/orders/902".into(),status:200,request_started_at:iso(at+11),received_at:iso(at+12),data:json!({"id":"902","exchangeId":"101","tournamentId":f.scope.tournament_id,"side":side,"action":"sell","priceLimit":price,"quantity":2,"quantityFilled":1,"open":false,"terminalReasonCode":"Canceled"})};let tape=baal_transport_service::Receipt{endpoint:"/orders/902/fills?limit=200".into(),status:200,request_started_at:iso(at+13),received_at:iso(at+14),data:json!({"orderId":"902","exchangeId":"101","tournamentId":f.scope.tournament_id,"coverage":{"complete":true},"pagination":{"hasMore":false,"nextCursor":null},"totalQuantityFilled":-1,"data":[{"id":9021,"price":price,"quantity":-1,"side":side,"filledAt":iso(at+4)}]})};let end=moloch_live_runtime::feedback::terminal(&q,"902",&f.scope.tournament_id,&order,&tape,&iso(at+15)).unwrap().unwrap();f.event(end);f.log.append(json!({"kind":"closed","qid":id,"orderReceiptHash":moloch_live_runtime::feedback::digest(&order).unwrap(),"fillReceiptHash":moloch_live_runtime::feedback::digest(&tape).unwrap()})).unwrap();f.book(at+16,"0.300000","0.320000");let p=plan(&f,&t,at+17).unwrap().unwrap();assert_eq!(p.event()["quantity"],1);assert_eq!(p.event()["cohortOrigin"]["exits"][0]["filledQuantity"],1);assert_eq!(p.event()["cohortOrigin"]["heldCost"],"0.880000");assert_eq!(f.bank.engine().wallet.quantity("101",moloch_replay::Side::No),1);}
#[test]fn original_closed_receipt_provenance_unknown_and_unreviewed_modes_fail_without_native_change(){for case in ["unclosed","actual","endpoint","unknown","no_policy"]{let(mut f,buy)=ready(true,2);let t=transport(&f);let id=buy["id"].as_u64().unwrap();match case{"unclosed"=>{f.log.closed.remove(&id);},"actual"=>f.log.acknowledgments.get_mut(&id).unwrap()["data"]["orderId"]=json!("999"),"endpoint"=>f.log.acknowledgments.get_mut(&id).unwrap()["endpoint"]=json!("/not-original"),"unknown"=>{f.log.append(json!({"kind":"intent","qid":999})).unwrap();f.log.append(json!({"kind":"attempt","qid":999})).unwrap();},_=>{let mut r=f.review.clone();r.passive_value_exit=None;r.reviewed_at_ms=NOW+13000;f.install(r.clone(),NOW+13000);f.review=r;}}let head=f.bank.durable_head();let p=plan(&f,&t,NOW+15000);assert!(p.is_err()||p.unwrap().is_none(),"{case}");assert_eq!(f.bank.durable_head(),head);assert_eq!(f.bank.engine().wallet.quantity("101",moloch_replay::Side::No),2);}}
#[test]fn source_expiry_and_review_change_do_not_renew_value_or_release_owned_units(){let(mut f,_)=ready(true,2);let t=transport(&f);let q=reserve(&mut f,&t,NOW+15000);let id=q["id"].as_u64().unwrap();let at=q["activationAt"].as_u64().unwrap();let spec=f.spec(&q);f.store.as_mut().unwrap().bind(&f.bank,&f.log,&f.scope,id,&spec,&f.trust,at).unwrap();top_up_close(&mut f,&q,at+1,"902",0,false);let sources=serde_json::to_value(&f.review.sources).unwrap();let mut r=f.review.clone();r.reviewed_at_ms=at+10;r.passive_value_exit=None;f.install(r.clone(),at+10);f.review=r;assert_eq!(serde_json::to_value(&f.review.sources).unwrap(),sources);assert_eq!(f.store.as_ref().unwrap().existing_exit_cancellations(&f.bank,&f.log,&f.trust,at+10).unwrap(),vec![id]);assert_eq!(f.bank.engine().wallet.quantity("101",moloch_replay::Side::No),2);assert_eq!(f.bank.engine().fills().len(),1);}
