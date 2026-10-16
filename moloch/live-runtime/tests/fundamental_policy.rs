use moloch_live_runtime::{daily_windows, dispatch, feedback, package_admission,fundamental_policy};
use fundamental_policy::*;
use baal_transport_service::{RequestSpec,Scope};
use moloch_replay::session::{ExclusiveLock,Session};
use serde_json::{Value,json};
use sha2::{Digest,Sha256};
use std::{collections::BTreeSet,path::PathBuf,sync::Arc};
const NOW:u64=1_792_108_800_000;
const ORIGINALS:[&[u8];3]=[b"synthetic forecast fixture, not a live publisher",b"synthetic official ballot fixture",b"synthetic official structured contract fixture"];
fn sha(b:&[u8])->String{format!("{:x}",Sha256::digest(b))}
fn iso(at:u64)->String{daily_windows::iso(at).unwrap()}
fn input(manifest:&str,at:u64)->ReviewInput{
    ReviewInput{version:1,reviewer_id:"test-operator".into(),assumption_label:"Synthetic fixture: model probability and Republican-party mapping assumed; not a guaranteed payoff".into(),
        run_id:"fundamental-fixture".into(),bank_manifest_sha256:manifest.into(),tournament_id:"cup".into(),market_id:"201".into(),exchange_id:"101".into(),contract_id:"301".into(),side:"NO".into(),
        ordinary_probability:"0.950000".into(),exceptional_probability_allowance:"0.010000".into(),minimum_exceptional_payout:"0.000000".into(),margin_per_share:"0.010000".into(),maximum_principal:"4.000000".into(),maximum_quantity:5,
        reviewed_at_ms:at,expires_at_ms:at+120000,hold_until_ms:NOW+600000,payout_mapping_is_operator_assumption:true,revoke_existing:false,
        sources:["forecast","official_ballot","official_contract_node"].iter().enumerate().map(|(i,k)|SourceRef{kind:k.to_string(),url:format!("https://fixture.invalid/{k}"),sha256:sha(ORIGINALS[i]),published_at_ms:at-100,observed_at_ms:at-50,maximum_age_ms:120000}).collect()}
}
fn approve(r:ReviewInput,trust:&mut BTreeSet<String>,now:u64)->Reviewed{
    trust.insert(review_sha256(&r).unwrap());
    let hashes=ORIGINALS.iter().map(|b|sha(b)).collect::<Vec<_>>();
    let sources=ORIGINALS.iter().enumerate().map(|(i,b)|SourceBytes{sha256:&hashes[i],bytes:b}).collect::<Vec<_>>();
    Reviewed::verify(r,&sources,trust,now).unwrap()
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

#[cfg(unix)]mod actual_session{
    use super::*;
    struct F{root:PathBuf,bank:Session,log:dispatch::DispatchLog,owner:Arc<ExclusiveLock>,store:Option<Cohorts>,scope:Scope,trust:BTreeSet<String>,seq:u64,review:ReviewInput}
    impl Drop for F{fn drop(&mut self){self.store.take();let _=std::fs::remove_dir_all(&self.root);}}
    impl F{
        fn new()->Self{
            static NEXT:std::sync::atomic::AtomicU64=std::sync::atomic::AtomicU64::new(0);
            let root=std::env::temp_dir().join(format!("fundamental-fixture-{}-{}-{}",std::process::id(),std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos(),NEXT.fetch_add(1,std::sync::atomic::Ordering::Relaxed)));
            std::fs::create_dir(&root).unwrap();let owner=Arc::new(ExclusiveLock::acquire(&root.join("owner.lock")).unwrap());
            let policy=json!({"initialCash":"1000.000000","maximumCommitted":"1000.000000","maximumMarketCost":"100.000000","maximumMarkets":3,"quantity":2,"minimumSpreadTicks":2,"entryStopAt":iso(NOW+3600000),"inventoryExitMode":"fixedGain","maximumInventoryAgeMs":60000,"inventoryMinimumGain":"0.005000","inventoryAgeClock":"execution"});
            let seed=json!({"runId":"fundamental-fixture","availableCash":"1000.000000","accountSnapshotHash":"a".repeat(64),"observedAt":iso(NOW-10000)});
            let bank=Session::open_live(&root.join("bank"),&policy,&seed).unwrap();let log=dispatch::DispatchLog::open(&root.join("dispatch.ndjson")).unwrap();
            let review=input(bank.manifest_hash().as_str().unwrap(),NOW);let scope=Scope{tournament_id:"cup".into(),tournament_slug:"cup".into(),exchanges:[("101".into(),"201".into())].into()};
            let mut f=Self{root,bank,log,owner,store:None,scope,trust:BTreeSet::new(),seq:0,review};
            f.book(NOW-5000,"0.100000","0.120000");f.book(NOW-4000,"0.100000","0.120000");
            f.store=Some(Cohorts::initialize(&f.root,&f.bank,&f.log,f.owner.clone()).unwrap());f.install(f.review.clone(),NOW);f
        }
        fn install(&mut self,r:ReviewInput,at:u64){let reviewed=approve(r,&mut self.trust,at);self.store.as_mut().unwrap().install(reviewed,&self.bank,&self.log,at).unwrap();}
        fn book(&mut self,at:u64,bid:&str,ask:&str){self.seq+=1;self.bank.accept("fixture-books",self.seq,&json!({"type":"book","receivedAt":iso(at),"book":{"marketId":"201","exchangeId":"101","tournamentId":"cup","sequence":self.seq.to_string(),"sourceAt":iso(at),"requestStartedAt":iso(at),"receivedAt":iso(at),"bids":[{"price":bid,"quantity":100}],"asks":[{"price":ask,"quantity":100}]}})).unwrap();}
        fn entry(&self)->Value{self.bank.engine().order_records().into_iter().find(|q|q["side"]=="NO"&&q["action"]=="buy"&&q["retired"]==false).unwrap()}
        fn spec(&self,q:&Value)->RequestSpec{RequestSpec{method:"POST".into(),endpoint:"/orders".into(),body:Some(json!({"idempotencyKey":format!("fundamental-fixture:{}",q["id"].as_u64().unwrap()),"tournamentId":"cup","exchangeId":"101","side":if q["side"]=="NO"{"no"}else{"yes"},"action":q["action"],"quantity":q["quantity"],"price":serde_json::from_str::<Value>(q["price"].as_str().unwrap()).unwrap(),"expirationDate":iso(q["expiresAt"].as_u64().unwrap())})),body_json:None}}
        fn bind_entry(&mut self)->(Value,RequestSpec){let q=self.entry();let spec=self.spec(&q);assert!(matches!(self.store.as_mut().unwrap().bind(&self.bank,&self.log,&self.scope,q["id"].as_u64().unwrap(),&spec,&self.trust,NOW).unwrap(),Decision::Entry{..}));(q,spec)}
        fn log_intent(&mut self,q:&Value,spec:&RequestSpec){let qid=q["id"].as_u64().unwrap();self.log.append(json!({"kind":"intent","qid":qid,"quote":q,"spec":spec,"fundamental":self.store.as_ref().unwrap().durable_tag(qid).unwrap()})).unwrap();self.log.append(json!({"kind":"attempt","qid":qid})).unwrap();}
        fn event(&mut self,e:Value){let n=self.bank.last_source_sequence("fixture-official")+1;self.bank.accept("fixture-official",n,&e).unwrap();}
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
    }
    #[test]fn genuine_native_quote_binds_whole_quantity_and_cannot_be_resized_on_wire(){let mut f=F::new();let q=f.entry();let mut spec=f.spec(&q);spec.body.as_mut().unwrap()["quantity"]=json!(1);let n=q["id"].as_u64().unwrap();assert!(f.store.as_mut().unwrap().bind(&f.bank,&f.log,&f.scope,n,&spec,&f.trust,NOW).is_err());assert!(matches!(f.assess(&q,NOW),Decision::Entry{..}));}
    #[test]fn policy_principal_cap_rejects_whole_quote_instead_of_silently_downsizing(){let mut f=F::new();let mut r=f.review.clone();r.reviewed_at_ms=NOW+1;r.maximum_principal="0.500000".into();f.install(r,NOW+1);assert_eq!(f.assess(&f.entry(),NOW+1),Decision::Hold{reason:"whole_native_quote_exceeds_reviewed_value_or_risk_cap".into()});}
    #[test]fn sixty_one_second_native_loss_exit_is_vetoed_without_changing_financial_engine(){let mut f=F::new();f.fill_entry(false);let(q,at)=f.age_exit(NOW+61003,"0.300000","0.320000");assert_eq!(q["inventoryExitReason"],"maximumAge");assert_eq!(f.assess(&q,at),Decision::Hold{reason:"terminal_value_hold_not_native_age_exit".into()});assert_eq!(f.bank.engine().policy()["maximumInventoryAgeMs"],60000);assert_eq!(f.bank.engine().wallet.quantity("101",moloch_replay::Side::No),2);assert_eq!(f.bank.engine().fills().len(),1);f.reopen();assert!(matches!(f.assess(&q,at),Decision::Hold{..}));}
    #[test]fn stale_review_cannot_buy_but_deadline_still_manages_existing_cohort(){let mut f=F::new();f.fill_entry(false);let(q,at)=f.age_exit(NOW+130000,"0.300000","0.320000");assert_eq!(f.assess(&q,at),Decision::Hold{reason:"stale_review_hold_until_deadline".into()});let(q,at)=f.age_exit(NOW+600001,"0.300000","0.320000");assert!(matches!(f.assess(&q,at),Decision::Exit{reason,..}if reason=="reviewed_holding_deadline"));}
    #[test]fn explicit_reviewed_revocation_allows_only_covered_actual_cohort_exit(){let mut f=F::new();f.fill_entry(false);let(q,at)=f.age_exit(NOW+61003,"0.300000","0.320000");let mut r=input(f.bank.manifest_hash().as_str().unwrap(),at+1);r.revoke_existing=true;f.install(r,at+1);assert!(matches!(f.assess(&q,at+1),Decision::Exit{reason,owned_quantity:2}if reason=="reviewed_source_revocation"));}
    #[test]fn refreshed_value_exit_compares_payout_before_entry_buffer(){let mut f=F::new();f.fill_entry(false);let(q,at)=f.age_exit(NOW+61003,"0.040000","0.060000");assert!(matches!(f.assess(&q,at),Decision::Hold{..}),"NO bid.94 remains below conditional expected payout.9405, even though above entry ceiling.9305");let(q,at)=f.age_exit(at+4000,"0.035000","0.055000");assert!(matches!(f.assess(&q,at),Decision::Exit{reason,..}if reason=="refreshed_value_exit"));}
    #[test]fn partial_late_fill_requires_original_entry_terminal_before_an_exit(){let mut f=F::new();f.fill_entry(true);let(q,at)=f.age_exit(NOW+61003,"0.300000","0.320000");assert_eq!(f.assess(&q,at),Decision::Hold{reason:"prior_owned_order_reconciliation_required".into()});assert_eq!(f.bank.engine().wallet.quantity("101",moloch_replay::Side::No),1);}
    #[test]fn restart_unknown_preserves_binding_and_never_grants_a_retry(){let mut f=F::new();let(q,spec)=f.bind_entry();f.log_intent(&q,&spec);assert!(f.log.require_resolved_attempts().is_err());f.reopen();assert!(f.log.require_resolved_attempts().is_err());assert!(matches!(f.assess(&q,NOW+1),Decision::Hold{..}));assert!(f.store.as_mut().unwrap().bind(&f.bank,&f.log,&f.scope,q["id"].as_u64().unwrap(),&spec,&f.trust,NOW+1).is_err());}
    #[test]fn physical_gate_rechecks_source_request_book_and_original_activation(){let mut f=F::new();let(q,spec)=f.bind_entry();let id=q["id"].as_u64().unwrap();f.store.as_ref().unwrap().check_bound(&f.bank,&f.log,&f.scope,id,&spec,&f.trust,NOW).unwrap();let mut altered=spec.clone();altered.body_json=Some("{}".into());assert!(f.store.as_ref().unwrap().check_bound(&f.bank,&f.log,&f.scope,id,&altered,&f.trust,NOW).is_err());assert!(f.store.as_ref().unwrap().check_bound(&f.bank,&f.log,&f.scope,id,&spec,&BTreeSet::new(),NOW).is_err());assert!(f.store.as_ref().unwrap().check_bound(&f.bank,&f.log,&f.scope,id,&spec,&f.trust,NOW+20000).is_err());}
    #[test]fn clean_prefix_truncation_is_detected_by_original_dispatch_tag(){let mut f=F::new();let(q,spec)=f.bind_entry();f.log_intent(&q,&spec);f.store.take();let p=f.root.join("fundamental.ndjson");let text=std::fs::read_to_string(&p).unwrap();let first=format!("{}\n",text.lines().next().unwrap());std::fs::write(&p,first).unwrap();assert!(Cohorts::open(&f.root,&f.bank,&f.log,f.owner.clone()).is_err());}
    #[test]fn original_dispatch_mutation_and_torn_sidecar_are_not_safe_restarts(){let mut f=F::new();let(q,spec)=f.bind_entry();f.log_intent(&q,&spec);let id=q["id"].as_u64().unwrap();f.log.intents.get_mut(&id).unwrap()["spec"]["body"]["quantity"]=json!(1);assert!(f.store.as_ref().unwrap().assess(&f.bank,&f.log,&f.scope,id,&f.trust,NOW).is_err());f.store.take();let p=f.root.join("fundamental.ndjson");use std::io::Write;let mut file=std::fs::OpenOptions::new().append(true).open(&p).unwrap();file.write_all(b"{").unwrap();drop(file);assert!(Cohorts::open(&f.root,&f.bank,&f.log,f.owner.clone()).is_err());}
    #[test]fn missing_sidecar_or_second_writer_is_not_automatic_initialization(){let mut f=F::new();assert!(Cohorts::open(&f.root,&f.bank,&f.log,f.owner.clone()).is_err());f.store.take();std::fs::remove_file(f.root.join("fundamental.ndjson")).unwrap();assert!(Cohorts::open(&f.root,&f.bank,&f.log,f.owner.clone()).is_err());}
    #[test]fn cohort_horizon_cannot_be_renewed_by_an_ordinary_source_refresh(){let mut f=F::new();f.fill_entry(false);let mut r=input(f.bank.manifest_hash().as_str().unwrap(),NOW+62000);r.hold_until_ms+=1;let reviewed=approve(r,&mut f.trust,NOW+62000);assert!(f.store.as_mut().unwrap().install(reviewed,&f.bank,&f.log,NOW+62000).is_err());}
    #[test]fn changed_review_requests_cancellation_of_known_partial_entry_without_inventing_unknown_id(){let mut f=F::new();let id=f.fill_entry(true);let mut r=input(f.bank.manifest_hash().as_str().unwrap(),NOW+62000);r.revoke_existing=true;f.install(r,NOW+62000);assert_eq!(f.store.as_ref().unwrap().entry_cancellations(&f.bank,&f.log,&f.trust,NOW+62000).unwrap(),vec![id]);let mut unknown=F::new();let(q,spec)=unknown.bind_entry();unknown.log_intent(&q,&spec);assert!(unknown.store.as_ref().unwrap().entry_cancellations(&unknown.bank,&unknown.log,&unknown.trust,NOW+130000).unwrap().is_empty());}
    #[test]fn unrelated_actual_holding_cannot_become_a_tagged_forecast_position(){let mut f=F::new();let q=f.entry();f.event(F::base(&q,"actual_order_ack",NOW+1,"201"));let mut fill=F::base(&q,"actual_fill",NOW+3,"201");fill["fillId"]=json!("401");fill["quantity"]=json!(2);fill["executedAt"]=json!(iso(NOW+2));fill["fee"]=json!("0.000000");f.event(fill);let qid=q["id"].as_u64().unwrap();assert!(f.store.as_ref().unwrap().assess(&f.bank,&f.log,&f.scope,qid,&f.trust,NOW+3).is_err());}
    #[test]fn expired_exit_read_hint_is_not_permanently_blocked_by_inactive_quote_gate(){let mut f=F::new();f.fill_entry(false);let(q,at)=f.age_exit(NOW+61003,"0.300000","0.320000");assert!(!f.store.as_ref().unwrap().exit_refresh_needed(&f.bank,&f.log,&f.scope,"101",&f.trust,at).unwrap());assert!(matches!(f.assess(&q,NOW+600001),Decision::Hold{..}),"old native quote is inactive, even at deadline");assert!(f.store.as_ref().unwrap().exit_refresh_needed(&f.bank,&f.log,&f.scope,"101",&f.trust,NOW+600001).unwrap(),"deadline still enables bounded source refresh before final quote assessment");}
}
