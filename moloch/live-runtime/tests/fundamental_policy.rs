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
#[test]fn omitted_execution_policy_preserves_pre_extension_review_hash_and_explicit_modes_need_new_approval(){
    let legacy=input(&"a".repeat(64),NOW);assert_eq!(review_sha256(&legacy).unwrap(),"fefdc6fcdc248bd39ec3e0886780ed114ea32b8d2fe419ee0286c70795cf14a5","frozen legacy canonical review fixture");assert!(serde_json::to_value(&legacy).unwrap().get("executionPolicy").is_none());
    let bytes=serde_json::to_vec(&legacy).unwrap();let decoded:ReviewInput=serde_json::from_slice(&bytes).unwrap();assert!(decoded.execution_policy.is_none());assert_eq!(serde_json::to_vec(&decoded).unwrap(),bytes);
    let mut extended=legacy.clone();extended.execution_policy=Some(ExecutionPolicy{version:1,allowed_modes:vec![ExecutionMode::BoundedLimitMayRest,ExecutionMode::ImproveOneTick,ExecutionMode::JoinBid]});assert_ne!(review_sha256(&extended).unwrap(),review_sha256(&legacy).unwrap());let trust=[review_sha256(&legacy).unwrap()].into();let hashes=ORIGINALS.iter().map(|b|sha(b)).collect::<Vec<_>>();let sources=ORIGINALS.iter().enumerate().map(|(i,b)|SourceBytes{sha256:&hashes[i],bytes:b}).collect::<Vec<_>>();assert!(Reviewed::verify(extended,&sources,&trust,NOW).is_err());
}
#[test]fn top_up_permission_is_explicit_omitted_by_default_and_bounded(){
 let old=input(&"a".repeat(64),NOW);assert_eq!(review_sha256(&old).unwrap(),"fefdc6fcdc248bd39ec3e0886780ed114ea32b8d2fe419ee0286c70795cf14a5");assert!(serde_json::to_value(&old).unwrap().get("sameSideTopUps").is_none());
 for (version,n) in [(1,1),(1,17),(2,2)]{let mut r=horizon_input(&"a".repeat(64),NOW);r.same_side_top_ups=Some(SameSideTopUps{version,maximum_entries:n});assert!(verify_originals(r.clone(),&originals_for(&r)).is_err());}
 let mut r=horizon_input(&"a".repeat(64),NOW);let old_sha=review_sha256(&r).unwrap();r.same_side_top_ups=Some(SameSideTopUps{version:1,maximum_entries:2});assert_ne!(review_sha256(&r).unwrap(),old_sha);verify_originals(r.clone(),&originals_for(&r)).unwrap();r.version=1;r.valuation_objective=None;assert!(verify_originals(r.clone(),&originals_for(&r)).is_err());
}
#[test]fn explicit_execution_policy_rejects_empty_duplicate_unknown_or_bad_version_modes(){
    for policy in [json!({"version":1,"allowedModes":[]}),json!({"version":1,"allowedModes":["joinBid","joinBid"]}),json!({"version":2,"allowedModes":["joinBid"]}),json!({"version":1,"allowedModes":["marketOrder"]})]{let mut raw=serde_json::to_value(input(&"a".repeat(64),NOW)).unwrap();raw["executionPolicy"]=policy;let parsed=serde_json::from_value::<ReviewInput>(raw);if let Ok(r)=parsed{let hash=review_sha256(&r).unwrap();let mut trust=BTreeSet::new();trust.insert(hash);let hashes=ORIGINALS.iter().map(|b|sha(b)).collect::<Vec<_>>();let sources=ORIGINALS.iter().enumerate().map(|(i,b)|SourceBytes{sha256:&hashes[i],bytes:b}).collect::<Vec<_>>();assert!(Reviewed::verify(r,&sources,&trust,NOW).is_err());}}
}
#[test]fn conditional_exception_payoff_and_entry_buffer_use_exact_conservative_arithmetic(){
    let mut t=BTreeSet::new();let r=input(&"a".repeat(64),NOW);assert_eq!(approve(r.clone(),&mut t,NOW).entry_ceiling().unwrap(),"0.930500");
    let mut r=r;r.minimum_exceptional_payout="0.900000".into();assert_eq!(approve(r.clone(),&mut t,NOW).entry_ceiling().unwrap(),"0.939500");
    r.ordinary_probability="0.950001".into();r.exceptional_probability_allowance="0.000001".into();r.minimum_exceptional_payout="0".into();r.margin_per_share="0".into();
    assert_eq!(approve(r,&mut t,NOW).entry_ceiling().unwrap(),"0.950000","fractional millionth cannot be rounded up");
}
#[test]fn input_cannot_approve_itself_replace_source_bytes_or_claim_guaranteed_mapping(){
    let mut t=BTreeSet::new();let r=input(&"a".repeat(64),NOW);let hashes=ORIGINALS.iter().map(|b|sha(b)).collect::<Vec<_>>();
    let sources=ORIGINALS.iter().enumerate().map(|(i,b)|SourceBytes{sha256:&hashes[i],bytes:b}).collect::<Vec<_>>();
    assert!(Reviewed::verify(r.clone(),&sources,&t,NOW).is_err());t.insert(review_sha256(&r).unwrap());
    let bad=[SourceBytes{sha256:&hashes[0],bytes:b"changed"},SourceBytes{sha256:&hashes[1],bytes:ORIGINALS[1]},SourceBytes{sha256:&hashes[2],bytes:ORIGINALS[2]}];
    assert!(Reviewed::verify(r.clone(),&bad,&t,NOW).is_err());
    let mut r=r;r.payout_mapping_is_operator_assumption=false;t.insert(review_sha256(&r).unwrap());assert!(Reviewed::verify(r,&sources,&t,NOW).is_err());
}
#[test]fn future_stale_and_out_of_range_model_inputs_are_rejected(){
    for case in ["future","expired","probability","exception","principal","source_clock"]{
        let mut r=input(&"a".repeat(64),NOW);match case{"future"=>r.reviewed_at_ms=NOW+1,"expired"=>r.expires_at_ms=NOW,"probability"=>r.ordinary_probability="1.000001".into(),"exception"=>r.exceptional_probability_allowance="-0.01".into(),"principal"=>r.maximum_principal="0".into(),_=>r.sources[0].published_at_ms=NOW+1}
        let trust=[review_sha256(&r).unwrap()].into();let hashes=ORIGINALS.iter().map(|b|sha(b)).collect::<Vec<_>>();let sources=ORIGINALS.iter().enumerate().map(|(i,b)|SourceBytes{sha256:&hashes[i],bytes:b}).collect::<Vec<_>>();
        assert!(Reviewed::verify(r,&sources,&trust,NOW).is_err(),"{case}");
    }
}
fn verify_with_node(mut review:ReviewInput,node:&[u8])->moloch_live_runtime::feedback::Result<Reviewed>{
    let mut originals=ORIGINALS.iter().map(|b|b.to_vec()).collect::<Vec<_>>();originals[2]=node.to_vec();
    let hashes=originals.iter().map(|b|sha(b)).collect::<Vec<_>>();review.sources[2].sha256=hashes[2].clone();
    let trust=[review_sha256(&review)?].into();let sources=originals.iter().enumerate().map(|(i,b)|SourceBytes{sha256:&hashes[i],bytes:b}).collect::<Vec<_>>();
    Reviewed::verify(review,&sources,&trust,NOW)
}
#[test]fn approved_original_node_bytes_must_match_exact_contract_scope_and_root_semantics(){
    for case in ["market","node_id","contract_id","node_type","party","review_hash"]{
        let mut review=input(&"a".repeat(64),NOW);let mut node:Value=serde_json::from_slice(ORIGINALS[2]).unwrap();
        match case{"market"=>node["market_id"]=json!("202"),"node_id"=>node["root"]["node_id"]=json!("302"),"contract_id"=>node["root"]["contract_id"]=json!("302"),"node_type"=>node["root"]["node_type"]=json!("group"),"party"=>node["root"]["contract_details"]["winnerName"]=json!("Democratic Party"),_=>review.official_contract_root_sha256="b".repeat(64)}
        assert!(verify_with_node(review,&serde_json::to_vec(&node).unwrap()).is_err(),"even an explicitly approved raw source hash cannot bypass {case}");
    }
}
#[test]fn original_node_receipt_envelope_is_accepted_only_for_the_exact_official_scope(){
    let review=input(&"a".repeat(64),NOW);let mut receipt=json!({"endpoint":"/markets/201/nodes?tournamentId=cup","requestStartedAt":iso(NOW-100),"receivedAt":iso(NOW-50),"status":200,"data":serde_json::from_slice::<Value>(ORIGINALS[2]).unwrap()});
    verify_with_node(review.clone(),&serde_json::to_vec(&receipt).unwrap()).unwrap();
    receipt["endpoint"]=json!("/markets/201/nodes?tournamentId=other");assert!(verify_with_node(review.clone(),&serde_json::to_vec(&receipt).unwrap()).is_err());
    receipt["endpoint"]=json!("/markets/201/nodes?tournamentId=cup");receipt["status"]=json!(503);assert!(verify_with_node(review,&serde_json::to_vec(&receipt).unwrap()).is_err());
}

fn verify_originals(r:ReviewInput,originals:&[Vec<u8>])->moloch_live_runtime::feedback::Result<Reviewed>{
    let at=r.reviewed_at_ms;let trusted=[review_sha256(&r)?].into();let hashes=originals.iter().map(|b|sha(b)).collect::<Vec<_>>();
    let sources=originals.iter().enumerate().map(|(i,b)|SourceBytes{sha256:&hashes[i],bytes:b}).collect::<Vec<_>>();Reviewed::verify(r,&sources,&trusted,at)
}
#[test]fn v2_deadline_cash_is_independent_of_terminal_probability_and_source_bound(){
    let mut r=horizon_input(&"a".repeat(64),NOW);set_deadline_cash(&mut r,"0.400000");
    r.ordinary_probability="0.999999".into();assert_eq!(verify_originals(r.clone(),&originals_for(&r)).unwrap().entry_ceiling().unwrap(),"0.390000");
    r.ordinary_probability="0.010000".into();let verified=verify_originals(r.clone(),&originals_for(&r)).unwrap();assert_eq!(verified.entry_ceiling().unwrap(),"0.390000");assert!(verified.supports_new_cohort());
    for case in ["missing","deadline","cash","model","scope","type"]{
        let mut bad=r.clone();let mut originals=originals_for(&bad);match case{
            "missing"=>{originals.pop();},"deadline"=>bad.hold_until_ms+=1,
            "cash"=>if let Some(ValuationObjective::DeadlineLiquidation{expected_cash_per_share,..})=&mut bad.valuation_objective{*expected_cash_per_share="0.990000".into();},
            "type"=>bad.sources[3].kind="forecast".into(),
            _=>{let mut raw:Value=serde_json::from_slice(&originals[3]).unwrap();if case=="scope"{raw["side"]=json!("YES")}else{raw["model"]["method"]=json!("changed model")};originals[3]=serde_json::to_vec(&raw).unwrap();let hash=sha(&originals[3]);bad.sources[3].sha256=hash.clone();if let Some(ValuationObjective::DeadlineLiquidation{source_sha256,..})=&mut bad.valuation_objective{*source_sha256=hash;}}
        }assert!(verify_originals(bad,&originals).is_err(),"{case}");
    }
}
#[test]fn terminal_requires_original_exact_official_timing_and_explicit_delayed_payout_policy(){
    let mut r=input(&"a".repeat(64),NOW);set_terminal(&mut r);assert_eq!(verify_originals(r.clone(),&originals_for(&r)).unwrap().entry_ceiling().unwrap(),"0.930500");
    for case in ["early_deadline","wrong_pointer","null","wrong_scope","wrong_clock"]{
        let mut bad=r.clone();let mut originals=originals_for(&bad);match case{
            "early_deadline"=>bad.hold_until_ms=NOW+200000,
            "wrong_pointer"=>if let Some(ValuationObjective::TerminalSettlement{timing_json_pointer,..})=&mut bad.valuation_objective{*timing_json_pointer="/electionDate".into();},
            _=>{let mut raw:Value=serde_json::from_slice(&originals[3]).unwrap();match case{"null"=>raw["scheduledSettlementAt"]=Value::Null,"wrong_scope"=>raw["contractId"]=json!("other"),_=>raw["scheduledSettlementAt"]=json!(iso(NOW+300001))};originals[3]=serde_json::to_vec(&raw).unwrap();let hash=sha(&originals[3]);bad.sources[3].sha256=hash.clone();if let Some(ValuationObjective::TerminalSettlement{timing_source_sha256,..})=&mut bad.valuation_objective{*timing_source_sha256=hash;}}
        }assert!(verify_originals(bad,&originals).is_err(),"{case}");
    }
    let mut null=input(&"a".repeat(64),NOW);null.version=2;null.valuation_objective=Some(ValuationObjective::TerminalSettlement{timing_source_sha256:null.sources[2].sha256.clone(),timing_json_pointer:"/root/settlement_date".into(),scheduled_settlement_at_ms:NOW+300000,delay_policy:TerminalDelayPolicy::HoldForActualSettlement});assert!(verify_originals(null.clone(),&originals_for(&null)).is_err(),"official null timing is not inferred from electionDate");
}
#[test]fn terminal_can_bind_original_node_timing_without_an_extra_source(){
    for receipt in [false,true]{
        let mut node:Value=serde_json::from_slice(ORIGINALS[2]).unwrap();node["root"]["settlement_date"]=json!(iso(NOW+300000));
        let mut r=input(&"a".repeat(64),NOW);r.version=2;r.official_contract_root_sha256=official_contract_root_sha256(&node).unwrap();
        let original=if receipt{json!({"status":200,"endpoint":"/markets/201/nodes?tournamentId=cup","data":node})}else{node};
        let mut originals=originals_for(&r);originals[2]=serde_json::to_vec(&original).unwrap();r.sources[2].sha256=sha(&originals[2]);
        r.valuation_objective=Some(ValuationObjective::TerminalSettlement{timing_source_sha256:r.sources[2].sha256.clone(),timing_json_pointer:if receipt{"/data/root/settlement_date"}else{"/root/settlement_date"}.into(),scheduled_settlement_at_ms:NOW+300000,delay_policy:TerminalDelayPolicy::HoldForActualSettlement});
        let verified=verify_originals(r.clone(),&originals).unwrap();assert!(verified.supports_new_cohort());assert_eq!(verified.entry_ceiling().unwrap(),"0.930500");assert_eq!(r.sources.len(),3);
    }
}
#[test]fn objective_version_omission_and_legacy_bytes_remain_unambiguous(){
    let old=input(&"a".repeat(64),NOW);let original=serde_json::to_vec(&old).unwrap();assert_eq!(review_sha256(&old).unwrap(),"fefdc6fcdc248bd39ec3e0886780ed114ea32b8d2fe419ee0286c70795cf14a5");assert!(!verify_originals(old.clone(),&originals_for(&old)).unwrap().supports_new_cohort());
    assert!(!serde_json::to_value(&old).unwrap().as_object().unwrap().contains_key("valuationObjective"));assert_eq!(serde_json::to_vec(&serde_json::from_slice::<ReviewInput>(&original).unwrap()).unwrap(),original);
    let mut absent=old.clone();absent.version=2;assert!(verify_originals(absent.clone(),&originals_for(&absent)).is_err());
    let mut mislabeled=horizon_input(&"a".repeat(64),NOW);mislabeled.version=1;assert!(verify_originals(mislabeled.clone(),&originals_for(&mislabeled)).is_err());
}

