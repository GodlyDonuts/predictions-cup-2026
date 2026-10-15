//! Diagnostic breadth capture. It never authorizes execution or credits fills.
use crate::feedback::Result;
use moloch_replay::{journal::Journal,money};
use baal_transport_service::prices::official_tick_units;
use serde_json::{Value,json};
use sha2::{Digest,Sha256};
use std::{collections::{BTreeMap,BTreeSet,VecDeque},fs::{File,OpenOptions},io::{BufRead,BufReader,Write},path::Path};
pub struct Recorder {file:File,chain:Journal,bytes:u64,pub error:Option<String>}
impl Recorder{
 pub fn new(path:&Path)->Result<Self>{let mut options=OpenOptions::new();options.write(true).create_new(true);
  #[cfg(unix)]{use std::os::unix::fs::OpenOptionsExt;options.mode(0o600);}
  let file=options.open(path)?;file.sync_all()?;File::open(path.parent().ok_or("BREADTH_PARENT")?)?.sync_all()?;Ok(Self{file,chain:Journal::default(),bytes:0,error:None})
 }
 pub fn observe(&mut self,event:&str,payload:&Value,received:&str){
  if self.error.is_some(){return}
  let result=(||->Result<()>{
   let mut row=json!({"sequence":self.chain.sequence+1,"previous":self.chain.previous,"event":{"source":"official_sig_realtime","event":event,"payload":payload,"receivedAt":received}});
   row["hash"]=json!(format!("{:x}",Sha256::digest(serde_json::to_vec(&row)?)));
   let data=serde_json::to_vec(&row)?;
   if data.len()>8*1024*1024||self.bytes+data.len() as u64>128*1024*1024{return Err("BREADTH_CAPTURE_LIMIT".into())}
   self.file.write_all(&data)?;self.file.write_all(b"\n")?;self.file.sync_data()?;self.chain.accept(&row)?;self.bytes+=data.len() as u64+1;Ok(())
  })();if let Err(e)=result{self.error=Some(e.to_string())}
 }
 pub fn status(&self)->Value{json!({"sequence":self.chain.sequence,"previous":self.chain.previous,"bytes":self.bytes,"error":self.error,"executionAuthority":false})}
}
fn instant(v:&Value)->Result<i64>{chrono::DateTime::parse_from_rfc3339(v.as_str().ok_or("BREADTH_CLOCK")?)?.timestamp_nanos_opt().ok_or("BREADTH_CLOCK_RANGE".into())}
fn numeric(v:&Value)->Result<String>{let s=v.as_str().map(str::to_owned).or_else(||v.as_u64().map(|n|n.to_string())).ok_or("BREADTH_ID")?;if s.is_empty()||s.starts_with('0')||!s.bytes().all(|b|b.is_ascii_digit()){return Err("BREADTH_ID".into())}Ok(s)}
#[derive(Clone)]struct Book{bid:i128,ask:i128,bid_qty:u64,ask_qty:u64,source:i64,received:i64,sequence:u64,bid_since:i64,ask_since:i64}
#[derive(Clone)]struct Print{at:i64,side:u8,price:i128,quantity:u64}
#[derive(Default)]struct Market{market:String,exchange:String,revision:Option<u64>,book:Option<Book>,prints:VecDeque<Print>,seen:BTreeMap<String,Value>,gaps:u64,observed:u64,unclassified:u64}
#[derive(Default)]pub struct Study{markets:BTreeMap<String,Market>,last:Option<i64>,frames:u64}
impl Study{
 pub fn accept(&mut self,e:&Value)->Result<()>{
  if e["source"]!="official_sig_realtime"{return Err("BREADTH_SOURCE".into())}let at=instant(&e["receivedAt"])?;if self.last.is_some_and(|old|at<old){return Err("BREADTH_RECEIPT_ORDER".into())}
  self.last=Some(at);self.frames+=1;
  if e["event"]=="stream_reset"{if e["payload"]["localBoundary"]!=true{return Err("BREADTH_RESET_PROOF".into())}for m in self.markets.values_mut(){m.book=None;m.prints.clear();m.revision=None;m.gaps+=1;}return Ok(())}
  if e["event"]!="markets_batch"{for m in self.markets.values_mut().filter(|m|e["payload"]["marketId"]==m.market){m.book=None;m.prints.clear();m.gaps+=1;}return Ok(())}
  let rows=e["payload"]["markets"].as_array().ok_or("BREADTH_BATCH")?;let mut seen=BTreeSet::new();
  for row in rows{let market=numeric(&row["marketId"])?;if !seen.insert(market.clone()){return Err("BREADTH_DUPLICATE_MARKET".into())}
   let b=&row["batch"];let d=&b["delivery"];let revision=d["revision"].as_u64().ok_or("BREADTH_REVISION")?;let previous=d["previousRevision"].as_u64().ok_or("BREADTH_REVISION")?;
   if d["model"]!="best-effort-authoritative-resync"||revision<=previous{return Err("BREADTH_DELIVERY".into())}
   let books=b["books"].as_array();let trades=b["trades"].as_array();
   let mut exchanges=BTreeSet::new();for entry in books.into_iter().flatten().chain(trades.into_iter().flatten()){exchanges.insert(numeric(&entry["exchangeId"])?);}
   for exchange in exchanges{
    let m=self.markets.entry(exchange.clone()).or_default();if !m.market.is_empty()&&m.market!=market{return Err("BREADTH_SCOPE".into())}m.market=market.clone();m.exchange=exchange.clone();
    let duplicate=m.revision.is_some_and(|r|revision<=r);let gap=m.revision.is_some_and(|r|previous>r)||b["resyncRequired"]==true||b["marketSettled"].as_array().is_some_and(|a|!a.is_empty());
    if gap{m.gaps+=1;m.book=None;m.prints.clear();}if !duplicate{m.revision=Some(revision);}
    if !duplicate&&!gap{
     for t in trades.into_iter().flatten().filter(|t|numeric(&t["exchangeId"]).ok().as_deref()==Some(exchange.as_str())){
      if numeric(&t["marketId"])?!=market{return Err("BREADTH_TRADE_SCOPE".into())}
      if t["id"].is_null()||t["sequence"].is_null(){continue}
      let id=t["id"].as_str().ok_or("BREADTH_TRADE_ID")?.to_owned();let executed=instant(&t["executedAt"])?;let price=official_tick_units(&t["price"])?;let qty=t["quantity"].as_u64().filter(|n|*n>0).ok_or("BREADTH_QUANTITY")?;let seq=t["sequence"].as_u64().ok_or("BREADTH_TRADE_SEQUENCE")?;
      if executed>at{return Err("BREADTH_FUTURE_TRADE".into())}
      let facts=json!([market,exchange,t["price"],qty,t["executedAt"],seq]);
      if let Some(old)=m.seen.get(&id){if old!=&facts{return Err("BREADTH_TRADE_CONFLICT".into())}continue}m.seen.insert(id,facts);m.observed+=1;
      let mut side=0;let mut quote=price;
      if let Some(book)=&m.book{if book.sequence<seq&&book.source<executed&&book.received<=executed&&executed-book.source<=10_000_000_000{
       if price<=book.bid&&executed-book.bid_since>=3_000_000_000{side=1;quote=book.bid}else if price>=book.ask&&executed-book.ask_since>=3_000_000_000{side=2;quote=book.ask}
      }}
      if side==0{m.unclassified+=1}m.prints.push_back(Print{at:executed,side,price:quote,quantity:qty});
     }
    }
    for book in books.into_iter().flatten().filter(|b|numeric(&b["exchangeId"]).ok().as_deref()==Some(exchange.as_str())){
     let source=instant(&book["asOf"]["at"])?;let sequence=book["asOf"]["sequence"].as_u64().ok_or("BREADTH_BOOK_SEQUENCE")?;
     if source>at{return Err("BREADTH_FUTURE_BOOK".into())}if m.book.as_ref().is_some_and(|old|old.sequence>sequence){continue}
     let (Some(bid),Some(ask))=(book["bids"].as_array().and_then(|a|a.first()),book["asks"].as_array().and_then(|a|a.first()))else{m.book=None;m.prints.clear();continue};
     let bid_price=official_tick_units(&bid["price"])?;let ask_price=official_tick_units(&ask["price"])?;let bid_qty=bid["quantity"].as_u64().filter(|q|*q>0).ok_or("BREADTH_DEPTH")?;let ask_qty=ask["quantity"].as_u64().filter(|q|*q>0).ok_or("BREADTH_DEPTH")?;if bid_price>=ask_price{return Err("BREADTH_CROSSED".into())}
     if m.book.as_ref().is_some_and(|old|old.sequence==sequence&&(old.bid!=bid_price||old.ask!=ask_price||old.bid_qty!=bid_qty||old.ask_qty!=ask_qty)){return Err("BREADTH_BOOK_CONFLICT".into())}
     let bid_since=m.book.as_ref().filter(|b|b.bid==bid_price).map_or(at,|b|b.bid_since);let ask_since=m.book.as_ref().filter(|b|b.ask==ask_price).map_or(at,|b|b.ask_since);
     m.book=Some(Book{bid:bid_price,ask:ask_price,bid_qty,ask_qty,source,received:at,sequence,bid_since,ask_since});
    }
    m.prints.retain(|t|t.at>=at-180_000_000_000);
   }
  }Ok(())
 }
 pub fn report(&self)->Value{
  let at=self.last.unwrap_or(0);let mut candidates=Vec::new();for m in self.markets.values(){
   let Some(b)=&m.book else{continue};let current:Vec<_>=m.prints.iter().filter(|t|t.at>=at-180_000_000_000).collect();
   let flow=|side:u8|current.iter().filter(|t|t.side==side&&t.price==if side==1{b.bid}else{b.ask}&&t.at>=if side==1{b.bid_since}else{b.ask_since}).map(|t|t.quantity as u128).sum::<u128>();
   let bid=flow(1);let ask=flow(2);let fresh=at>=b.source&&at-b.source<=10_000_000_000;let eligible=fresh&&bid>0&&ask>0;
   let raw_bid=current.iter().filter(|p|p.side==1).map(|p|p.quantity as u128).sum::<u128>();let raw_ask=current.iter().filter(|p|p.side==2).map(|p|p.quantity as u128).sum::<u128>();
   candidates.push(json!({"marketId":m.market,"exchangeId":m.exchange,"bookSourceAgeMs":(at-b.source)/1_000_000,"bid":money(b.bid),"ask":money(b.ask),"spread":money(b.ask-b.bid),"bidDepth":b.bid_qty,"askDepth":b.ask_qty,"observedTrades":m.observed,"recentTrades":current.len(),"recentShares":current.iter().map(|p|p.quantity as u128).sum::<u128>().to_string(),"recentBidCompatibleShares":raw_bid.to_string(),"recentAskCompatibleShares":raw_ask.to_string(),"historicalTwoSidedFlow":fresh&&raw_bid>0&&raw_ask>0,"currentBidCompatibleShares":bid.to_string(),"currentAskCompatibleShares":ask.to_string(),"balancedTurnover":(bid as f64/b.bid_qty as f64).min(ask as f64/b.ask_qty as f64),"eligibleForRestReconciliation":eligible,"historicalGaps":m.gaps,"unclassifiedTrades":m.unclassified}));
  }
   candidates.sort_by(|a,b|b["eligibleForRestReconciliation"].as_bool().cmp(&a["eligibleForRestReconciliation"].as_bool()).then_with(||b["balancedTurnover"].as_f64().unwrap().total_cmp(&a["balancedTurnover"].as_f64().unwrap())).then_with(||b["recentTrades"].as_u64().cmp(&a["recentTrades"].as_u64())));
  json!({"frames":self.frames,"atMs":self.last.map(|t|t/1_000_000),"marketsObserved":self.markets.len(),"candidates":candidates,"contestOrders":0,"profitabilityEstablished":false,"use":"discovery hypotheses only; source completeness, actual queue priority and profitable exits unproven"})
 }
}
pub fn replay(path:&Path)->Result<Value>{
 let mut journal=Journal::default();let mut study=Study::default();for line in BufReader::new(File::open(path)?).lines(){let row:Value=serde_json::from_str(&line?)?;study.accept(&journal.accept(&row)?)?}
 let mut report=study.report();report["sourceSequence"]=json!(journal.sequence);report["sourcePrevious"]=json!(journal.previous);Ok(report)
}
#[cfg(test)]mod tests{
 use super::*;
 fn frame(sec:u64,revision:u64,previous:u64,bid:f64,ask:f64,trades:Value)->Value{
  json!({"source":"official_sig_realtime","event":"markets_batch","receivedAt":format!("2026-10-15T00:00:{sec:02}Z"),"payload":{"markets":[{"marketId":"1","batch":{"delivery":{"model":"best-effort-authoritative-resync","revision":revision,"previousRevision":previous},"books":[{"exchangeId":"2","asOf":{"sequence":revision*10,"at":format!("2026-10-15T00:00:{sec:02}Z")},"bids":[{"price":bid,"quantity":100}],"asks":[{"price":ask,"quantity":100}]}],"trades":trades}}]}})
 }
 fn trade(id:&str,sec:u64,seq:u64,price:f64)->Value{json!({"id":id,"marketId":"1","exchangeId":"2","sequence":seq,"executedAt":format!("2026-10-15T00:00:{sec:02}Z"),"price":price,"quantity":5})}
 #[test]fn future_batch_book_cannot_classify_earlier_trades(){let mut s=Study::default();s.accept(&frame(5,1,0,0.4,0.6,json!([trade("a",4,9,0.4)]))).unwrap();assert_eq!(s.report()["candidates"][0]["currentBidCompatibleShares"],"0");}
 #[test]fn both_stable_relative_sides_required_and_price_changes_clear_eligibility(){let mut s=Study::default();s.accept(&frame(0,1,0,0.4,0.6,json!([]))).unwrap();s.accept(&frame(5,2,1,0.4,0.6,json!([trade("a",4,11,0.4),trade("b",4,12,0.6)]))).unwrap();assert_eq!(s.report()["candidates"][0]["eligibleForRestReconciliation"],true);s.accept(&frame(6,3,2,0.405,0.6,json!([]))).unwrap();assert_eq!(s.report()["candidates"][0]["eligibleForRestReconciliation"],false);assert_eq!(s.report()["candidates"][0]["historicalTwoSidedFlow"],true);}
 #[test]fn duplicates_do_not_add_volume_and_revision_gaps_clear_flow(){let mut s=Study::default();s.accept(&frame(0,1,0,0.4,0.6,json!([]))).unwrap();let f=frame(5,2,1,0.4,0.6,json!([trade("a",4,11,0.4)]));s.accept(&f).unwrap();s.accept(&f).unwrap();assert_eq!(s.report()["candidates"][0]["observedTrades"],1);s.accept(&frame(6,4,3,0.4,0.6,json!([]))).unwrap();assert_eq!(s.report()["candidates"][0]["currentBidCompatibleShares"],"0");}
 #[test]fn reconnect_cannot_reuse_pre_reconnect_flow(){let mut s=Study::default();s.accept(&frame(0,1,0,0.4,0.6,json!([]))).unwrap();s.accept(&frame(5,2,1,0.4,0.6,json!([trade("a",4,11,0.4),trade("b",4,12,0.6)]))).unwrap();s.accept(&json!({"source":"official_sig_realtime","event":"stream_reset","payload":{"localBoundary":true},"receivedAt":"2026-10-15T00:00:06Z"})).unwrap();assert_eq!(s.report()["candidates"].as_array().unwrap().len(),0);s.accept(&frame(7,3,2,0.4,0.6,json!([]))).unwrap();assert_eq!(s.report()["candidates"][0]["eligibleForRestReconciliation"],false);assert_eq!(s.report()["candidates"][0]["historicalTwoSidedFlow"],false);}
 #[test]fn equivalent_numeric_ids_are_supported(){let mut s=Study::default();let mut f=frame(0,1,0,0.4,0.6,json!([]));f["payload"]["markets"][0]["marketId"]=json!(1);f["payload"]["markets"][0]["batch"]["books"][0]["exchangeId"]=json!(2);s.accept(&f).unwrap();assert_eq!(s.report()["candidates"][0]["exchangeId"],"2");}
 #[test]fn submillisecond_later_receipts_cannot_classify_earlier_execution(){
  let mut s=Study::default();s.accept(&frame(0,1,0,0.4,0.6,json!([]))).unwrap();
  let mut b=frame(4,2,1,0.4,0.6,json!([]));b["receivedAt"]=json!("2026-10-15T00:00:04.000900Z");b["payload"]["markets"][0]["batch"]["books"][0]["asOf"]["at"]=json!("2026-10-15T00:00:04.000100Z");s.accept(&b).unwrap();
  let mut t=trade("early",4,21,0.4);t["executedAt"]=json!("2026-10-15T00:00:04.000500Z");s.accept(&frame(5,3,2,0.4,0.6,json!([t]))).unwrap();assert_eq!(s.report()["candidates"][0]["currentBidCompatibleShares"],"0");
 }
 #[test]fn durable_recording_replays_and_tampering_fails(){let p=std::env::temp_dir().join(format!("breadth-{}.jsonl",std::process::id()));{let mut r=Recorder::new(&p).unwrap();let f=frame(0,1,0,0.4,0.6,json!([]));r.observe("markets_batch",&f["payload"],f["receivedAt"].as_str().unwrap());assert!(r.error.is_none());}assert_eq!(replay(&p).unwrap()["sourceSequence"],1);let s=std::fs::read_to_string(&p).unwrap().replace("\"marketId\":\"1\"","\"marketId\":\"3\"");std::fs::write(&p,s).unwrap();assert!(replay(&p).is_err());std::fs::remove_file(p).unwrap();}
}
