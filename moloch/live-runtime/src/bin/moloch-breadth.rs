use std::{path::Path,fs::OpenOptions,io::Write};
fn main()->Result<(),Box<dyn std::error::Error>>{
 let a:Vec<_>=std::env::args().collect();if a.len()!=3{return Err("usage: moloch-breadth SOURCE_JSONL NEW_REPORT_JSON".into())}
 let report=moloch_live_runtime::breadth::replay(Path::new(&a[1]))?;
 let mut out=OpenOptions::new().write(true).create_new(true).open(&a[2])?;out.write_all(&serde_json::to_vec_pretty(&report)?)?;out.write_all(b"\n")?;out.sync_all()?;println!("{}",serde_json::json!({"sourceSequence":report["sourceSequence"],"marketsObserved":report["marketsObserved"],"topCandidates":report["candidates"].as_array().map(|a|a.iter().take(8).collect::<Vec<_>>())}));Ok(())
}
