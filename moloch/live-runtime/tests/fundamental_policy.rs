// Use the genuine exported capability type accepted by ProductionAuthority.
use moloch_live_runtime::{daily_windows, dispatch, fundamental_policy};
use fundamental_policy::*;
use baal_transport_service::{RequestSpec,Scope,Permit};
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
        fn slice_spec(&mut self)->(Value,RequestSpec){let q=self.entry();let mut r=self.review.clone();r.reviewed_at_ms=NOW+1;r.maximum_quantity=1;r.maximum_principal=q["price"].as_str().unwrap().into();self.install(r,NOW+1);let mut spec=self.spec(&q);spec.body.as_mut().unwrap()["quantity"]=json!(1);(q,spec)}
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
        fn authority(&self)->ReviewAuthority{let path=self.root.join("review-authority.json");let bytes=serde_json::to_vec(&json!({"version":1,"trustedReviewSha256":self.trust})).unwrap();std::fs::write(&path,&bytes).unwrap();ReviewAuthority::load(&path,&sha(&bytes)).unwrap()}
        fn permit(&self,q:&Value,at:u64)->Permit{Permit{run_id:"fundamental-fixture".into(),intent_id:q["id"].as_u64().unwrap().to_string(),owner:"Codex".into(),expires_at_ms:at+10000,maximum_quantity:q["quantity"].as_u64().unwrap() as u32,maximum_cost:moloch_replay::money(baal_transport_service::decimal_units(q["price"].as_str().unwrap()).unwrap()*q["quantity"].as_u64().unwrap() as i128)}}
    }
    #[test]fn genuine_native_quote_binds_whole_quantity_and_cannot_be_resized_on_wire(){let mut f=F::new();let q=f.entry();let mut spec=f.spec(&q);spec.body.as_mut().unwrap()["quantity"]=json!(1);let n=q["id"].as_u64().unwrap();assert!(f.store.as_mut().unwrap().bind(&f.bank,&f.log,&f.scope,n,&spec,&f.trust,NOW).is_err());assert!(matches!(f.assess(&q,NOW),Decision::Entry{..}));}
    #[test]fn policy_principal_cap_rejects_whole_quote_instead_of_silently_downsizing(){let mut f=F::new();let mut r=f.review.clone();r.reviewed_at_ms=NOW+1;r.maximum_principal="0.500000".into();f.install(r,NOW+1);assert_eq!(f.assess(&f.entry(),NOW+1),Decision::Hold{reason:"whole_native_quote_exceeds_reviewed_value_or_risk_cap".into()});}
    #[test]fn sixty_one_second_native_loss_exit_is_vetoed_without_changing_financial_engine(){let mut f=F::new();f.fill_entry(false);let(q,at)=f.age_exit(NOW+61003,"0.300000","0.320000");assert_eq!(q["inventoryExitReason"],"maximumAge");assert_eq!(f.assess(&q,at),Decision::Hold{reason:"terminal_value_hold_not_native_age_exit".into()});assert_eq!(f.bank.engine().policy()["maximumInventoryAgeMs"],60000);assert_eq!(f.bank.engine().wallet.quantity("101",moloch_replay::Side::No),2);assert_eq!(f.bank.engine().fills().len(),1);f.reopen();assert!(matches!(f.assess(&q,at),Decision::Hold{..}));}
    #[test]fn stale_review_cannot_buy_but_deadline_still_manages_existing_cohort(){let mut f=F::new();f.fill_entry(false);let(q,at)=f.age_exit(NOW+130000,"0.300000","0.320000");assert_eq!(f.assess(&q,at),Decision::Hold{reason:"stale_review_hold_until_deadline".into()});let(q,at)=f.age_exit(NOW+600001,"0.300000","0.320000");assert!(matches!(f.assess(&q,at),Decision::Exit{reason,..}if reason=="reviewed_holding_deadline"));}
    #[test]fn explicit_reviewed_revocation_allows_only_covered_actual_cohort_exit(){let mut f=F::new();f.fill_entry(false);let(q,at)=f.age_exit(NOW+61003,"0.300000","0.320000");let mut r=input(f.bank.manifest_hash().as_str().unwrap(),at+1);r.revoke_existing=true;f.install(r,at+1);assert!(matches!(f.assess(&q,at+1),Decision::Exit{reason,owned_quantity:2}if reason=="reviewed_source_revocation"));}
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
    #[test]fn cohort_horizon_cannot_be_renewed_by_an_ordinary_source_refresh(){let mut f=F::new();f.fill_entry(false);let mut r=input(f.bank.manifest_hash().as_str().unwrap(),NOW+62000);r.hold_until_ms+=1;let reviewed=approve(r,&mut f.trust,NOW+62000);assert!(f.store.as_mut().unwrap().install(reviewed,&f.bank,&f.log,NOW+62000).is_err());}
    #[test]fn changed_review_requests_cancellation_of_known_partial_entry_without_inventing_unknown_id(){let mut f=F::new();let id=f.fill_entry(true);let mut r=input(f.bank.manifest_hash().as_str().unwrap(),NOW+62000);r.revoke_existing=true;f.install(r,NOW+62000);assert_eq!(f.store.as_ref().unwrap().entry_cancellations(&f.bank,&f.log,&f.trust,NOW+62000).unwrap(),vec![id]);let mut unknown=F::new();let(q,spec)=unknown.bind_entry();unknown.log_intent(&q,&spec);assert!(unknown.store.as_ref().unwrap().entry_cancellations(&unknown.bank,&unknown.log,&unknown.trust,NOW+130000).unwrap().is_empty());}
    #[test]fn unrelated_actual_holding_cannot_become_a_tagged_forecast_position(){let mut f=F::new();let q=f.entry();f.event(F::base(&q,"actual_order_ack",NOW+1,"201"));let mut fill=F::base(&q,"actual_fill",NOW+3,"201");fill["fillId"]=json!("401");fill["quantity"]=json!(2);fill["executedAt"]=json!(iso(NOW+2));fill["fee"]=json!("0.000000");f.event(fill);let qid=q["id"].as_u64().unwrap();assert!(f.store.as_ref().unwrap().assess(&f.bank,&f.log,&f.scope,qid,&f.trust,NOW+3).is_err());}
    #[test]fn expired_exit_read_hint_is_not_permanently_blocked_by_inactive_quote_gate(){let mut f=F::new();f.fill_entry(false);let(q,at)=f.age_exit(NOW+61003,"0.300000","0.320000");assert!(!f.store.as_ref().unwrap().exit_refresh_needed(&f.bank,&f.log,&f.scope,"101",&f.trust,at).unwrap());assert!(matches!(f.assess(&q,NOW+600001),Decision::Hold{..}),"old native quote is inactive, even at deadline");assert!(f.store.as_ref().unwrap().exit_refresh_needed(&f.bank,&f.log,&f.scope,"101",&f.trust,NOW+600001).unwrap(),"deadline still enables bounded source refresh before final quote assessment");}
    #[test]fn opaque_token_mints_before_attempt_and_survives_only_its_original_tagged_append(){let mut f=F::new();let(q,spec)=f.bind_entry();let id=q["id"].as_u64().unwrap();let p=f.permit(&q,NOW);let a=f.authority();let store=f.store.as_ref().unwrap();let token=store.physical_decision(&f.bank,&f.log,&f.scope,id,&spec,&p,&a,NOW).unwrap();assert_eq!(token.native_head(),&f.bank.durable_head());assert_eq!(token.review_sha256(),review_sha256(&f.review).unwrap());let report=f.bank.engine().report().unwrap();token.validate_frozen(&q,&report,id,NOW).unwrap();token.authorize(&f.scope,&spec,&p,NOW).unwrap();f.log_intent(&q,&spec);f.store.as_ref().unwrap().check_bound(&f.bank,&f.log,&f.scope,id,&spec,&f.trust,NOW).unwrap();token.authorize(&f.scope,&spec,&p,NOW).unwrap();assert!(f.log.require_resolved_attempts().is_err(),"the policy token never resolves or retries an attempted write");}
    #[test]fn opaque_token_rejects_changed_frozen_economics_scope_permit_and_raw_body(){let mut f=F::new();let(q,spec)=f.bind_entry();let id=q["id"].as_u64().unwrap();let p=f.permit(&q,NOW);let a=f.authority();let token=f.store.as_ref().unwrap().physical_decision(&f.bank,&f.log,&f.scope,id,&spec,&p,&a,NOW).unwrap();let mut report=f.bank.engine().report().unwrap();report["cash"]=json!("999.000000");assert!(token.validate_frozen(&q,&report,id,NOW).is_err());let mut q2=q.clone();q2["quantity"]=json!(1);assert!(token.validate_frozen(&q2,&f.bank.engine().report().unwrap(),id,NOW).is_err());let mut p2=p.clone();p2.maximum_quantity+=1;assert!(token.authorize(&f.scope,&spec,&p2,NOW).is_err());let mut s2=spec.clone();s2.body_json=Some("{}".into());assert!(token.authorize(&f.scope,&s2,&p,NOW).is_err());let mut scope=f.scope.clone();scope.tournament_slug="other".into();assert!(token.authorize(&scope,&spec,&p,NOW).is_err());}
    #[test]fn opaque_token_expires_at_original_official_book_clock_not_permit_only(){let mut f=F::new();let(q,spec)=f.bind_entry();let id=q["id"].as_u64().unwrap();let p=f.permit(&q,NOW);let a=f.authority();let token=f.store.as_ref().unwrap().physical_decision(&f.bank,&f.log,&f.scope,id,&spec,&p,&a,NOW).unwrap();assert!(token.expires_at_ms()<p.expires_at_ms);token.authorize(&f.scope,&spec,&p,token.expires_at_ms()-1).unwrap();assert!(token.authorize(&f.scope,&spec,&p,token.expires_at_ms()).is_err());assert!(token.authorize(&f.scope,&spec,&p,NOW-1).is_err());}
    #[test]fn opaque_token_rejects_same_inode_sidecar_prefix_edit_and_trust_file_mutation(){let mut f=F::new();let(q,spec)=f.bind_entry();let id=q["id"].as_u64().unwrap();let p=f.permit(&q,NOW);let a=f.authority();let token=f.store.as_ref().unwrap().physical_decision(&f.bank,&f.log,&f.scope,id,&spec,&p,&a,NOW).unwrap();let path=f.root.join("fundamental.ndjson");let mut bytes=std::fs::read(&path).unwrap();let pos=bytes.iter().position(|b|*b==b't').unwrap();bytes[pos]=b'z';std::fs::write(&path,&bytes).unwrap();assert!(token.authorize(&f.scope,&spec,&p,NOW).is_err());let mut f=F::new();let(q,spec)=f.bind_entry();let id=q["id"].as_u64().unwrap();let p=f.permit(&q,NOW);let a=f.authority();let token=f.store.as_ref().unwrap().physical_decision(&f.bank,&f.log,&f.scope,id,&spec,&p,&a,NOW).unwrap();std::fs::write(f.root.join("review-authority.json"),b"{}").unwrap();assert!(token.authorize(&f.scope,&spec,&p,NOW).is_err());}
    #[test]fn opaque_token_rejects_replaced_original_dispatch_inode_and_new_review_tail(){let mut f=F::new();let(q,spec)=f.bind_entry();let id=q["id"].as_u64().unwrap();let p=f.permit(&q,NOW);let a=f.authority();let token=f.store.as_ref().unwrap().physical_decision(&f.bank,&f.log,&f.scope,id,&spec,&p,&a,NOW).unwrap();let path=f.root.join("dispatch.ndjson");std::fs::rename(&path,f.root.join("dispatch-original.ndjson")).unwrap();std::fs::write(&path,b"").unwrap();assert!(token.authorize(&f.scope,&spec,&p,NOW).is_err());let mut f=F::new();let(q,spec)=f.bind_entry();let id=q["id"].as_u64().unwrap();let p=f.permit(&q,NOW);let a=f.authority();let token=f.store.as_ref().unwrap().physical_decision(&f.bank,&f.log,&f.scope,id,&spec,&p,&a,NOW).unwrap();let r=input(f.bank.manifest_hash().as_str().unwrap(),NOW+1);f.install(r,NOW+1);assert!(token.authorize(&f.scope,&spec,&p,NOW+1).is_err());}
    #[test]fn only_actual_deadline_exit_can_mint_without_fresh_forecast(){let mut f=F::new();f.fill_entry(false);let(q,at)=f.age_exit(NOW+600001,"0.300000","0.320000");let at=at.max(q["activationAt"].as_u64().unwrap());let spec=f.spec(&q);let id=q["id"].as_u64().unwrap();f.store.as_mut().unwrap().bind(&f.bank,&f.log,&f.scope,id,&spec,&f.trust,at).unwrap();let p=f.permit(&q,at);let a=f.authority();let token=f.store.as_ref().unwrap().physical_decision(&f.bank,&f.log,&f.scope,id,&spec,&p,&a,at).unwrap();token.validate_frozen(&q,&f.bank.engine().report().unwrap(),id,at).unwrap();token.authorize(&f.scope,&spec,&p,at).unwrap();assert!(at>f.review.expires_at_ms);}
    #[test]fn consumed_initialization_marker_blocks_recreation_after_sidecar_loss(){let mut f=F::new();f.store.take();std::fs::remove_file(f.root.join("fundamental.ndjson")).unwrap();assert!(f.root.join("fundamental.initialized.json").exists());assert!(Cohorts::initialize(&f.root,&f.bank,&f.log,f.owner.clone()).is_err());assert!(Cohorts::open(&f.root,&f.bank,&f.log,f.owner.clone()).is_err());}
    fn settings(f:&F)->Settings{let authority=f.authority();let review=f.root.join("review-input.json");let bytes=serde_json::to_vec(&f.review).unwrap();std::fs::write(&review,&bytes).unwrap();let sources=ORIGINALS.iter().enumerate().map(|(i,b)|{let path=f.root.join(format!("source-{i}.bin"));std::fs::write(&path,b).unwrap();SourceFile{path,sha256:sha(b)}}).collect();let authority_path=f.root.join("review-authority.json");Settings{version:1,authority_sha256:sha(&std::fs::read(&authority_path).unwrap()),authority_path,reviews:vec![ReviewFile{path:review,file_sha256:sha(&bytes),source_files:sources}],allow_initialization:false}}
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
        f.reopen();assert_eq!(f.bank.engine().wallet.quantity("101",moloch_replay::Side::No),1);assert_eq!(f.bank.engine().fills().len(),1);
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
            contract:receipt("/contracts/201",json!({"market_id":"201","root":{"node_type":"contract","node_id":"301","contract_id":"301","children":[],"parent_id":null,"contract_type":"Party Winner","contract_details":{}}})),
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
}
