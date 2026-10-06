//! Additive scanner; original Overlay economics and historical base verifier
//! are reused unchanged. Avoid two redundant service-Publisher reconstruction
//! passes when the original import evidence already verifies opening provenance.
use super::*;
use baal_service::overlay::Overlay;
use baal_portfolio_engine::{import_historical_base_for_overlay,js_stringify,publication::Publication};
use std::io::{BufRead,BufReader,Read};
fn scan(pin:&PrefixBinding,mut apply:impl FnMut(&V)->Result<()>)->Result<u64>{
 let f=fs::File::open(&pin.path).map_err(|_|"RESTORE_REPLAY_SOURCE_READ")?;need(f.metadata().map_err(|_|"RESTORE_REPLAY_SOURCE_METADATA")?.len()>=pin.bytes,"SOURCE_TRUNCATED")?;
 let mut reader=BufReader::with_capacity(1024*1024,f.take(pin.bytes));let mut line=vec![];let mut bytes=0;let mut rows=0;let mut sha=Sha256::new();
 loop{line.clear();let n=reader.by_ref().take(16*1024*1024+2).read_until(b'\n',&mut line).map_err(|_|"RESTORE_REPLAY_SOURCE_READ")?;if n==0{break}need(n<=16*1024*1024+1&&line.last()==Some(&10),"SOURCE_PARTIAL_OR_OVERSIZE")?;let row:V=serde_json::from_slice(&line).map_err(|_|"RESTORE_REPLAY_SOURCE_JSON")?;need(js_stringify(&row).as_bytes()==&line[..line.len()-1],"ORIGINAL_CANONICAL_BYTES")?;bytes+=n as u64;sha.update(&line);apply(&row)?;rows+=1;}
 need(bytes==pin.bytes&&format!("{:x}",sha.finalize())==pin.sha256,"SEALED_SOURCE_HASH")?;Ok(rows)
}
pub(super)fn restore(bank:V,o:&OverlayBinding,public:&PrefixBinding)->Result<(Overlay,V)>{
 let opening_bytes=file(&o.opening)?;let opening:V=serde_json::from_slice(&opening_bytes).map_err(|_|"RESTORE_REPLAY_OPENING_JSON")?;
 need(file(&o.operator_code).map(|v|hash(&v))?==opening["codeHash"].as_str().unwrap_or(""),"ORIGINAL_OPERATOR_CODE")?;
 let bytes=opening["sourcePrefixBytes"].as_u64().ok_or("RESTORE_REPLAY_OPENING_SOURCE")?;let prefix_hash=opening["sourcePrefixHash"].as_str().ok_or("RESTORE_REPLAY_OPENING_SOURCE")?;
 let(_,base)=import_historical_base_for_overlay(bank.clone(),&o.base_journal.path,o.base_journal.bytes,&o.base_journal.sha256,&public.path,bytes,prefix_hash,&opening)?;
 need(base.public_source_bytes==bytes&&base.public_source_hash==prefix_hash&&json!(base.public_source_sequence)==opening["sourcePrefixSequence"]&&base.public_source_tail_hash==opening["sourcePrefixTailHash"].as_str().unwrap_or(""),"ORIGINAL_OPENING_PUBLIC_FRONTIER")?;
 let mut engine=Overlay::from_opening(bank.clone(),&opening)?;let mut previous=String::new();let mut sequence=0u64;
 let rows=scan(&o.operator_journal,|row|{let mut body=row.clone();body.as_object_mut().ok_or("RESTORE_REPLAY_OPERATOR_BODY")?.shift_remove("hash");sequence+=1;
  need(row["sequence"]==sequence&&row["previous"]==previous&&row["hash"]==digest(&body),"ORIGINAL_OPERATOR_CHAIN")?;previous=row["hash"].as_str().ok_or("RESTORE_REPLAY_OPERATOR_HASH")?.into();
  if sequence==1{need(row["event"]["kind"]=="genesis"&&row["event"]["openingHash"]==hash(&opening_bytes),"ORIGINAL_OPERATOR_GENESIS")?;}else{engine.apply(&row["event"])?;}
  need(digest(&engine.state)==digest(&row["state"]),"EXACT_ORIGINAL_OVERLAY_CHECKPOINT")?;Ok(())})?;
 need(rows>0,"ORIGINAL_OVERLAY_EMPTY")?;let target=engine.state["sourceSequence"].as_i64().ok_or("RESTORE_REPLAY_OVERLAY_SOURCE")?;let wanted=engine.state["sourceHash"].as_str().ok_or("RESTORE_REPLAY_OVERLAY_SOURCE")?.to_owned();
 let mut publication=Publication::new(&bank["universe"])?;let mut source_bound=false;
 let public_rows=scan(public,|row|{publication.apply(row)?;if publication.sequence()==target{need(publication.tail_hash()==wanted,"EXACT_OVERLAY_PUBLIC_FRONTIER")?;source_bound=true;}Ok(())})?;
 need(source_bound&&publication.sequence()>=target,"OVERLAY_FUTURE_FRONTIER")?;
 let evidence=json!({"schemaVersion":1,"mode":"offline_native_exact_overlay_restore_without_duplicate_opening_scan","bankHash":digest(&bank),"openingHash":hash(&opening_bytes),"operatorJournalBytes":o.operator_journal.bytes,"operatorJournalHash":o.operator_journal.sha256,"operatorJournalRecords":rows,"operatorJournalTailHash":previous,"stateHash":digest(&engine.state),"walletSourceSequence":target,"walletSourceHash":wanted,"publicSourceBytes":public.bytes,"publicSourceHash":public.sha256,"publicSourceRows":public_rows,"allOriginalCheckpointsVerified":true,"historicalBaseNotCurrent":true,"authoritativeOverlayApplied":true,"baseImport":base,"officialHoldingsAdopted":false,"competitionOrders":0,"profitabilityEstablished":false});
 Ok((engine,evidence))
}