#[cfg(unix)]mod actual_session{
    use super::*;
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
    #[test]fn fixture_venue_watermark_survives_gap_without_skipping_session_cursor(){
        let mut f=F::new();f.event(json!({"type":"gap","exchangeId":"101","receivedAt":iso(NOW+1)}));assert!(f.bank.engine().live_package_books().is_empty());f.book(NOW+2,"0.100000","0.120000");assert_eq!(f.bank.engine().live_package_books()[0]["sequence"],"3");assert_eq!(f.bank.last_source_sequence("fixture-books"),3);
        let(mut f,_transport,_facts)=entry_fixture();assert_eq!(f.bank.engine().live_package_books()[0]["sequence"],"99");assert_eq!(f.bank.last_source_sequence("fixture-books"),2);
        for (at,version,cursor) in [(NOW+30,"100",3),(NOW+32,"101",4)]{f.event(json!({"type":"gap","exchangeId":"101","receivedAt":iso(at)}));assert!(f.bank.engine().live_package_books().is_empty());f.book(at+1,"0.100000","0.120000");let raw=f.bank.engine().live_package_books().remove(0);assert_eq!(raw["sequence"],version);assert_eq!(raw["sourceAt"],iso(at+1));assert_eq!(f.bank.last_source_sequence("fixture-books"),cursor);}
        let mut rollback=f.bank.engine().live_package_books().remove(0);rollback["sequence"]=json!("100");rollback["sourceAt"]=json!(iso(NOW+34));rollback["requestStartedAt"]=json!(iso(NOW+34));rollback["receivedAt"]=json!(iso(NOW+34));let head=f.bank.durable_head();assert!(f.bank.accept("fixture-real-rollback",1,&json!({"type":"book","receivedAt":iso(NOW+34),"book":rollback})).is_err());assert_eq!(f.bank.durable_head(),head);
    }
    #[test]fn original_seven_receipt_entry_plan_uses_reviewed_deadline_cash_not_terminal_payout(){
        let(mut f,t,facts)=entry_fixture();let mut r=f.review.clone();r.reviewed_at_ms=NOW+1;r.ordinary_probability="0.999999".into();set_deadline_cash(&mut r,"0.910000");f.install(r.clone(),NOW+1);
        f.retire_drafts(NOW+24);let authority=f.authority();let hint=f.store.as_ref().unwrap().entry_hints(&f.bank,&f.log,&f.scope,&f.trust,NOW+25).unwrap().remove(0);
        let plan=f.store.as_ref().unwrap().prepare_entry(&f.bank,&f.log,&t,&f.scope,&hint,&facts,&authority,NOW+25).unwrap().unwrap();
        assert_eq!(plan.event()["expectedPayout"],"0.910000");assert_eq!(plan.event()["reviewSha256"],review_sha256(&r).unwrap());assert_eq!(plan.event()["marginPerShare"],"0.010000");
        f.bank.accept("objective-entry",1,plan.event()).unwrap();let q=f.bank.engine().order_records().into_iter().find(|q|q["reviewedFundamentalEntry"]["reviewSha256"]==review_sha256(&r).unwrap()).unwrap();assert!(baal_transport_service::decimal_units(q["price"].as_str().unwrap()).unwrap()<=900000);
    }
    #[test]fn high_terminal_probability_cannot_originate_above_independently_reviewed_deadline_cash(){
        let mut f=F::new();let mut r=f.review.clone();r.reviewed_at_ms=NOW+1;r.ordinary_probability="0.999999".into();set_deadline_cash(&mut r,"0.400000");f.install(r.clone(),NOW+1);
        let q=f.entry();assert!(matches!(f.assess(&q,NOW+1),Decision::Hold{..}),"terminal odds cannot lift deadline ceiling");
        f.book(NOW+1000,"0.700000","0.800000");let hint=f.store.as_ref().unwrap().entry_hints(&f.bank,&f.log,&f.scope,&f.trust,NOW+1000).unwrap().remove(0);
        let comparison=f.store.as_ref().unwrap().compare_entry_actions(&f.bank,&f.scope,&hint,&f.trust,NOW+1000).unwrap().unwrap();
        assert!(!comparison.candidates.is_empty());assert!(comparison.candidates.iter().all(|c|c.limit_price()<=390000));
        let model_hash=comparison.candidates.first().map(|c|c.provenance()["modelSha256"].clone());
        r.reviewed_at_ms=NOW+1001;r.ordinary_probability="0.010000".into();f.install(r.clone(),NOW+1001);
        let hint=f.store.as_ref().unwrap().entry_hints(&f.bank,&f.log,&f.scope,&f.trust,NOW+1001).unwrap().remove(0);
        let comparison=f.store.as_ref().unwrap().compare_entry_actions(&f.bank,&f.scope,&hint,&f.trust,NOW+1001).unwrap().unwrap();assert!(comparison.candidates.iter().all(|c|c.limit_price()<=390000));
        if let(Some(a),Some(b))=(model_hash,comparison.candidates.first()){assert_eq!(a,b.provenance()["modelSha256"],"terminal p is not the deadline cash model");}
    }
    #[test]fn new_legacy_cohort_is_withheld_but_v2_requires_separate_explicit_approval(){
        let mut f=F::setup(false,"cup");let r=input(f.bank.manifest_hash().as_str().unwrap(),NOW);f.install(r.clone(),NOW);let q=f.entry();
        assert!(matches!(f.assess(&q,NOW),Decision::Hold{reason}if reason=="legacy_review_requires_explicit_valuation_objective_for_new_cohort"));
        assert!(f.store.as_ref().unwrap().entry_hints(&f.bank,&f.log,&f.scope,&f.trust,NOW).unwrap().is_empty());
        let mut revised=horizon_input(f.bank.manifest_hash().as_str().unwrap(),NOW+1);set_deadline_cash(&mut revised,"0.940500");let bytes=originals_for(&revised);let hashes=bytes.iter().map(|b|sha(b)).collect::<Vec<_>>();let refs=bytes.iter().enumerate().map(|(i,b)|SourceBytes{sha256:&hashes[i],bytes:b}).collect::<Vec<_>>();assert!(Reviewed::verify(revised.clone(),&refs,&f.trust,NOW+1).is_err());
        f.install(revised,NOW+1);assert!(matches!(f.assess(&q,NOW+1),Decision::Entry{..}));
    }
    #[test]fn terminal_objective_survives_schedule_delay_without_extending_an_existing_legacy_cohort(){
        let mut f=F::setup(false,"cup");let mut r=f.review.clone();set_terminal(&mut r);f.review=r.clone();f.install(r,NOW);f.fill_entry(false);
        let mut changed=f.review.clone();changed.reviewed_at_ms=NOW+62000;set_deadline_cash(&mut changed,"0.940500");let reviewed=approve(changed,&mut f.trust,NOW+62000);assert!(f.store.as_mut().unwrap().install(reviewed,&f.bank,&f.log,NOW+62000).unwrap_err().to_string().contains("CANNOT_REINTERPRET_COHORT_OBJECTIVE"));
        let(q,at)=f.age_exit(NOW+600001,"0.100000","0.120000");assert!(matches!(f.assess(&q,at),Decision::Hold{reason}if reason=="terminal_settlement_hold_for_actual_payout"));
        f.reopen();assert!(matches!(f.assess(&q,at),Decision::Hold{..}),"restart cannot turn schedule delay into forced liquidation");
        let mut deadline=F::new();deadline.fill_entry(false);let(q,at)=deadline.age_exit(NOW+600001,"0.100000","0.120000");assert!(matches!(deadline.assess(&q,at),Decision::Exit{reason,..}if reason=="reviewed_holding_deadline"));
    }
    fn refreshed_terminal(r:&ReviewInput,at:u64)->(ReviewInput,Vec<Vec<u8>>){
        let mut r=r.clone();r.reviewed_at_ms=at;r.expires_at_ms=at+120000;
        for source in &mut r.sources{source.observed_at_ms=at-50;}
        let mut originals=originals_for(&r);let mut timing:Value=serde_json::from_slice(&originals[3]).unwrap();timing["capturedAt"]=json!(iso(at-50));originals[3]=serde_json::to_vec(&timing).unwrap();let hash=sha(&originals[3]);r.sources[3].sha256=hash.clone();if let Some(ValuationObjective::TerminalSettlement{timing_source_sha256,..})=&mut r.valuation_objective{*timing_source_sha256=hash;}(r,originals)
    }
    #[test]fn delayed_terminal_fresh_revocation_still_mints_exact_covered_sell_physical_token(){
        let mut f=F::setup(false,"cup");let mut initial=f.review.clone();set_terminal(&mut initial);f.install(initial.clone(),NOW);f.fill_entry(false);
        let mut initial=initial;initial.revoke_existing=true;let(r,originals)=refreshed_terminal(&initial,NOW+620000);let reviewed=verify_originals(r.clone(),&originals).unwrap();f.trust.insert(review_sha256(&r).unwrap());f.store.as_mut().unwrap().install(reviewed,&f.bank,&f.log,NOW+620000).unwrap();
        assert_eq!(r.hold_until_ms,NOW+600000);assert!(f.store.as_ref().unwrap().entry_hints(&f.bank,&f.log,&f.scope,&f.trust,NOW+620000).unwrap().is_empty());
        let(q,at)=f.age_exit(NOW+620001,"0.100000","0.120000");let active=at.max(q["activationAt"].as_u64().unwrap());assert!(matches!(f.assess(&q,active),Decision::Exit{reason,..}if reason=="reviewed_source_revocation"));
        let id=q["id"].as_u64().unwrap();let spec=f.spec(&q);f.store.as_mut().unwrap().bind(&f.bank,&f.log,&f.scope,id,&spec,&f.trust,active).unwrap();let permit=f.permit(&q,active);let authority=f.authority();
        let token=f.store.as_ref().unwrap().physical_decision(&f.bank,&f.log,&f.scope,id,&spec,&permit,&authority,active).unwrap();assert!(token.expires_at_ms()>active);token.validate_frozen(&q,&f.bank.engine().report().unwrap(),id,active).unwrap();token.authorize(&f.scope,&spec,&permit,active).unwrap();drop(token);drop(authority);f.reopen();assert!(matches!(f.assess(&q,active),Decision::Exit{..}));
        // Even a newly source-verified schedule cannot renew this cohort.
        let revision_at=active+1;let(mut changed,mut originals)=refreshed_terminal(&r,revision_at);if let Some(ValuationObjective::TerminalSettlement{scheduled_settlement_at_ms,..})=&mut changed.valuation_objective{*scheduled_settlement_at_ms+=1;}
        let mut timing:Value=serde_json::from_slice(&originals[3]).unwrap();timing["scheduledSettlementAt"]=json!(iso(NOW+300001));originals[3]=serde_json::to_vec(&timing).unwrap();let hash=sha(&originals[3]);changed.sources[3].sha256=hash.clone();if let Some(ValuationObjective::TerminalSettlement{timing_source_sha256,..})=&mut changed.valuation_objective{*timing_source_sha256=hash;}
        let reviewed=verify_originals(changed.clone(),&originals).unwrap();f.trust.insert(review_sha256(&changed).unwrap());let error=f.store.as_mut().unwrap().install(reviewed,&f.bank,&f.log,revision_at).unwrap_err().to_string();assert!(error.contains("CANNOT_REINTERPRET_COHORT_OBJECTIVE"),"unexpected earlier guard: {error}");
    }
    #[test]fn inherited_delayed_terminal_current_value_exit_plan_preserves_original_hold_clock(){
        let(mut f,t,adoption)=inherited_fixture();let snapshot=ExistingSnapshot::capture(&f.bank,&f.log,&t,"fixture-adoption",1,&adoption,"101").unwrap();let mut r=f.review.clone();set_terminal(&mut r);r.existing_origin=Some(snapshot.approval().clone());let reviewed=approve(r.clone(),&mut f.trust,NOW+3001);f.store.as_mut().unwrap().install_existing(reviewed,snapshot,&f.bank,&f.log,&t,NOW+3001).unwrap();
        r.ordinary_probability="0.600000".into();let(r,originals)=refreshed_terminal(&r,NOW+620000);let reviewed=verify_originals(r.clone(),&originals).unwrap();f.trust.insert(review_sha256(&r).unwrap());f.store.as_mut().unwrap().install(reviewed,&f.bank,&f.log,NOW+620000).unwrap();
        f.book(NOW+620001,"0.300000","0.320000");f.retire_drafts(NOW+623002);
        let plan=f.store.as_ref().unwrap().prepare_existing_exit(&f.bank,&f.log,&t,&f.scope,"101",&f.trust,NOW+623002).unwrap().unwrap();assert_eq!(plan.event()["reason"],"refreshed_value_exit");assert_eq!(plan.event()["holdingDeadlineMs"],NOW+600000);assert_eq!(plan.event()["expectedPayout"],"0.594000");assert!(daily_windows::clock(plan.event()["expiresAt"].as_str().unwrap()).unwrap()>NOW+623002);f.bank.accept("delayed-terminal-existing-exit",1,plan.event()).unwrap();
    }
    // A sealed historical fixture represents a V1 sidecar created by the old
    // executable. The NEW product path must not originate such an entry.
    fn legacy_bound_history()->F{
        let mut f=F::setup(false,"cup");let old=input(f.bank.manifest_hash().as_str().unwrap(),NOW);f.trust.insert(review_sha256(&old).unwrap());let q=f.entry();let spec=f.spec(&q);
        let binding=json!({"qid":q["id"],"spec":spec,"specSha256":sha(&serde_json::to_vec(&spec).unwrap()),"quote":q,"reviewSha256":review_sha256(&old).unwrap(),"boundAtMs":NOW});
        let cohort=json!({"entry":binding,"firstFillIndex":f.bank.engine().fills().len(),"priorFillsSha256":sha(&serde_json::to_vec(f.bank.engine().fills()).unwrap()),"bankHeadAtEntry":f.bank.durable_head(),"holdUntilMs":old.hold_until_ms,"exits":{}});
        f.store.take();let mut rows=Vec::new();let mut previous="0".repeat(64);
        for(i,event)in [json!({"kind":"review","review":old,"sha256":review_sha256(&old).unwrap()}),json!({"kind":"entry","exchangeId":"101","cohort":cohort})].into_iter().enumerate(){let mut row=json!({"sequence":i+1,"previous":previous,"manifestHash":f.bank.manifest_hash(),"runId":"fundamental-fixture","atMs":NOW,"event":event});previous=sha(&serde_json::to_vec(&row).unwrap());row["hash"]=json!(previous);serde_json::to_writer(&mut rows,&row).unwrap();rows.push(b'\n');}
        std::fs::write(f.root.join("fundamental.ndjson"),rows).unwrap();f.review=old;f.reopen();f
    }
    #[test]fn original_v1_bound_history_resumes_manages_and_keeps_its_exact_deadline(){
        let mut f=legacy_bound_history();let q=f.entry();assert!(matches!(f.assess(&q,NOW),Decision::Entry{..}),"only exact original bound entry can resume");
        f.fill_entry(false);let rows=std::fs::read(f.root.join("fundamental.ndjson")).unwrap();let(q,at)=f.age_exit(NOW+600001,"0.100000","0.120000");assert!(matches!(f.assess(&q,at),Decision::Exit{reason,..}if reason=="reviewed_holding_deadline"));f.reopen();assert_eq!(std::fs::read(f.root.join("fundamental.ndjson")).unwrap(),rows);assert!(matches!(f.assess(&q,at),Decision::Exit{..}));
        let mut revise=horizon_input(f.bank.manifest_hash().as_str().unwrap(),NOW+62000);let reviewed=approve(revise.clone(),&mut f.trust,NOW+62000);assert!(f.store.as_mut().unwrap().install(reviewed,&f.bank,&f.log,NOW+62000).is_err(),"active V1 cannot acquire a V2 interpretation");revise.hold_until_ms+=1;set_deadline_cash(&mut revise,"0.940500");let reviewed=approve(revise,&mut f.trust,NOW+62000);assert!(f.store.as_mut().unwrap().install(reviewed,&f.bank,&f.log,NOW+62000).is_err());
    }

    // These receipts follow the same original ACK -> full tape -> later terminal
    // feedback path as the sole worker. No account HTTP is performed.
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
    fn top_up_fixture()->(F,baal_transport_service::Transport,moloch_live_runtime::authority::SnapshotSet,Value){
        let(mut f,t,mut facts)=entry_fixture();let authority=f.authority();let hint=f.store.as_ref().unwrap().entry_hints(&f.bank,&f.log,&f.scope,&f.trust,NOW+25).unwrap().remove(0);
        let plan=f.store.as_ref().unwrap().prepare_entry(&f.bank,&f.log,&t,&f.scope,&hint,&facts,&authority,NOW+25).unwrap().unwrap();f.bank.accept("first-reviewed-entry",1,plan.event()).unwrap();drop(plan);drop(authority);
        let first=f.bank.engine().order_records().into_iter().find(|q|q["retired"]==false&&q.get("reviewedFundamentalEntry").is_some()).unwrap();let id=first["id"].as_u64().unwrap();let spec=f.spec(&first);f.store.as_mut().unwrap().bind(&f.bank,&f.log,&f.scope,id,&spec,&f.trust,NOW+3026).unwrap();top_up_close(&mut f,&first,NOW+3026,"901",2,true);
        let mut review=f.review.clone();review.reviewed_at_ms=NOW+5999;review.maximum_quantity=4;review.same_side_top_ups=Some(SameSideTopUps{version:1,maximum_entries:2});f.install(review.clone(),NOW+5999);f.review=review;
        for(i,r)in [&mut facts.account,&mut facts.positions,&mut facts.open_orders,&mut facts.cash_balance,&mut facts.market,&mut facts.contract,&mut facts.book].into_iter().enumerate(){r.request_started_at=iso(NOW+6010+i as u64*2);r.received_at=iso(NOW+6011+i as u64*2);}
        facts.positions.data=json!({"positions":[{"exchangeId":"101","quantity":-2}]});let cash:Value=serde_json::from_str(&moloch_replay::money(f.bank.engine().wallet.cash)).unwrap();facts.cash_balance.data=json!({"balance":cash,"availableAdvance":0,"buyingPower":cash});
        let raw=&mut facts.book.data["contexts"][0]["orderbook"]["exchanges"][0];raw["asOf"]=json!({"sequence":100,"at":iso(NOW+6023)});raw["bids"][0]["price"]=json!(0.11);raw["asks"][0]["price"]=json!(0.13);
        let b=&facts.book;let events=moloch_replay::source::receipt_events(&json!({"source":"official_sig_api_v1","kind":"book","tournamentId":f.scope.tournament_id,"marketId":"201","exchangeId":"101","endpoint":b.endpoint,"status":b.status,"requestStartedAt":b.request_started_at,"receivedAt":b.received_at,"data":b.data})).unwrap();for(i,e)in events.iter().enumerate(){f.bank.accept("top-up-original-book",i as u64+1,e).unwrap();}f.retire_drafts(NOW+6024);
        (f,t,facts,first)
    }
    fn install_cohort_planning(f:&mut F,at:u64){
        let report=f.bank.engine().report().unwrap();let e=json!({"type":"managed_inventory_planning_v1","source":"moloch_live_dispatcher","receivedAt":iso(at),"processedAt":iso(at),"bankScope":{"runId":"fundamental-fixture","bankManifestHash":f.bank.manifest_hash(),"tournamentId":f.scope.tournament_id},"bankHead":f.bank.durable_head(),"expectedReportSha256":sha(&serde_json::to_vec(&report).unwrap()),"managedExchangeIds":["101"],"approvedReviewSetSha256":"b".repeat(64),"generationSha256":"0".repeat(64),"controlSha256":"c".repeat(64),"expectedPreviousModeSha256":null});f.event(e);
    }
    #[test]fn original_closed_entry_gets_distinct_reviewed_exit_without_adoption_and_restart_preserves_binding(){
        let(mut f,t,_facts,first)=top_up_fixture();assert!(f.store.as_ref().unwrap().existing_exit_hint(&f.bank,&f.log,&f.scope,"101",&f.trust,NOW+7000).unwrap().is_none());
        install_cohort_planning(&mut f,NOW+9000);f.retire_drafts(NOW+9001);f.book(NOW+12000,"0.030000","0.050000");
        let hint=f.store.as_ref().unwrap().existing_exit_hint(&f.bank,&f.log,&f.scope,"101",&f.trust,NOW+12001).unwrap().unwrap();assert_eq!(hint.reason,"refreshed_value_exit");
        let plan=f.store.as_ref().unwrap().prepare_existing_exit(&f.bank,&f.log,&t,&f.scope,"101",&f.trust,NOW+15000).unwrap().unwrap();let e=plan.event().clone();assert_eq!(e["type"],"reviewed_cohort_exit_reserved_v1");assert!(e.get("adoptionRef").is_none());assert!(e.get("adoptionReceiptHash").is_none());assert_eq!(e["cohortOrigin"]["entries"][0]["qid"],first["id"]);assert_eq!(e["cohortOrigin"]["entries"][0]["actualOrderId"],"901");
        let cash=f.bank.engine().wallet.cash;f.event(e);assert_eq!(f.bank.engine().wallet.cash,cash);assert_eq!(f.bank.engine().wallet.quantity("101",moloch_replay::Side::No),2);
        let q=f.bank.engine().order_records().into_iter().find(|q|q["retired"]==false&&q["reviewedInventoryExit"]["originKind"]=="original_entry_cohort").unwrap();f.book(NOW+18001,"0.030000","0.050000");assert!(matches!(f.assess(&q,NOW+18002),Decision::Exit{..}));let id=q["id"].as_u64().unwrap();let spec=f.spec(&q);f.store.as_mut().unwrap().bind(&f.bank,&f.log,&f.scope,id,&spec,&f.trust,NOW+18002).unwrap();top_up_close(&mut f,&q,NOW+18003,"902",2,true);f.reopen();assert_eq!(f.bank.engine().wallet.quantity("101",moloch_replay::Side::No),0);assert!(f.store.as_ref().unwrap().durable_tag(first["id"].as_u64().unwrap()).is_some());assert!(f.store.as_ref().unwrap().durable_tag(id).is_some());assert!(f.store.as_ref().unwrap().existing_exit_hint(&f.bank,&f.log,&f.scope,"101",&f.trust,NOW+18012).unwrap().is_none());
    }
    #[test]fn new_cohort_exit_requires_current_value_and_original_closed_ack_not_just_mode_or_retirement(){
        for case in ["hold","unclosed","actual","endpoint"]{let(mut f,t,_facts,first)=top_up_fixture();install_cohort_planning(&mut f,NOW+9000);f.retire_drafts(NOW+9001);let id=first["id"].as_u64().unwrap();if case=="hold"{f.book(NOW+12000,"0.300000","0.320000");}else{f.book(NOW+12000,"0.030000","0.050000");match case{"unclosed"=>{f.log.closed.remove(&id);},"actual"=>f.log.acknowledgments.get_mut(&id).unwrap()["data"]["orderId"]=json!("999"),_=>f.log.acknowledgments.get_mut(&id).unwrap()["endpoint"]=json!("/not-original-orders")}}
            let head=f.bank.durable_head();let result=f.store.as_ref().unwrap().prepare_existing_exit(&f.bank,&f.log,&t,&f.scope,"101",&f.trust,NOW+15000);assert!(result.is_err()||result.unwrap().is_none(),"{case}");assert_eq!(f.bank.durable_head(),head);assert_eq!(f.bank.engine().wallet.quantity("101",moloch_replay::Side::No),2);}
    }
    #[test]fn fully_deployed_top_up_cohort_can_exit_without_new_acquisition_headroom(){
        let(mut f,t,facts,first)=top_up_fixture();let top=top_up_reserve(&mut f,&t,&facts);top_up_close(&mut f,&top,NOW+11527,"902",2,true);assert_eq!(f.bank.engine().wallet.quantity("101",moloch_replay::Side::No),f.review.maximum_quantity);
        install_cohort_planning(&mut f,NOW+15000);f.retire_drafts(NOW+15001);f.book(NOW+18000,"0.030000","0.050000");let plan=f.store.as_ref().unwrap().prepare_existing_exit(&f.bank,&f.log,&t,&f.scope,"101",&f.trust,NOW+21000).unwrap().unwrap();let e=plan.event();assert_eq!(e["cohortOrigin"]["entries"].as_array().unwrap().len(),2);assert_eq!(e["cohortOrigin"]["heldQuantity"],4);assert_eq!(e["quantity"],2,"unchanged native per-order size");assert_eq!(e["cohortOrigin"]["entries"][0]["qid"],first["id"]);assert_eq!(e["cohortOrigin"]["entries"][1]["qid"],top["id"]);let e=e.clone();f.event(e);assert_eq!(f.bank.engine().wallet.quantity("101",moloch_replay::Side::No),4);assert_eq!(f.bank.engine().wallet.market_cost("101"),3_500_000);
    }
    #[test]fn cohort_mode_preserves_explicit_revocation_and_due_deadline_even_below_forecast_value(){
        for revoked in [false,true]{let(mut f,t,_facts,_)=top_up_fixture();if revoked{let mut r=f.review.clone();r.reviewed_at_ms=NOW+7000;r.revoke_existing=true;f.install(r,NOW+7000);}let at=if revoked{NOW+15000}else{f.review.hold_until_ms+1000};install_cohort_planning(&mut f,at-3000);f.retire_drafts(at-2999);f.book(at,"0.300000","0.320000");let plan=f.store.as_ref().unwrap().prepare_existing_exit(&f.bank,&f.log,&t,&f.scope,"101",&f.trust,at+3000).unwrap().unwrap();assert_eq!(plan.event()["reason"],if revoked{"reviewed_source_revocation"}else{"reviewed_holding_deadline"});assert_eq!(plan.event()["limitPrice"],"0.680000");}
    }
    fn open_partial_cohort_exit(deadline:bool)->(F,Value,u64){
        let(mut f,t,_facts,_)=top_up_fixture();let at=if deadline{f.review.hold_until_ms+1000}else{NOW+12000};install_cohort_planning(&mut f,at-3000);f.retire_drafts(at-2999);f.book(at,"0.030000","0.050000");
        let plan=f.store.as_ref().unwrap().prepare_existing_exit(&f.bank,&f.log,&t,&f.scope,"101",&f.trust,at+3000).unwrap().unwrap();let e=plan.event().clone();f.event(e);let q=f.bank.engine().order_records().into_iter().find(|q|q["retired"]==false&&q["reviewedInventoryExit"]["originKind"]=="original_entry_cohort").unwrap();
        f.book(at+6001,"0.030000","0.050000");let id=q["id"].as_u64().unwrap();let spec=f.spec(&q);f.store.as_mut().unwrap().bind(&f.bank,&f.log,&f.scope,id,&spec,&f.trust,at+6002).unwrap();top_up_close(&mut f,&q,at+6003,"902",1,false);
        assert_eq!(f.bank.engine().wallet.quantity("101",moloch_replay::Side::No),1);assert_eq!(f.bank.engine().live_order_bindings()[&id],"902");assert!(!f.log.closed.contains(&id));let now=at+6012;(f,q,now)
    }
    #[test]fn partial_cohort_sell_current_value_is_retained_but_revised_value_uses_original_cancel_lane(){
        let(mut f,q,at)=open_partial_cohort_exit(false);let id=q["id"].as_u64().unwrap();let head=f.bank.durable_head();let cash=f.bank.engine().wallet.cash;
        assert!(f.store.as_ref().unwrap().existing_exit_cancellations(&f.bank,&f.log,&f.trust,at).unwrap().is_empty());assert_eq!(f.bank.durable_head(),head);
        let mut r=f.review.clone();r.reviewed_at_ms=at+1;r.ordinary_probability="0.990000".into();set_deadline_cash(&mut r,"0.980100");f.install(r,at+1);
        assert_eq!(f.store.as_ref().unwrap().existing_exit_cancellations(&f.bank,&f.log,&f.trust,at+2).unwrap(),vec![id]);assert_eq!(f.bank.engine().wallet.cash,cash);assert_eq!(f.bank.engine().wallet.quantity("101",moloch_replay::Side::No),1);assert!(!f.log.closed.contains(&id));
        f.reopen();assert_eq!(f.store.as_ref().unwrap().existing_exit_cancellations(&f.bank,&f.log,&f.trust,at+3).unwrap(),vec![id]);
    }
    #[test]fn partial_cohort_sell_stale_source_or_revoked_trust_cancels_without_releasing_remaining_lot(){
        for stale in [false,true]{let(f,q,at)=open_partial_cohort_exit(false);let id=q["id"].as_u64().unwrap();let trust=if stale{f.trust.clone()}else{BTreeSet::new()};let now=if stale{f.review.expires_at_ms+1}else{at};let before=f.bank.durable_head();assert_eq!(f.store.as_ref().unwrap().existing_exit_cancellations(&f.bank,&f.log,&trust,now).unwrap(),vec![id]);assert_eq!(f.bank.durable_head(),before);assert_eq!(f.bank.engine().wallet.quantity("101",moloch_replay::Side::No),1);assert!(!f.log.closed.contains(&id));}
    }
    #[test]fn partial_deadline_cohort_sell_retains_stale_review_exception(){let(f,q,at)=open_partial_cohort_exit(true);assert_eq!(q["reviewedInventoryExit"]["reason"],"reviewed_holding_deadline");assert!(at>f.review.expires_at_ms);assert!(f.store.as_ref().unwrap().existing_exit_cancellations(&f.bank,&f.log,&BTreeSet::new(),at).unwrap().is_empty());assert_eq!(f.bank.engine().wallet.quantity("101",moloch_replay::Side::No),1);}
    #[test]fn cohort_cancel_never_uses_mutated_ack_or_invented_actual_id(){for case in ["id","endpoint","missing"]{let(mut f,q,at)=open_partial_cohort_exit(false);let id=q["id"].as_u64().unwrap();match case{"id"=>f.log.acknowledgments.get_mut(&id).unwrap()["data"]["orderId"]=json!("999"),"endpoint"=>f.log.acknowledgments.get_mut(&id).unwrap()["endpoint"]=json!("/not-original-orders"),_=>{f.log.acknowledgments.remove(&id);}}let before=f.bank.durable_head();let result=f.store.as_ref().unwrap().existing_exit_cancellations(&f.bank,&f.log,&BTreeSet::new(),at);if case=="missing"{assert!(result.unwrap().is_empty());}else{assert!(result.is_err());}assert_eq!(f.bank.durable_head(),before);assert_eq!(f.bank.engine().wallet.quantity("101",moloch_replay::Side::No),1);}}
    #[test]fn pending_top_up_cannot_nominate_an_undispatchable_priority_exit(){
        let(mut f,t,facts,_)=top_up_fixture();let top=top_up_reserve(&mut f,&t,&facts);install_cohort_planning(&mut f,NOW+15000);f.retire_drafts(NOW+15001);f.book(NOW+18000,"0.030000","0.050000");
        let head=f.bank.durable_head();assert!(f.store.as_ref().unwrap().existing_exit_hint(&f.bank,&f.log,&f.scope,"101",&f.trust,NOW+21000).unwrap().is_none());assert!(f.store.as_ref().unwrap().prepare_existing_exit(&f.bank,&f.log,&t,&f.scope,"101",&f.trust,NOW+21000).unwrap().is_none());
        assert_eq!(f.bank.durable_head(),head);assert_eq!(f.bank.engine().wallet.quantity("101",moloch_replay::Side::No),2);assert!(!f.log.closed.contains(&top["id"].as_u64().unwrap()));f.reopen();assert!(f.store.as_ref().unwrap().existing_exit_hint(&f.bank,&f.log,&f.scope,"101",&f.trust,NOW+21001).unwrap().is_none());
    }
    fn top_up_reserve(f:&mut F,t:&baal_transport_service::Transport,facts:&moloch_live_runtime::authority::SnapshotSet)->Value{
        let authority=f.authority();let hint=f.store.as_ref().unwrap().entry_hints(&f.bank,&f.log,&f.scope,&f.trust,NOW+8525).unwrap().remove(0);
        let plan=f.store.as_ref().unwrap().prepare_entry(&f.bank,&f.log,t,&f.scope,&hint,facts,&authority,NOW+8525).unwrap().unwrap();assert!(plan.event().get("sameSideTopUp").is_some());plan.validate_current(&f.bank,&f.log,t,&authority,NOW+8525).unwrap();let seq=f.bank.last_source_sequence("top-up-reservation")+1;f.bank.accept("top-up-reservation",seq,plan.event()).unwrap();drop(plan);drop(authority);
        let q=f.bank.engine().order_records().into_iter().find(|q|q["retired"]==false&&q["reviewedFundamentalEntry"].get("sameSideTopUp").is_some()).unwrap();let id=q["id"].as_u64().unwrap();let spec=f.spec(&q);f.store.as_mut().unwrap().bind(&f.bank,&f.log,&f.scope,id,&spec,&f.trust,NOW+11526).unwrap();q
    }
    #[test]fn explicit_top_up_genuine_seven_facts_bind_new_qid_and_preserve_original_tags_and_cash(){
        let(mut f,t,facts,first)=top_up_fixture();let old=first["id"].as_u64().unwrap();let old_tag=f.store.as_ref().unwrap().durable_tag(old);let cash=f.bank.engine().wallet.cash;let q=top_up_reserve(&mut f,&t,&facts);let id=q["id"].as_u64().unwrap();assert_ne!(id,old);assert_eq!(q["quantity"],2);assert_eq!(q["price"],"0.870000");assert_eq!(f.bank.engine().wallet.cash,cash);assert_eq!(f.bank.engine().wallet.quantity("101",moloch_replay::Side::No),2);assert_eq!(f.store.as_ref().unwrap().durable_tag(old),old_tag);
        let spec=f.spec(&q);let permit=f.permit(&q,NOW+11526);let authority=f.authority();let token=f.store.as_ref().unwrap().physical_decision(&f.bank,&f.log,&f.scope,id,&spec,&permit,&authority,NOW+11526).unwrap();token.authorize(&f.scope,&spec,&permit,NOW+11526).unwrap();let mut changed=spec.clone();changed.body.as_mut().unwrap()["quantity"]=json!(1);assert!(token.authorize(&f.scope,&changed,&permit,NOW+11526).is_err());drop(token);drop(authority);f.reopen();assert_eq!(f.store.as_ref().unwrap().durable_tag(old),old_tag);assert!(f.store.as_ref().unwrap().durable_tag(id).is_some());
        top_up_close(&mut f,&q,NOW+11527,"902",2,true);f.reopen();assert_eq!(f.bank.engine().wallet.quantity("101",moloch_replay::Side::No),4);assert_eq!(f.bank.engine().wallet.market_cost("101"),3_500_000);assert!(f.store.as_ref().unwrap().entry_hints(&f.bank,&f.log,&f.scope,&f.trust,NOW+11536).unwrap().is_empty());
    }
    #[test]fn top_up_original_closure_and_attribution_fail_closed_on_mutated_maps_or_unknown(){
        for case in ["unclosed","ack","endpoint","forged_closed","unknown","cost_cap","quantity_cap","no_opt_in"]{let(mut f,_t,_facts,first)=top_up_fixture();let id=first["id"].as_u64().unwrap();match case{"unclosed"=>{f.log.closed.remove(&id);},"ack"=>{f.log.acknowledgments.get_mut(&id).unwrap()["data"]["orderId"]=json!("999");},"endpoint"=>{f.log.acknowledgments.get_mut(&id).unwrap()["endpoint"]=json!("/not-original-orders");},"forged_closed"=>{let path=f.root.join("dispatch.ndjson");let raw=std::fs::read(&path).unwrap();let cut=raw[..raw.len()-1].iter().rposition(|b|*b==b'\n').unwrap()+1;std::fs::write(&path,&raw[..cut]).unwrap();f.log=dispatch::DispatchLog::open(&path).unwrap();assert!(!f.log.closed.contains(&id));f.log.closed.insert(id);},"unknown"=>{f.log.append(json!({"kind":"intent","qid":9999})).unwrap();f.log.append(json!({"kind":"attempt","qid":9999})).unwrap();},_=>{let mut r=f.review.clone();r.reviewed_at_ms=NOW+7000;match case{"cost_cap"=>r.maximum_principal="1.760000".into(),"quantity_cap"=>r.maximum_quantity=2,_=>r.same_side_top_ups=None};f.install(r,NOW+7000);}}let result=f.store.as_ref().unwrap().entry_hints(&f.bank,&f.log,&f.scope,&f.trust,NOW+8525);assert!(result.is_err()||result.unwrap().is_empty(),"{case}");}
    }
    #[test]fn top_up_full_packet_rejects_wrong_or_missing_signed_position_and_stale_plan(){
        for case in 0..3{let(mut f,t,mut facts,_)=top_up_fixture();let authority=f.authority();let hint=f.store.as_ref().unwrap().entry_hints(&f.bank,&f.log,&f.scope,&f.trust,NOW+8525).unwrap().remove(0);if case==0{facts.positions.data["positions"][0]["quantity"]=json!(2);}if case==1{facts.positions.data["positions"]=json!([]);}let result=f.store.as_ref().unwrap().prepare_entry(&f.bank,&f.log,&t,&f.scope,&hint,&facts,&authority,NOW+8525);if case<2{assert!(result.is_err());}else{let plan=result.unwrap().unwrap();f.log.append(json!({"kind":"source","qid":1})).unwrap();assert!(plan.validate_current(&f.bank,&f.log,&t,&authority,NOW+8526).is_err());}}
    }
    #[test]fn multi_entry_partial_fifo_exits_never_reset_basis_or_enable_global_rearm(){
        let(mut f,t,facts,_)=top_up_fixture();let q=top_up_reserve(&mut f,&t,&facts);top_up_close(&mut f,&q,NOW+11527,"902",1,true);f.reopen();assert_eq!(f.bank.engine().wallet.market_cost("101"),2_630_000);assert!(f.store.as_ref().unwrap().abandonment_hints(&f.bank,&f.log,&f.scope,NOW+11536).unwrap().is_empty());assert!(f.store.as_ref().unwrap().rearm_hints(&f.bank,&f.log,&f.scope,NOW+11536).unwrap().is_empty());
        let(q,at)=f.age_exit(NOW+70000,"0.040000","0.050000");let at=at.max(q["activationAt"].as_u64().unwrap());assert!(matches!(f.assess(&q,at),Decision::Exit{..}));let id=q["id"].as_u64().unwrap();let spec=f.spec(&q);f.store.as_mut().unwrap().bind(&f.bank,&f.log,&f.scope,id,&spec,&f.trust,at).unwrap();top_up_close(&mut f,&q,at+1,"903",q["quantity"].as_u64().unwrap(),true);f.reopen();assert_eq!(f.bank.engine().wallet.quantity("101",moloch_replay::Side::No),1);assert_eq!(f.bank.engine().wallet.market_cost("101"),870_000);assert_eq!(f.bank.engine().wallet.realized,140_000);assert!(f.store.as_ref().unwrap().entry_hints(&f.bank,&f.log,&f.scope,&f.trust,at+10).unwrap().is_empty());
    }
    #[test]fn unattempted_top_up_positive_retirement_keeps_holdings_and_releases_only_policy_pending(){
        for positive in [true,false]{let(mut f,t,facts,first)=top_up_fixture();let q=top_up_reserve(&mut f,&t,&facts);let id=q["id"].as_u64().unwrap();let old_tag=f.store.as_ref().unwrap().durable_tag(first["id"].as_u64().unwrap());let spec=f.spec(&q);f.log.append(json!({"kind":"intent","qid":id,"quote":q,"spec":spec,"fundamental":f.store.as_ref().unwrap().durable_tag(id)})).unwrap();let proof=f.log.proof(id);if positive{f.log.append(json!({"kind":"not_sent","qid":id,"proof":proof})).unwrap();}
            let retirement=json!({"type":"unsubmitted_intent_retired","source":"moloch_live_dispatcher","orderId":id,"confirmedNetworkAttempts":0,"receiptHash":sha(&serde_json::to_vec(&proof).unwrap()),"receivedAt":iso(NOW+11530),"processedAt":iso(NOW+11530),"deferPlanningForPackage":true});f.bank.accept("top-up-never-sent",1,&retirement).unwrap();let mut facts=moloch_live_runtime::authority::CancelSnapshotSet{account:facts.account,positions:facts.positions,open_orders:facts.open_orders,cash_balance:facts.cash_balance};for(i,r)in [&mut facts.account,&mut facts.positions,&mut facts.open_orders,&mut facts.cash_balance].into_iter().enumerate(){r.request_started_at=iso(NOW+11531+i as u64*2);r.received_at=iso(NOW+11532+i as u64*2);}
            let hint=f.store.as_ref().unwrap().abandonment_hints(&f.bank,&f.log,&f.scope,NOW+11540).unwrap().into_iter().find(|h|h.qid==id).unwrap();let plan=f.store.as_ref().unwrap().prepare_abandon(&f.bank,&f.log,&t,&f.scope,&hint,&facts,"top-up-never-sent",1,&retirement,NOW+11540);if !positive{assert!(plan.is_err());continue}
            let report=f.bank.engine().report().unwrap();let head=f.bank.durable_head();let tag=f.store.as_ref().unwrap().durable_tag(id);f.store.as_mut().unwrap().apply_abandon(plan.unwrap(),&f.bank,&f.log,&t,NOW+11540).unwrap();f.reopen();assert_eq!(f.bank.engine().report().unwrap(),report);assert_eq!(f.bank.durable_head(),head);assert_eq!(f.store.as_ref().unwrap().durable_tag(id),tag);assert_eq!(f.store.as_ref().unwrap().durable_tag(first["id"].as_u64().unwrap()),old_tag);assert_eq!(f.bank.engine().wallet.quantity("101",moloch_replay::Side::No),2);assert!(f.store.as_ref().unwrap().validate_acknowledgment(id,&F::base(&q,"actual_order_ack",NOW+11541,"999")).is_err());
            assert_eq!(f.store.as_ref().unwrap().entry_hints(&f.bank,&f.log,&f.scope,&f.trust,NOW+11541).unwrap().len(),1,"proved never-sent top-up does not spend a physical-entry allowance");
        }
    }

    fn entry_fixture()->(F,baal_transport_service::Transport,moloch_live_runtime::authority::SnapshotSet){
        entry_fixture_quantity(2)
    }
    fn entry_fixture_quantity(quantity:u64)->(F,baal_transport_service::Transport,moloch_live_runtime::authority::SnapshotSet){
        use baal_transport_service::{Config,DisabledCredentials,ReadOnlyAuthority,SystemClock,Transport,Receipt};
        let mut f=F::setup_quantity(true,"11111111-1111-1111-1111-111111111111",quantity);let tid=f.scope.tournament_id.clone();
        let receipt=|i:u64,endpoint:String,data:Value|Receipt{endpoint,status:200,request_started_at:iso(NOW+10+i*2),received_at:iso(NOW+11+i*2),data};
        let facts=moloch_live_runtime::authority::SnapshotSet{
            account:receipt(0,format!("/tournaments/{}",f.scope.tournament_slug),json!({"id":tid,"slug":f.scope.tournament_slug,"status":"active"})),
            positions:receipt(1,format!("/tournaments/{}/portfolio/positions",f.scope.tournament_slug),json!({"positions":[]})),
            open_orders:receipt(2,format!("/orders?status=open&limit=200&tournamentId={tid}"),json!({"data":[],"coverage":{"complete":true},"pagination":{"hasMore":false,"nextCursor":null}})),
            cash_balance:receipt(3,format!("/account?tournamentId={tid}"),json!({"balance":1000,"availableAdvance":0,"buyingPower":1000})),
            market:receipt(4,format!("/markets/201?tournamentId={tid}"),json!({"id":"201","status":"open","isComposite":false,"isMultiOutcome":false,"contexts":[{"status":"open","type":"tournament","tournament":{"id":tid}}]})),
            contract:receipt(5,format!("/markets/201/nodes?tournamentId={tid}"),serde_json::from_slice(ORIGINALS[2]).unwrap()),
            book:receipt(6,format!("/markets/201/orderbook?tournamentId={tid}&depth=20"),json!({"contexts":[{"type":"tournament","tournament":{"id":tid},"orderbook":{"exchanges":[{"exchangeId":"101","asOf":{"sequence":99,"at":iso(NOW+23)},"bids":[{"price":0.10,"quantity":100}],"asks":[{"price":0.12,"quantity":100}]}]}}]})),
        };
        let b=&facts.book;let events=moloch_replay::source::receipt_events(&json!({"source":"official_sig_api_v1","kind":"book","tournamentId":tid,"marketId":"201","exchangeId":"101","endpoint":b.endpoint,"status":b.status,"requestStartedAt":b.request_started_at,"receivedAt":b.received_at,"data":b.data})).unwrap();
        for (i,e) in events.iter().enumerate(){f.bank.accept("original-preflight-book",i as u64+1,e).unwrap();}
        f.retire_drafts(NOW+24);
        let transport=Transport::new_for_mock(Config{queue_dir:f.root.join("original-account-queue"),account_scope:"synthetic-entry-account".into(),scope:f.scope.clone(),maximum_pages:2,writes_enabled:false},Box::new(DisabledCredentials),Box::new(SystemClock),Box::new(ReadOnlyAuthority),"http://127.0.0.1:9/api/v1".into()).unwrap();(f,transport,facts)
    }
    // A second independently source-bound review and genuine normalized book,
    // sharing this exact Session/dispatch/SDK queue. No network is contacted.
    fn other_entry_packet(f:&mut F,mut facts:moloch_live_runtime::authority::SnapshotSet,at:u64)->(baal_transport_service::Transport,moloch_live_runtime::authority::SnapshotSet){
        use baal_transport_service::{Config,DisabledCredentials,ReadOnlyAuthority,SystemClock,Transport};
        let mut r=f.review.clone();r.exchange_id="102".into();r.market_id="202".into();r.contract_id="302".into();r.reviewed_at_ms=at;
        let mut node:Value=serde_json::from_slice(ORIGINALS[2]).unwrap();node["market_id"]=json!("202");node["root"]["node_id"]=json!("302");node["root"]["contract_id"]=json!("302");
        let node_bytes=serde_json::to_vec(&node).unwrap();r.official_contract_root_sha256=official_contract_root_sha256(&node).unwrap();r.sources[2].sha256=sha(&node_bytes);set_deadline_cash(&mut r,"0.940500");
        let mut originals=originals_for(&r);originals[2]=node_bytes;let hashes=originals.iter().map(|b|sha(b)).collect::<Vec<_>>();let sources=originals.iter().enumerate().map(|(i,b)|SourceBytes{sha256:&hashes[i],bytes:b}).collect::<Vec<_>>();f.trust.insert(review_sha256(&r).unwrap());
        let reviewed=Reviewed::verify(r,&sources,&f.trust,at).unwrap();f.store.as_mut().unwrap().install(reviewed,&f.bank,&f.log,at).unwrap();f.scope.exchanges.insert("102".into(),"202".into());
        for(i,r)in [&mut facts.account,&mut facts.positions,&mut facts.open_orders,&mut facts.cash_balance,&mut facts.market,&mut facts.contract,&mut facts.book].into_iter().enumerate(){r.request_started_at=iso(at+10+i as u64*2);r.received_at=iso(at+11+i as u64*2);}
        let tid=&f.scope.tournament_id;facts.market.endpoint=format!("/markets/202?tournamentId={tid}");facts.market.data["id"]=json!("202");facts.contract.endpoint=format!("/markets/202/nodes?tournamentId={tid}");facts.contract.data=node;
        facts.book.endpoint=format!("/markets/202/orderbook?tournamentId={tid}&depth=20");let raw=&mut facts.book.data["contexts"][0]["orderbook"]["exchanges"][0];raw["exchangeId"]=json!("102");raw["asOf"]=json!({"sequence":1,"at":iso(at+23)});raw["bids"][0]["price"]=json!(0.10);raw["asks"][0]["price"]=json!(0.12);
        let b=&facts.book;let events=moloch_replay::source::receipt_events(&json!({"source":"official_sig_api_v1","kind":"book","tournamentId":tid,"marketId":"202","exchangeId":"102","endpoint":b.endpoint,"status":b.status,"requestStartedAt":b.request_started_at,"receivedAt":b.received_at,"data":b.data})).unwrap();for(i,e)in events.iter().enumerate(){f.bank.accept("other-original-preflight-book",i as u64+1,e).unwrap();}f.retire_drafts(at+24);
        let t=Transport::new_for_mock(Config{queue_dir:f.root.join("original-account-queue"),account_scope:"synthetic-entry-account".into(),scope:f.scope.clone(),maximum_pages:2,writes_enabled:false},Box::new(DisabledCredentials),Box::new(SystemClock),Box::new(ReadOnlyAuthority),"http://127.0.0.1:9/api/v1".into()).unwrap();(t,facts)
    }
    #[test]fn target_hint_prepare_matches_public_eligibility_without_rebuilding_unselected_top_up(){
        let(mut f,t,facts,_)=top_up_fixture();drop(t);let(t,facts)=other_entry_packet(&mut f,facts,NOW+8500);let authority=f.authority();let store=f.store.as_ref().unwrap();
        let hints=store.entry_hints(&f.bank,&f.log,&f.scope,&f.trust,NOW+8525).unwrap();assert_eq!(hints.iter().map(|h|h.exchange_id.as_str()).collect::<Vec<_>>(),vec!["101","102"]);let hint=hints.into_iter().find(|h|h.exchange_id=="102").unwrap();
        let plan=store.prepare_entry(&f.bank,&f.log,&t,&f.scope,&hint,&facts,&authority,NOW+8525).unwrap().unwrap();let expected=plan.event().clone();drop(plan);
        let old=f.log.acknowledgments.keys().copied().next().unwrap();f.log.acknowledgments.get_mut(&old).unwrap()["endpoint"]=json!("/not-original-orders");
        assert!(store.entry_hints(&f.bank,&f.log,&f.scope,&f.trust,NOW+8525).is_err(),"public enumeration still reconstructs every eligible top-up");
        let head=f.bank.durable_head();let report=f.bank.engine().report().unwrap();let plan=store.prepare_entry(&f.bank,&f.log,&t,&f.scope,&hint,&facts,&authority,NOW+8525).unwrap().unwrap();assert_eq!(plan.event(),&expected);plan.validate_current(&f.bank,&f.log,&t,&authority,NOW+8525).unwrap();assert_eq!(f.bank.durable_head(),head);assert_eq!(f.bank.engine().report().unwrap(),report);assert!(!f.log.intents.values().any(|i|i["quote"]["exchangeId"]=="102"));
    }
    #[test]fn target_hint_prepare_preserves_all_eligible_review_scope_checks(){
        let(mut f,t,facts)=entry_fixture();drop(t);let(t,facts)=other_entry_packet(&mut f,facts,NOW+30);let authority=f.authority();let store=f.store.as_ref().unwrap();let hint=store.entry_hints(&f.bank,&f.log,&f.scope,&f.trust,NOW+55).unwrap().into_iter().find(|h|h.exchange_id=="102").unwrap();let mut scope=f.scope.clone();scope.exchanges.remove("101");
        let error=store.prepare_entry(&f.bank,&f.log,&t,&scope,&hint,&facts,&authority,NOW+55).err().expect("unselected eligible review still requires exact scope");assert_eq!(error.to_string(),"FUNDAMENTAL_ENTRY_SCOPE");
    }
    #[test]fn target_hint_prepare_still_blocks_global_unknown_before_target_packet(){
        let(mut f,t,facts)=entry_fixture();let authority=f.authority();let hint=f.store.as_ref().unwrap().entry_hints(&f.bank,&f.log,&f.scope,&f.trust,NOW+25).unwrap().remove(0);f.log.append(json!({"kind":"intent","qid":9999})).unwrap();f.log.append(json!({"kind":"attempt","qid":9999})).unwrap();let head=f.bank.durable_head();let bytes=std::fs::read(f.root.join("dispatch.ndjson")).unwrap();
        assert!(f.store.as_ref().unwrap().prepare_entry(&f.bank,&f.log,&t,&f.scope,&hint,&facts,&authority,NOW+25).is_err());assert_eq!(f.bank.durable_head(),head);assert_eq!(std::fs::read(f.root.join("dispatch.ndjson")).unwrap(),bytes);assert!(f.log.attempted.contains(&9999));assert!(!f.log.closed.contains(&9999));
    }
    #[test]fn target_hint_prepare_still_requires_selected_top_up_original_closure(){
        let(mut f,t,facts,first)=top_up_fixture();let authority=f.authority();let hint=f.store.as_ref().unwrap().entry_hints(&f.bank,&f.log,&f.scope,&f.trust,NOW+8525).unwrap().remove(0);let id=first["id"].as_u64().unwrap();f.log.closed.remove(&id);let head=f.bank.durable_head();let count=f.log.intents.len();
        assert!(f.store.as_ref().unwrap().prepare_entry(&f.bank,&f.log,&t,&f.scope,&hint,&facts,&authority,NOW+8525).unwrap().is_none());assert_eq!(f.bank.durable_head(),head);assert_eq!(f.log.intents.len(),count);assert_eq!(f.bank.engine().wallet.quantity("101",moloch_replay::Side::No),2);
    }
    #[test]fn independent_entry_hint_survives_missing_book_and_no_live_native_quote(){
        let mut f=F::new();f.retire_drafts(NOW);f.event(json!({"type":"gap","exchangeId":"101","receivedAt":iso(NOW+1)}));
        let hints=f.store.as_ref().unwrap().entry_hints(&f.bank,&f.log,&f.scope,&f.trust,NOW+1).unwrap();assert_eq!(hints.len(),1);assert_eq!(hints[0].contract_id,"301");assert!(!f.bank.engine().order_records().iter().any(|q|q["retired"]==false));
    }
    #[test]fn independent_entry_uses_original_seven_facts_and_real_native_derived_quantity(){
        let(mut f,t,facts)=entry_fixture();let authority=f.authority();let store=f.store.as_ref().unwrap();let hint=store.entry_hints(&f.bank,&f.log,&f.scope,&f.trust,NOW+25).unwrap().remove(0);
        let plan=store.prepare_entry(&f.bank,&f.log,&t,&f.scope,&hint,&facts,&authority,NOW+25).unwrap().unwrap();plan.validate_current(&f.bank,&f.log,&t,&authority,NOW+25).unwrap();
        assert!(plan.event().get("quantity").is_none(),"only native derives quantity");assert_eq!(plan.event()["expectedPayout"],"0.940500");assert_eq!(plan.event()["limitPrice"],"0.880000");
        let before=f.bank.engine().wallet.cash;f.bank.accept("reviewed-entry",1,plan.event()).unwrap();
        let q=f.bank.engine().order_records().into_iter().find(|q|q["retired"]==false&&q.get("reviewedFundamentalEntry").is_some()).unwrap();assert_eq!(q["side"],"NO");assert_eq!(q["quantity"],2);assert_eq!(q["reviewedFundamentalEntry"]["reviewSha256"],hint.review_sha256);
        assert_eq!(f.bank.engine().wallet.cash,before);assert!(f.bank.engine().fills().is_empty());assert!(plan.validate_current(&f.bank,&f.log,&t,&authority,NOW+26).is_err());
        let spec=f.spec(&q);assert!(matches!(f.store.as_mut().unwrap().bind(&f.bank,&f.log,&f.scope,q["id"].as_u64().unwrap(),&spec,&f.trust,NOW+26).unwrap(),Decision::Entry{..}));
    }
    #[test]fn independent_entry_rejects_changed_rules_cash_positions_scope_and_unadmitted_book(){
        for case in 0..9{let(f,t,mut facts)=entry_fixture();let authority=f.authority();let store=f.store.as_ref().unwrap();let hint=store.entry_hints(&f.bank,&f.log,&f.scope,&f.trust,NOW+25).unwrap().remove(0);
            match case{0=>facts.contract.data["root"]["title"]=json!("changed resolver root"),1=>facts.cash_balance.data["balance"]=json!(999),2=>facts.positions.data["positions"]=json!([{"exchangeId":"101","quantity":1}]),3=>facts.book.data["contexts"][0]["orderbook"]["exchanges"][0]["bids"][0]["quantity"]=json!(99),4=>facts.book.endpoint.push_str("&other=1"),5=>facts.open_orders.data["coverage"]["complete"]=json!(false),6=>facts.positions.request_started_at=iso(NOW-20001),7=>facts.open_orders.data["data"]=json!([{"id":"7","exchangeId":"101"}]),_=>facts.contract.request_started_at=iso(NOW+1)}
            assert!(store.prepare_entry(&f.bank,&f.log,&t,&f.scope,&hint,&facts,&authority,NOW+25).is_err(),"case {case}");
        }
    }
    #[test]fn independent_entry_hint_never_clears_unknown_acceptance_or_reuses_existing_cohort(){
        let mut f=F::new();let(q,spec)=f.bind_entry();f.log_intent(&q,&spec);assert!(f.store.as_ref().unwrap().entry_hints(&f.bank,&f.log,&f.scope,&f.trust,NOW+1).is_err());
        let mut f=F::new();f.fill_entry(false);assert!(f.store.as_ref().unwrap().entry_hints(&f.bank,&f.log,&f.scope,&f.trust,NOW+5).unwrap().is_empty());
    }
    #[cfg(target_os="linux")]
    fn prove_pre_admission_abort(f:&mut F,q:&Value,tagged:bool)->(baal_transport_service::Config,Value){
        use baal_transport_service::{Config,TransportError,admission::AccountAdmission};
        let id=q["id"].as_u64().unwrap();let spec=f.spec(q);let mut permit=f.permit(q,NOW);permit.expires_at_ms=NOW-1;
        let error=TransportError{code:"PERMIT_EXPIRED_OR_INVALID".into(),status:None,may_have_executed:false,retry_after_ms:0,order_id:None,api_code:None};
        let tag=if tagged{f.store.as_ref().unwrap().durable_tag(id)}else{None};
        f.log.append(json!({"kind":"intent","qid":id,"quote":q,"spec":spec,"permit":permit,"fundamental":tag})).unwrap();
        f.log.append(json!({"kind":"attempt","qid":id,"at":iso(NOW),"captureAtMs":NOW})).unwrap();f.log.append(json!({"kind":"error","qid":id,"at":iso(NOW+1),"error":error})).unwrap();
        let dir=f.root.join("frozen-sdk-original");std::fs::create_dir(&dir).unwrap();{let mut queue=AccountAdmission::open(&dir,"synthetic-aborted-account","fixture").unwrap();queue.command(json!({"op":"status","scope":"synthetic-aborted-account","now":NOW-1000})).unwrap();}
        let config=Config{queue_dir:dir,account_scope:"synthetic-aborted-account".into(),scope:f.scope.clone(),maximum_pages:2,writes_enabled:false};
        let proof=baal_transport_service::verify_frozen_ordinary_pre_admission(&config,&spec,&permit,NOW,&error).unwrap();let evidence=proof.evidence().clone();
        f.log.append_aborted_before_admission(id,&proof,&iso(NOW+2)).unwrap();f.event(json!({"type":"unsubmitted_intent_retired","source":"moloch_live_dispatcher","orderId":id,"confirmedNetworkAttempts":0,"receiptHash":sha(&serde_json::to_vec(&evidence).unwrap()),"receivedAt":iso(NOW+3),"processedAt":iso(NOW+3),"deferPlanningForPackage":true}));
        (config,evidence)
    }
    #[cfg(target_os="linux")]
    #[test]fn verified_ordinary_abort_unblocks_forecast_only_after_original_sdk_restart_verification(){
        let mut f=F::setup(false,"cup");let ordinary=f.bank.engine().order_records().into_iter().find(|q|q["side"]=="YES"&&q["action"]=="buy"&&q["retired"]==false).unwrap();let old=ordinary["id"].as_u64().unwrap();let(config,evidence)=prove_pre_admission_abort(&mut f,&ordinary,false);
        let report=f.bank.engine().report().unwrap();let head=f.bank.durable_head();f.log=dispatch::DispatchLog::open(&f.root.join("dispatch.ndjson")).unwrap();assert!(!f.log.proved_no_network(old));assert!(f.log.require_resolved_attempts().is_err());
        let r=approve(f.review.clone(),&mut f.trust,NOW+4);assert!(f.store.as_mut().unwrap().install(r,&f.bank,&f.log,NOW+4).is_err(),"a rehashed abort row alone cannot waive same-market liability");
        let(spec,permit,at,error)=f.log.pre_admission_originals(old).unwrap();let proof=baal_transport_service::verify_retained_ordinary_pre_admission(&config,&spec,&permit,at,&error,&evidence).unwrap();f.log.verify_aborted_before_admission(old,&proof).unwrap();f.install(f.review.clone(),NOW+4);
        assert_eq!(f.bank.engine().report().unwrap(),report);assert_eq!(f.bank.durable_head(),head);assert_eq!(f.store.as_ref().unwrap().entry_hints(&f.bank,&f.log,&f.scope,&f.trust,NOW+4).unwrap().len(),1);
        let q=f.entry();assert!(matches!(f.assess(&q,NOW+4),Decision::Entry{..}));let id=q["id"].as_u64().unwrap();let spec=f.spec(&q);f.store.as_mut().unwrap().bind(&f.bank,&f.log,&f.scope,id,&spec,&f.trust,NOW+4).unwrap();let p=f.permit(&q,NOW+4);let a=f.authority();let token=f.store.as_ref().unwrap().physical_decision(&f.bank,&f.log,&f.scope,id,&spec,&p,&a,NOW+4).unwrap();token.authorize(&f.scope,&spec,&p,NOW+4).unwrap();drop(token);
        f.log.append(json!({"kind":"intent","qid":9999})).unwrap();f.log.append(json!({"kind":"attempt","qid":9999})).unwrap();assert!(f.log.require_resolved_attempts().is_err());assert_eq!(f.assess(&q,NOW+5),Decision::Hold{reason:"unknown_physical_acceptance".into()});assert!(f.store.as_ref().unwrap().check_bound(&f.bank,&f.log,&f.scope,id,&spec,&f.trust,NOW+5).is_err());
        assert!(f.log.attempted.contains(&old)&&!f.log.never_attempted(old)&&!f.log.acknowledgments.contains_key(&old)&&!f.log.closed.contains(&old));assert!(f.bank.engine().fills().is_empty());
    }
    #[cfg(target_os="linux")]
    #[test]fn tagged_aborted_entry_retains_exclusive_cohort_and_cannot_mint_a_retry(){
        let mut f=F::new();let(q,spec)=f.bind_entry();let id=q["id"].as_u64().unwrap();prove_pre_admission_abort(&mut f,&q,true);f.reopen();
        assert!(f.log.proved_no_network(id));f.log.require_resolved_attempts().unwrap();assert_eq!(f.assess(&q,NOW+4),Decision::Hold{reason:"proved_aborted_intent_cannot_retry".into()});
        assert!(f.store.as_ref().unwrap().entry_hints(&f.bank,&f.log,&f.scope,&f.trust,NOW+4).unwrap().is_empty(),"physical proof alone does not abandon/rearm the tagged source cohort");
        let p=f.permit(&q,NOW+4);let a=f.authority();assert!(f.store.as_ref().unwrap().check_bound(&f.bank,&f.log,&f.scope,id,&spec,&f.trust,NOW+4).unwrap_err().to_string().contains("ABORTED_INTENT_CANNOT_RETRY"));assert!(f.store.as_ref().unwrap().physical_decision(&f.bank,&f.log,&f.scope,id,&spec,&p,&a,NOW+4).is_err());
        assert!(!f.log.never_attempted(id)&&!f.log.closed.contains(&id)&&!f.log.acknowledgments.contains_key(&id));assert!(f.bank.engine().fills().is_empty());
    }
    #[cfg(target_os="linux")]
    fn proved_abort_abandon_fixture()->(F,baal_transport_service::Transport,baal_transport_service::Config,Value,moloch_live_runtime::authority::CancelSnapshotSet,Value,Value){
        use baal_transport_service::{Transport,DisabledCredentials,SystemClock,ReadOnlyAuthority};
        let mut f=F::setup(true,"11111111-1111-4111-8111-111111111111");let(q,_)=f.bind_entry();let(config,evidence)=prove_pre_admission_abort(&mut f,&q,true);let id=q["id"].as_u64().unwrap();
        // This synthetic ordinary pair still has its untouched opposite draft;
        // production manager setup retires such never-attempted drafts. Keep
        // the real liability guard: retire only that genuine unsent sibling.
        let siblings:Vec<_>=f.bank.engine().order_records().into_iter().filter(|o|o["exchangeId"]=="101"&&o["retired"]==false).collect();
        assert!(!siblings.is_empty());for sibling in &siblings{let sibling_id=sibling["id"].as_u64().unwrap();assert_ne!(sibling_id,id);assert!(f.log.never_attempted(sibling_id)&&!f.log.intents.contains_key(&sibling_id)&&!f.bank.engine().live_order_bindings().contains_key(&sibling_id));}
        f.retire_drafts(NOW+4);assert!(!f.bank.engine().order_records().iter().any(|o|o["exchangeId"]=="101"&&o["retired"]==false));
        // Use the EXACT event bytes accepted by Session, including processedAt.
        let original=std::fs::read_to_string(f.root.join("bank/events.jsonl")).unwrap();let retirement=original.lines().map(|l|serde_json::from_str::<Value>(l).unwrap()["event"].clone()).find(|e|e["type"]=="unsubmitted_intent_retired"&&e["orderId"]==id).unwrap();
        let t=Transport::new_for_mock(config.clone(),Box::new(DisabledCredentials),Box::new(SystemClock),Box::new(ReadOnlyAuthority),"http://127.0.0.1:9/api/v1".into()).unwrap();
        let receipt=|endpoint:String,data:Value,i:u64|baal_transport_service::Receipt{endpoint,request_started_at:iso(NOW+31+i*2),received_at:iso(NOW+32+i*2),status:200,data};
        let facts=moloch_live_runtime::authority::CancelSnapshotSet{
            account:receipt("/tournaments/cup".into(),json!({"id":f.scope.tournament_id,"slug":"cup","status":"active"}),0),
            positions:receipt("/tournaments/cup/portfolio/positions".into(),json!({"positions":[]}),1),
            open_orders:receipt(format!("/orders?status=open&limit=200&tournamentId={}",f.scope.tournament_id),json!({"data":[],"coverage":{"complete":true},"pagination":{"hasMore":false,"nextCursor":null}}),2),
            cash_balance:receipt(format!("/account?tournamentId={}",f.scope.tournament_id),json!({"balance":1000,"availableAdvance":0,"buyingPower":1000}),3)};
        (f,t,config,evidence,facts,retirement,q)
    }
    #[cfg(target_os="linux")]
    #[test]fn sdk_proved_abort_archives_cohort_without_retry_or_financial_mutation_and_survives_reopen(){
        let(mut f,t,_config,_evidence,facts,retirement,q)=proved_abort_abandon_fixture();let id=q["id"].as_u64().unwrap();let original_tag=f.store.as_ref().unwrap().durable_tag(id).unwrap();let report=f.bank.engine().report().unwrap();let head=f.bank.durable_head();
        let hint=f.store.as_ref().unwrap().abandonment_hints(&f.bank,&f.log,&f.scope,NOW+40).unwrap().remove(0);let reference=&retirement["sourceRef"];
        let plan=f.store.as_ref().unwrap().prepare_abandon(&f.bank,&f.log,&t,&f.scope,&hint,&facts,reference["id"].as_str().unwrap(),reference["sequence"].as_u64().unwrap(),&retirement,NOW+40).unwrap();
        f.store.as_mut().unwrap().apply_abandon(plan,&f.bank,&f.log,&t,NOW+40).unwrap();f.reopen();
        assert_eq!(f.bank.durable_head(),head);assert_eq!(f.bank.engine().report().unwrap(),report);assert_eq!(f.store.as_ref().unwrap().durable_tag(id),Some(original_tag));
        assert!(f.log.attempted.contains(&id)&&f.log.proved_no_network(id)&&!f.log.never_attempted(id)&&!f.log.closed.contains(&id)&&!f.log.acknowledgments.contains_key(&id));
        assert_eq!(f.store.as_ref().unwrap().entry_hints(&f.bank,&f.log,&f.scope,&f.trust,NOW+41).unwrap().len(),1);
        let spec=f.spec(&q);assert!(f.store.as_ref().unwrap().check_bound(&f.bank,&f.log,&f.scope,id,&spec,&f.trust,NOW+41).is_err());assert!(f.store.as_ref().unwrap().validate_acknowledgment(id,&F::base(&q,"actual_order_ack",NOW+42,"999")).is_err());
        let log=std::fs::read_to_string(f.root.join("fundamental.ndjson")).unwrap();assert_eq!(log.lines().map(|r|serde_json::from_str::<Value>(r).unwrap()).filter(|r|r["event"]["kind"]=="abandon_proved_no_network").count(),1);
    }
    #[cfg(target_os="linux")]
    #[test]fn proved_abort_abandonment_requires_opaque_rehydration_and_exact_retirement_and_flat_facts(){
        for case in 0..6{let(mut f,t,config,evidence,mut facts,mut retirement,q)=proved_abort_abandon_fixture();let id=q["id"].as_u64().unwrap();let hint=f.store.as_ref().unwrap().abandonment_hints(&f.bank,&f.log,&f.scope,NOW+40).unwrap().remove(0);let reference=retirement["sourceRef"].clone();
            match case{0=>{f.log=dispatch::DispatchLog::open(&f.root.join("dispatch.ndjson")).unwrap();assert!(!f.log.proved_no_network(id));},1=>retirement["receiptHash"]=json!("a".repeat(64)),2=>facts.positions.data["positions"]=json!([{"exchangeId":"101","quantity":1}]),3=>facts.open_orders.data["data"]=json!([{"id":"999","exchangeId":"101"}]),4=>{std::fs::write(config.queue_dir.join(evidence["sourceArchive"].as_str().unwrap()),b"{}").unwrap();},_=>{f.log.intents.get_mut(&id).unwrap()["spec"]["body"]["quantity"]=json!(1);}}
            assert!(f.store.as_ref().unwrap().prepare_abandon(&f.bank,&f.log,&t,&f.scope,&hint,&facts,reference["id"].as_str().unwrap(),reference["sequence"].as_u64().unwrap(),&retirement,NOW+40).is_err(),"case {case}");
        }
    }
    #[cfg(target_os="linux")]
    #[test]fn proved_abort_archive_changes_after_mint_and_unhydrated_restart_remain_fail_closed(){
        for mutate_archive in [false,true]{let(mut f,t,config,evidence,facts,retirement,q)=proved_abort_abandon_fixture();let id=q["id"].as_u64().unwrap();let hint=f.store.as_ref().unwrap().abandonment_hints(&f.bank,&f.log,&f.scope,NOW+40).unwrap().remove(0);let reference=&retirement["sourceRef"];
            let plan=f.store.as_ref().unwrap().prepare_abandon(&f.bank,&f.log,&t,&f.scope,&hint,&facts,reference["id"].as_str().unwrap(),reference["sequence"].as_u64().unwrap(),&retirement,NOW+40).unwrap();
            if mutate_archive{std::fs::write(config.queue_dir.join(evidence["sourceArchive"].as_str().unwrap()),b"{}").unwrap();assert!(f.store.as_mut().unwrap().apply_abandon(plan,&f.bank,&f.log,&t,NOW+40).is_err());}
            else{f.store.as_mut().unwrap().apply_abandon(plan,&f.bank,&f.log,&t,NOW+40).unwrap();f.store.take();f.log=dispatch::DispatchLog::open(&f.root.join("dispatch.ndjson")).unwrap();assert!(Cohorts::open(&f.root,&f.bank,&f.log,f.owner.clone()).is_err());let(spec,p,at,error)=f.log.pre_admission_originals(id).unwrap();let proof=baal_transport_service::verify_retained_ordinary_pre_admission(&config,&spec,&p,at,&error,&evidence).unwrap();f.log.verify_aborted_before_admission(id,&proof).unwrap();f.reopen();assert_eq!(f.store.as_ref().unwrap().entry_hints(&f.bank,&f.log,&f.scope,&f.trust,NOW+41).unwrap().len(),1);}
        }
    }
    #[test]fn independent_entry_plan_expires_and_detects_dispatch_or_review_file_change(){
        for case in 0..3{let(mut f,t,facts)=entry_fixture();let authority=f.authority();let store=f.store.as_ref().unwrap();let hint=store.entry_hints(&f.bank,&f.log,&f.scope,&f.trust,NOW+25).unwrap().remove(0);let plan=store.prepare_entry(&f.bank,&f.log,&t,&f.scope,&hint,&facts,&authority,NOW+25).unwrap().unwrap();
            match case{0=>assert!(plan.validate_current(&f.bank,&f.log,&t,&authority,plan.valid_until_ms()).is_err()),1=>{f.log.append(json!({"kind":"source","qid":999,"diagnostic":"actual local journal advance"})).unwrap();assert!(plan.validate_current(&f.bank,&f.log,&t,&authority,NOW+26).is_err());},_=>{std::fs::write(f.root.join("review-authority.json"),b"changed").unwrap();assert!(plan.validate_current(&f.bank,&f.log,&t,&authority,NOW+26).is_err());}}
        }
    }
    fn abandonment_fixture(positive:Option<bool>)->(F,baal_transport_service::Transport,moloch_live_runtime::authority::CancelSnapshotSet,Value,Value){
        let(mut f,t,facts)=entry_fixture();let authority=f.authority();let hint=f.store.as_ref().unwrap().entry_hints(&f.bank,&f.log,&f.scope,&f.trust,NOW+25).unwrap().remove(0);
        let plan=f.store.as_ref().unwrap().prepare_entry(&f.bank,&f.log,&t,&f.scope,&hint,&facts,&authority,NOW+25).unwrap().unwrap();f.bank.accept("reviewed-entry",1,plan.event()).unwrap();
        let q=f.bank.engine().order_records().into_iter().find(|q|q["retired"]==false&&q.get("reviewedFundamentalEntry").is_some()).unwrap();let id=q["id"].as_u64().unwrap();let spec=f.spec(&q);
        f.store.as_mut().unwrap().bind(&f.bank,&f.log,&f.scope,id,&spec,&f.trust,NOW+26).unwrap();
        f.log.append(json!({"kind":"intent","qid":id,"quote":q,"spec":spec,"fundamental":f.store.as_ref().unwrap().durable_tag(id).unwrap()})).unwrap();
        let mut proof=f.log.proof(id);if positive==Some(false){proof["networkAttempts"]=json!(1);}if positive.is_some(){f.log.append(json!({"kind":"not_sent","qid":id,"proof":proof})).unwrap();}
        let retirement=json!({"type":"unsubmitted_intent_retired","source":"moloch_live_dispatcher","orderId":id,"confirmedNetworkAttempts":0,"receiptHash":sha(&serde_json::to_vec(&proof).unwrap()),"receivedAt":iso(NOW+30),"processedAt":iso(NOW+30),"deferPlanningForPackage":true});
        f.bank.accept("never-sent-retirement",1,&retirement).unwrap();
        let mut facts=moloch_live_runtime::authority::CancelSnapshotSet{account:facts.account,positions:facts.positions,open_orders:facts.open_orders,cash_balance:facts.cash_balance};
        for(i,r)in [&mut facts.account,&mut facts.positions,&mut facts.open_orders,&mut facts.cash_balance].into_iter().enumerate(){r.request_started_at=iso(NOW+31+i as u64*2);r.received_at=iso(NOW+32+i as u64*2);}
        (f,t,facts,retirement,q)
    }
    #[test]fn never_sent_abandonment_archives_original_tag_and_reopens_without_financial_mutation(){
        let(mut f,t,facts,retirement,q)=abandonment_fixture(Some(true));let id=q["id"].as_u64().unwrap();let original=f.store.as_ref().unwrap().durable_tag(id).unwrap();let head=f.bank.durable_head();let report=f.bank.engine().report().unwrap();
        let hint=f.store.as_ref().unwrap().abandonment_hints(&f.bank,&f.log,&f.scope,NOW+40).unwrap().remove(0);
        let prepare=|f:&F|f.store.as_ref().unwrap().prepare_abandon(&f.bank,&f.log,&t,&f.scope,&hint,&facts,"never-sent-retirement",1,&retirement,NOW+40).unwrap();let plan=prepare(&f);let duplicate=prepare(&f);
        f.store.as_mut().unwrap().apply_abandon(plan,&f.bank,&f.log,&t,NOW+40).unwrap();assert!(f.store.as_mut().unwrap().apply_abandon(duplicate,&f.bank,&f.log,&t,NOW+41).is_err());
        assert_eq!(f.bank.durable_head(),head);assert_eq!(f.bank.engine().report().unwrap(),report);assert_eq!(f.store.as_ref().unwrap().durable_tag(id),Some(original.clone()));
        f.reopen();assert_eq!(f.store.as_ref().unwrap().durable_tag(id),Some(original));assert_eq!(f.store.as_ref().unwrap().entry_hints(&f.bank,&f.log,&f.scope,&f.trust,NOW+41).unwrap().len(),1);
        let mut ack=F::base(&q,"actual_order_ack",NOW+42,"901");ack["quantity"]=q["quantity"].clone();assert!(f.store.as_ref().unwrap().validate_acknowledgment(id,&ack).is_err());
    }
    #[test]fn archived_original_intent_tag_survives_a_new_live_cohort_on_restart(){
        let(mut f,t,facts,retirement,q)=abandonment_fixture(Some(true));let old=q["id"].as_u64().unwrap();let old_tag=f.store.as_ref().unwrap().durable_tag(old).unwrap();
        let hint=f.store.as_ref().unwrap().abandonment_hints(&f.bank,&f.log,&f.scope,NOW+40).unwrap().remove(0);let plan=f.store.as_ref().unwrap().prepare_abandon(&f.bank,&f.log,&t,&f.scope,&hint,&facts,"never-sent-retirement",1,&retirement,NOW+40).unwrap();f.store.as_mut().unwrap().apply_abandon(plan,&f.bank,&f.log,&t,NOW+40).unwrap();
        let mut book=f.bank.engine().live_package_books()[0].clone();book["sequence"]=json!("100");for key in ["sourceAt","requestStartedAt","receivedAt"]{book[key]=json!(iso(NOW+4000));}f.bank.accept("post-abandon-book",1,&json!({"type":"book","receivedAt":iso(NOW+4000),"book":book})).unwrap();let mut r=horizon_input(f.bank.manifest_hash().as_str().unwrap(),NOW+4051);r.tournament_id=f.scope.tournament_id.clone();set_deadline_cash(&mut r,"0.940500");f.install(r,NOW+4051);
        let new=f.entry();let spec=f.spec(&new);let new_id=new["id"].as_u64().unwrap();assert_ne!(new_id,old);f.store.as_mut().unwrap().bind(&f.bank,&f.log,&f.scope,new_id,&spec,&f.trust,NOW+4051).unwrap();
        f.log.append(json!({"kind":"intent","qid":new_id,"quote":new,"spec":spec,"fundamental":f.store.as_ref().unwrap().durable_tag(new_id).unwrap()})).unwrap();f.reopen();assert_eq!(f.store.as_ref().unwrap().durable_tag(old),Some(old_tag));assert!(f.store.as_ref().unwrap().durable_tag(new_id).is_some());
    }
    #[test]fn timeout_or_absence_or_forged_not_sent_never_proves_abandonment(){
        for proof in [None,Some(false)]{let(f,t,facts,retirement,_)=abandonment_fixture(proof);let hint=f.store.as_ref().unwrap().abandonment_hints(&f.bank,&f.log,&f.scope,NOW+40).unwrap().remove(0);
            assert!(f.store.as_ref().unwrap().prepare_abandon(&f.bank,&f.log,&t,&f.scope,&hint,&facts,"never-sent-retirement",1,&retirement,NOW+40).is_err());}
    }
    #[test]fn abandonment_requires_exact_committed_retirement_and_fresh_complete_flat_account(){
        for case in 0..7{let(f,t,mut facts,mut retirement,_)=abandonment_fixture(Some(true));let hint=f.store.as_ref().unwrap().abandonment_hints(&f.bank,&f.log,&f.scope,NOW+40).unwrap().remove(0);
            match case{0=>retirement["receiptHash"]=json!("f".repeat(64)),1=>retirement["processedAt"]=json!(iso(NOW+29)),2=>facts.positions.data["positions"]=json!([{"exchangeId":"101","quantity":-1}]),3=>facts.open_orders.data["data"]=json!([{"exchangeId":"101","id":"901"}]),4=>facts.open_orders.data["coverage"]["complete"]=json!(false),5=>facts.account.request_started_at=iso(NOW-20000),_=>facts.positions.data["pagination"]=json!({"hasMore":false})}
            assert!(f.store.as_ref().unwrap().prepare_abandon(&f.bank,&f.log,&t,&f.scope,&hint,&facts,"never-sent-retirement",1,&retirement,NOW+40).is_err(),"case {case}");}
    }
    #[test]fn abandonment_plan_rejects_prefix_or_original_journal_drift(){
        for case in 0..2{let(mut f,t,facts,retirement,_)=abandonment_fixture(Some(true));let hint=f.store.as_ref().unwrap().abandonment_hints(&f.bank,&f.log,&f.scope,NOW+40).unwrap().remove(0);let plan=f.store.as_ref().unwrap().prepare_abandon(&f.bank,&f.log,&t,&f.scope,&hint,&facts,"never-sent-retirement",1,&retirement,NOW+40).unwrap();
            if case==0{f.event(json!({"type":"tick","receivedAt":iso(NOW+41)}));}else{f.log.append(json!({"kind":"source","qid":hint.qid})).unwrap();}
            assert!(f.store.as_mut().unwrap().apply_abandon(plan,&f.bank,&f.log,&t,NOW+41).is_err());}
    }
    #[test]fn attempted_partial_filled_and_late_archived_attempt_cannot_reset_eligibility(){
        let mut f=F::new();let(q,spec)=f.bind_entry();f.log_intent(&q,&spec);assert!(f.store.as_ref().unwrap().abandonment_hints(&f.bank,&f.log,&f.scope,NOW+1).is_err());
        let mut f=F::new();f.fill_entry(true);assert!(f.store.as_ref().unwrap().abandonment_hints(&f.bank,&f.log,&f.scope,NOW+4).unwrap().is_empty());
        let(mut f,t,facts,retirement,q)=abandonment_fixture(Some(true));let hint=f.store.as_ref().unwrap().abandonment_hints(&f.bank,&f.log,&f.scope,NOW+40).unwrap().remove(0);let plan=f.store.as_ref().unwrap().prepare_abandon(&f.bank,&f.log,&t,&f.scope,&hint,&facts,"never-sent-retirement",1,&retirement,NOW+40).unwrap();f.store.as_mut().unwrap().apply_abandon(plan,&f.bank,&f.log,&t,NOW+40).unwrap();
        f.log.append(json!({"kind":"attempt","qid":q["id"]})).unwrap();assert!(f.store.as_ref().unwrap().entry_hints(&f.bank,&f.log,&f.scope,&f.trust,NOW+41).is_err());f.store.take();assert!(Cohorts::open(&f.root,&f.bank,&f.log,f.owner.clone()).is_err());
    }
    fn closed_zero_fixture(recovered:bool)->(F,baal_transport_service::Transport,moloch_live_runtime::authority::CancelSnapshotSet,baal_transport_service::Receipt,baal_transport_service::Receipt,Value,Value){closed_zero_fixture_quantity(recovered,2)}
    fn closed_zero_fixture_quantity(recovered:bool,n:u64)->(F,baal_transport_service::Transport,moloch_live_runtime::authority::CancelSnapshotSet,baal_transport_service::Receipt,baal_transport_service::Receipt,Value,Value){
        use baal_transport_service::Receipt;let(mut f,t,facts)=entry_fixture();let authority=f.authority();let hint=f.store.as_ref().unwrap().entry_hints(&f.bank,&f.log,&f.scope,&f.trust,NOW+25).unwrap().remove(0);
        let plan=f.store.as_ref().unwrap().prepare_entry(&f.bank,&f.log,&t,&f.scope,&hint,&facts,&authority,NOW+25).unwrap().unwrap();f.bank.accept("reviewed-entry",1,plan.event()).unwrap();let q=f.bank.engine().order_records().into_iter().find(|q|q["retired"]==false&&q.get("reviewedFundamentalEntry").is_some()).unwrap();let id=q["id"].as_u64().unwrap();let mut spec=f.spec(&q);spec.body.as_mut().unwrap()["quantity"]=json!(n);
        f.store.as_mut().unwrap().bind_quantity(&f.bank,&f.log,&f.scope,id,n,&spec,&f.trust,NOW+26).unwrap();f.log_intent(&q,&spec);
        let order_data=json!({"id":"901","exchangeId":"101","tournamentId":f.scope.tournament_id,"side":"no","action":"buy","priceLimit":0.88,"quantity":n,"quantityFilled":0,"open":false,"createdAt":iso(NOW+3100),"expirationDate":iso(q["expiresAt"].as_u64().unwrap()),"terminalReasonCode":"UserRequested"});
        let order=Receipt{endpoint:"/orders/901".into(),status:200,request_started_at:iso(NOW+3200),received_at:iso(NOW+3201),data:order_data.clone()};
        let fills=Receipt{endpoint:"/orders/901/fills?limit=200".into(),status:200,request_started_at:iso(NOW+3202),received_at:iso(NOW+3203),data:json!({"orderId":"901","exchangeId":"101","tournamentId":f.scope.tournament_id,"totalQuantityFilled":0,"avgFillPrice":null,"coverage":{"complete":true,"projectedThroughSequence":120},"pagination":{"hasMore":false,"nextCursor":null},"data":[]})};
        let event=if recovered{let mut ack=order.clone();ack.request_started_at=iso(NOW+3100);ack.received_at=iso(NOW+3101);ack.data["open"]=json!(true);ack.data["terminalReasonCode"]=Value::Null;let mut prior_fills=fills.clone();prior_fills.request_started_at=iso(NOW+3102);prior_fills.received_at=iso(NOW+3103);
            let event=moloch_live_runtime::feedback::recovered_acknowledgment(&q,"901",&f.scope.tournament_id,&ack,&prior_fills,&iso(NOW+3104)).unwrap();f.log.append(json!({"kind":"recovered_ack","qid":id,"actualOrderId":"901","order":ack,"fills":prior_fills,"event":event,"proofPath":"synthetic-original-recovery"})).unwrap();event
        }else{let ack=Receipt{endpoint:"/orders".into(),status:200,request_started_at:iso(NOW+3100),received_at:iso(NOW+3101),data:json!({"orderId":"901","exchangeId":"101","tournamentId":f.scope.tournament_id,"side":"no","action":"buy","quantity":n,"price":0.88,"status":"accepted"})};let event=moloch_live_runtime::feedback::acknowledgment(&q,&ack,&iso(NOW+3102)).unwrap();f.log.append(json!({"kind":"ack","qid":id,"receipt":ack})).unwrap();event};
        f.store.as_ref().unwrap().validate_acknowledgment(id,&event).unwrap();f.bank.accept("original-order-ack",1,&event).unwrap();
        let terminal=moloch_live_runtime::feedback::terminal(&q,"901",&f.scope.tournament_id,&order,&fills,&iso(NOW+3204)).unwrap().unwrap();f.bank.accept("closed-zero-terminal",1,&terminal).unwrap();f.log.append(json!({"kind":"closed","qid":id,"orderReceiptHash":moloch_live_runtime::feedback::digest(&order).unwrap(),"fillReceiptHash":moloch_live_runtime::feedback::digest(&fills).unwrap()})).unwrap();f.retire_drafts(NOW+3205);
        let mut account=moloch_live_runtime::authority::CancelSnapshotSet{account:facts.account,positions:facts.positions,open_orders:facts.open_orders,cash_balance:facts.cash_balance};for(i,r)in [&mut account.account,&mut account.positions,&mut account.open_orders,&mut account.cash_balance].into_iter().enumerate(){r.request_started_at=iso(NOW+3210+i as u64*2);r.received_at=iso(NOW+3211+i as u64*2);}
        (f,t,account,order,fills,terminal,q)
    }
    #[test]fn genuinely_closed_zero_fill_post_and_recovered_orders_rearm_with_preserved_original_binding(){
        for recovered in [false,true]{let(mut f,t,account,order,fills,terminal,q)=closed_zero_fixture(recovered);let old=q["id"].as_u64().unwrap();let tag=f.store.as_ref().unwrap().durable_tag(old).unwrap();let head=f.bank.durable_head();let report=f.bank.engine().report().unwrap();let hint=f.store.as_ref().unwrap().rearm_hints(&f.bank,&f.log,&f.scope,NOW+3220).unwrap().remove(0);
            let prepare=|f:&F|f.store.as_ref().unwrap().prepare_rearm(&f.bank,&f.log,&t,&f.scope,&hint,&account,&order,&fills,"closed-zero-terminal",1,&terminal,NOW+3220).unwrap();let plan=prepare(&f);let duplicate=prepare(&f);f.store.as_mut().unwrap().apply_rearm(plan,&f.bank,&f.log,&t,NOW+3220).unwrap();assert!(f.store.as_mut().unwrap().apply_rearm(duplicate,&f.bank,&f.log,&t,NOW+3221).is_err());
            assert_eq!(f.bank.durable_head(),head);assert_eq!(f.bank.engine().report().unwrap(),report);f.reopen();assert_eq!(f.store.as_ref().unwrap().durable_tag(old),Some(tag));assert_eq!(f.store.as_ref().unwrap().entry_hints(&f.bank,&f.log,&f.scope,&f.trust,NOW+3221).unwrap().len(),1);assert!(f.log.attempted.contains(&old)&&f.log.closed.contains(&old));
        }
    }
    #[test]fn closed_zero_rearm_rejects_mutated_or_incomplete_original_receipts_and_terminal_reference(){
        for case in 0..9{let(f,t,mut account,mut order,mut fills,mut terminal,_)=closed_zero_fixture(false);let hint=f.store.as_ref().unwrap().rearm_hints(&f.bank,&f.log,&f.scope,NOW+3220).unwrap().remove(0);
            match case{0=>fills.data["coverage"]["complete"]=json!(false),1=>fills.data["totalQuantityFilled"]=json!(-1),2=>fills.endpoint="/portfolio/fills".into(),3=>order.data["id"]=json!("902"),4=>order.data["open"]=json!(true),5=>terminal["receiptHash"]=json!("a".repeat(64)),6=>account.positions.data["positions"]=json!([{"exchangeId":"101","quantity":1}]),7=>account.open_orders.data["data"]=json!([{"id":"901","exchangeId":"101"}]),_=>account.account.request_started_at=iso(NOW-20000)}
            assert!(f.store.as_ref().unwrap().prepare_rearm(&f.bank,&f.log,&t,&f.scope,&hint,&account,&order,&fills,"closed-zero-terminal",1,&terminal,NOW+3220).is_err(),"case {case}");}
    }
    #[test]fn zero_fill_rearm_preserves_original_larger_draft_and_exact_smaller_physical_slice(){
        let(mut f,t,account,order,fills,terminal,q)=closed_zero_fixture_quantity(false,1);assert_eq!(q["quantity"],2);let id=q["id"].as_u64().unwrap();assert_eq!(f.bank.engine().order_records().into_iter().find(|q|q["id"]==id).unwrap()["quantity"],1);
        let hint=f.store.as_ref().unwrap().rearm_hints(&f.bank,&f.log,&f.scope,NOW+3220).unwrap().remove(0);let plan=f.store.as_ref().unwrap().prepare_rearm(&f.bank,&f.log,&t,&f.scope,&hint,&account,&order,&fills,"closed-zero-terminal",1,&terminal,NOW+3220).unwrap();f.store.as_mut().unwrap().apply_rearm(plan,&f.bank,&f.log,&t,NOW+3220).unwrap();f.reopen();assert!(f.store.as_ref().unwrap().durable_tag(id).is_some());assert_eq!(f.bank.engine().wallet.quantity("101",moloch_replay::Side::No),0);
    }
    #[test]fn rearmed_closed_binding_stays_archived_beside_a_new_cohort_and_rejects_late_ack(){
        let(mut f,t,account,order,fills,terminal,q)=closed_zero_fixture(false);let old=q["id"].as_u64().unwrap();let tag=f.store.as_ref().unwrap().durable_tag(old).unwrap();let hint=f.store.as_ref().unwrap().rearm_hints(&f.bank,&f.log,&f.scope,NOW+3220).unwrap().remove(0);let plan=f.store.as_ref().unwrap().prepare_rearm(&f.bank,&f.log,&t,&f.scope,&hint,&account,&order,&fills,"closed-zero-terminal",1,&terminal,NOW+3220).unwrap();f.store.as_mut().unwrap().apply_rearm(plan,&f.bank,&f.log,&t,NOW+3220).unwrap();
        let mut book=f.bank.engine().live_package_books()[0].clone();book["sequence"]=json!("100");for key in ["sourceAt","requestStartedAt","receivedAt"]{book[key]=json!(iso(NOW+6000));}f.bank.accept("rearmed-book",1,&json!({"type":"book","receivedAt":iso(NOW+6000),"book":book})).unwrap();let mut review=horizon_input(f.bank.manifest_hash().as_str().unwrap(),NOW+6051);review.tournament_id=f.scope.tournament_id.clone();set_deadline_cash(&mut review,"0.940500");f.install(review,NOW+6051);
        let next=f.entry();let spec=f.spec(&next);let new=next["id"].as_u64().unwrap();assert_ne!(old,new);f.store.as_mut().unwrap().bind(&f.bank,&f.log,&f.scope,new,&spec,&f.trust,NOW+6051).unwrap();f.reopen();assert_eq!(f.store.as_ref().unwrap().durable_tag(old),Some(tag));assert!(f.store.as_ref().unwrap().durable_tag(new).is_some());assert!(f.store.as_ref().unwrap().validate_acknowledgment(old,&F::base(&q,"actual_order_ack",NOW+6052,"902")).is_err());
    }
    #[test]fn zero_fill_rearm_is_not_available_for_unknown_or_partial_actual_execution(){
        let mut f=F::new();let(q,spec)=f.bind_entry();f.log_intent(&q,&spec);assert!(f.store.as_ref().unwrap().rearm_hints(&f.bank,&f.log,&f.scope,NOW+1).is_err());let mut f=F::new();f.fill_entry(true);assert!(f.store.as_ref().unwrap().rearm_hints(&f.bank,&f.log,&f.scope,NOW+4).unwrap().is_empty());
    }
    #[test]fn zero_fill_rearm_plan_withholds_on_bank_prefix_or_original_log_advance(){
        for case in 0..2{let(mut f,t,account,order,fills,terminal,_)=closed_zero_fixture(false);let hint=f.store.as_ref().unwrap().rearm_hints(&f.bank,&f.log,&f.scope,NOW+3220).unwrap().remove(0);let plan=f.store.as_ref().unwrap().prepare_rearm(&f.bank,&f.log,&t,&f.scope,&hint,&account,&order,&fills,"closed-zero-terminal",1,&terminal,NOW+3220).unwrap();
            if case==0{f.event(json!({"type":"tick","receivedAt":iso(NOW+3221)}));}else{f.log.append(json!({"kind":"source","qid":hint.qid})).unwrap();}assert!(f.store.as_mut().unwrap().apply_rearm(plan,&f.bank,&f.log,&t,NOW+3221).is_err());}
    }
    fn actual_startup_ack(f:&F,qid:u64,at:u64)->Value{
        let q=f.bank.engine().order_records().into_iter().find(|q|q["id"]==qid).unwrap();let ack:baal_transport_service::Receipt=serde_json::from_value(f.log.acknowledgments[&qid].clone()).unwrap();
        if let Some(recovery)=f.log.recovered.get(&qid){moloch_live_runtime::feedback::recovered_acknowledgment(&q,recovery["actualOrderId"].as_str().unwrap(),&f.scope.tournament_id,&ack,&serde_json::from_value(recovery["fills"].clone()).unwrap(),&iso(at)).unwrap()}
        else{moloch_live_runtime::feedback::acknowledgment(&q,&ack,&iso(at)).unwrap()}
    }
    #[test]fn tagged_archived_original_ack_restores_after_restart_beside_new_cohort_but_new_ack_is_denied(){
        for recovered in [false,true]{let(mut f,t,account,order,fills,terminal,q)=closed_zero_fixture_quantity(recovered,1);let old=q["id"].as_u64().unwrap();let old_tag=f.store.as_ref().unwrap().durable_tag(old).unwrap();
            let hint=f.store.as_ref().unwrap().rearm_hints(&f.bank,&f.log,&f.scope,NOW+3220).unwrap().remove(0);let plan=f.store.as_ref().unwrap().prepare_rearm(&f.bank,&f.log,&t,&f.scope,&hint,&account,&order,&fills,"closed-zero-terminal",1,&terminal,NOW+3220).unwrap();f.store.as_mut().unwrap().apply_rearm(plan,&f.bank,&f.log,&t,NOW+3220).unwrap();f.reopen();
            let mut book=f.bank.engine().live_package_books()[0].clone();book["sequence"]=json!("100");for key in ["sourceAt","requestStartedAt","receivedAt"]{book[key]=json!(iso(NOW+6000));}f.bank.accept("restart-book",1,&json!({"type":"book","receivedAt":iso(NOW+6000),"book":book})).unwrap();let mut review=horizon_input(f.bank.manifest_hash().as_str().unwrap(),NOW+6051);review.tournament_id=f.scope.tournament_id.clone();set_deadline_cash(&mut review,"0.940500");f.install(review,NOW+6051);
            let next=f.entry();let spec=f.spec(&next);let new=next["id"].as_u64().unwrap();f.store.as_mut().unwrap().bind(&f.bank,&f.log,&f.scope,new,&spec,&f.trust,NOW+6051).unwrap();f.log.append(json!({"kind":"intent","qid":new,"quote":next,"spec":spec,"fundamental":f.store.as_ref().unwrap().durable_tag(new).unwrap()})).unwrap();f.reopen();
            let restored=actual_startup_ack(&f,old,NOW+6052);let store=f.store.as_ref().unwrap();store.validate_restored_acknowledgment(&f.bank,&f.log,old,&restored).unwrap();assert!(store.validate_acknowledgment(old,&restored).is_err(),"live admission cannot use startup exception");
            for case in 0..6{let mut changed=restored.clone();match case{0=>changed["actualOrderId"]=json!("902"),1=>changed["receiptHash"]=json!("a".repeat(64)),2=>changed["quantity"]=json!(2),3=>changed["side"]=json!("YES"),4=>changed["price"]=json!("0.875000"),_=>changed["processedAt"]=json!(iso(NOW+3105))}assert!(store.validate_restored_acknowledgment(&f.bank,&f.log,old,&changed).is_err(),"case {case}");}
            let cash=f.bank.engine().wallet.cash;f.bank.accept("startup-original-ack",1,&restored).unwrap();f.reopen();assert_eq!(f.bank.engine().wallet.cash,cash);assert!(f.bank.engine().fills().is_empty());assert_eq!(f.store.as_ref().unwrap().durable_tag(old),Some(old_tag));assert!(f.store.as_ref().unwrap().durable_tag(new).is_some());assert_eq!(f.bank.engine().live_order_bindings()[&old],"901");
        }
    }
    #[test]fn startup_restore_requires_actual_original_closed_log_and_never_allows_never_sent_archive(){
        let(mut f,t,account,order,fills,terminal,q)=closed_zero_fixture(false);let old=q["id"].as_u64().unwrap();let hint=f.store.as_ref().unwrap().rearm_hints(&f.bank,&f.log,&f.scope,NOW+3220).unwrap().remove(0);let plan=f.store.as_ref().unwrap().prepare_rearm(&f.bank,&f.log,&t,&f.scope,&hint,&account,&order,&fills,"closed-zero-terminal",1,&terminal,NOW+3220).unwrap();f.store.as_mut().unwrap().apply_rearm(plan,&f.bank,&f.log,&t,NOW+3220).unwrap();let restored=actual_startup_ack(&f,old,NOW+3221);f.log.acknowledgments.get_mut(&old).unwrap()["data"]["orderId"]=json!("902");assert!(f.store.as_ref().unwrap().validate_restored_acknowledgment(&f.bank,&f.log,old,&restored).is_err());
        let(mut f,t,facts,retirement,q)=abandonment_fixture(Some(true));let id=q["id"].as_u64().unwrap();let hint=f.store.as_ref().unwrap().abandonment_hints(&f.bank,&f.log,&f.scope,NOW+40).unwrap().remove(0);let plan=f.store.as_ref().unwrap().prepare_abandon(&f.bank,&f.log,&t,&f.scope,&hint,&facts,"never-sent-retirement",1,&retirement,NOW+40).unwrap();f.store.as_mut().unwrap().apply_abandon(plan,&f.bank,&f.log,&t,NOW+40).unwrap();assert!(f.store.as_ref().unwrap().validate_restored_acknowledgment(&f.bank,&f.log,id,&F::base(&q,"actual_order_ack",NOW+41,"901")).is_err());
    }
    #[test]fn action_comparison_derives_review_caps_sources_and_conditional_model_without_mutation(){
        let(mut f,_,_)=entry_fixture();let mut review=f.review.clone();review.reviewed_at_ms=NOW+24;review.maximum_quantity=1;review.maximum_principal="1.000000".into();f.install(review.clone(),NOW+24);
        let store=f.store.as_ref().unwrap();let hint=store.entry_hints(&f.bank,&f.log,&f.scope,&f.trust,NOW+25).unwrap().remove(0);let authority=f.authority();let head=f.bank.durable_head();let report=f.bank.engine().report().unwrap();
        let compared=store.compare_entry_actions_with_authority(&f.bank,&f.scope,&hint,&authority,NOW+25).unwrap().unwrap();assert_eq!(compared.candidates.len(),3);let sources=sha(&serde_json::to_vec(&review.sources).unwrap());
        for candidate in &compared.candidates{assert_eq!(candidate.quantity(),1);assert!(candidate.gross_funding()<=1_000_000);assert!(candidate.limit_price()<=930_500);let p=candidate.provenance();assert_eq!(p["reviewSha256"],review_sha256(&review).unwrap());assert_eq!(p["sourceSha256"],sources);assert_eq!(p["reviewBounds"]["expectedPayout"],940_500);assert_eq!(p["reviewBounds"]["maximumQuantity"],1);assert_eq!(p["reviewBounds"]["maximumPrincipal"],1_000_000);assert_eq!(p["reviewBounds"]["horizonMs"],review.hold_until_ms-(NOW+25));assert!(p["executionProbability"].is_null());assert_eq!(p["modelSha256"].as_str().unwrap().len(),64);}
        assert_eq!(f.bank.durable_head(),head);assert_eq!(f.bank.engine().report().unwrap(),report);
    }
    #[test]fn action_comparison_withholds_revoked_expired_untrusted_or_changed_review(){
        for case in 0..5{let(mut f,_,_)=entry_fixture();let mut review=f.review.clone();review.reviewed_at_ms=NOW+24;if case==0{review.revoke_existing=true;}f.install(review,NOW+24);let store=f.store.as_ref().unwrap();
            let mut hint=EntryHint{exchange_id:"101".into(),market_id:"201".into(),contract_id:"301".into(),side:"NO".into(),review_sha256:review_sha256(&{let mut r=f.review.clone();r.reviewed_at_ms=NOW+24;if case==0{r.revoke_existing=true;}r}).unwrap()};
            let trusted=if case==2{BTreeSet::new()}else{f.trust.clone()};let now=if case==1{NOW+120000}else{NOW+25};if case==3{hint.review_sha256="a".repeat(64);}if case==4{hint.side="YES".into();}
            let result=store.compare_entry_actions(&f.bank,&f.scope,&hint,&trusted,now);if case>=3{assert!(result.is_err());}else{assert!(result.unwrap().is_none());}
        }
    }
    #[test]fn action_comparison_checks_actual_pinned_authority_and_preserves_diagnostic_funding_limits(){
        let(mut f,_,_)=entry_fixture();let mut review=f.review.clone();review.reviewed_at_ms=NOW+24;review.maximum_principal="0.910000".into();review.maximum_quantity=1;f.install(review,NOW+24);let authority=f.authority();let store=f.store.as_ref().unwrap();let hint=store.entry_hints(&f.bank,&f.log,&f.scope,&f.trust,NOW+25).unwrap().remove(0);
        let compared=store.compare_entry_actions_with_authority(&f.bank,&f.scope,&hint,&authority,NOW+25).unwrap().unwrap();assert!(compared.candidates.iter().all(|c|c.gross_funding()<=910_000));let bounded=compared.candidates.iter().find(|c|c.mode()==moloch_live_runtime::alpha_proposal::Mode::BoundedLimitMayRest).unwrap();assert_eq!((bounded.quantity(),bounded.limit_price(),bounded.gross_funding()),(1,900_000,900_000),"fund the actual ask limit even when the unused reviewed ceiling exceeds the budget");
        std::fs::write(f.root.join("review-authority.json"),b"changed pinned review approval").unwrap();assert!(store.compare_entry_actions_with_authority(&f.bank,&f.scope,&hint,&authority,NOW+25).is_err());
    }
    fn reviewed_mode_fixture(modes:Vec<ExecutionMode>)->(F,baal_transport_service::Transport,moloch_live_runtime::authority::SnapshotSet,ReviewAuthority,EntryPlan){
        let(mut f,t,facts)=entry_fixture();let mut r=f.review.clone();r.reviewed_at_ms=NOW+24;r.execution_policy=Some(ExecutionPolicy{version:1,allowed_modes:modes});f.install(r.clone(),NOW+24);f.review=r;let authority=f.authority();let hint=f.store.as_ref().unwrap().entry_hints(&f.bank,&f.log,&f.scope,&f.trust,NOW+25).unwrap().remove(0);let plan=f.store.as_ref().unwrap().prepare_entry(&f.bank,&f.log,&t,&f.scope,&hint,&facts,&authority,NOW+25).unwrap().unwrap();(f,t,facts,authority,plan)
    }
    #[test]fn bounded_marginal_sizing_reaches_genuine_seven_receipt_plan_and_native_reservation(){
        for(budget,quantity,reserved)in [("2.160000",5,2_150_000),("2.100000",4,1_720_000)]{
            let(mut f,t,mut facts)=entry_fixture_quantity(5);let mut review=f.review.clone();review.reviewed_at_ms=NOW+24;review.maximum_principal=budget.into();review.maximum_quantity=5;review.execution_policy=Some(ExecutionPolicy{version:1,allowed_modes:vec![ExecutionMode::BoundedLimitMayRest]});set_deadline_cash(&mut review,"0.450000");f.install(review.clone(),NOW+24);f.review=review;
            // Original YES bids .58/.57 are the target NO asks .42/.43.
            // The independent NO bid has enough genuine liquidation capacity.
            facts.book.request_started_at=iso(NOW+32);facts.book.received_at=iso(NOW+33);let raw=&mut facts.book.data["contexts"][0]["orderbook"]["exchanges"][0];raw["asOf"]=json!({"sequence":100,"at":iso(NOW+33)});raw["bids"]=json!([{"price":0.58,"quantity":2},{"price":0.57,"quantity":3}]);raw["asks"]=json!([{"price":0.60,"quantity":20}]);
            let b=&facts.book;let events=moloch_replay::source::receipt_events(&json!({"source":"official_sig_api_v1","kind":"book","tournamentId":f.scope.tournament_id,"marketId":"201","exchangeId":"101","endpoint":b.endpoint,"status":b.status,"requestStartedAt":b.request_started_at,"receivedAt":b.received_at,"data":b.data})).unwrap();for(i,e)in events.iter().enumerate(){f.bank.accept("marginal-original-book",i as u64+1,e).unwrap();}f.retire_drafts(NOW+34);
            let authority=f.authority();let store=f.store.as_ref().unwrap();let hint=store.entry_hints(&f.bank,&f.log,&f.scope,&f.trust,NOW+35).unwrap().remove(0);let original_facts=serde_json::to_value([&facts.account,&facts.positions,&facts.open_orders,&facts.cash_balance,&facts.market,&facts.contract,&facts.book]).unwrap();let head=f.bank.durable_head();let cash=f.bank.engine().wallet.cash;
            let plan=store.prepare_entry(&f.bank,&f.log,&t,&f.scope,&hint,&facts,&authority,NOW+35).unwrap().unwrap();assert_eq!(plan.event()["executionMode"],"boundedLimitMayRest");assert_eq!(plan.event()["limitPrice"],"0.430000");assert_eq!(plan.event()["acquisitionCeiling"],"0.440000");assert_eq!(plan.event()["reviewMaximumQuantity"],quantity);assert_eq!(plan.event()["reviewMaximumPrincipal"],budget);plan.validate_current(&f.bank,&f.log,&t,&authority,NOW+35).unwrap();assert_eq!(f.bank.durable_head(),head);assert_eq!(serde_json::to_value([&facts.account,&facts.positions,&facts.open_orders,&facts.cash_balance,&facts.market,&facts.contract,&facts.book]).unwrap(),original_facts);
            // The new source book created an ordinary draft at+33. Its real
            // planning write still consumes the unchanged native write gap.
            let before=f.bank.engine().report().unwrap();let mut preview=f.bank.engine().clone();assert_eq!(preview.advance(plan.event()).unwrap_err(),"REVIEWED_ENTRY_WRITE_BUDGET");assert_eq!(f.bank.durable_head(),head);assert_eq!(f.bank.engine().report().unwrap(),before);drop(plan);
            let later=NOW+35+f.bank.engine().policy()["writeGapMs"].as_u64().unwrap();let plan=store.prepare_entry(&f.bank,&f.log,&t,&f.scope,&hint,&facts,&authority,later).unwrap().unwrap();plan.validate_current(&f.bank,&f.log,&t,&authority,later).unwrap();assert_eq!(plan.event()["reviewMaximumQuantity"],quantity);assert_eq!(plan.event()["limitPrice"],"0.430000");assert_eq!(serde_json::to_value([&facts.account,&facts.positions,&facts.open_orders,&facts.cash_balance,&facts.market,&facts.contract,&facts.book]).unwrap(),original_facts);
            f.bank.accept("marginal-reviewed-reservation",1,plan.event()).unwrap();let q=f.bank.engine().order_records().into_iter().find(|q|q["retired"]==false&&q.get("reviewedFundamentalEntry").is_some()).unwrap();assert_eq!(q["quantity"],quantity);assert_eq!(q["price"],"0.430000");assert_eq!(f.bank.engine().wallet.reserved().unwrap(),reserved);assert_eq!(f.bank.engine().wallet.cash,cash);assert!(f.bank.engine().fills().is_empty());assert!(f.log.attempted.is_empty());assert!(f.log.intents.is_empty());
        }
    }
    #[test]fn separately_reviewed_modes_reserve_real_whole_actions_and_only_bounded_token_can_cross(){
        for(mode,price)in [(ExecutionMode::JoinBid,"0.880000"),(ExecutionMode::ImproveOneTick,"0.885000"),(ExecutionMode::BoundedLimitMayRest,"0.900000")]{let(mut f,_,_,authority,plan)=reviewed_mode_fixture(vec![mode]);assert_eq!(plan.event()["limitPrice"],price);if mode==ExecutionMode::JoinBid{assert!(plan.event().get("executionMode").is_none());}else{assert!(plan.event()["modelSha256"].as_str().is_some_and(|s|s.len()==64));assert_eq!(plan.event()["sourceSha256"],sha(&serde_json::to_vec(&f.review.sources).unwrap()));assert_eq!(plan.event()["acquisitionCeiling"],"0.930000");}
            f.bank.accept("reviewed-action",1,plan.event()).unwrap();drop(plan);let q=f.bank.engine().order_records().into_iter().find(|q|q["retired"]==false&&q.get("reviewedFundamentalEntry").is_some()).unwrap();let id=q["id"].as_u64().unwrap();let spec=f.spec(&q);if mode!=ExecutionMode::JoinBid{let mut slice=spec.clone();slice.body.as_mut().unwrap()["quantity"]=json!(1);assert!(f.store.as_mut().unwrap().bind_quantity(&f.bank,&f.log,&f.scope,id,1,&slice,&f.trust,NOW+26).is_err());}
            f.store.as_mut().unwrap().bind(&f.bank,&f.log,&f.scope,id,&spec,&f.trust,NOW+26).unwrap();f.reopen();let active=q["activationAt"].as_u64().unwrap();let permit=f.permit(&q,active);let token=f.store.as_ref().unwrap().physical_decision(&f.bank,&f.log,&f.scope,id,&spec,&permit,&authority,active).unwrap();assert_eq!(token.allows_marketable_reviewed_buy(),mode==ExecutionMode::BoundedLimitMayRest);token.validate_frozen(&q,&f.bank.engine().report().unwrap(),id,active).unwrap();token.authorize(&f.scope,&spec,&permit,active).unwrap();assert!(f.bank.engine().fills().is_empty());assert_eq!(f.bank.engine().wallet.cash,1_000_000_000);
            let mut changed=spec.clone();changed.body.as_mut().unwrap()["price"]=json!(0.93);assert!(token.authorize(&f.scope,&changed,&permit,active).is_err());
        }
    }
    #[test]fn reviewed_execution_order_is_explicit_and_unmarked_ordinary_draft_has_no_crossing_authority(){
        let(_,_,_,_,plan)=reviewed_mode_fixture(vec![ExecutionMode::ImproveOneTick,ExecutionMode::BoundedLimitMayRest,ExecutionMode::JoinBid]);assert_eq!(plan.event()["executionMode"],"improveOneTick");
        let mut f=F::new();let q=f.entry();let mut r=f.review.clone();r.reviewed_at_ms=NOW+1;r.execution_policy=Some(ExecutionPolicy{version:1,allowed_modes:vec![ExecutionMode::BoundedLimitMayRest]});f.install(r,NOW+1);assert!(matches!(f.assess(&q,NOW+1),Decision::Hold{reason}if reason=="reviewed_execution_reservation_required_or_changed"));
    }
    #[test]fn reviewed_action_hashes_and_later_review_change_cannot_be_substituted_at_physical_boundary(){
        for key in ["modelSha256","sourceSha256"]{let(mut f,_,_,_,plan)=reviewed_mode_fixture(vec![ExecutionMode::BoundedLimitMayRest]);let mut altered=plan.event().clone();altered[key]=json!("a".repeat(64));f.bank.accept("altered-provenance",1,&altered).unwrap();let q=f.bank.engine().order_records().into_iter().find(|q|q["retired"]==false&&q.get("reviewedFundamentalEntry").is_some()).unwrap();assert!(matches!(f.assess(&q,NOW+26),Decision::Hold{..}));let spec=f.spec(&q);assert!(f.store.as_mut().unwrap().bind(&f.bank,&f.log,&f.scope,q["id"].as_u64().unwrap(),&spec,&f.trust,NOW+26).is_err());}
        let(mut f,_,_,authority,plan)=reviewed_mode_fixture(vec![ExecutionMode::BoundedLimitMayRest]);f.bank.accept("reviewed-action",1,plan.event()).unwrap();drop(plan);let q=f.bank.engine().order_records().into_iter().find(|q|q["retired"]==false&&q.get("reviewedFundamentalEntry").is_some()).unwrap();let id=q["id"].as_u64().unwrap();let spec=f.spec(&q);f.store.as_mut().unwrap().bind(&f.bank,&f.log,&f.scope,id,&spec,&f.trust,NOW+26).unwrap();let active=q["activationAt"].as_u64().unwrap();let permit=f.permit(&q,active);let token=f.store.as_ref().unwrap().physical_decision(&f.bank,&f.log,&f.scope,id,&spec,&permit,&authority,active).unwrap();let mut changed=f.review.clone();changed.reviewed_at_ms=active+1;changed.execution_policy=Some(ExecutionPolicy{version:1,allowed_modes:vec![ExecutionMode::JoinBid]});f.install(changed,active+1);assert!(token.authorize(&f.scope,&spec,&permit,active+1).is_err());assert!(f.store.as_ref().unwrap().physical_decision(&f.bank,&f.log,&f.scope,id,&spec,&permit,&authority,active+1).is_err());
    }
    #[test]fn production_authority_crossing_requires_exact_real_reviewed_bounded_token_and_fresh_original_facts(){
        use baal_transport_service::Authority;
        use moloch_live_runtime::authority::{Activation,ProductionAuthority};
        fn authority(f:&F,managed:bool)->ProductionAuthority{
            let binary=f.root.join("authority-fixture-binary");let control=f.root.join("authority-fixture-control");std::fs::write(&binary,b"synthetic checked executable").unwrap();std::fs::write(&control,b"immutable synthetic control").unwrap();
            let a=ProductionAuthority::new(Activation{run_id:"fundamental-fixture".into(),entry_until_ms:NOW+3600000,maximum_committed:"1000.000000".into(),maximum_market_cost:"100.000000".into(),maximum_quantity:2,tournament_id:f.scope.tournament_id.clone(),tournament_slug:f.scope.tournament_slug.clone(),binary_sha256:sha(b"synthetic checked executable")},f.scope.clone(),vec![f.root.join("STOP")],control,binary,f.owner.clone()).unwrap();
            a.bind_stream(Arc::new(std::sync::atomic::AtomicBool::new(false)),NOW+3600000);if managed{a.require_fundamental_exchanges(&["101".to_string()].into()).unwrap();}a
        }
        let(mut f,_,facts,review_authority,plan)=reviewed_mode_fixture(vec![ExecutionMode::BoundedLimitMayRest]);f.bank.accept("reviewed-bounded-action",1,plan.event()).unwrap();let q=f.bank.engine().order_records().into_iter().find(|q|q["retired"]==false&&q.get("reviewedFundamentalEntry").is_some()).unwrap();let id=q["id"].as_u64().unwrap();let spec=f.spec(&q);let active=q["activationAt"].as_u64().unwrap();let permit=f.permit(&q,active);f.store.as_mut().unwrap().bind(&f.bank,&f.log,&f.scope,id,&spec,&f.trust,NOW+26).unwrap();
        let token=Arc::new(f.store.as_ref().unwrap().physical_decision(&f.bank,&f.log,&f.scope,id,&spec,&permit,&review_authority,active).unwrap());assert!(token.allows_marketable_reviewed_buy());assert_eq!(q["price"],"0.900000","actual NO limit equals the original executable NO ask");let a=authority(&f,true);
        a.prepare_with_policies(&f.bank,id,spec.clone(),permit.clone(),facts.clone(),None,Some(token.clone())).unwrap();a.authorize(&f.scope,&spec,&permit,true,active).unwrap();
        let mut changed=spec.clone();changed.body.as_mut().unwrap()["price"]=json!(0.93);assert!(a.authorize(&f.scope,&changed,&permit,true,active).is_err());
        assert!(a.authorize(&f.scope,&spec,&permit,true,token.expires_at_ms()).is_err());
        assert!(a.prepare_with_policies(&f.bank,id,spec.clone(),permit.clone(),facts.clone(),None,None).unwrap_err().to_string().contains("LIVE_FUNDAMENTAL_DECISION_REQUIRED"));
        let ordinary=authority(&f,false);ordinary.prepare(&f.bank,id,spec.clone(),permit.clone(),facts.clone()).unwrap();assert!(ordinary.authorize(&f.scope,&spec,&permit,true,active).unwrap_err().code.contains("LIVE_ENTRY_MUST_BE_PASSIVE"),"native metadata alone grants no crossing exemption");
        let mut stale=facts.clone();stale.book.request_started_at=iso(NOW-20000);a.prepare_with_policies(&f.bank,id,spec.clone(),permit.clone(),stale,None,Some(token.clone())).unwrap();assert!(a.authorize(&f.scope,&spec,&permit,true,active).is_err());
        let(mut forged,_,forged_facts,forged_review,forged_plan)=reviewed_mode_fixture(vec![ExecutionMode::BoundedLimitMayRest]);let mut e=forged_plan.event().clone();e["modelSha256"]=json!("a".repeat(64));forged.bank.accept("forged-model-binding",1,&e).unwrap();let q=forged.bank.engine().order_records().into_iter().find(|q|q["retired"]==false&&q.get("reviewedFundamentalEntry").is_some()).unwrap();let id=q["id"].as_u64().unwrap();let spec=forged.spec(&q);let active=q["activationAt"].as_u64().unwrap();let permit=forged.permit(&q,active);assert!(forged.store.as_mut().unwrap().bind(&forged.bank,&forged.log,&forged.scope,id,&spec,&forged.trust,NOW+26).is_err());assert!(forged.store.as_ref().unwrap().physical_decision(&forged.bank,&forged.log,&forged.scope,id,&spec,&permit,&forged_review,active).is_err());let denied=authority(&forged,true);assert!(denied.prepare_with_policies(&forged.bank,id,spec,permit,forged_facts,None,None).is_err());
        assert!(f.bank.engine().fills().is_empty());assert_eq!(f.bank.engine().wallet.cash,1_000_000_000);
    }
    fn inherited_fixture()->(F,baal_transport_service::Transport,Value){
        use baal_transport_service::{Config,DisabledCredentials,ReadOnlyAuthority,SystemClock,Transport};
        let mut f=F::setup(false,"11111111-1111-1111-1111-111111111111");f.retire_drafts(NOW-1);
        let adoption=json!({"type":"actual_position_adoption","source":"official_sig_api_v1","authority":"human_moloch_inherited_management_20261015","complete":true,"receiptHash":"d".repeat(64),"requestStartedAt":iso(NOW-1),"receivedAt":iso(NOW),"processedAt":iso(NOW),"tournamentId":f.scope.tournament_id,"openingCost":"1.900000","effectiveMaximumMarkets":4,"positions":[{"exchangeId":"101","marketId":"201","quantity":-3,"lotPrincipal":"1.900000","reportedCostBasis":"1.900000","reportedMinusLotPrincipal":"0.000000","basisClassification":"exact","lots":[{"lotId":"501","quantity":1,"side":"NO","entryPrice":"0.900000","openedAt":iso(NOW-86400000)},{"lotId":"502","quantity":2,"side":"NO","entryPrice":"0.500000","openedAt":iso(NOW-43200000)}]}]});
        f.bank.accept("fixture-adoption",1,&adoption).unwrap();f.retire_drafts(NOW+3001);
        let config=Config{queue_dir:f.root.join("original-account-queue"),account_scope:"synthetic-existing-account".into(),scope:f.scope.clone(),maximum_pages:2,writes_enabled:false};
        let transport=Transport::new_for_mock(config,Box::new(DisabledCredentials),Box::new(SystemClock),Box::new(ReadOnlyAuthority),"http://127.0.0.1:9/api/v1".into()).unwrap();(f,transport,adoption)
    }
    fn install_inherited(f:&mut F,transport:&baal_transport_service::Transport,adoption:&Value,p:&str){
        let snapshot=ExistingSnapshot::capture(&f.bank,&f.log,transport,"fixture-adoption",1,adoption,"101").unwrap();let mut r=f.review.clone();r.ordinary_probability=p.into();let cash=baal_transport_service::decimal_units(p).unwrap()*990000/1000000;set_deadline_cash(&mut r,&moloch_replay::money(cash));r.existing_origin=Some(snapshot.approval().clone());let reviewed=approve(r.clone(),&mut f.trust,NOW+3001);
        f.store.as_mut().unwrap().install_existing(reviewed,snapshot,&f.bank,&f.log,transport,NOW+3001).unwrap();f.review=r;
    }
    fn inherited_exit(f:&mut F,transport:&baal_transport_service::Transport,at:u64,bid:&str,ask:&str)->(Value,u64){
        f.book(at,bid,ask);let prepared=at+3001;f.retire_drafts(prepared);
        let plan=f.store.as_ref().unwrap().prepare_existing_exit(&f.bank,&f.log,transport,&f.scope,"101",&f.trust,prepared).unwrap().unwrap();
        f.bank.accept("reviewed-existing-exits",f.bank.last_source_sequence("reviewed-existing-exits")+1,plan.event()).unwrap();
        let q=f.bank.engine().order_records().into_iter().find(|q|q["retired"]==false&&q.get("reviewedInventoryExit").is_some()).unwrap();let active=q["activationAt"].as_u64().unwrap();(q,active)
    }
    #[test]fn genuine_native_quote_binds_whole_quantity_and_cannot_be_resized_on_wire(){let mut f=F::new();let q=f.entry();let mut spec=f.spec(&q);spec.body.as_mut().unwrap()["quantity"]=json!(1);let n=q["id"].as_u64().unwrap();assert!(f.store.as_mut().unwrap().bind(&f.bank,&f.log,&f.scope,n,&spec,&f.trust,NOW).is_err());assert!(matches!(f.assess(&q,NOW),Decision::Entry{..}));}
    #[test]fn policy_principal_cap_rejects_whole_quote_instead_of_silently_downsizing(){let mut f=F::new();let mut r=f.review.clone();r.reviewed_at_ms=NOW+1;r.maximum_principal="0.500000".into();f.install(r,NOW+1);assert_eq!(f.assess(&f.entry(),NOW+1),Decision::Hold{reason:"whole_native_quote_exceeds_reviewed_value_or_risk_cap".into()});}
    #[test]fn sixty_one_second_native_loss_exit_is_vetoed_without_changing_financial_engine(){let mut f=F::new();f.fill_entry(false);let(q,at)=f.age_exit(NOW+61003,"0.300000","0.320000");assert_eq!(q["inventoryExitReason"],"maximumAge");assert_eq!(f.assess(&q,at),Decision::Hold{reason:"terminal_value_hold_not_native_age_exit".into()});assert_eq!(f.bank.engine().policy()["maximumInventoryAgeMs"],60000);assert_eq!(f.bank.engine().wallet.quantity("101",moloch_replay::Side::No),2);assert_eq!(f.bank.engine().fills().len(),1);f.reopen();assert!(matches!(f.assess(&q,at),Decision::Hold{..}));}
    #[test]fn stale_review_cannot_buy_but_deadline_still_manages_existing_cohort(){let mut f=F::new();f.fill_entry(false);let(q,at)=f.age_exit(NOW+130000,"0.300000","0.320000");assert_eq!(f.assess(&q,at),Decision::Hold{reason:"stale_review_hold_until_deadline".into()});let(q,at)=f.age_exit(NOW+600001,"0.300000","0.320000");assert!(matches!(f.assess(&q,at),Decision::Exit{reason,..}if reason=="reviewed_holding_deadline"));}
    #[test]fn explicit_reviewed_revocation_allows_only_covered_actual_cohort_exit(){let mut f=F::new();f.fill_entry(false);let(q,at)=f.age_exit(NOW+61003,"0.300000","0.320000");let mut r=horizon_input(f.bank.manifest_hash().as_str().unwrap(),at+1);r.revoke_existing=true;f.install(r,at+1);assert!(matches!(f.assess(&q,at+1),Decision::Exit{reason,owned_quantity:2}if reason=="reviewed_source_revocation"));}
    #[test]fn refreshed_value_exit_compares_payout_before_entry_buffer(){let mut f=F::new();f.fill_entry(false);let(q,at)=f.age_exit(NOW+61003,"0.040000","0.060000");assert!(matches!(f.assess(&q,at),Decision::Hold{..}),"NO bid.94 remains below conditional expected payout.9405, even though above entry ceiling.9305");let(q,at)=f.age_exit(at+4000,"0.035000","0.055000");assert!(matches!(f.assess(&q,at),Decision::Exit{reason,..}if reason=="refreshed_value_exit"));}
    #[test]fn better_observed_bid_cannot_authorize_an_old_below_value_sell_limit(){
        let mut f=F::new();f.fill_entry(false);
        let(q,at)=f.age_exit(NOW+61003,"0.080000","0.100000");
        assert_eq!(q["price"],"0.900000");
        // The next genuine book is inside the native write gap, leaving the
        // original limit intact despite current executable NO bids at.945.
        f.book(at+1,"0.035000","0.055000");
        let current=f.bank.engine().order_records().into_iter().find(|x|x["id"]==q["id"]).unwrap();
        assert_eq!(current["price"],q["price"]);assert!(current["cancelEffectiveAt"].is_null());
        assert!(matches!(f.assess(&q,at+1),Decision::Hold{..}));
        assert!(f.store.as_ref().unwrap().exit_refresh_needed(&f.bank,&f.log,&f.scope,"101",&f.trust,at+1).unwrap(),"value now justifies refreshing the actual native limit, not selling under it");
        let(repriced,now)=f.age_exit(at+4000,"0.035000","0.055000");
        assert_eq!(repriced["price"],"0.945000");
        assert!(matches!(f.assess(&repriced,now),Decision::Exit{reason,..}if reason=="refreshed_value_exit"));
    }
    #[test]fn partial_late_fill_requires_original_entry_terminal_before_an_exit(){let mut f=F::new();f.fill_entry(true);let(q,at)=f.age_exit(NOW+61003,"0.300000","0.320000");assert_eq!(f.assess(&q,at),Decision::Hold{reason:"prior_owned_order_reconciliation_required".into()});assert_eq!(f.bank.engine().wallet.quantity("101",moloch_replay::Side::No),1);}
    #[test]fn restart_unknown_preserves_binding_and_never_grants_a_retry(){let mut f=F::new();let(q,spec)=f.bind_entry();f.log_intent(&q,&spec);assert!(f.log.require_resolved_attempts().is_err());f.reopen();assert!(f.log.require_resolved_attempts().is_err());assert!(matches!(f.assess(&q,NOW+1),Decision::Hold{..}));assert!(f.store.as_mut().unwrap().bind(&f.bank,&f.log,&f.scope,q["id"].as_u64().unwrap(),&spec,&f.trust,NOW+1).is_err());}
    #[test]fn physical_gate_rechecks_source_request_book_and_original_activation(){let mut f=F::new();let(q,spec)=f.bind_entry();let id=q["id"].as_u64().unwrap();f.store.as_ref().unwrap().check_bound(&f.bank,&f.log,&f.scope,id,&spec,&f.trust,NOW).unwrap();let mut altered=spec.clone();altered.body_json=Some("{}".into());assert!(f.store.as_ref().unwrap().check_bound(&f.bank,&f.log,&f.scope,id,&altered,&f.trust,NOW).is_err());assert!(f.store.as_ref().unwrap().check_bound(&f.bank,&f.log,&f.scope,id,&spec,&BTreeSet::new(),NOW).is_err());assert!(f.store.as_ref().unwrap().check_bound(&f.bank,&f.log,&f.scope,id,&spec,&f.trust,NOW+20000).is_err());}
    #[test]fn clean_prefix_truncation_is_detected_by_original_dispatch_tag(){let mut f=F::new();let(q,spec)=f.bind_entry();f.log_intent(&q,&spec);f.store.take();let p=f.root.join("fundamental.ndjson");let text=std::fs::read_to_string(&p).unwrap();let first=format!("{}\n",text.lines().next().unwrap());std::fs::write(&p,first).unwrap();assert!(Cohorts::open(&f.root,&f.bank,&f.log,f.owner.clone()).is_err());}
    #[test]fn original_dispatch_mutation_and_torn_sidecar_are_not_safe_restarts(){let mut f=F::new();let(q,spec)=f.bind_entry();f.log_intent(&q,&spec);let id=q["id"].as_u64().unwrap();f.log.intents.get_mut(&id).unwrap()["spec"]["body"]["quantity"]=json!(1);assert!(f.store.as_ref().unwrap().assess(&f.bank,&f.log,&f.scope,id,&f.trust,NOW).is_err());f.store.take();let p=f.root.join("fundamental.ndjson");use std::io::Write;let mut file=std::fs::OpenOptions::new().append(true).open(&p).unwrap();file.write_all(b"{").unwrap();drop(file);assert!(Cohorts::open(&f.root,&f.bank,&f.log,f.owner.clone()).is_err());}
    #[test]fn missing_sidecar_or_second_writer_is_not_automatic_initialization(){let mut f=F::new();assert!(Cohorts::open(&f.root,&f.bank,&f.log,f.owner.clone()).is_err());f.store.take();std::fs::remove_file(f.root.join("fundamental.ndjson")).unwrap();assert!(Cohorts::open(&f.root,&f.bank,&f.log,f.owner.clone()).is_err());}
    #[test]fn cohort_horizon_cannot_be_renewed_by_an_ordinary_source_refresh(){let mut f=F::new();f.fill_entry(false);let mut r=horizon_input(f.bank.manifest_hash().as_str().unwrap(),NOW+62000);r.hold_until_ms+=1;set_deadline_cash(&mut r,"0.940500");let reviewed=approve(r,&mut f.trust,NOW+62000);assert!(f.store.as_mut().unwrap().install(reviewed,&f.bank,&f.log,NOW+62000).is_err());}
    #[test]fn changed_review_requests_cancellation_of_known_partial_entry_without_inventing_unknown_id(){let mut f=F::new();let id=f.fill_entry(true);let mut r=horizon_input(f.bank.manifest_hash().as_str().unwrap(),NOW+62000);r.revoke_existing=true;f.install(r,NOW+62000);assert_eq!(f.store.as_ref().unwrap().entry_cancellations(&f.bank,&f.log,&f.trust,NOW+62000).unwrap(),vec![id]);let mut unknown=F::new();let(q,spec)=unknown.bind_entry();unknown.log_intent(&q,&spec);assert!(unknown.store.as_ref().unwrap().entry_cancellations(&unknown.bank,&unknown.log,&unknown.trust,NOW+130000).unwrap().is_empty());}
    #[test]fn unrelated_actual_holding_cannot_become_a_tagged_forecast_position(){let mut f=F::new();let q=f.entry();f.event(F::base(&q,"actual_order_ack",NOW+1,"201"));let mut fill=F::base(&q,"actual_fill",NOW+3,"201");fill["fillId"]=json!("401");fill["quantity"]=json!(2);fill["executedAt"]=json!(iso(NOW+2));fill["fee"]=json!("0.000000");f.event(fill);let qid=q["id"].as_u64().unwrap();assert!(f.store.as_ref().unwrap().assess(&f.bank,&f.log,&f.scope,qid,&f.trust,NOW+3).is_err());}
    #[test]fn cap_only_revision_does_not_permanently_prioritize_historical_age_exit(){
        let mut f=F::new();let entry=f.fill_entry(false);let(q,at)=f.age_exit(NOW+61003,"0.300000","0.320000");
        let original=f.store.as_ref().unwrap().durable_tag(entry);assert!(original.is_some());let old_hash=review_sha256(&f.review).unwrap();
        let mut r=f.review.clone();r.reviewed_at_ms=at+1;r.maximum_principal="20.000000".into();f.install(r.clone(),at+1);f.review=r;
        assert_ne!(review_sha256(&f.review).unwrap(),old_hash);
        assert!(!f.store.as_ref().unwrap().exit_refresh_needed(&f.bank,&f.log,&f.scope,"101",&f.trust,at+1).unwrap());
        assert!(matches!(f.assess(&q,at+1),Decision::Hold{..}));f.reopen();
        assert_eq!(f.store.as_ref().unwrap().durable_tag(entry),original);
        assert!(!f.store.as_ref().unwrap().exit_refresh_needed(&f.bank,&f.log,&f.scope,"101",&f.trust,at+1).unwrap());
        let mut r=f.review.clone();r.reviewed_at_ms=at+2;r.revoke_existing=true;f.install(r,at+2);
        assert!(f.store.as_ref().unwrap().exit_refresh_needed(&f.bank,&f.log,&f.scope,"101",&f.trust,at+2).unwrap());
    }
    #[test]fn changed_review_value_requires_actual_current_executable_value(){
        let mut f=F::new();f.fill_entry(false);let(_,at)=f.age_exit(NOW+61003,"0.300000","0.320000");
        let mut r=f.review.clone();r.reviewed_at_ms=at+1;r.ordinary_probability="0.700000".into();set_deadline_cash(&mut r,"0.693000");f.install(r,at+1);
        assert!(!f.store.as_ref().unwrap().exit_refresh_needed(&f.bank,&f.log,&f.scope,"101",&f.trust,at+1).unwrap(),"NO bid .68 remains below current value plus exit margin");
        f.book(at+4000,"0.270000","0.290000");
        assert!(f.store.as_ref().unwrap().exit_refresh_needed(&f.bank,&f.log,&f.scope,"101",&f.trust,at+4000).unwrap());
    }
    #[test]fn expired_exit_read_hint_is_not_permanently_blocked_by_inactive_quote_gate(){let mut f=F::new();f.fill_entry(false);let(q,at)=f.age_exit(NOW+61003,"0.300000","0.320000");assert!(!f.store.as_ref().unwrap().exit_refresh_needed(&f.bank,&f.log,&f.scope,"101",&f.trust,at).unwrap());assert!(matches!(f.assess(&q,NOW+600001),Decision::Hold{..}),"old native quote is inactive, even at deadline");assert!(f.store.as_ref().unwrap().exit_refresh_needed(&f.bank,&f.log,&f.scope,"101",&f.trust,NOW+600001).unwrap(),"deadline still enables bounded source refresh before final quote assessment");}
    #[test]fn opaque_token_mints_before_attempt_and_survives_only_its_original_tagged_append(){let mut f=F::new();let(q,spec)=f.bind_entry();let id=q["id"].as_u64().unwrap();let p=f.permit(&q,NOW);let a=f.authority();let store=f.store.as_ref().unwrap();let token=store.physical_decision(&f.bank,&f.log,&f.scope,id,&spec,&p,&a,NOW).unwrap();assert_eq!(token.native_head(),&f.bank.durable_head());assert_eq!(token.review_sha256(),review_sha256(&f.review).unwrap());let report=f.bank.engine().report().unwrap();token.validate_frozen(&q,&report,id,NOW).unwrap();token.authorize(&f.scope,&spec,&p,NOW).unwrap();f.log_intent(&q,&spec);f.store.as_ref().unwrap().check_bound(&f.bank,&f.log,&f.scope,id,&spec,&f.trust,NOW).unwrap();token.authorize(&f.scope,&spec,&p,NOW).unwrap();assert!(f.log.require_resolved_attempts().is_err(),"the policy token never resolves or retries an attempted write");}
    #[test]fn opaque_token_rejects_changed_frozen_economics_scope_permit_and_raw_body(){let mut f=F::new();let(q,spec)=f.bind_entry();let id=q["id"].as_u64().unwrap();let p=f.permit(&q,NOW);let a=f.authority();let token=f.store.as_ref().unwrap().physical_decision(&f.bank,&f.log,&f.scope,id,&spec,&p,&a,NOW).unwrap();let mut report=f.bank.engine().report().unwrap();report["cash"]=json!("999.000000");assert!(token.validate_frozen(&q,&report,id,NOW).is_err());let mut q2=q.clone();q2["quantity"]=json!(1);assert!(token.validate_frozen(&q2,&f.bank.engine().report().unwrap(),id,NOW).is_err());let mut p2=p.clone();p2.maximum_quantity+=1;assert!(token.authorize(&f.scope,&spec,&p2,NOW).is_err());let mut s2=spec.clone();s2.body_json=Some("{}".into());assert!(token.authorize(&f.scope,&s2,&p,NOW).is_err());let mut scope=f.scope.clone();scope.tournament_slug="other".into();assert!(token.authorize(&scope,&spec,&p,NOW).is_err());}
    #[test]fn opaque_token_expires_at_original_official_book_clock_not_permit_only(){let mut f=F::new();let(q,spec)=f.bind_entry();let id=q["id"].as_u64().unwrap();let p=f.permit(&q,NOW);let a=f.authority();let token=f.store.as_ref().unwrap().physical_decision(&f.bank,&f.log,&f.scope,id,&spec,&p,&a,NOW).unwrap();assert!(token.expires_at_ms()<p.expires_at_ms);token.authorize(&f.scope,&spec,&p,token.expires_at_ms()-1).unwrap();assert!(token.authorize(&f.scope,&spec,&p,token.expires_at_ms()).is_err());assert!(token.authorize(&f.scope,&spec,&p,NOW-1).is_err());}
    #[test]fn opaque_token_rejects_same_inode_sidecar_prefix_edit_and_trust_file_mutation(){let mut f=F::new();let(q,spec)=f.bind_entry();let id=q["id"].as_u64().unwrap();let p=f.permit(&q,NOW);let a=f.authority();let token=f.store.as_ref().unwrap().physical_decision(&f.bank,&f.log,&f.scope,id,&spec,&p,&a,NOW).unwrap();let path=f.root.join("fundamental.ndjson");let mut bytes=std::fs::read(&path).unwrap();let pos=bytes.iter().position(|b|*b==b't').unwrap();bytes[pos]=b'z';std::fs::write(&path,&bytes).unwrap();assert!(token.authorize(&f.scope,&spec,&p,NOW).is_err());let mut f=F::new();let(q,spec)=f.bind_entry();let id=q["id"].as_u64().unwrap();let p=f.permit(&q,NOW);let a=f.authority();let token=f.store.as_ref().unwrap().physical_decision(&f.bank,&f.log,&f.scope,id,&spec,&p,&a,NOW).unwrap();std::fs::write(f.root.join("review-authority.json"),b"{}").unwrap();assert!(token.authorize(&f.scope,&spec,&p,NOW).is_err());}
    #[test]fn opaque_token_rejects_replaced_original_dispatch_inode_and_new_review_tail(){let mut f=F::new();let(q,spec)=f.bind_entry();let id=q["id"].as_u64().unwrap();let p=f.permit(&q,NOW);let a=f.authority();let token=f.store.as_ref().unwrap().physical_decision(&f.bank,&f.log,&f.scope,id,&spec,&p,&a,NOW).unwrap();let path=f.root.join("dispatch.ndjson");std::fs::rename(&path,f.root.join("dispatch-original.ndjson")).unwrap();std::fs::write(&path,b"").unwrap();assert!(token.authorize(&f.scope,&spec,&p,NOW).is_err());let mut f=F::new();let(q,spec)=f.bind_entry();let id=q["id"].as_u64().unwrap();let p=f.permit(&q,NOW);let a=f.authority();let token=f.store.as_ref().unwrap().physical_decision(&f.bank,&f.log,&f.scope,id,&spec,&p,&a,NOW).unwrap();let r=horizon_input(f.bank.manifest_hash().as_str().unwrap(),NOW+1);f.install(r,NOW+1);assert!(token.authorize(&f.scope,&spec,&p,NOW+1).is_err());}
    #[test]fn only_actual_deadline_exit_can_mint_without_fresh_forecast(){let mut f=F::new();f.fill_entry(false);let(q,at)=f.age_exit(NOW+600001,"0.300000","0.320000");let at=at.max(q["activationAt"].as_u64().unwrap());let spec=f.spec(&q);let id=q["id"].as_u64().unwrap();f.store.as_mut().unwrap().bind(&f.bank,&f.log,&f.scope,id,&spec,&f.trust,at).unwrap();let p=f.permit(&q,at);let a=f.authority();let token=f.store.as_ref().unwrap().physical_decision(&f.bank,&f.log,&f.scope,id,&spec,&p,&a,at).unwrap();token.validate_frozen(&q,&f.bank.engine().report().unwrap(),id,at).unwrap();token.authorize(&f.scope,&spec,&p,at).unwrap();assert!(at>f.review.expires_at_ms);}
    #[test]fn consumed_initialization_marker_blocks_recreation_after_sidecar_loss(){let mut f=F::new();f.store.take();std::fs::remove_file(f.root.join("fundamental.ndjson")).unwrap();assert!(f.root.join("fundamental.initialized.json").exists());assert!(Cohorts::initialize(&f.root,&f.bank,&f.log,f.owner.clone()).is_err());assert!(Cohorts::open(&f.root,&f.bank,&f.log,f.owner.clone()).is_err());}
    fn settings(f:&F)->Settings{let authority=f.authority();let review=f.root.join("review-input.json");let bytes=serde_json::to_vec(&f.review).unwrap();std::fs::write(&review,&bytes).unwrap();let originals=originals_for(&f.review);let sources=originals.iter().enumerate().map(|(i,b)|{let path=f.root.join(format!("source-{i}.bin"));std::fs::write(&path,b).unwrap();SourceFile{path,sha256:sha(b)}}).collect();let authority_path=f.root.join("review-authority.json");Settings{version:1,authority_sha256:sha(&std::fs::read(&authority_path).unwrap()),authority_path,reviews:vec![ReviewFile{path:review,file_sha256:sha(&bytes),source_files:sources}],allow_initialization:false}}
    #[test]fn settings_loads_exact_immutable_files_and_recovers_stale_cohort_without_extending_it(){let mut f=F::new();f.fill_entry(false);let config=settings(&f);let loaded=config.load().unwrap();f.store.take();let(store,authority,stale)=loaded.open(&f.root,&f.bank,&f.log,f.owner.clone(),NOW+130000).unwrap();assert_eq!(stale.len(),1);assert!(store.manages("101"));assert!(authority.trusted().contains(&review_sha256(&f.review).unwrap()));f.store=Some(store);}
    #[test]fn settings_hashes_reject_mutated_source_and_initialization_flag_cannot_reset(){let mut f=F::new();let mut config=settings(&f);std::fs::write(&config.reviews[0].source_files[0].path,b"replacement").unwrap();assert!(config.load().is_err());config=settings(&f);config.allow_initialization=true;f.store.take();std::fs::remove_file(f.root.join("fundamental.ndjson")).unwrap();assert!(config.load().unwrap().open(&f.root,&f.bank,&f.log,f.owner.clone(),NOW).is_err());}
    #[test]fn explicit_slice_keeps_native_quote_and_reservation_until_actual_smaller_ack(){
        let mut f=F::new();let(q,spec)=f.slice_spec();let id=q["id"].as_u64().unwrap();assert_eq!(q["quantity"],2);
        let before=f.bank.engine().wallet.reserved().unwrap();let price=baal_transport_service::decimal_units(q["price"].as_str().unwrap()).unwrap();
        assert!(matches!(f.assess(&q,NOW+1),Decision::Hold{..}),"ordinary whole-native API retains its size guard");
        assert_eq!(f.store.as_ref().unwrap().planned_entry_quantity(&f.bank,&f.log,&f.scope,id,&f.trust,NOW+1).unwrap(),Some(1));
        f.store.as_mut().unwrap().bind_quantity(&f.bank,&f.log,&f.scope,id,1,&spec,&f.trust,NOW+1).unwrap();
        assert_eq!(f.bank.engine().order_records().into_iter().find(|x|x["id"]==id).unwrap(),q);assert_eq!(f.bank.engine().wallet.reserved().unwrap(),before);
        let mut permit=f.permit(&q,NOW+1);permit.maximum_quantity=1;permit.maximum_cost=moloch_replay::money(price);let authority=f.authority();
        let token=f.store.as_ref().unwrap().physical_decision(&f.bank,&f.log,&f.scope,id,&spec,&permit,&authority,NOW+1).unwrap();assert_eq!(token.planned_quantity(),1);
        token.validate_frozen(&q,&f.bank.engine().report().unwrap(),id,NOW+1).unwrap();token.authorize(&f.scope,&spec,&permit,NOW+1).unwrap();
        f.log_intent(&q,&spec);let mut ack=F::base(&q,"actual_order_ack",NOW+2,"201");ack["quantity"]=json!(1);
        f.store.as_ref().unwrap().validate_acknowledgment(id,&ack).unwrap();f.log.append(json!({"kind":"ack","qid":id,"receipt":{"orderId":"201","quantity":1}})).unwrap();f.event(ack);
        assert_eq!(f.bank.engine().wallet.reserved().unwrap(),before-price);assert_eq!(f.bank.engine().order_records().into_iter().find(|x|x["id"]==id).unwrap()["quantity"],1);
        let mut fill=F::base(&q,"actual_fill",NOW+4,"201");fill["quantity"]=json!(1);fill["fillId"]=json!("401");fill["executedAt"]=json!(iso(NOW+3));fill["fee"]=json!("0.000000");f.event(fill);
        let mut terminal=F::base(&q,"actual_order_terminal",NOW+5,"201");terminal["status"]=json!("filled");terminal["fillsComplete"]=json!(true);terminal["filledQuantity"]=json!(1);f.event(terminal);f.log.append(json!({"kind":"closed","qid":id})).unwrap();
        // A real process restart drops its opaque capabilities as well as the
        // store; the held descriptor deliberately keeps its OS writer lock.
        drop(token);f.reopen();assert_eq!(f.bank.engine().wallet.quantity("101",moloch_replay::Side::No),1);assert_eq!(f.bank.engine().fills().len(),1);
        assert!(f.store.as_ref().unwrap().planned_entry_quantity(&f.bank,&f.log,&f.scope,id,&f.trust,NOW+5).unwrap().is_none());
    }
    #[test]fn review_sizing_floors_principal_and_never_changes_native_price(){
        let mut f=F::new();let q=f.entry();let id=q["id"].as_u64().unwrap();let price=baal_transport_service::decimal_units(q["price"].as_str().unwrap()).unwrap();
        let mut r=f.review.clone();r.reviewed_at_ms=NOW+1;r.maximum_quantity=5;r.maximum_principal=moloch_replay::money(2*price-1);f.install(r,NOW+1);
        assert_eq!(f.store.as_ref().unwrap().planned_entry_quantity(&f.bank,&f.log,&f.scope,id,&f.trust,NOW+1).unwrap(),Some(1));
        assert_eq!(f.bank.engine().order_records().into_iter().find(|x|x["id"]==id).unwrap(),q);
        let mut r=f.review.clone();r.reviewed_at_ms=NOW+2;r.maximum_principal=moloch_replay::money(price-1);f.install(r,NOW+2);
        assert_eq!(f.store.as_ref().unwrap().planned_entry_quantity(&f.bank,&f.log,&f.scope,id,&f.trust,NOW+2).unwrap(),None);
    }
    #[test]fn exact_unattempted_binding_continues_after_restart_without_another_sidecar_append(){
        let mut f=F::new();let(q,spec)=f.slice_spec();let id=q["id"].as_u64().unwrap();
        f.store.as_mut().unwrap().bind_quantity(&f.bank,&f.log,&f.scope,id,1,&spec,&f.trust,NOW+1).unwrap();let bytes=std::fs::read(f.root.join("fundamental.ndjson")).unwrap();f.reopen();
        assert_eq!(f.store.as_ref().unwrap().planned_entry_quantity(&f.bank,&f.log,&f.scope,id,&f.trust,NOW+2).unwrap(),Some(1));
        assert!(matches!(f.store.as_ref().unwrap().assess_quantity(&f.bank,&f.log,&f.scope,id,1,&f.trust,NOW+2).unwrap(),Decision::Entry{..}));
        f.store.as_mut().unwrap().bind_quantity(&f.bank,&f.log,&f.scope,id,1,&spec,&f.trust,NOW+2).unwrap();assert_eq!(std::fs::read(f.root.join("fundamental.ndjson")).unwrap(),bytes);
        let mut changed=spec.clone();changed.body_json=Some(serde_json::to_string(changed.body.as_ref().unwrap()).unwrap());
        assert!(f.store.as_mut().unwrap().bind_quantity(&f.bank,&f.log,&f.scope,id,1,&changed,&f.trust,NOW+2).is_err());
        let full=f.spec(&q);assert!(f.store.as_mut().unwrap().bind_quantity(&f.bank,&f.log,&f.scope,id,2,&full,&f.trust,NOW+2).is_err());
        assert!(f.store.as_ref().unwrap().planned_entry_quantity(&f.bank,&f.log,&f.scope,id,&f.trust,NOW+20000).unwrap().is_none());
        let mut revised=f.review.clone();revised.reviewed_at_ms=NOW+3;f.install(revised,NOW+3);
        assert!(f.store.as_ref().unwrap().planned_entry_quantity(&f.bank,&f.log,&f.scope,id,&f.trust,NOW+3).unwrap().is_none(),"a new review cannot silently replace the bound entry capability");
    }
    #[test]fn intent_or_unknown_attempt_cannot_resume_or_mint_replacement_slices(){
        for attempted in [false,true]{let mut f=F::new();let(q,spec)=f.slice_spec();let id=q["id"].as_u64().unwrap();f.store.as_mut().unwrap().bind_quantity(&f.bank,&f.log,&f.scope,id,1,&spec,&f.trust,NOW+1).unwrap();
            f.log.append(json!({"kind":"intent","qid":id,"quote":q,"spec":spec,"fundamental":f.store.as_ref().unwrap().durable_tag(id).unwrap()})).unwrap();if attempted{f.log.append(json!({"kind":"attempt","qid":id})).unwrap();}f.reopen();
            assert!(f.store.as_ref().unwrap().planned_entry_quantity(&f.bank,&f.log,&f.scope,id,&f.trust,NOW+2).unwrap().is_none());
            assert!(f.store.as_mut().unwrap().bind_quantity(&f.bank,&f.log,&f.scope,id,1,&spec,&f.trust,NOW+2).is_err());
        }
    }
    #[test]fn actual_ack_requires_the_fixed_slice_even_when_native_would_accept_more(){
        let mut f=F::new();let(q,spec)=f.slice_spec();let id=q["id"].as_u64().unwrap();f.store.as_mut().unwrap().bind_quantity(&f.bank,&f.log,&f.scope,id,1,&spec,&f.trust,NOW+1).unwrap();f.log_intent(&q,&spec);
        let mut ack=F::base(&q,"actual_order_ack",NOW+2,"201");for n in [0,2,3]{ack["quantity"]=json!(n);assert!(f.store.as_ref().unwrap().validate_acknowledgment(id,&ack).is_err());}ack["quantity"]=json!(1);f.store.as_ref().unwrap().validate_acknowledgment(id,&ack).unwrap();
        let mut changed=ack.clone();changed["exchangeId"]=json!("102");assert!(f.store.as_ref().unwrap().validate_acknowledgment(id,&changed).is_err());
        assert!(f.store.as_ref().unwrap().validate_acknowledgment(id+100,&ack).is_err());
        ack["quantity"]=json!(2);f.event(ack);f.store.take();assert!(Cohorts::open(&f.root,&f.bank,&f.log,f.owner.clone()).is_err(),"native's generic <=original guard is insufficient; replay requires sealed physical n");
    }
    #[test]fn explicit_slice_rejects_zero_increase_and_unsealed_body_quantity(){
        let mut f=F::new();let(q,spec)=f.slice_spec();let id=q["id"].as_u64().unwrap();
        for n in [0,3]{assert!(f.store.as_mut().unwrap().bind_quantity(&f.bank,&f.log,&f.scope,id,n,&spec,&f.trust,NOW+1).is_err());}
        let mut changed=spec.clone();changed.body.as_mut().unwrap()["quantity"]=json!(2);assert!(f.store.as_mut().unwrap().bind_quantity(&f.bank,&f.log,&f.scope,id,1,&changed,&f.trust,NOW+1).is_err());
        assert_eq!(f.store.as_ref().unwrap().managed_exchanges(),["101".to_string()].into());
        let unrelated=json!({"type":"actual_order_ack","exchangeId":"999","quantity":3});f.store.as_ref().unwrap().validate_acknowledgment(999,&unrelated).unwrap();
    }
    #[test]fn genuine_review_sized_capability_authorizes_production_and_rejects_changed_quantity(){
        use baal_transport_service::{Authority,Receipt};
        use moloch_live_runtime::authority::{Activation,ProductionAuthority,SnapshotSet};
        let mut f=F::new();let(q,spec)=f.slice_spec();let id=q["id"].as_u64().unwrap();let at=NOW+1;
        f.store.as_mut().unwrap().bind_quantity(&f.bank,&f.log,&f.scope,id,1,&spec,&f.trust,at).unwrap();
        let mut permit=f.permit(&q,at);permit.maximum_quantity=1;permit.maximum_cost=q["price"].as_str().unwrap().into();let reviewed=f.authority();
        let token=Arc::new(f.store.as_ref().unwrap().physical_decision(&f.bank,&f.log,&f.scope,id,&spec,&permit,&reviewed,at).unwrap());
        let binary=f.root.join("synthetic-authority-binary");let control=f.root.join("synthetic-authority-control");let binary_bytes=b"synthetic test executable; never executed";std::fs::write(&binary,binary_bytes).unwrap();std::fs::write(&control,b"synthetic immutable Control").unwrap();
        let activation=Activation{run_id:"fundamental-fixture".into(),entry_until_ms:NOW+3600000,maximum_committed:"1000.000000".into(),maximum_market_cost:"100.000000".into(),maximum_quantity:2,tournament_id:"cup".into(),tournament_slug:"cup".into(),binary_sha256:sha(binary_bytes)};
        let authority=ProductionAuthority::new(activation,f.scope.clone(),vec![f.root.join("STOP")],control,binary,f.owner.clone()).unwrap();authority.bind_stream(Arc::new(std::sync::atomic::AtomicBool::new(false)),at+60000);
        authority.require_fundamental_exchanges(&f.store.as_ref().unwrap().managed_exchanges()).unwrap();authority.require_maker_review(&"c".repeat(64)).unwrap();
        let receipt=|endpoint:&str,data:Value|Receipt{endpoint:endpoint.into(),request_started_at:iso(at-1),received_at:iso(at),status:200,data};
        let facts=SnapshotSet{
            account:receipt("/tournaments/cup",json!({"id":"cup","slug":"cup","status":"active","endDate":iso(NOW+3600000)})),
            cash_balance:receipt("/account?tournamentId=cup",json!({"balance":1000,"availableAdvance":0,"buyingPower":1000})),
            positions:receipt("/tournaments/cup/portfolio/positions",json!({"positions":[]})),
            open_orders:receipt("/orders?status=open&limit=200&tournamentId=cup",json!({"data":[],"coverage":{"complete":true},"pagination":{"hasMore":false,"nextCursor":null}})),
            market:receipt("/markets/201",json!({"id":"201","status":"open","isComposite":false,"isMultiOutcome":false,"contexts":[{"status":"open"}]})),
            contract:receipt("/markets/201/nodes?tournamentId=cup",serde_json::from_slice(ORIGINALS[2]).unwrap()),
            book:receipt("/markets/201/orderbook?depth=20",json!({"contexts":[{"orderbook":{"exchanges":[{"exchangeId":"101","asOf":{"sequence":3,"at":iso(at)},"bids":[{"price":0.10,"quantity":100}],"asks":[{"price":0.12,"quantity":100}]}]}}]})),
        };
        assert!(authority.prepare(&f.bank,id,spec.clone(),permit.clone(),facts.clone()).is_err(),"managed BUY cannot silently use the legacy maker path");
        authority.prepare_with_policies(&f.bank,id,spec.clone(),permit.clone(),facts.clone(),None,Some(token.clone())).unwrap();
        authority.authorize(&f.scope,&spec,&permit,false,at).unwrap();
        f.log_intent(&q,&spec);f.store.as_ref().unwrap().check_bound(&f.bank,&f.log,&f.scope,id,&spec,&f.trust,at).unwrap();
        authority.authorize(&f.scope,&spec,&permit,true,at).unwrap();assert!(f.log.require_resolved_attempts().is_err(),"no HTTP is performed and the supplemental capability cannot resolve unknown acceptance");
        let mut changed=spec.clone();changed.body.as_mut().unwrap()["quantity"]=json!(2);
        assert!(authority.authorize(&f.scope,&changed,&permit,true,at).is_err());
        // Re-preparing the same larger native quote is also insufficient: the
        // genuine opaque policy capability remains bound to physical slice1.
        authority.prepare_with_policies(&f.bank,id,changed.clone(),permit.clone(),facts,None,Some(token)).unwrap();
        assert!(authority.authorize(&f.scope,&changed,&permit,true,at).is_err());
        assert_eq!(f.bank.engine().order_records().into_iter().find(|x|x["id"]==id).unwrap()["quantity"],2);
    }
    #[test]fn opaque_contract_gate_rechecks_the_reviewed_root_before_any_physical_authorization(){
        use baal_transport_service::Receipt;
        let mut f=F::new();let(q,spec)=f.bind_entry();let id=q["id"].as_u64().unwrap();let permit=f.permit(&q,NOW);let authority=f.authority();let token=f.store.as_ref().unwrap().physical_decision(&f.bank,&f.log,&f.scope,id,&spec,&permit,&authority,NOW).unwrap();
        let receipt=Receipt{endpoint:"/markets/201/nodes?tournamentId=cup".into(),request_started_at:iso(NOW-1),received_at:iso(NOW),status:200,data:serde_json::from_slice(ORIGINALS[2]).unwrap()};token.validate_contract(&receipt).unwrap();
        for case in ["market","node_id","contract_id","party","title","endpoint","status"]{let mut changed=receipt.clone();match case{"market"=>changed.data["market_id"]=json!("202"),"node_id"=>changed.data["root"]["node_id"]=json!("302"),"contract_id"=>{changed.data["root"]["contract_id"]=json!("302");changed.data["root"]["id"]=json!("301");},"party"=>changed.data["root"]["contract_details"]["winnerName"]=json!("Democratic Party"),"title"=>changed.data["root"]["title"]=json!("Changed resolver description"),"endpoint"=>changed.endpoint="/markets/201/nodes?tournamentId=other".into(),_=>changed.status=503}assert!(token.validate_contract(&changed).is_err(),"changed {case}");}
        let mut reordered=receipt.clone();let root=serde_json::to_string_pretty(&reordered.data["root"]).unwrap();reordered.data["root"]=serde_json::from_str(&root).unwrap();token.validate_contract(&reordered).unwrap();
    }
    #[test]fn existing_origin_binds_real_adoption_fifo_without_fabricating_entry_or_changing_money(){
        let(mut f,transport,adoption)=inherited_fixture();let head=f.bank.durable_head();let report=f.bank.engine().report().unwrap();install_inherited(&mut f,&transport,&adoption,"0.950000");
        assert_eq!(f.bank.durable_head(),head);assert_eq!(f.bank.engine().report().unwrap(),report);assert!(f.bank.engine().fills().is_empty());assert!(f.bank.engine().live_order_bindings().is_empty());
        assert_eq!(report["ownedCost"],"1.900000");assert_eq!(report["openingInventoryCost"],"1.900000");assert_eq!(report["inheritedRealizedGross"],"0.000000");
        let rows=std::fs::read_to_string(f.root.join("fundamental.ndjson")).unwrap();let last:Value=serde_json::from_str(rows.lines().last().unwrap()).unwrap();assert!(last["event"]["cohort"].get("entry").is_none());assert_eq!(last["event"]["cohort"]["existing"]["quantity"],3);
        f.reopen();assert!(f.store.as_ref().unwrap().manages("101"));assert!(f.store.as_ref().unwrap().prepare_existing_exit(&f.bank,&f.log,&transport,&f.scope,"101",&f.trust,NOW+3001).unwrap().is_none(),"high reviewed value keeps inherited inventory without consulting its cost floor");
    }
    #[test]fn existing_snapshot_rejects_uncommitted_adoption_stale_head_and_insufficient_baseline_cap(){
        let(mut f,transport,adoption)=inherited_fixture();assert!(ExistingSnapshot::capture(&f.bank,&f.log,&transport,"fixture-adoption",2,&adoption,"101").is_err());
        let mut wrong=adoption.clone();wrong["receiptHash"]=json!("a".repeat(64));assert!(ExistingSnapshot::capture(&f.bank,&f.log,&transport,"fixture-adoption",1,&wrong,"101").is_err());
        let snapshot=ExistingSnapshot::capture(&f.bank,&f.log,&transport,"fixture-adoption",1,&adoption,"101").unwrap();let mut review=f.review.clone();review.existing_origin=Some(snapshot.approval().clone());review.maximum_principal="1.890000".into();let reviewed=approve(review,&mut f.trust,NOW+3001);
        assert!(f.store.as_mut().unwrap().install_existing(reviewed,snapshot,&f.bank,&f.log,&transport,NOW+3001).is_err());assert!(!f.store.as_ref().unwrap().manages("101"));
        let snapshot=ExistingSnapshot::capture(&f.bank,&f.log,&transport,"fixture-adoption",1,&adoption,"101").unwrap();let mut review=f.review.clone();review.existing_origin=Some(snapshot.approval().clone());let reviewed=approve(review,&mut f.trust,NOW+3001);
        f.event(json!({"type":"tick","receivedAt":iso(NOW+3002)}));assert!(f.store.as_mut().unwrap().install_existing(reviewed,snapshot,&f.bank,&f.log,&transport,NOW+3002).is_err());
    }
    #[test]fn covered_value_exit_below_opening_cost_uses_actual_fifo_and_preserves_inherited_attribution(){
        let(mut f,transport,adoption)=inherited_fixture();install_inherited(&mut f,&transport,&adoption,"0.600000");let cash=f.bank.engine().wallet.cash;
        let(q,at)=inherited_exit(&mut f,&transport,NOW+4000,"0.300000","0.320000");assert_eq!(q["price"],"0.680000");assert_eq!(q["quantity"],2);assert_eq!(f.bank.engine().wallet.cash,cash);assert!(f.bank.engine().fills().is_empty());
        f.book(at,"0.300000","0.320000");let retained=f.bank.engine().order_records().into_iter().find(|x|x["id"]==q["id"]).unwrap();assert_eq!(retained,q,"ordinary fixed-gain planning must not replace/cancel the reviewed reservation");
        let spec=f.spec(&q);let id=q["id"].as_u64().unwrap();f.store.as_mut().unwrap().bind(&f.bank,&f.log,&f.scope,id,&spec,&f.trust,at).unwrap();let p=f.permit(&q,at);let authority=f.authority();let token=f.store.as_ref().unwrap().physical_decision(&f.bank,&f.log,&f.scope,id,&spec,&p,&authority,at).unwrap();token.authorize(&f.scope,&spec,&p,at).unwrap();f.log_intent(&q,&spec);
        let ack=F::base(&q,"actual_order_ack",at+1,"901");f.store.as_ref().unwrap().validate_acknowledgment(id,&ack).unwrap();f.log.append(json!({"kind":"ack","qid":id,"receipt":{"orderId":"901","quantity":2}})).unwrap();f.event(ack);
        let mut fill=F::base(&q,"actual_fill",at+3,"901");fill["fillId"]=json!("902");fill["executedAt"]=json!(iso(at+2));fill["fee"]=json!("0.000000");f.event(fill.clone());f.event(fill);
        let mut terminal=F::base(&q,"actual_order_terminal",at+4,"901");terminal["status"]=json!("filled");terminal["fillsComplete"]=json!(true);terminal["filledQuantity"]=json!(2);f.event(terminal);f.log.append(json!({"kind":"closed","qid":id})).unwrap();drop(token);f.reopen();
        let report=f.bank.engine().report().unwrap();assert_eq!(report["inheritedRealizedGross"],"-0.040000");assert_eq!(report["newTradingRealized"],"0.000000");assert_eq!(report["ownedCost"],"0.500000");assert_eq!(f.bank.engine().fills().len(),1);
        assert_eq!(report["positions"]["101"]["NO"],json!([{"quantity":1,"price":"0.500000"}]));
        let tag=f.store.as_ref().unwrap().durable_tag(id).unwrap();assert!(tag.get("entryQid").is_none());assert_eq!(tag["originKind"],"reviewed_existing_snapshot");
    }
    #[test]fn inherited_deadline_allows_fresh_covered_loss_exit_without_a_current_forecast(){
        let(mut f,transport,adoption)=inherited_fixture();install_inherited(&mut f,&transport,&adoption,"0.950000");let(q,at)=inherited_exit(&mut f,&transport,NOW+600001,"0.300000","0.320000");assert_eq!(q["price"],"0.680000");assert_eq!(q["reviewedInventoryExit"]["reason"],"reviewed_holding_deadline");
        let spec=f.spec(&q);let id=q["id"].as_u64().unwrap();f.store.as_mut().unwrap().bind(&f.bank,&f.log,&f.scope,id,&spec,&f.trust,at).unwrap();let authority=f.authority();let permit=f.permit(&q,at);let token=f.store.as_ref().unwrap().physical_decision(&f.bank,&f.log,&f.scope,id,&spec,&permit,&authority,at).unwrap();token.authorize(&f.scope,&spec,&permit,at).unwrap();assert!(at>f.review.expires_at_ms);
    }
    #[test]fn native_covered_exit_rejects_mutated_economics_depth_scope_and_prefix_atomically(){
        let(mut f,transport,adoption)=inherited_fixture();install_inherited(&mut f,&transport,&adoption,"0.600000");f.book(NOW+4000,"0.300000","0.320000");f.retire_drafts(NOW+7001);
        let plan=f.store.as_ref().unwrap().prepare_existing_exit(&f.bank,&f.log,&transport,&f.scope,"101",&f.trust,NOW+7001).unwrap().unwrap();let before=f.bank.durable_head();let report=f.bank.engine().report().unwrap();
        for case in ["quantity","marginal","book","head","report","adoption","value","deadline","scope"]{let mut bad=plan.event().clone();match case{"quantity"=>bad["quantity"]=json!(4),"marginal"=>bad["limitPrice"]=json!("0.675000"),"book"=>bad["bookSha256"]=json!("e".repeat(64)),"head"=>bad["bankHead"]["sequence"]=json!(0),"report"=>bad["expectedReportSha256"]=json!("e".repeat(64)),"adoption"=>bad["adoptionRef"]=json!({}),"value"=>bad["expectedPayout"]=json!("0.940500"),"deadline"=>{bad["reason"]=json!("reviewed_holding_deadline");bad["holdingDeadlineMs"]=json!(NOW+999999);},_=>bad["bankScope"]["runId"]=json!("different")};assert!(f.bank.accept("invalid-covered-exit",1,&bad).is_err(),"{case}");assert_eq!(f.bank.durable_head(),before);assert_eq!(f.bank.engine().report().unwrap(),report);}
        f.bank.accept("valid-covered-exit",1,plan.event()).unwrap();assert!(f.bank.engine().fills().is_empty());assert_eq!(f.bank.engine().wallet.quantity("101",moloch_replay::Side::No),3);
        assert!(f.bank.accept("duplicate-covered-exit",1,plan.event()).is_err(),"the exact prefix and current liability prevent a second reservation");
    }
    #[test]fn inherited_origin_never_uses_a_synthetic_entry_or_an_unknown_order_as_authority(){
        let(mut f,transport,adoption)=inherited_fixture();f.log.append(json!({"kind":"intent","qid":999,"spec":{"fixture":"unknown"}})).unwrap();f.log.append(json!({"kind":"attempt","qid":999})).unwrap();
        assert!(ExistingSnapshot::capture(&f.bank,&f.log,&transport,"fixture-adoption",1,&adoption,"101").is_err());assert_eq!(f.bank.engine().wallet.quantity("101",moloch_replay::Side::No),3);assert!(f.bank.engine().fills().is_empty());
    }
    #[test]fn reviewed_revocation_exits_below_cost_and_revised_value_cancels_only_known_open_exit(){
        let(mut f,transport,adoption)=inherited_fixture();install_inherited(&mut f,&transport,&adoption,"0.950000");let mut r=f.review.clone();r.reviewed_at_ms=NOW+4000;r.revoke_existing=true;f.install(r.clone(),NOW+4000);
        let(q,at)=inherited_exit(&mut f,&transport,NOW+5000,"0.300000","0.320000");assert_eq!(q["reviewedInventoryExit"]["reason"],"reviewed_source_revocation");assert_eq!(q["price"],"0.680000");assert!(q["expiresAt"].as_u64().unwrap()<=r.sources[0].observed_at_ms+r.sources[0].maximum_age_ms);
        let spec=f.spec(&q);let id=q["id"].as_u64().unwrap();f.store.as_mut().unwrap().bind(&f.bank,&f.log,&f.scope,id,&spec,&f.trust,at).unwrap();f.log_intent(&q,&spec);
        assert!(f.store.as_ref().unwrap().existing_exit_cancellations(&f.bank,&f.log,&f.trust,at).unwrap().is_empty(),"an unknown physical attempt has no invented actual ID");
        f.log.append(json!({"kind":"ack","qid":id,"receipt":{"orderId":"901"}})).unwrap();f.event(F::base(&q,"actual_order_ack",at+1,"901"));
        r.reviewed_at_ms=at+2;r.revoke_existing=false;r.ordinary_probability="0.990000".into();set_deadline_cash(&mut r,"0.980100");f.install(r,at+2);
        assert_eq!(f.store.as_ref().unwrap().existing_exit_cancellations(&f.bank,&f.log,&f.trust,at+2).unwrap(),vec![id]);
    }
    #[test]fn existing_exit_hint_precedes_preflight_and_blocking_draft_retirement_without_creating_an_order(){
        let(mut f,transport,adoption)=inherited_fixture();install_inherited(&mut f,&transport,&adoption,"0.600000");f.book(NOW+4000,"0.300000","0.320000");let head=f.bank.durable_head();let orders=f.bank.engine().order_records();
        let hint=f.store.as_ref().unwrap().existing_exit_hint(&f.bank,&f.log,&f.scope,"101",&f.trust,NOW+4000).unwrap().unwrap();assert_eq!(hint.exchange_id,"101");assert_eq!(hint.market_id,"201");assert_eq!(hint.contract_id,"301");assert_eq!(hint.reason,"refreshed_value_exit");assert_eq!(f.bank.durable_head(),head);assert_eq!(f.bank.engine().order_records(),orders);
        assert!(f.store.as_ref().unwrap().prepare_existing_exit(&f.bank,&f.log,&transport,&f.scope,"101",&f.trust,NOW+4000).is_err(),"the advisory hint does not bypass the existing never-sent ordinary draft");
        f.retire_drafts(NOW+7001);assert!(f.store.as_ref().unwrap().prepare_existing_exit(&f.bank,&f.log,&transport,&f.scope,"101",&f.trust,NOW+7001).unwrap().is_some());
        let(mut f,transport,adoption)=inherited_fixture();install_inherited(&mut f,&transport,&adoption,"0.950000");assert!(f.store.as_ref().unwrap().existing_exit_hint(&f.bank,&f.log,&f.scope,"101",&f.trust,NOW+3001).unwrap().is_none());
        assert_eq!(f.store.as_ref().unwrap().existing_exit_hint(&f.bank,&f.log,&f.scope,"101",&f.trust,NOW+20000).unwrap().unwrap().reason,"reviewed_inventory_book_refresh");
        assert_eq!(f.store.as_ref().unwrap().existing_exit_hint(&f.bank,&f.log,&f.scope,"101",&f.trust,NOW+600001).unwrap().unwrap().reason,"reviewed_holding_deadline");
    }
    #[test]fn an_executable_ordinary_inherited_sell_is_a_policy_hold_until_reviewed_native_reservation(){
        let(mut f,transport,adoption)=inherited_fixture();install_inherited(&mut f,&transport,&adoption,"0.600000");f.book(NOW+4000,"0.020000","0.040000");
        let q=f.bank.engine().order_records().into_iter().find(|q|q["retired"]==false&&q["action"]=="sell").unwrap();assert!(q.get("reviewedInventoryExit").is_none());
        assert_eq!(f.assess(&q,NOW+4000),Decision::Hold{reason:"reviewed_existing_exit_reservation_required".into()},"normal unsubmitted proposals must be vetoed without stopping the worker or pretending they have an origin-bound reservation");
    }
    fn inherited_v2_packet(f:&mut F,mut facts:moloch_live_runtime::authority::SnapshotSet,at:u64)->moloch_live_runtime::authority::SnapshotSet{
        for(i,r)in [&mut facts.account,&mut facts.positions,&mut facts.open_orders,&mut facts.cash_balance,&mut facts.market,&mut facts.contract,&mut facts.book].into_iter().enumerate(){r.request_started_at=iso(at+10+i as u64*2);r.received_at=iso(at+11+i as u64*2);}
        let held=f.bank.engine().wallet.quantity("101",moloch_replay::Side::No);facts.positions.data=json!({"positions":[{"exchangeId":"101","marketId":"201","quantity":-(held as i64)}]});
        let cash:Value=serde_json::from_str(&moloch_replay::money(moloch_live_runtime::authority::reconciled_expected_cash(&f.bank).unwrap())).unwrap();facts.cash_balance.data=json!({"balance":cash,"availableAdvance":0,"buyingPower":cash});
        let raw=&mut facts.book.data["contexts"][0]["orderbook"]["exchanges"][0];raw["asOf"]=json!({"sequence":at-NOW+1000,"at":iso(at+23)});raw["bids"][0]["price"]=json!(0.11);raw["asks"][0]["price"]=json!(0.13);
        let b=&facts.book;let events=moloch_replay::source::receipt_events(&json!({"source":"official_sig_api_v1","kind":"book","tournamentId":f.scope.tournament_id,"marketId":"201","exchangeId":"101","endpoint":b.endpoint,"status":b.status,"requestStartedAt":b.request_started_at,"receivedAt":b.received_at,"data":b.data})).unwrap();
        for e in events{let n=f.bank.last_source_sequence("inherited-v2-book")+1;f.bank.accept("inherited-v2-book",n,&e).unwrap();}f.retire_drafts(at+24);facts
    }
    fn inherited_v2_fixture()->(F,baal_transport_service::Transport,moloch_live_runtime::authority::SnapshotSet,Value){
        let(unused,unused_t,facts)=entry_fixture();drop(unused_t);drop(unused);
        let(mut f,t,adoption)=inherited_fixture();install_inherited(&mut f,&t,&adoption,"0.950000");let mut r=f.review.clone();r.reviewed_at_ms=NOW+4000;r.maximum_principal="8.000000".into();r.maximum_quantity=7;r.same_side_top_ups=Some(SameSideTopUps{version:2,maximum_entries:4});f.install(r.clone(),NOW+4000);f.review=r;install_cohort_planning(&mut f,NOW+4001);
        let facts=inherited_v2_packet(&mut f,facts,NOW+5000);let authority=f.authority();let hint=f.store.as_ref().unwrap().entry_hints(&f.bank,&f.log,&f.scope,&f.trust,NOW+7525).unwrap().remove(0);
        let plan=f.store.as_ref().unwrap().prepare_entry(&f.bank,&f.log,&t,&f.scope,&hint,&facts,&authority,NOW+7525).unwrap().unwrap();assert_eq!(plan.event()["sameSideTopUp"]["version"],2);assert!(plan.event()["sameSideTopUp"]["priorEntries"].as_array().unwrap().is_empty());let e=plan.event().clone();drop(plan);drop(authority);f.event(e);
        let q=f.bank.engine().order_records().into_iter().find(|q|q["retired"]==false&&q.get("reviewedFundamentalEntry").is_some()).unwrap();let spec=f.spec(&q);f.store.as_mut().unwrap().bind(&f.bank,&f.log,&f.scope,q["id"].as_u64().unwrap(),&spec,&f.trust,NOW+7526).unwrap();(f,t,facts,q)
    }
    #[test]fn inherited_v2_genuine_addition_closed_then_partial_exit_and_next_addition_preserve_whole_fifo(){
        let(mut f,t,facts,q)=inherited_v2_fixture();assert_eq!(f.bank.engine().wallet.quantity("101",moloch_replay::Side::No),3);assert_eq!(f.bank.engine().wallet.market_cost("101"),1900000);assert!(f.bank.engine().fills().is_empty());assert_eq!(q["quantity"],2);top_up_close(&mut f,&q,NOW+10527,"901",2,true);f.reopen();assert_eq!(f.bank.engine().wallet.quantity("101",moloch_replay::Side::No),5);
        f.book(NOW+14000,"0.030000","0.050000");f.retire_drafts(NOW+14001);let plan=f.store.as_ref().unwrap().prepare_existing_exit(&f.bank,&f.log,&t,&f.scope,"101",&f.trust,NOW+17001).unwrap().unwrap();assert_eq!(plan.event()["cohortOrigin"]["version"],2);assert!(plan.event().get("adoptionRef").is_none());let e=plan.event().clone();f.event(e);let sell=f.bank.engine().order_records().into_iter().find(|q|q["retired"]==false&&q["action"]=="sell").unwrap();let at=sell["activationAt"].as_u64().unwrap();let spec=f.spec(&sell);f.store.as_mut().unwrap().bind(&f.bank,&f.log,&f.scope,sell["id"].as_u64().unwrap(),&spec,&f.trust,at).unwrap();top_up_close(&mut f,&sell,at+1,"902",2,true);f.reopen();assert_eq!(f.bank.engine().wallet.positions()["101"]["NO"],json!([{"quantity":1,"price":"0.500000"},{"quantity":2,"price":"0.870000","executedAt":iso(NOW+10530)}]));
        let facts=inherited_v2_packet(&mut f,facts,NOW+23000);let authority=f.authority();let hint=f.store.as_ref().unwrap().entry_hints(&f.bank,&f.log,&f.scope,&f.trust,NOW+25525).unwrap().remove(0);let p=f.store.as_ref().unwrap().prepare_entry(&f.bank,&f.log,&t,&f.scope,&hint,&facts,&authority,NOW+25525).unwrap().unwrap();assert_eq!(p.event()["sameSideTopUp"]["priorExits"][0]["qid"],sell["id"]);assert_eq!(p.event()["sameSideTopUp"]["heldCost"],"2.240000");let e=p.event().clone();drop(p);f.event(e);assert_eq!(f.bank.engine().wallet.quantity("101",moloch_replay::Side::No),3);
    }
    #[test]fn inherited_v2_open_or_unknown_physical_addition_cannot_authorize_more_entries_or_exits(){for unknown in [false,true]{let(mut f,t,_,q)=inherited_v2_fixture();if unknown{let spec=f.spec(&q);f.log_intent(&q,&spec);}else{top_up_close(&mut f,&q,NOW+10527,"901",1,false);}let head=f.bank.durable_head();let result=f.store.as_ref().unwrap().entry_hints(&f.bank,&f.log,&f.scope,&f.trust,NOW+10536);assert!(result.is_err()||result.unwrap().is_empty());let p=f.store.as_ref().unwrap().prepare_existing_exit(&f.bank,&f.log,&t,&f.scope,"101",&f.trust,NOW+10536);assert!(p.is_err()||p.unwrap().is_none());assert_eq!(f.bank.durable_head(),head);}}
    #[test]fn inherited_v2_unused_addition_requires_positive_not_sent_and_native_retirement_before_cleanup(){for positive in [false,true]{let(mut f,t,facts,q)=inherited_v2_fixture();let id=q["id"].as_u64().unwrap();let proof=f.log.proof(id);if positive{f.log.append(json!({"kind":"not_sent","qid":id,"proof":proof})).unwrap();}let e=json!({"type":"unsubmitted_intent_retired","source":"moloch_live_dispatcher","orderId":id,"confirmedNetworkAttempts":0,"receiptHash":sha(&serde_json::to_vec(&proof).unwrap()),"receivedAt":iso(NOW+11000),"processedAt":iso(NOW+11000),"deferPlanningForPackage":true});f.bank.accept("inherited-unused",1,&e).unwrap();let mut packet=moloch_live_runtime::authority::CancelSnapshotSet{account:facts.account,positions:facts.positions,open_orders:facts.open_orders,cash_balance:facts.cash_balance};for(i,r)in [&mut packet.account,&mut packet.positions,&mut packet.open_orders,&mut packet.cash_balance].into_iter().enumerate(){r.request_started_at=iso(NOW+11001+i as u64*2);r.received_at=iso(NOW+11002+i as u64*2);}
        let hint=f.store.as_ref().unwrap().abandonment_hints(&f.bank,&f.log,&f.scope,NOW+11010).unwrap().into_iter().find(|h|h.qid==id).unwrap();let p=f.store.as_ref().unwrap().prepare_abandon(&f.bank,&f.log,&t,&f.scope,&hint,&packet,"inherited-unused",1,&e,NOW+11010);if !positive{assert!(p.is_err());continue}let before=f.bank.engine().report().unwrap();f.store.as_mut().unwrap().apply_abandon(p.unwrap(),&f.bank,&f.log,&t,NOW+11010).unwrap();f.reopen();assert_eq!(f.bank.engine().report().unwrap(),before);assert!(f.store.as_ref().unwrap().validate_acknowledgment(id,&F::base(&q,"actual_order_ack",NOW+11011,"999")).is_err());assert_eq!(f.store.as_ref().unwrap().entry_hints(&f.bank,&f.log,&f.scope,&f.trust,NOW+11011).unwrap().len(),1);}}
    #[cfg(target_os="linux")]mod generation_reload{
        use super::*;
        use moloch_live_runtime::{fundamental_reload as reload,authority::CancelSnapshotSet};
        fn files(f:&F,r:&ReviewInput)->Settings{
            let hash=review_sha256(r).unwrap();let path=f.root.join(format!("reload-review-{hash}.json"));
            let bytes=serde_json::to_vec(r).unwrap();if !path.exists(){std::fs::write(&path,&bytes).unwrap();}
            let mut trusted=f.trust.clone();trusted.insert(hash.clone());
            let authority_bytes=serde_json::to_vec(&json!({"version":1,"trustedReviewSha256":trusted})).unwrap();
            let authority_path=f.root.join(format!("reload-authority-{}.json",sha(&authority_bytes)));if !authority_path.exists(){std::fs::write(&authority_path,&authority_bytes).unwrap();}
            let source_files=originals_for(r).into_iter().map(|b|{let hash=sha(&b);let path=f.root.join(format!("reload-source-{hash}.bin"));if !path.exists(){std::fs::write(&path,&b).unwrap();}SourceFile{path,sha256:hash}}).collect();
            Settings{version:1,authority_path,authority_sha256:sha(&authority_bytes),reviews:vec![ReviewFile{path,file_sha256:sha(&bytes),source_files}],allow_initialization:false}
        }
        fn base()->Vec<u8>{serde_json::to_vec(&json!({"fundamentalReload":true,"fundamentalPolicy":null,"activation":{"maximumCommitted":"1000.000000"},"accountScope":"fixture"})).unwrap()}
        fn approved(f:&F,settings:&Settings,at:u64)->reload::Approved{approved_against(f,settings,at,&base())}
        fn approved_against(f:&F,settings:&Settings,at:u64,base:&[u8])->reload::Approved{
            let mut control:Value=serde_json::from_slice(base).unwrap();control["fundamentalPolicy"]=serde_json::to_value(settings).unwrap();
            let bytes=serde_json::to_vec(&control).unwrap();let control_path=f.root.join(format!("reload-control-{}.json",sha(&bytes)));if !control_path.exists(){std::fs::write(&control_path,&bytes).unwrap();}
            let channel=reload::Channel::bind(&f.root,f.owner.clone()).unwrap();let status=channel.status(&base,&f.bank,f.store.as_ref().unwrap().generation_sha()).unwrap();
            let request=json!({"version":1,"requestId":format!("fixture-{}",sha(&bytes)),"workerPid":status["workerPid"],"workerStartTicks":status["workerStartTicks"],"ownerLease":status["ownerLease"],"baseControlSha256":sha(&base),"bankManifestSha256":f.bank.manifest_hash(),"runId":"fundamental-fixture","previousGenerationSha256":f.store.as_ref().unwrap().generation_sha(),"controlPath":control_path,"controlSha256":sha(&bytes),"issuedAtMs":at,"expiresAtMs":at+60000});
            let bytes=serde_json::to_vec_pretty(&request).unwrap();let path=f.root.join(format!("request-{}.json",sha(&bytes)));std::fs::write(&path,&bytes).unwrap();let hash=sha(&bytes);let root=f.root.clone();
            let client=std::thread::spawn(move||reload::send(&root,&path,&hash).map_err(|e|e.to_string()));
            let mut pending=None;for _ in 0..1000{pending=channel.poll().unwrap();if pending.is_some(){break}std::thread::sleep(std::time::Duration::from_millis(1));}
            let mut pending=pending.expect("actual local owner request");let proof=pending.approved.take().unwrap();pending.reply(&json!({"state":"authenticated_fixture_only"})).unwrap();drop(pending);assert!(client.join().unwrap().is_ok());proof
        }
        fn facts(f:&F,at:u64)->CancelSnapshotSet{
            let r=|endpoint:String,data:Value|baal_transport_service::Receipt{endpoint,request_started_at:iso(at),received_at:iso(at),status:200,data};let tid=&f.scope.tournament_id;
            let positions=f.scope.exchanges.iter().filter_map(|(ex,market)|{let n=f.bank.engine().wallet.quantity(ex,moloch_replay::Side::Yes) as i128-f.bank.engine().wallet.quantity(ex,moloch_replay::Side::No) as i128;if n==0{None}else{Some(json!({"exchangeId":ex,"marketId":market,"quantity":n}))}}).collect::<Vec<_>>();
            let cash:Value=serde_json::from_str(&moloch_replay::money(moloch_live_runtime::authority::reconciled_expected_cash(&f.bank).unwrap())).unwrap();
            CancelSnapshotSet{account:r("/tournaments/cup".into(),json!({"id":tid,"slug":"cup","status":"active"})),positions:r("/tournaments/cup/portfolio/positions".into(),json!({"positions":positions})),open_orders:r(format!("/orders?status=open&limit=200&tournamentId={tid}"),json!({"data":[],"coverage":{"complete":true},"pagination":{"hasMore":false,"nextCursor":null}})),cash_balance:r(format!("/account?tournamentId={tid}"),json!({"balance":cash,"availableAdvance":0,"buyingPower":cash}))}
        }
        fn revised(f:&F)->ReviewInput{let mut r=f.review.clone();r.reviewed_at_ms=NOW+10;r.expires_at_ms=NOW+120000;r}
        fn commit(f:&mut F,r:&ReviewInput,at:u64)->(Settings,reload::Approved){
            let settings=files(f,r);let proof=approved(f,&settings,at);let loaded=proof.load(&base()).unwrap();let facts=facts(f,at);
            f.store.as_mut().unwrap().install_generation(&proof,&loaded,&f.bank,&f.log,&f.scope,&facts,&base(),at,||Ok(())).unwrap();(settings,proof)
        }
        #[test]fn inherited_hot_adoption_is_one_generation_or_no_mutation_and_survives_restart(){for deny in [true,false]{let(mut f,t,adoption)=inherited_fixture();let snapshot=ExistingSnapshot::capture(&f.bank,&f.log,&t,"fixture-adoption",1,&adoption,"101").unwrap();let mut r=f.review.clone();r.existing_origin=Some(snapshot.approval().clone());r.same_side_top_ups=Some(SameSideTopUps{version:2,maximum_entries:3});r.reviewed_at_ms=NOW+4000;let settings=files(&f,&r);let proof=approved(&f,&settings,NOW+4000);let loaded=proof.load(&base()).unwrap();let packet=facts(&f,NOW+4000);let original=std::fs::read(f.root.join("fundamental.ndjson")).unwrap();let head=f.bank.durable_head();let wallet=f.bank.engine().wallet.positions();let result=f.store.as_mut().unwrap().install_generation_with_existing(&proof,&loaded,&f.bank,&f.log,&f.scope,&packet,&base(),NOW+4000,&t,vec![snapshot],||if deny{Err("synthetic final boundary rejection".into())}else{Ok(())});if deny{assert!(result.is_err());assert_eq!(std::fs::read(f.root.join("fundamental.ndjson")).unwrap(),original);assert!(!f.store.as_ref().unwrap().manages("101"));}else{result.unwrap();let bytes=std::fs::read_to_string(f.root.join("fundamental.ndjson")).unwrap();assert_eq!(bytes.lines().count(),1);let row:Value=serde_json::from_str(bytes.lines().next().unwrap()).unwrap();assert_eq!(row["event"]["existingOriginsVersion"],1);assert!(row["event"]["existingOrigins"]["101"].get("entry").is_none());f.reopen();assert!(f.store.as_ref().unwrap().manages("101"));assert_eq!(f.store.as_ref().unwrap().generation_sha(),proof.metadata()["generationSha256"].as_str().unwrap());}assert_eq!(f.bank.durable_head(),head);assert_eq!(f.bank.engine().wallet.positions(),wallet);}}
        #[test]fn one_real_owner_generation_appends_once_without_financial_event_or_second_lock(){
            let mut f=F::new();let before_head=f.bank.durable_head();let before_report=f.bank.engine().report().unwrap();let before=std::fs::read(f.root.join("fundamental.ndjson")).unwrap();let r=revised(&f);
            let(settings,proof)=commit(&mut f,&r,NOW+10);let loaded=proof.load(&base()).unwrap();let active=reload::Active::installed(f.store.as_ref().unwrap(),proof,&loaded).unwrap();active.validate_full().unwrap();
            let after=std::fs::read(f.root.join("fundamental.ndjson")).unwrap();assert!(after.starts_with(&before));assert_eq!(after.iter().filter(|b|**b==b'\n').count(),before.iter().filter(|b|**b==b'\n').count()+1);
            assert_eq!(f.bank.durable_head(),before_head);assert_eq!(f.bank.engine().report().unwrap(),before_report);
            assert!(Cohorts::open(&f.root,&f.bank,&f.log,f.owner.clone()).is_err(),"original held lock is never opened twice");
            f.reopen();let(recovered,loaded)=reload::Active::recover(f.store.as_ref().unwrap(),&base()).unwrap().unwrap();assert_eq!(recovered.generation_sha(),f.store.as_ref().unwrap().generation_sha());recovered.validate_full().unwrap();loaded.validate_files().unwrap();assert_eq!(settings.reviews.len(),1);
        }
        #[test]fn new_scope_requires_authentic_retired_drafts_and_fresh_flat_original_facts(){
            let mut f=F::setup(false,"cup");let r=revised(&f);let settings=files(&f,&r);let proof=approved(&f,&settings,NOW+10);let loaded=proof.load(&base()).unwrap();let facts=facts(&f,NOW+10);let before=std::fs::read(f.root.join("fundamental.ndjson")).unwrap();
            assert!(f.store.as_mut().unwrap().install_generation(&proof,&loaded,&f.bank,&f.log,&f.scope,&facts,&base(),NOW+10,||Ok(())).unwrap_err().to_string().contains("NEW_SCOPE_NOT_FLAT"));assert_eq!(std::fs::read(f.root.join("fundamental.ndjson")).unwrap(),before);
            f.retire_drafts(NOW+1);f.store.as_mut().unwrap().install_generation(&proof,&loaded,&f.bank,&f.log,&f.scope,&facts,&base(),NOW+10,||Ok(())).unwrap();assert!(f.store.as_ref().unwrap().manages("101"));
        }
        #[test]fn changed_owned_cohort_preserves_actual_fifo_and_cannot_extend_hold(){
            let mut f=F::new();f.fill_entry(false);let head=f.bank.durable_head();let lots=f.bank.engine().wallet.positions();let mut r=revised(&f);r.hold_until_ms+=1;set_deadline_cash(&mut r,"0.940500");let settings=files(&f,&r);let proof=approved(&f,&settings,NOW+10);let loaded=proof.load(&base()).unwrap();let mut facts=facts(&f,NOW+10);facts.positions.data=json!({"positions":[{"exchangeId":"101","quantity":-2}]});
            let before=std::fs::read(f.root.join("fundamental.ndjson")).unwrap();assert!(f.store.as_mut().unwrap().install_generation(&proof,&loaded,&f.bank,&f.log,&f.scope,&facts,&base(),NOW+10,||Ok(())).unwrap_err().to_string().contains("CANNOT_EXTEND_COHORT"));assert_eq!(before,std::fs::read(f.root.join("fundamental.ndjson")).unwrap());assert_eq!(head,f.bank.durable_head());assert_eq!(lots,f.bank.engine().wallet.positions());
        }
        #[test]fn fresh_account_open_order_and_partial_coverage_never_install(){
            for bad in 0..4{let mut f=F::setup(false,"cup");f.retire_drafts(NOW+1);let r=revised(&f);let settings=files(&f,&r);let proof=approved(&f,&settings,NOW+10);let loaded=proof.load(&base()).unwrap();let mut facts=facts(&f,NOW+10);
                match bad{0=>facts.positions.data=json!({"positions":[{"exchangeId":"101","quantity":1}]}),1=>facts.open_orders.data["data"]=json!([{"exchangeId":"101","id":"999"}]),2=>facts.open_orders.data["coverage"]["complete"]=json!(false),_=>facts.positions.endpoint="/tournaments/foreign/portfolio/positions".into()}
                let before=std::fs::read(f.root.join("fundamental.ndjson")).unwrap();assert!(f.store.as_mut().unwrap().install_generation(&proof,&loaded,&f.bank,&f.log,&f.scope,&facts,&base(),NOW+10,||Ok(())).is_err());assert_eq!(before,std::fs::read(f.root.join("fundamental.ndjson")).unwrap());assert_eq!(f.store.as_ref().unwrap().generation_sha(),reload::ZERO);
            }
        }
        #[test]fn changed_source_or_control_and_unapproved_loaded_settings_cannot_commit(){
            for bad in 0..3{let mut f=F::new();let r=revised(&f);let settings=files(&f,&r);let proof=approved(&f,&settings,NOW+10);let loaded=proof.load(&base()).unwrap();let facts=facts(&f,NOW+10);let before=std::fs::read(f.root.join("fundamental.ndjson")).unwrap();
                let loaded=if bad==2{let mut other=r.clone();other.maximum_quantity=4;files(&f,&other).load().unwrap()}else{if bad==0{std::fs::write(&settings.reviews[0].source_files[0].path,b"changed original source").unwrap();}else{let meta=proof.control_bytes().unwrap();let value:Value=serde_json::from_slice(&meta).unwrap();let hash=sha(&meta);std::fs::write(f.root.join(format!("reload-control-{hash}.json")),serde_json::to_vec(&json!({"changed":value})).unwrap()).unwrap();}loaded};
                assert!(f.store.as_mut().unwrap().install_generation(&proof,&loaded,&f.bank,&f.log,&f.scope,&facts,&base(),NOW+10,||Ok(())).is_err());assert_eq!(before,std::fs::read(f.root.join("fundamental.ndjson")).unwrap());
            }
        }
        #[test]fn original_attempt_or_unclosed_cancel_cannot_hide_in_generation(){
            for cancel in [false,true]{let mut f=F::new();let r=revised(&f);let settings=files(&f,&r);let proof=approved(&f,&settings,NOW+10);
                f.log.append(json!({"kind":"intent","qid":999})).unwrap();f.log.append(json!({"kind":"attempt","qid":999})).unwrap();
                if cancel{f.log.append(json!({"kind":"ack","qid":999,"receipt":{"orderId":"999"}})).unwrap();f.log.append(json!({"kind":"cancel_attempt","qid":999})).unwrap();}
                assert!(proof.validate(&f.bank,&f.log,&f.owner,&base(),reload::ZERO,NOW+10).is_err());assert_eq!(f.store.as_ref().unwrap().generation_sha(),reload::ZERO);
            }
        }
        #[test]fn restart_after_generation_before_native_exclusion_resumes_exact_overlay(){
            let mut f=F::new();let r=revised(&f);let(_,proof)=commit(&mut f,&r,NOW+10);let wanted=proof.generation_sha().to_owned();let head=f.bank.durable_head();assert!(f.bank.engine().ordinary_entry_exclusions().is_empty());drop(proof);
            f.reopen();let(active,loaded)=reload::Active::recover(f.store.as_ref().unwrap(),&base()).unwrap().unwrap();assert_eq!(wanted,active.generation_sha());assert_eq!(head,f.bank.durable_head());assert_eq!(loaded.settings_sha().len(),64);
            let before=std::fs::read(f.root.join("fundamental.ndjson")).unwrap();drop(active);let _=reload::Active::recover(f.store.as_ref().unwrap(),&base()).unwrap().unwrap();assert_eq!(before,std::fs::read(f.root.join("fundamental.ndjson")).unwrap(),"recovery cannot append/remint or reset history");
            assert!(reload::Active::recover(f.store.as_ref().unwrap(),b"{}").is_err());
        }
        #[test]fn genuine_generation_binary_only_restart_preserves_original_approval_and_rejects_all_other_drift(){
            let mut f=F::new();let r=revised(&f);let settings=files(&f,&r);
            let mut initial:Value=serde_json::from_slice(&base()).unwrap();initial["activation"]["binarySha256"]=json!("a".repeat(64));initial["activation"]["quantity"]=json!(5);initial["activation"]["entryUntilMs"]=json!(NOW+600000);initial["scope"]=serde_json::to_value(&f.scope).unwrap();initial["credentials"]=json!("/immutable/credentials");let initial=serde_json::to_vec(&initial).unwrap();
            let proof=approved_against(&f,&settings,NOW+10,&initial);let loaded=proof.load(&initial).unwrap();let packet=facts(&f,NOW+10);
            f.store.as_mut().unwrap().install_generation(&proof,&loaded,&f.bank,&f.log,&f.scope,&packet,&initial,NOW+10,||Ok(())).unwrap();
            let metadata=f.store.as_ref().unwrap().active_generation().unwrap().clone();let original_control=proof.control_bytes().unwrap();drop(proof);drop(loaded);let original_history=std::fs::read(f.root.join("fundamental.ndjson")).unwrap();let head=f.bank.durable_head();f.reopen();
            let mut upgraded:Value=serde_json::from_slice(&original_control).unwrap();upgraded["activation"]["binarySha256"]=json!("b".repeat(64));let bytes=serde_json::to_vec(&upgraded).unwrap();
            let(active,loaded)=reload::Active::recover(f.store.as_ref().unwrap(),&bytes).unwrap().unwrap();active.validate_full().unwrap();loaded.validate_files().unwrap();
            assert_eq!(active.recovery_receipt(),&json!({"originalBaseSha256":sha(&initial),"currentBaseSha256":sha(&bytes),"recoveredGenerationSha256":metadata["generationSha256"],"binaryOnlyRebase":true}));
            assert_eq!(f.store.as_ref().unwrap().active_generation(),Some(&metadata));assert_eq!(metadata["request"]["baseControlSha256"],sha(&initial));assert_eq!(std::fs::read(f.root.join("fundamental.ndjson")).unwrap(),original_history);assert_eq!(f.bank.durable_head(),head);drop(active);drop(loaded);
            let mut planning=upgraded.clone();planning["managedInventoryPlanning"]=json!(true);let planning_bytes=serde_json::to_vec(&planning).unwrap();
            let(active,loaded)=reload::Active::recover(f.store.as_ref().unwrap(),&planning_bytes).unwrap().unwrap();active.validate_full().unwrap();loaded.validate_files().unwrap();
            assert_eq!(active.recovery_receipt()["binaryOnlyRebase"],false);assert_eq!(active.recovery_receipt()["managedInventoryPlanningOptIn"],true);
            assert_eq!(f.store.as_ref().unwrap().active_generation(),Some(&metadata));assert_eq!(std::fs::read(f.root.join("fundamental.ndjson")).unwrap(),original_history);assert_eq!(f.bank.durable_head(),head);drop(active);drop(loaded);
            planning["activation"]["entryUntilMs"]=json!(NOW+600001);assert!(reload::Active::recover(f.store.as_ref().unwrap(),&serde_json::to_vec(&planning).unwrap()).is_err());
            for case in ["cash","quantity","deadline","scope","credentials","unknown","reviewpath","authority","source","model","binary","missingbinary"]{
                let mut bad=upgraded.clone();match case{
                    "cash"=>bad["activation"]["maximumCommitted"]=json!("1001.000000"),"quantity"=>bad["activation"]["quantity"]=json!(6),"deadline"=>bad["activation"]["entryUntilMs"]=json!(NOW+600001),
                    "scope"=>bad["scope"]["tournamentId"]=json!("wrong"),"credentials"=>bad["credentials"]=json!("/different"),"unknown"=>bad["unapprovedFutureField"]=json!(true),
                    "reviewpath"=>bad["fundamentalPolicy"]["reviews"][0]["path"]=json!("/different-review"),"authority"=>bad["fundamentalPolicy"]["authoritySha256"]=json!("c".repeat(64)),
                    "source"=>bad["fundamentalPolicy"]["reviews"][0]["sourceFiles"][0]["sha256"]=json!("c".repeat(64)),"model"=>bad["fundamentalPolicy"]["reviews"][0]["fileSha256"]=json!("c".repeat(64)),
                    "binary"=>bad["activation"]["binarySha256"]=json!("not-a-hash"),_=>{bad["activation"].as_object_mut().unwrap().remove("binarySha256");}
                }
                assert!(reload::Active::recover(f.store.as_ref().unwrap(),&serde_json::to_vec(&bad).unwrap()).is_err(),"{case}");
            }
            // Same-original-base behavior is unchanged, and never upgrades the
            // recorded request to the new binary/control's authority.
            let(active,_)=reload::Active::recover(f.store.as_ref().unwrap(),&initial).unwrap().unwrap();assert_eq!(active.recovery_receipt()["binaryOnlyRebase"],false);assert_eq!(f.store.as_ref().unwrap().active_generation(),Some(&metadata));
        }
        #[test]fn active_pins_detect_mutation_and_financial_control_diff_is_never_reloadable(){
            let mut f=F::new();let r=revised(&f);let(settings,proof)=commit(&mut f,&r,NOW+10);let loaded=proof.load(&base()).unwrap();let active=reload::Active::installed(f.store.as_ref().unwrap(),proof,&loaded).unwrap();active.validate().unwrap();std::fs::write(&settings.reviews[0].source_files[0].path,b"mutated sourcebytes").unwrap();assert!(active.validate().is_err(),"source mutation changes pinned Unix metadata");
            let mut next:Value=serde_json::from_slice(&base()).unwrap();next["fundamentalPolicy"]=serde_json::to_value(settings).unwrap();next["activation"]["maximumCommitted"]=json!("1001.000000");assert!(reload::only_fundamental_changed(&base(),&serde_json::to_vec(&next).unwrap()).is_err());
        }
        #[test]fn opaque_generation_revokes_old_preparation_but_preserves_known_cancel_path(){
            use moloch_live_runtime::authority::{ProductionAuthority,Activation};use baal_transport_service::Authority;
            let mut f=F::new();let id=f.fill_entry(true);let original=f.bank.engine().order_records().into_iter().find(|q|q["id"]==id).unwrap();
            let binary=f.root.join("fixture-binary");std::fs::write(&binary,b"offline fixture executable bytes").unwrap();let control=f.root.join("base-control.json");std::fs::write(&control,base()).unwrap();
            let authority=ProductionAuthority::new(Activation{run_id:"fundamental-fixture".into(),entry_until_ms:NOW+3600000,maximum_committed:"1000.000000".into(),maximum_market_cost:"100.000000".into(),maximum_quantity:2,tournament_id:f.scope.tournament_id.clone(),tournament_slug:f.scope.tournament_slug.clone(),binary_sha256:sha(b"offline fixture executable bytes")},f.scope.clone(),vec![],control,binary,f.owner.clone()).unwrap();
            authority.require_fundamental_exchanges(&f.store.as_ref().unwrap().managed_exchanges()).unwrap();authority.bind_owned("201".into());
            let spec=RequestSpec{method:"DELETE".into(),endpoint:"/orders/201".into(),body:None,body_json:None};let mut permit=f.permit(&original,NOW+10);permit.intent_id=format!("{id}-cancel");let mut packet=facts(&f,NOW+10);packet.open_orders.data["data"]=json!([{"id":"201","exchangeId":"101"}]);
            authority.prepare_cancel(&f.bank,id,spec.clone(),permit.clone(),packet.clone()).unwrap();authority.authorize(&f.scope,&spec,&permit,true,NOW+10).unwrap();
            let r=f.review.clone();let settings=files(&f,&r);let proof=approved(&f,&settings,NOW+10);let loaded=proof.load(&base()).unwrap();f.store.as_mut().unwrap().install_generation(&proof,&loaded,&f.bank,&f.log,&f.scope,&packet,&base(),NOW+10,||Ok(())).unwrap();let active=reload::Active::installed(f.store.as_ref().unwrap(),proof,&loaded).unwrap();
            authority.begin_fundamental_generation();assert!(authority.authorize(&f.scope,&spec,&permit,true,NOW+10).is_err());authority.install_fundamental_generation(active).unwrap();assert!(authority.authorize(&f.scope,&spec,&permit,true,NOW+10).is_err(),"old prepared capability stays cleared");
            authority.prepare_cancel(&f.bank,id,spec.clone(),permit.clone(),packet).unwrap();authority.authorize(&f.scope,&spec,&permit,true,NOW+10).unwrap();
        }
        #[test]fn final_guard_failure_and_torn_generation_never_publish_partial_review_state(){
            let mut f=F::new();let r=revised(&f);let settings=files(&f,&r);let proof=approved(&f,&settings,NOW+10);let loaded=proof.load(&base()).unwrap();let facts=facts(&f,NOW+10);let before=std::fs::read(f.root.join("fundamental.ndjson")).unwrap();
            assert!(f.store.as_mut().unwrap().install_generation(&proof,&loaded,&f.bank,&f.log,&f.scope,&facts,&base(),NOW+10,||Err("synthetic final account generation changed".into())).is_err());assert_eq!(before,std::fs::read(f.root.join("fundamental.ndjson")).unwrap());assert!(!f.store.as_ref().unwrap().generation_poisoned());assert_eq!(f.store.as_ref().unwrap().generation_sha(),reload::ZERO);
            f.store.as_mut().unwrap().install_generation(&proof,&loaded,&f.bank,&f.log,&f.scope,&facts,&base(),NOW+10,||Ok(())).unwrap();let mut committed=std::fs::read(f.root.join("fundamental.ndjson")).unwrap();f.store.take();committed.pop();std::fs::write(f.root.join("fundamental.ndjson"),committed).unwrap();assert!(Cohorts::open(&f.root,&f.bank,&f.log,f.owner.clone()).is_err());
        }
        #[test]fn recovery_uses_stable_held_lease_identity_not_original_pid_body_length(){
            use std::io::Write;let mut f=F::new();let lock_path=f.root.join("owner.lock");std::fs::OpenOptions::new().append(true).open(&lock_path).unwrap().write_all(b"    ").unwrap();
            let r=revised(&f);let(_,proof)=commit(&mut f,&r,NOW+10);let previous_bytes=f.owner.descriptor_identity().unwrap()["bytes"].clone();drop(proof);f.store.take();
            let temporary=Arc::new(ExclusiveLock::acquire(&f.root.join("temporary.lock")).unwrap());drop(std::mem::replace(&mut f.owner,temporary));f.owner=Arc::new(ExclusiveLock::acquire(&lock_path).unwrap());assert_ne!(previous_bytes,f.owner.descriptor_identity().unwrap()["bytes"]);
            f.reopen();assert!(reload::Active::recover(f.store.as_ref().unwrap(),&base()).unwrap().is_some());
        }
        #[test]fn expired_request_and_original_account_packet_are_not_extended(){
            let mut f=F::new();let r=revised(&f);let settings=files(&f,&r);let proof=approved(&f,&settings,NOW+10);let loaded=proof.load(&base()).unwrap();let facts=facts(&f,NOW+10);
            assert!(proof.validate(&f.bank,&f.log,&f.owner,&base(),reload::ZERO,NOW+60010).is_err());
            assert!(f.store.as_mut().unwrap().install_generation(&proof,&loaded,&f.bank,&f.log,&f.scope,&facts,&base(),NOW+20011,||Ok(())).is_err());assert_eq!(f.store.as_ref().unwrap().generation_sha(),reload::ZERO);
        }
    }

}
