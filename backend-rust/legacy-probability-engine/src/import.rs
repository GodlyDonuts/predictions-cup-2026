use super::*;
use std::{fs::File,io::{Read,BufRead,BufReader},path::Path};
pub struct ImportedHistory {pub engine:Engine,pub evidence:V}
struct Rows {reader:BufReader<std::io::Take<File>>,bytes:u64,sha:Sha256}
impl Rows {
 fn open(path:&Path,bytes:u64)->Result<Self>{let file=File::open(path).map_err(|_|"IMPORT_FILE_UNAVAILABLE")?;need(file.metadata().map_err(|_|"IMPORT_FILE_UNAVAILABLE")?.len()>=bytes,"IMPORT_SOURCE_TRUNCATED")?;Ok(Self{reader:BufReader::new(file.take(bytes)),bytes:0,sha:Sha256::new()})}
 fn next(&mut self)->Result<Option<V>>{let mut line=Vec::new();let n=self.reader.read_until(b'\n',&mut line).map_err(|_|"IMPORT_READ_FAILED")?;if n==0{return Ok(None);}need(n<=1024*1024&&line.last()==Some(&b'\n'),"IMPORT_INCOMPLETE_OR_LARGE_RECORD")?;self.bytes+=n as u64;self.sha.update(&line);let row=serde_json::from_slice(&line).map_err(|_|"IMPORT_RECORD_JSON")?;Ok(Some(row))}
 fn hash(&self)->String{format!("{:x}",self.sha.clone().finalize())}
}
struct Publication {sequence:u64,hash:String,scope_hash:String,last_at:i64,session:Option<String>,sessions:BTreeSet<String>,closed:bool,global:i64,markets:BTreeMap<String,i64>,versions:BTreeMap<String,V>,specs:BTreeMap<String,String>}
impl Publication {
 fn new(bank:&V)->Result<Self>{let mut rows=bank["universe"]["exchanges"].as_array().ok_or("PUBLIC_UNIVERSE")?.iter().map(|r|json!({"marketId":r["marketId"],"exchangeId":r["exchangeId"],"option":"YES"})).collect::<Vec<_>>();rows.sort_by_key(|r|r["marketId"].as_str().unwrap().parse::<u64>().unwrap());let scope=json!({"tournamentId":bank["universe"]["tournamentId"],"exchanges":rows});let specs=rows.iter().map(|r|Ok((text(&r["marketId"])?.into(),text(&r["exchangeId"])?.into()))).collect::<Result<BTreeMap<_,_>>>()?;Ok(Self{sequence:0,hash:String::new(),scope_hash:sorted_hash(&scope),last_at:0,session:None,sessions:BTreeSet::new(),closed:true,global:0,markets:BTreeMap::new(),versions:BTreeMap::new(),specs})}
 fn accept(&mut self,row:&V,bank:&V)->Result<()> {
  let mut body=row.clone();let received=text(&body["hash"])?.to_string();body.as_object_mut().ok_or("PUBLIC_ROW")?.remove("hash");let sequence=row["sequence"].as_u64().ok_or("PUBLIC_SEQUENCE")?;let at=time(&row["at"])?;
  need(row["schemaVersion"]==1&&sequence==self.sequence+1&&row["previousHash"]==self.hash&&received==sorted_hash(&body)&&row["tournamentId"]==bank["universe"]["tournamentId"]&&row["scopeHash"]==self.scope_hash&&at>=self.last_at,"PUBLIC_CHAIN_SCOPE_OR_CLOCK")?;
  let session=text(&row["sessionId"])?;let kind=text(&row["kind"])?;
  if kind=="session_start"{need(self.sessions.insert(session.into())&&sorted_hash(&row["payload"]["scope"])==self.scope_hash,"PUBLIC_SESSION_START")?;self.session=Some(session.into());self.closed=false;self.global=at;self.markets.clear();}
  else {need(!self.closed&&self.session.as_deref()==Some(session),"PUBLIC_SESSION_CLOSED")?;match kind{
   "session_stop"=>{need(row["payload"]["reason"].as_str().is_some_and(|r|["STOP","COLLECTOR_FAILURE","NORMAL_SHUTDOWN"].contains(&r)),"PUBLIC_STOP_REASON")?;self.closed=true;self.global=at;},
   "invalidation"=>{let market=&row["payload"]["marketId"];if let Some(m)=market.as_str(){need(self.specs.contains_key(m),"PUBLIC_INVALIDATION_SCOPE")?;self.markets.insert(m.into(),at);}else{need(market.is_null(),"PUBLIC_INVALIDATION_SCOPE")?;self.global=at;}},
   "book"=>{let b=&row["payload"]["book"];let market=text(&b["marketId"])?;let exchange=text(&b["exchangeId"])?;let id=text(&b["instrument"])?;let started=time(&b["requestStartedAt"])?;let received=time(&b["receivedAt"])?;let source=ns(&b["sourceAt"])?;let source_sequence=text(&b["sequence"])?.parse::<u64>().map_err(|_|"PUBLIC_SOURCE_SEQUENCE")?;
    need(self.specs.get(market).is_some_and(|e|e==exchange)&&id==format!("{exchange}:YES")&&b["tournamentId"]==row["tournamentId"]&&b["freshnessPolicy"]=="authoritative_receipt_v1"&&started<=received&&received<=at&&at-received<=20000&&received-started<=20000&&started>=self.global&&started>=*self.markets.get(market).unwrap_or(&0)&&source<=(received+2000)*1_000_000&&b["sourceAt"]==canonical_time(&b["sourceAt"])?&&b["versionKey"]==format!("{source_sequence}:{}",text(&b["sourceAt"])?)&&num(&b["sourceEventAgeMs"])?==received-time(&b["sourceAt"])?,"PUBLIC_BOOK_SCOPE_OR_CLOCK")?;
    for side in ["bids","asks"]{let levels=b[side].as_array().ok_or("PUBLIC_DEPTH")?;need(!levels.is_empty()&&levels.len()<=20,"PUBLIC_DEPTH")?;let mut previous=None;for level in levels{let p=amount(&level["price"])?;need((TICK..=1_000_000-TICK).contains(&p)&&p%TICK==0&&num(&level["quantity"])? >0&&previous.is_none_or(|old|if side=="bids"{p<old}else{p>old}),"PUBLIC_DEPTH_LEVEL")?;previous=Some(p);}}
    need(amount(&b["bids"][0]["price"])?<amount(&b["asks"][0]["price"])? ,"PUBLIC_CROSSED_BOOK")?;
    let depth=sorted_hash(&json!({"bids":b["bids"],"asks":b["asks"]}));if let Some(old)=self.versions.get(id){let old_sequence=text(&old["sequence"])?.parse::<u64>().map_err(|_|"PUBLIC_SOURCE_SEQUENCE")?;need(source_sequence>=old_sequence&&source>=ns(&old["sourceAt"])?&&received>=time(&old["receivedAt"])?&&(source_sequence!=old_sequence||b["versionKey"]==old["versionKey"]&&old["depthHash"]==depth),"PUBLIC_SOURCE_ROLLBACK_OR_CONFLICT")?;}
    self.versions.insert(id.into(),json!({"sequence":b["sequence"],"sourceAt":b["sourceAt"],"versionKey":b["versionKey"],"receivedAt":b["receivedAt"],"depthHash":depth}));
   },_=>return Err("PUBLIC_ROW_KIND".into()),
  }}
  self.sequence=sequence;self.hash=received;self.last_at=at;Ok(())
 }
}
#[allow(clippy::too_many_arguments)]
pub fn import_history(bank:V,opening:Option<V>,checkpoint_path:&Path,prefix_bytes:u64,prefix_sha:&str,public_source_path:&Path,public_prefix_bytes:u64,public_prefix_sha:&str)->Result<ImportedHistory>{
 let started=std::time::Instant::now();let mut engine=if bank["schemaVersion"]==2{Engine::new(bank.clone())?}else{Engine::from_opening(bank.clone(),opening.ok_or("OPENING_REQUIRED")?)?};
 let mut checkpoints=Rows::open(checkpoint_path,prefix_bytes)?;let mut public=Rows::open(public_source_path,public_prefix_bytes)?;let mut publication=Publication::new(&bank)?;let cutoff_sequence=bank["sourcePrefixSequence"].as_u64().ok_or("BANK_PREFIX")?;
 while publication.sequence<cutoff_sequence{let row=public.next()?.ok_or("PUBLIC_PREFIX_TRUNCATED")?;publication.accept(&row,&bank)?;}
 need(public.bytes==bank["sourcePrefixBytes"].as_u64().ok_or("BANK_PREFIX")?&&public.hash()==bank["sourcePrefixHash"]&&publication.hash==bank["sourcePrefixJournalHash"],"BANK_PUBLIC_PREFIX_PIN")?;
 let bank_hash=digest(&bank);let mut sequence=0u64;let mut previous=bank_hash.clone();let mut last_frontier=publication.sequence;let mut last_source_hash=publication.hash.clone();let mut last_source_bytes=public.bytes;
 while let Some(row)=checkpoints.next()?{let mut body=row.clone();let hash=text(&body["hash"])?.to_string();body.as_object_mut().ok_or("CHECKPOINT_OBJECT")?.remove("hash");sequence+=1;if !(row["sequence"]==sequence&&row["previous"]==previous&&row["bankHash"]==bank_hash&&digest(&body)==hash){return Err(format!("CHECKPOINT_CHAIN:{sequence}:bank:{}:expectedBank:{}:body:{}:expectedBody:{}",digest(&bank),row["bankHash"],digest(&body),hash));}
  if row["kind"]=="genesis"{need(sequence==1&&row["stateHash"]==digest(engine.state()),"GENESIS_STATE_PARITY")?;}
  else {need(row["kind"]=="batch"&&row["sourceSequence"].as_u64().is_some_and(|s|s>publication.sequence),"CHECKPOINT_FRONTIER")?;let desired=row["sourceSequence"].as_u64().unwrap();let mut batch=Vec::new();while publication.sequence<desired{let source=public.next()?.ok_or("PUBLIC_SOURCE_TRUNCATED")?;publication.accept(&source,&bank)?;batch.push(source);}
   need(public.bytes==row["sourceBytes"].as_u64().ok_or("CHECKPOINT_BYTES")?&&publication.hash==row["sourceHash"],"CHECKPOINT_SOURCE_FRONTIER")?;
   for source in &batch{engine.advance(source,&row["availableAt"],&row["accountEligibility"]).map_err(|e|format!("{e}:checkpoint:{sequence}"))?;}need(row["stateHash"]==digest(engine.state()),&format!("CHECKPOINT_STATE_PARITY:checkpoint:{sequence}"))?;
   last_frontier=publication.sequence;last_source_hash=publication.hash.clone();last_source_bytes=public.bytes;
  }previous=hash;
 }
 need(sequence>0&&checkpoints.bytes==prefix_bytes&&checkpoints.hash()==prefix_sha,"CHECKPOINT_SEALED_PREFIX")?;
 while let Some(row)=public.next()?{publication.accept(&row,&bank)?;}
 need(public.bytes==public_prefix_bytes&&public.hash()==public_prefix_sha,"PUBLIC_SEALED_PREFIX")?;
 let evidence=json!({"schemaVersion":1,"mode":"native_original_probability_checkpoint_replay","cohort":bank["name"],"bankHash":digest(&bank),"checkpoints":sequence,"checkpointBytes":prefix_bytes,"checkpointSha256":prefix_sha,"checkpointTailHash":previous,"publicPrefixBytes":public_prefix_bytes,"publicPrefixSha256":public_prefix_sha,"sourceSequence":last_frontier,"sourceHash":last_source_hash,"sourceBytes":last_source_bytes,"publicSnapshotSequence":publication.sequence,"publicSnapshotHash":publication.hash,"stateHash":digest(engine.state()),"everyOriginalCheckpointMatched":true,"elapsedMs":started.elapsed().as_millis(),"networkRequests":0,"competitionOrders":0,"profitabilityEstablished":false});
 Ok(ImportedHistory{engine,evidence})
}
