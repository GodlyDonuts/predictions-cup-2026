//! Actual adoption is the source of the managed REST capacity. The immutable
//! constructor policy still describes the fresh-market allocation.
use moloch_replay::engine::Engine;
use serde_json::{Value,json};

fn original_policy()->Value{json!({"initialCash":"40000.000000","maximumCommitted":"40000.000000","maximumMarketCost":"10000.000000","maximumMarkets":3,"quantity":5,"latencyMs":3000,"lifetimeMs":120000,"bookAgeMs":10000,"maximumSourceSilenceMs":60000,"writeGapMs":2500,"writesPerMinute":24,"inventoryExitMode":"fixedGain","maximumInventoryAgeMs":60000,"inventoryMinimumGain":"0.005000","inventoryAgeClock":"execution"})}
fn adoption()->Value{
    let positions:Vec<_>=(0..28).map(|i|json!({"exchangeId":(900+i).to_string(),"marketId":(100+i).to_string(),"quantity":5,"lotPrincipal":"2.000000","reportedCostBasis":"2.000000","reportedMinusLotPrincipal":"0.000000","basisClassification":"exact","lots":[{"lotId":(7000+i).to_string(),"quantity":5,"side":"YES","entryPrice":"0.400000","openedAt":"2026-10-09T02:00:00.1234567+00:00"}]})).collect();
    json!({"type":"actual_position_adoption","source":"official_sig_api_v1","authority":"human_moloch_inherited_management_20261015","complete":true,"tournamentId":"cup","receiptHash":"a".repeat(64),"requestStartedAt":"2026-10-15T00:00:00Z","receivedAt":"2026-10-15T00:00:00.001Z","processedAt":"2026-10-15T00:00:00.002Z","openingCost":"56.000000","effectiveMaximumMarkets":31,"positions":positions})
}
fn book(exchange:&str,market:&str,at:&str)->Value{json!({"type":"book","receivedAt":at,"book":{"tournamentId":"cup","marketId":market,"exchangeId":exchange,"requestStartedAt":at,"receivedAt":at,"sourceAt":at,"sequence":"1","bids":[{"price":"0.200000","quantity":100}],"asks":[{"price":"0.300000","quantity":100}]}})}
fn adopted()->Engine{let mut e=Engine::new_live(&original_policy()).unwrap();e.advance(&adoption()).unwrap();e}

#[test]
fn twenty_eight_actual_positions_expand_managed_capacity_but_preserve_risk_allocation(){
    let original=original_policy();let e=adopted();let report=e.report().unwrap();
    assert_eq!(original["maximumMarkets"],3);
    assert_eq!(e.policy()["maximumMarkets"],31);
    assert_eq!(report["inheritedInventory"]["freshMarketLimit"],3);
    assert_eq!(e.required_exchanges().len(),28);
    assert!(e.required_exchanges().len()>original["maximumMarkets"].as_u64().unwrap() as usize);
    assert!(e.required_exchanges().len()<=e.policy()["maximumMarkets"].as_u64().unwrap() as usize);
    for field in ["initialCash","maximumCommitted","maximumMarketCost","quantity","writeGapMs","writesPerMinute","maximumInventoryAgeMs"]{assert_eq!(e.policy()[field],original[field],"adoption changed {field}");}
    assert_eq!(report["openingInventoryCost"],"56.000000");
    assert_eq!(report["cash"],"39944.000000");
    assert_eq!(report["orders"],0);assert_eq!(report["fills"],0);e.wallet.audit().unwrap();
}

#[test]
fn thirty_one_managed_slots_still_allow_only_three_fresh_funded_markets(){
    let mut e=adopted();
    for (exchange,market,at) in [("2000","200","2026-10-15T00:00:01Z"),("2001","201","2026-10-15T00:00:04Z"),("2002","202","2026-10-15T00:00:07Z"),("2003","203","2026-10-15T00:00:10Z")]{e.advance(&book(exchange,market,at)).unwrap();}
    let required=e.required_exchanges();let fresh:Vec<_>=required.iter().filter(|ex|ex.parse::<u32>().unwrap()>=2000).collect();
    assert_eq!(fresh.len(),3);assert_eq!(required.len(),31);
    assert!(required.contains("2000")&&required.contains("2001")&&required.contains("2002"));assert!(!required.contains("2003"));
    assert_eq!(e.policy()["maximumMarkets"],31);assert_eq!(e.report().unwrap()["inheritedInventory"]["freshMarketLimit"],3);
    assert!(e.order_records().iter().all(|q|q["exchangeId"]!="2003"));
    assert_eq!(e.report().unwrap()["fills"],0);e.wallet.audit().unwrap();
}
