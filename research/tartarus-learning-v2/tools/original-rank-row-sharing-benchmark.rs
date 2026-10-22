//! Allocation-only benchmark on an unchanged original native report.
//! This does not replay Cohorts::check or measure production dispatch latency.
use serde_json::{Value,json};
use sha2::{Digest,Sha256};
use std::{sync::Arc,time::Instant};
fn inspected(q:&Value)->[Value;6]{
 [q["exchangeId"].clone(),q["side"].clone(),q["marketId"].clone(),
  q["action"].clone(),q["price"].clone(),q["quantity"].clone()]
}
fn main()->Result<(),Box<dyn std::error::Error>>{
 let args:Vec<_>=std::env::args().collect();assert_eq!(args.len(),2);
 let raw=std::fs::read(&args[1])?;let original:Value=serde_json::from_slice(&raw)?;
 let rows:Vec<_>=original["orders"].as_array().ok_or("ORIGINAL_ORDERS")?.iter()
  .filter(|q|q.get("reviewedRankCampaign").is_some()).collect();
 assert!(!rows.is_empty());let baseline:Vec<_>=rows.iter().map(|q|inspected(q)).collect();
 let encoded:usize=rows.iter().map(|q|serde_json::to_vec(q).map(|b|b.len())).collect::<Result<Vec<_>,_>>()?.iter().sum();
 let mut runs=vec![];
 for shared in [false,true,true,false]{
  let started=Instant::now();let mut retained=Vec::new();
  if shared{retained=rows.iter().map(|q|Arc::new((*q).clone())).collect();}
  for _ in 0..8{
   for(i,q)in rows.iter().enumerate(){
    if shared{let q=retained[i].clone();assert_eq!(inspected(&q),baseline[i]);std::hint::black_box(q);}
    else{let q=(*q).clone();assert_eq!(inspected(&q),baseline[i]);std::hint::black_box(q);}
   }
  }
  drop(retained);
  runs.push(json!({"shared":shared,"passes":8,"seconds":started.elapsed().as_secs_f64()}));
 }
 assert_eq!(original,serde_json::from_slice::<Value>(&raw)?);
 println!("{}",json!({"sourcePath":args[1],"sourceBytes":raw.len(),"sourceSha256":format!("{:x}",Sha256::digest(&raw)),
  "rankRows":rows.len(),"encodedRankRowBytes":encoded,"runs":runs,"exactInspectedFieldsMatch":true,
  "fullOriginalUnchanged":true,"endToEndDispatchBenchmark":false,"accountClientsCreated":0,"productionChanged":false}));
 Ok(())
}
