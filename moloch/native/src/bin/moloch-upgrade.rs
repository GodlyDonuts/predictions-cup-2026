//! Offline proposal generation only: no account access or automatic approval.
use moloch_replay::session::propose_live_engine_upgrade;
use std::{fs::OpenOptions,io::Write,path::Path};
fn main()->Result<(),Box<dyn std::error::Error>>{
 let args:Vec<String>=std::env::args().collect();if args.len()!=5{return Err("usage: moloch-upgrade BANK OLD_BINARY_SHA256 APPROVED_AT NEW_PROPOSAL_OUTPUT".into())}
 let proposal=propose_live_engine_upgrade(Path::new(&args[1]),&args[2],&args[3])?;
 let output=Path::new(&args[4]);if !output.is_absolute(){return Err("UPGRADE_OUTPUT_ABSOLUTE".into())}
 let mut options=OpenOptions::new();options.write(true).create_new(true);#[cfg(unix)]{use std::os::unix::fs::OpenOptionsExt;options.mode(0o600);}
 let mut file=options.open(output)?;serde_json::to_writer(&mut file,&proposal)?;file.write_all(b"\n")?;file.sync_all()?;
 println!("{}",serde_json::json!({"proposal":output,"hash":proposal["hash"],"written":true,"activated":false,"accountRequests":0}));Ok(())
}
