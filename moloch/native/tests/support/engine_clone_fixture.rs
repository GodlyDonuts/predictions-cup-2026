//! Synthetic offline fixture shared by the benchmark and its tests. No clients.
use moloch_replay::{engine::Engine,money};
use serde_json::{Value,json};
use sha2::{Digest,Sha256};
pub const MARKETS:usize=237;
pub const DEPTH:usize=20;
pub const BASE_MS:u64=1_760_544_000_000;
pub const WARM_END_MS:u64=BASE_MS+7500;
pub type Result<T>=std::result::Result<T,Box<dyn std::error::Error>>;
pub fn iso(ms:u64)->String{chrono::DateTime::from_timestamp_millis(ms as i64).unwrap().to_rfc3339_opts(chrono::SecondsFormat::Millis,true)}
pub fn policy()->Value{json!({"initialCash":"40000.000000","maximumCommitted":"40000.000000","maximumMarketCost":"10000.000000","quantity":1000,"maximumMarkets":31,"latencyMs":3000,"lifetimeMs":600000,"bookAgeMs":10000,"maximumSourceSilenceMs":60000,"minimumSpreadTicks":1,"minimumEdge":"0.001000","participationBps":100,"feeBps":0,"feePerShare":"0.000000","quoteMode":"pairedSpread","entriesEnabled":true,"fillModel":"strictThrough","minimumQuoteAgeMs":2500,"writeGapMs":2500,"writesPerMinute":24,"requireCurrentFlow":false})}
pub fn book(index:usize,sequence:u64,at:u64)->Value{
 let bids:Vec<_>=(0..DEPTH).map(|i|json!({"price":money(450000-5000*i as i128),"quantity":1000+i})).collect();
 let asks:Vec<_>=(0..DEPTH).map(|i|json!({"price":money(455000+5000*i as i128),"quantity":1000+i})).collect();
 let at=iso(at);let id=(1000+index).to_string();
 json!({"type":"book","receivedAt":at,"book":{"tournamentId":"synthetic-clone-benchmark","marketId":id,"exchangeId":id,"sequence":sequence.to_string(),"requestStartedAt":at,"sourceAt":at,"receivedAt":at,"transport":"official_sig_realtime","bids":bids,"asks":asks}})
}
pub fn tick(at:u64)->Value{json!({"type":"tick","receivedAt":iso(at)})}
pub fn gap(index:usize,at:u64,duplicate:bool)->Value{let id=(1000+index).to_string();json!({"type":"gap","receivedAt":iso(at),"exchangeIds":if duplicate{vec![id.clone(),id]}else{vec![id]}})}
pub fn warm()->Result<Engine>{
 let mut e=Engine::new_live(&policy())?;
 for i in 0..MARKETS{e.advance(&book(i,1,BASE_MS))?;}
 // Existing native pacing is preserved. These three later ticks populate
 // additional ordinary draft orders while all 237 books remain fresh.
 for offset in [2500,5000,7500]{e.advance(&tick(BASE_MS+offset))?;}
 if e.known_exchanges().len()!=MARKETS||e.order_records().is_empty(){return Err("BENCH_FIXTURE_NOT_WARM".into())}
 Ok(e)
}
pub fn fingerprint(e:&Engine)->Result<String>{
 let snapshot=json!({"report":e.report()?,"orders":e.order_records(),"fills":e.fills(),"knownExchanges":e.known_exchanges(),"requiredExchanges":e.required_exchanges(),"actualOrderBindings":e.live_order_bindings()});
 Ok(format!("{:x}",Sha256::digest(serde_json::to_vec(&snapshot)?)))
}
pub fn workload(kind:&str,n:usize)->Vec<Value>{
 (0..n).map(|i|{let at=WARM_END_MS+1+i as u64;match kind{
  "tick"=>tick(at),
  "book"=>book(i%MARKETS,2+(i/MARKETS) as u64,at),
  "gap_and_resync"=>if i%2==0{gap((i/2)%MARKETS,at,false)}else{book((i/2)%MARKETS,2+(i/(2*MARKETS)) as u64,at)},
  _=>panic!("unsupported synthetic workload"),
 }}).collect()
}
pub fn rollback_probe(e:&mut Engine,at:u64)->Result<Value>{
 let before=fingerprint(e)?;
 // The first gap member is mutated before the second duplicate is rejected.
 let error=e.advance(&gap(0,at,true)).err().ok_or("BENCH_REJECTION_EXPECTED")?;
 let after=fingerprint(e)?;
 if before!=after{return Err("BENCH_TRANSACTIONAL_ROLLBACK_CHANGED".into())}
 Ok(json!({"error":error,"financialStateUnchanged":true,"beforeSha256":before,"afterSha256":after}))
}
