//! Synthetic, private, fresh-fixture benchmark. No account/client is constructed.
use moloch_replay::session::{BatchEntry,Session,SESSION_BATCH_MAX_EVENTS};
use serde_json::{Value,json};
use sha2::{Digest,Sha256};
use std::{env,fs::{self,OpenOptions},io::Write,path::Path,time::{Instant,SystemTime,UNIX_EPOCH}};
type Result<T>=std::result::Result<T,Box<dyn std::error::Error>>;
fn digest(bytes:&[u8])->String{format!("{:x}",Sha256::digest(bytes))}
fn private_new(path:&Path,bytes:&[u8])->Result<()>{let mut o=OpenOptions::new();o.create_new(true).write(true);#[cfg(unix)]{use std::os::unix::fs::OpenOptionsExt;o.mode(0o600);}let mut f=o.open(path)?;f.write_all(bytes)?;f.sync_all()?;Ok(())}
fn dataset(rounds:usize)->Vec<Value>{
 let mut events=Vec::with_capacity(237*rounds);let base=chrono::DateTime::parse_from_rfc3339("2026-10-15T00:00:00Z").unwrap().timestamp_millis();
 for round in 0..rounds{let at=chrono::DateTime::from_timestamp_millis(base+round as i64*1000).unwrap().to_rfc3339_opts(chrono::SecondsFormat::Millis,true);
  for market in 0..237{let id=(1000+market).to_string();events.push(json!({"type":"book","receivedAt":at,"processedAt":at,"book":{"tournamentId":"synthetic-cup","marketId":id,"exchangeId":id,"sequence":(round+1).to_string(),"sourceAt":at,"requestStartedAt":at,"receivedAt":at,"transport":"official_sig_realtime","bids":[{"price":"0.450000","quantity":1000}],"asks":[{"price":"0.455000","quantity":1000}]}}));}
 }events
}
fn run_one(root:&Path,events:&[Value],batch:usize,policy:&Value)->Result<Value>{
 let mut s=Session::open(root,None,policy)?;let mut commits=0usize;let mut round_micros=Vec::new();let mut raw_bytes=0usize;let start=Instant::now();
 for(round,burst)in events.chunks(237).enumerate(){let round_start=Instant::now();for(offset,chunk)in burst.chunks(batch).enumerate(){let first=round*237+offset*batch+1;
  if batch==1 {if !s.accept("synthetic-broadcast",first as u64,&chunk[0])?{return Err("UNEXPECTED_DUPLICATE".into())}commits+=1;}
  else{let input=chunk.iter().enumerate().map(|(i,event)|BatchEntry{source_id:"synthetic-broadcast",sequence:(first+i)as u64,event}).collect::<Vec<_>>();let outcome=s.accept_batch(&input)?;if outcome.appended_rows!=chunk.len()||outcome.accepted.iter().any(|v|!*v){return Err("UNEXPECTED_DUPLICATE".into())}commits+=usize::from(outcome.appended_rows>0);raw_bytes+=outcome.encoded_journal_bytes;}
 }round_micros.push(round_start.elapsed().as_micros());}
 let micros=start.elapsed().as_micros();let cursor=s.last_source_sequence("synthetic-broadcast");let expected=s.engine().report()?;drop(s);
 let journal=fs::read(root.join("events.jsonl"))?;let head=fs::read(root.join("head.json"))?;let report=fs::read(root.join("report.json"))?;
 let restored=Session::open(root,None,policy)?;if restored.engine().report()?!=expected||restored.last_source_sequence("synthetic-broadcast")!=cursor{return Err("RESTART_PARITY".into())}drop(restored);
 if fs::read(root.join("head.json"))?!=head||fs::read(root.join("report.json"))?!=report{return Err("RESTART_PUBLICATION_PARITY".into())}
 if batch>1&&raw_bytes!=journal.len(){return Err("ENCODED_BYTE_PARITY".into())}
 Ok(json!({"mode":if batch==1{"serial_accept"}else{"accept_batch"},"maximumEventsPerCall":batch,"events":events.len(),"durableCommits":commits,"ingestWallMicros":micros,"roundWallMicros":round_micros,"sourceCursor":cursor,"journalBytes":journal.len(),"journalSha256":digest(&journal),"headSha256":digest(&head),"reportSha256":digest(&report),"restartParity":true,"syncCallModel":{"measuredBySyscallTrace":false,"journalSyncAll":commits,"atomicProjectionFileSyncAll":2*commits,"unixDirectorySyncAll":if cfg!(unix){2*commits}else{0},"excludesInitializationRestartAndFixtureWrites":true}}))
}
fn run()->Result<()>{
 let args=env::args().skip(1).collect::<Vec<_>>();let mode=args.first().map(String::as_str).unwrap_or("both");let rounds=args.get(1).map(|s|s.parse()).transpose()?.unwrap_or(2usize);let batch=args.get(2).map(|s|s.parse()).transpose()?.unwrap_or(32usize);
 if !["both","serial","batch"].contains(&mode)||rounds==0||rounds>20||!(2..=SESSION_BATCH_MAX_EVENTS).contains(&batch)||args.len()>3{return Err("USAGE: moloch-session-batch-bench [both|serial|batch] [rounds1..20] [batch2..32]".into())}
 let root=env::temp_dir().join(format!("moloch-session-batch-offline-{}-{}",std::process::id(),SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()));fs::create_dir(&root)?;#[cfg(unix)]{use std::os::unix::fs::PermissionsExt;fs::set_permissions(&root,fs::Permissions::from_mode(0o700))?;}
 let events=dataset(rounds);let mut input=Vec::new();for event in &events{serde_json::to_writer(&mut input,event)?;input.push(b'\n');}private_new(&root.join("input.jsonl"),&input)?;
 let policy=json!({"quantity":1,"maximumMarkets":3});private_new(&root.join("policy.json"),&serde_json::to_vec(&policy)?)?;
 let mut results=Vec::new();if mode!="batch"{results.push(run_one(&root.join("serial"),&events,1,&policy)?)}if mode!="serial"{results.push(run_one(&root.join("batch"),&events,batch,&policy)?)}
 let parity=if results.len()==2{for key in ["events","sourceCursor","journalBytes","journalSha256","headSha256","reportSha256"]{if results[0][key]!=results[1][key]{return Err(format!("SERIAL_BATCH_PARITY:{key}").into())}}Some(true)}else{None};
 let result=json!({"schemaVersion":1,"fixtureDirectory":root,"architecture":env::consts::ARCH,"os":env::consts::OS,"syntheticOnly":true,"marketsPerRound":237,"rounds":rounds,"inputSha256":digest(&input),"engineIdentity":moloch_replay::session::engine_identity(),"serialVersusBatchByteParity":parity,"results":results,"limitations":["Synthetic fixed-clock data and fresh banks; no production latency/financial speed claim.","Both mode runs serial first; filesystem cache/order bias remains.","syncCallModel is source-derived, not a measured syscall count; use separate serial/batch syscall traces for verification.","Initialization, input creation and restart checks are excluded from ingest timers.","Fixture is retained privately; no account, service, credentials or live bank are accessed."]});
 private_new(&root.join("results.json"),&serde_json::to_vec_pretty(&result)?)?;println!("{}",result);Ok(())
}
fn main(){if let Err(e)=run(){eprintln!("{e}");std::process::exit(1)}}
