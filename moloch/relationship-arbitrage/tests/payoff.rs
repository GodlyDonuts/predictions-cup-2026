use moloch_relationship_arbitrage::*;

fn source() -> Source { Source { sha256:"a".repeat(64),received_ms:1000 } }
fn contract(n: usize) -> Contract {
    Contract { exchange_id:format!("{}",n+1),market_id:format!("{}",n+11),contract_id:format!("contract-{n}"),settlement:Settlement::Verified {rules:source(),complete:true,exceptions:vec![]} }
}
fn declaration(logic: Logic) -> Declaration {
    Declaration { relationship_id:"00000000-0000-4000-8000-000000000001".into(),version:1,active:true,source:source(),settlement_logic_source:Some(source()),logic }
}
fn component(logic: Logic) -> Compiled { compile(vec![contract(0),contract(1)],&[declaration(logic)],1000,100).unwrap() }
fn exclusion() -> Compiled { component(Logic::MutuallyExclusive {members:vec![0,1],exhaustive:false}) }
fn ins(contract: usize,side: Side) -> Instrument { Instrument{contract,side} }
fn levels(p: i128) -> Vec<Level> { vec![Level{price:p,quantity:100}] }
fn books() -> Vec<Book> {
    (0..2).map(|i|Book {exchange_id:format!("{}",i+1),source:source(),yes_asks:levels(600_000),no_asks:levels(if i==0 {450_000}else{505_000}),yes_bids:levels(595_000),no_bids:levels(if i==0 {445_000}else{500_000})}).collect()
}
fn limits() -> Limits {
    Limits {now_ms:1000,max_book_age_ms:20,max_book_skew_ms:10,cash_available:100*UNIT,max_package_cost:100*UNIT,max_orphan_cash_loss:100*UNIT,fee_per_share_per_leg:0,buffer_per_share:5000,minimum_surplus:1,max_quantity:100}
}
fn dual_no() -> Vec<Instrument> {vec![ins(0,Side::No),ins(1,Side::No)]}

