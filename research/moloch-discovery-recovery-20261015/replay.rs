//! Offline old/new discovery comparison on one frozen, hash-linked capture.
//! No account client, native bank open, financial mutation or execution.
pub mod breadth { pub use moloch_live_runtime::breadth::*; }
pub mod feedback { pub use moloch_live_runtime::feedback::*; }
#[path="legacy-universe.rs"] mod legacy;
use moloch_live_runtime::universe::Universe;
use moloch_replay::journal::Journal;
use serde_json::{Value,json};
use sha2::{Digest,Sha256};
use std::{collections::{BTreeMap,BTreeSet},fs::File,io::{BufRead,BufReader,Read},path::Path};
type Result<T> = feedback::Result<T>;
fn main()->Result<()>{
 let args:Vec<_>=std::env::args().collect();if args.len()!=3{return Err("usage: discovery-replay PRIVATE_INPUT FROZEN_CAPTURE".into())}
 let input:Value=serde_json::from_slice(&std::fs::read(&args[1])?)?;
 let exchanges:BTreeMap<String,String>=serde_json::from_value(input["scope"]["exchanges"].clone())?;
 let required:BTreeSet<String>=serde_json::from_value(input["required"].clone())?;
 let maximum=input["managedMaximum"].as_u64().ok_or("MAXIMUM")? as usize;
 let discovery=input["discoveryMaximum"].as_u64().ok_or("DISCOVERY_MAXIMUM")? as usize;
 let tournament=input["scope"]["tournamentId"].as_str().ok_or("TOURNAMENT")?;
 let mut old=legacy::Universe::new(&exchanges,&json!({"positions":[]}),maximum)?.with_discovery_limit(discovery)?;
 let mut new=Universe::new(&exchanges,&json!({"positions":[]}),maximum)?.with_discovery_limit(discovery)?.with_tournament(tournament)?;
 let mut probes=Vec::new();let path=Path::new(&args[2]);if std::fs::symlink_metadata(path)?.file_type().is_symlink(){return Err("SYMLINK".into())}
 let mut reader=BufReader::new(File::open(path)?);let mut chain=Journal::default();let mut hash=Sha256::new();let mut bytes=0u64;let mut at=0u64;
 loop{
  let mut line=Vec::new();let n=reader.by_ref().take(16*1024*1024+1).read_until(b'\n',&mut line)?;
  if n==0{break}if n>16*1024*1024||line.last()!=Some(&b'\n'){return Err("ROW_BOUNDARY".into())}
  bytes+=n as u64;hash.update(&line);let event=chain.accept(&serde_json::from_slice(&line)?)?;
  if event["source"]!="official_sig_realtime"{return Err("SOURCE".into())}
  let kind=event["event"].as_str().ok_or("EVENT")?;let received=event["receivedAt"].as_str().ok_or("CLOCK")?;
  at=moloch_live_runtime::daily_windows::clock(received)?;
  old.observe(kind,&event["payload"],received)?;new.observe(kind,&event["payload"],received)?;
  if chain.sequence%20==0||chain.sequence==132{let b=old.select(&required)?;let a=new.select_at(&required,at)?;probes.push(json!({"frame":chain.sequence,"at":received,"beforeDiscovery":b.iter().filter(|e|!required.contains(*e)).count(),"afterDiscovery":a.iter().filter(|e|!required.contains(*e)).count(),"ownedPreserved":required.iter().all(|e|a.contains(e)),"bounded":a.len()<=maximum}));}
 }
 let digest=format!("{:x}",hash.finalize());if input["captureSha256"]!=digest||input["captureBytes"]!=bytes||input["frames"]!=chain.sequence||moloch_live_runtime::daily_windows::clock(input["lastReceivedAt"].as_str().ok_or("INPUT_CLOCK")?)?!=at{return Err("FROZEN_INPUT_CHANGED".into())}
 let before=old.select(&required)?;let before_new:Vec<_>=before.iter().filter(|e|!required.contains(*e)).cloned().collect();
 let mut packets=Vec::new();let mut observed=BTreeSet::new();
 for _ in 0..3{
  let selected=new.select_at(&required,at)?;let discovered:Vec<_>=selected.iter().filter(|e|!required.contains(*e)).cloned().collect();
  if selected.len()>maximum||discovered.len()>discovery||required.len()<=maximum&&!required.iter().all(|e|selected.contains(e)){return Err("READ_CAP_OR_OWNED_LOSS".into())}
  observed.extend(discovered.iter().cloned());packets.push(json!({"selected":selected.len(),"owned":selected.len()-discovered.len(),"discovery":discovered.len()}));
 }
 let expired=new.select_at(&required,at.checked_add(60_001).ok_or("CLOCK_RANGE")?)?;
 let expired_new=expired.iter().filter(|e|!required.contains(*e)).count();
 println!("{}",json!({"schemaVersion":1,"mode":"offline_same_prefix_discovery_comparison","captureFrames":chain.sequence,"captureBytes":bytes,"captureSha256":digest,"captureHead":chain.previous,"rowHashChainVerified":true,"replayClockMs":at,"requiredOwnedMarkets":required.len(),"maximumManaged":maximum,"maximumDiscovery":discovery,"intermediateProbes":probes,"before":{"selected":before.len(),"owned":before.len()-before_new.len(),"discovery":before_new.len(),"status":old.status()},"afterPackets":packets,"distinctDiscoveredAcrossThreePackets":observed.len(),"discoveryAfterActivityExpired":expired_new,"afterStatus":new.status(),"accountRequests":0,"sourceMutated":false,"financialGateRelaxed":false,"executionAuthorized":false,"profitabilityEstablished":false}));Ok(())
}