#[test] fn exclusion_no_floor_does_not_need_exhaustiveness() {
    let c=exclusion(); assert_eq!(c.states.len(),3);
    let p=c.evaluate(&dual_no(),10,&books(),&limits()).unwrap();
    assert_eq!(p.terminal_floor,10*UNIT); assert_eq!(p.acquisition_cost,9_550_000);
    assert_eq!(p.guaranteed_surplus,400_000); assert_eq!(p.worst_orphan_cash_loss,9_550_000);
}
#[test] fn third_party_state_blocks_yes_pair() {
    assert_eq!(exclusion().evaluate(&[ins(0,Side::Yes),ins(1,Side::Yes)],1,&books(),&limits()).unwrap_err(),"NO_GUARANTEED_SURPLUS");
}
#[test] fn exhaustive_yes_pair_certified_only_with_full_logic() {
    let c=component(Logic::MutuallyExclusive {members:vec![0,1],exhaustive:true});
    let mut b=books(); b[0].yes_asks=levels(450_000); b[1].yes_asks=levels(505_000);
    assert_eq!(c.evaluate(&[ins(0,Side::Yes),ins(1,Side::Yes)],1,&b,&limits()).unwrap().guaranteed_surplus,40_000);
}
#[test] fn implication_uses_correct_direction() {
    let c=component(Logic::Implication {antecedent:0,consequent:1});
    let mut b=books();b[1].yes_asks=levels(500_000);
    assert_eq!(c.evaluate(&[ins(0,Side::No),ins(1,Side::Yes)],1,&b,&limits()).unwrap().terminal_floor,UNIT);
    assert!(c.evaluate(&[ins(0,Side::Yes),ins(1,Side::No)],1,&b,&limits()).is_err());
}
#[test] fn no_cancellation_case_is_omitted() {
    let mut a=contract(0);
    a.settlement=Settlement::Verified{rules:source(),complete:true,exceptions:vec![(0,0)]};
    let c=compile(vec![a,contract(1)],&[declaration(Logic::MutuallyExclusive{members:vec![0,1],exhaustive:false})],1000,100).unwrap();
    assert_eq!(c.states.len(),5);
    assert!(c.evaluate(&dual_no(),1,&books(),&limits()).is_err());
}
#[test] fn half_refunds_reduce_the_floor() {
    let mut contracts=vec![contract(0),contract(1)];
    for c in &mut contracts {c.settlement=Settlement::Verified{rules:source(),complete:true,exceptions:vec![(500_000,500_000)]};}
    let c=compile(contracts,&[declaration(Logic::Complement{a:0,b:1})],1000,100).unwrap();
    // One cancelled contract plus YES on the other gives dual-NO payout .5.
    assert!(c.evaluate(&dual_no(),1,&books(),&limits()).is_err());
}
#[test] fn incomplete_rules_rejected() {
    let mut a=contract(0);a.settlement=Settlement::Unknown;
    assert_eq!(compile(vec![a],&[],1000,100).unwrap_err(),"SETTLEMENT_UNKNOWN");
    let mut a=contract(0);a.settlement=Settlement::Verified{rules:source(),complete:false,exceptions:vec![]};
    assert_eq!(compile(vec![a],&[],1000,100).unwrap_err(),"SETTLEMENT_INCOMPLETE");
}
#[test] fn refund_equal_to_ordinary_payout_is_still_exceptional() {
    let mut a=contract(0);
    a.settlement=Settlement::Verified{rules:source(),complete:true,exceptions:vec![(0,UNIT)]};
    let c=compile(vec![a,contract(1)],&[declaration(Logic::Complement{a:0,b:1})],1000,100).unwrap();
    let mut b=books();b[0].yes_asks=levels(400_000);b[1].yes_asks=levels(400_000);
    assert!(c.evaluate(&[ins(0,Side::Yes),ins(1,Side::Yes)],1,&b,&limits()).is_err());
}
#[test] fn active_price_constraint_is_not_a_settlement_certificate() {
    let mut d=declaration(Logic::Complement{a:0,b:1});d.settlement_logic_source=None;
    assert_eq!(compile(vec![contract(0),contract(1)],&[d],1000,100).unwrap_err(),"SETTLEMENT_LOGIC_UNVERIFIED");
}
#[test] fn unsupported_enums_rejected() {
    assert_eq!(compile(vec![contract(0),contract(1)],&[declaration(Logic::Unsupported)],1000,100).unwrap_err(),"UNSUPPORTED_LOGIC");
}
#[test] fn self_or_outside_references_rejected() {
    for logic in [Logic::Complement{a:0,b:0},Logic::Implication{antecedent:0,consequent:3}] {
        assert_eq!(compile(vec![contract(0),contract(1)],&[declaration(logic)],1000,100).unwrap_err(),"LOGIC_REFERENCE");
    }
}
#[test] fn contradictory_graph_rejected() {
    let a=declaration(Logic::Complement{a:0,b:1});
    let mut b=declaration(Logic::Implication{antecedent:0,consequent:1});b.relationship_id="00000000-0000-4000-8000-000000000002".into();
    let mut d=declaration(Logic::Implication{antecedent:1,consequent:0});d.relationship_id="00000000-0000-4000-8000-000000000003".into();
    assert_eq!(compile(vec![contract(0),contract(1)],&[a,b,d],1000,100).unwrap_err(),"INCONSISTENT_LOGIC");
}
#[test] fn declared_scope_is_complete() {
    assert_eq!(compile(vec![contract(0),contract(1)],&[],1000,100).unwrap_err(),"MISSING_RELATIONSHIPS");
    assert_eq!(compile(vec![contract(0),contract(1),contract(2)],&[declaration(Logic::Complement{a:0,b:1})],1000,100).unwrap_err(),"UNRELATED_CONTRACT");
}
#[test] fn and_and_or_truth_tables() {
    for logic in [Logic::And{inputs:vec![0,1],output:2},Logic::Or{inputs:vec![0,1],output:2}] {
        let c=compile(vec![contract(0),contract(1),contract(2)],&[declaration(logic)],1000,100).unwrap();
        assert_eq!(c.states.len(),4);
    }
    assert_eq!(component(Logic::Not{input:0,output:1}).states.len(),2);
}
#[test] fn three_leg_exhaustive_package() {
    let c=compile(vec![contract(0),contract(1),contract(2)],&[declaration(Logic::MutuallyExclusive{members:vec![0,1,2],exhaustive:true})],1000,100).unwrap();
    let mut b=books();b.push(b[0].clone());b[2].exchange_id="3".into();for x in &mut b{x.yes_asks=levels(300_000);}
    let p=c.evaluate(&[ins(0,Side::Yes),ins(1,Side::Yes),ins(2,Side::Yes)],10,&b,&limits()).unwrap();
    assert_eq!(p.guaranteed_surplus,950_000);
}
#[test] fn single_market_complement_is_supported() {
    let c=compile(vec![contract(0)],&[],1000,100).unwrap();
    let mut b=books();b.truncate(1);b[0].yes_asks=levels(500_000);
    assert_eq!(c.evaluate(&[ins(0,Side::Yes),ins(0,Side::No)],1,&b,&limits()).unwrap().terminal_floor,UNIT);
}
#[test] fn full_depth_changes_economics() {
    let mut b=books();b[1].no_asks=vec![Level{price:505_000,quantity:1},Level{price:600_000,quantity:10}];
    assert!(exclusion().evaluate(&dual_no(),1,&b,&limits()).is_ok());
    assert_eq!(exclusion().evaluate(&dual_no(),10,&b,&limits()).unwrap_err(),"NO_GUARANTEED_SURPLUS");
}
#[test] fn insufficient_depth_does_not_extrapolate() {
    assert_eq!(depth_cost(&levels(500_000),101,true).unwrap_err(),"INSUFFICIENT_DEPTH");
}
#[test] fn malformed_unused_depth_also_rejected() {
    assert_eq!(depth_cost(&[Level{price:500_000,quantity:1},Level{price:-1,quantity:1}],1,true).unwrap_err(),"DEPTH_LEVEL");
    assert_eq!(depth_cost(&[Level{price:500_000,quantity:1},Level{price:400_000,quantity:1}],1,true).unwrap_err(),"DEPTH_ORDER");
}
#[test] fn fees_and_buffer_cannot_be_ignored() {
    let mut l=limits();l.fee_per_share_per_leg=25_000;
    assert_eq!(exclusion().evaluate(&dual_no(),1,&books(),&l).unwrap_err(),"NO_GUARANTEED_SURPLUS");
    l.fee_per_share_per_leg=0;l.buffer_per_share=45_000;
    assert_eq!(exclusion().evaluate(&dual_no(),1,&books(),&l).unwrap_err(),"NO_GUARANTEED_SURPLUS");
}
#[test] fn real_cash_and_gross_package_caps_bind() {
    let mut l=limits();l.cash_available=UNIT;
    assert_eq!(exclusion().evaluate(&dual_no(),2,&books(),&l).unwrap_err(),"PACKAGE_CAPITAL");
    l.cash_available=100*UNIT;l.max_package_cost=UNIT;
    assert_eq!(exclusion().evaluate(&dual_no(),2,&books(),&l).unwrap_err(),"PACKAGE_CAPITAL");
}
#[test] fn gross_orphan_bound_survives_evaporated_bids_and_partial_fills() {
    let mut b=books();b[0].no_bids.clear();let mut l=limits();
    let p=exclusion().evaluate(&dual_no(),10,&b,&l).unwrap();
    assert_eq!(p.observed_orphan_unwind_loss,None);
    assert_eq!(p.worst_orphan_cash_loss,p.acquisition_cost+p.fees);
    l.max_orphan_cash_loss=p.acquisition_cost-1;
    assert_eq!(exclusion().evaluate(&dual_no(),10,&b,&l).unwrap_err(),"ORPHAN_CASH_BOUND");
}
#[test] fn observed_liquidation_loss_is_diagnostic() {
    let p=exclusion().evaluate(&dual_no(),10,&books(),&limits()).unwrap();
    assert_eq!(p.observed_orphan_unwind_loss,Some(50_000));
    assert!(p.worst_orphan_cash_loss>p.observed_orphan_unwind_loss.unwrap());
}
#[test] fn stale_future_and_skewed_books_rejected() {
    let c=exclusion();let mut b=books();b[0].source.received_ms=979;
    assert_eq!(c.evaluate(&dual_no(),1,&b,&limits()).unwrap_err(),"SOURCE_STALE");
    b[0].source.received_ms=1001;
    assert_eq!(c.evaluate(&dual_no(),1,&b,&limits()).unwrap_err(),"SOURCE_STALE");
    b[0].source.received_ms=985;
    assert_eq!(c.evaluate(&dual_no(),1,&b,&limits()).unwrap_err(),"BOOK_SKEW");
}
#[test] fn stale_compiled_proofs_rejected() {
    let mut l=limits();l.now_ms=1101;
    assert_eq!(exclusion().evaluate(&dual_no(),1,&books(),&l).unwrap_err(),"COMPILED_STALE");
}
#[test] fn receipt_and_exchange_scope_not_titles() {
    let mut b=books();b[0].exchange_id="wrong".into();
    assert_eq!(exclusion().evaluate(&dual_no(),1,&b,&limits()).unwrap_err(),"BOOK_SCOPE");
    b=books();b[0].source.sha256="missing".into();
    assert_eq!(exclusion().evaluate(&dual_no(),1,&b,&limits()).unwrap_err(),"SOURCE_HASH");
}
#[test] fn duplicate_and_inactive_declarations_rejected() {
    let d=declaration(Logic::Complement{a:0,b:1});
    assert_eq!(compile(vec![contract(0),contract(1)],&[d.clone(),d],1000,100).unwrap_err(),"DECLARATION_ID_STATUS");
    let mut d=declaration(Logic::Complement{a:0,b:1});d.active=false;
    assert_eq!(compile(vec![contract(0),contract(1)],&[d],1000,100).unwrap_err(),"DECLARATION_ID_STATUS");
}
#[test] fn bounded_scan_reproducible_and_ranked() {
    let c=exclusion();let p=c.scan(&[1,10],&books(),&limits()).unwrap();
    assert!(!p.is_empty());assert!(p.windows(2).all(|w|w[0].guaranteed_surplus>=w[1].guaranteed_surplus));
    assert_eq!(p[0].quantity,10);
    assert_eq!(c.scan(&[1,1],&books(),&limits()).unwrap_err(),"SCAN_QUANTITY");
}
#[test] fn marginal_portfolio_value_recognizes_completing_leg() {
    let c=exclusion();let holding=vec![(ins(0,Side::No),5)];
    assert_eq!(c.portfolio_floor(&holding).unwrap(),0);
    assert_eq!(c.marginal_floor(&holding,ins(1,Side::No),5).unwrap(),5*UNIT);
    assert_eq!(c.marginal_floor(&holding,ins(0,Side::No),5).unwrap(),0);
}
#[test] fn extreme_quantity_does_not_wrap() {
    let mut l=limits();l.max_quantity=u64::MAX;
    let p=exclusion().evaluate(&dual_no(),u64::MAX,&books(),&l);
    assert_eq!(p.unwrap_err(),"INSUFFICIENT_DEPTH");
}
