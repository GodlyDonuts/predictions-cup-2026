use std::{fs,collections::BTreeSet,time::{SystemTime,UNIX_EPOCH}};
use serde_json::{Value,json};
use moloch_live_runtime::fundamental_policy as f;
fn main()->Result<(),Box<dyn std::error::Error>>{
 let a:Vec<String>=std::env::args().collect();
 match a[1].as_str(){
 "hash"=>{let r:f::ReviewInput=serde_json::from_slice(&fs::read(&a[2])?)?;println!("{}",json!({"canonicalSha256":f::review_sha256(&r)?,"exchangeId":r.exchange_id,"sourceVerified":false}));},
 "verify"=>{let jobs:Value=serde_json::from_slice(&fs::read(&a[2])?)?;let authority:Value=serde_json::from_slice(&fs::read(&a[3])?)?;let trusted:BTreeSet<String>=serde_json::from_value(authority["trustedReviewSha256"].clone())?;let now=SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis() as u64;let mut out=vec![];
 for job in jobs["reviews"].as_array().ok_or("JOBS")?{let r:f::ReviewInput=serde_json::from_slice(&fs::read(job["path"].as_str().ok_or("PATH")?)?)?;let refs=job["sourceFiles"].as_array().ok_or("SOURCES")?;let data=refs.iter().map(|s|fs::read(s["path"].as_str().ok_or("SOURCE_PATH")?).map_err(|e|Box::<dyn std::error::Error>::from(e))).collect::<Result<Vec<_>,_>>()?;
 let originals=refs.iter().zip(&data).map(|(s,b)|Ok(f::SourceBytes{sha256:s["sha256"].as_str().ok_or("SHA")?,bytes:b})).collect::<Result<Vec<_>,Box<dyn std::error::Error>>>()?;
 let reviewed=f::Reviewed::verify(r,&originals,&trusted,now)?;out.push(json!({"key":job["key"],"canonicalSha256":reviewed.sha256(),"entryCeiling":reviewed.entry_ceiling()?,"supportsNewCohort":reviewed.supports_new_cohort(),"sourceHashesAndContractObjectiveVerified":true}));}
 println!("{}",json!({"reviews":out,"typedCurrentReviewChecksPassed":true,"executionAuthority":false,"calibrationVerified":false}));},
 "settings"=>{let s:f::Settings=serde_json::from_slice(&fs::read(&a[2])?)?;let loaded=s.load()?;loaded.validate_files()?;println!("{}",json!({"settingsCanonicalSha256":loaded.settings_sha(),"settingsLoadPassed":true,"sourcesVerified":true,"executionAuthority":false}));},
 _=>return Err("hash REVIEW | verify JOBS AUTHORITY | settings SETTINGS".into())
 }Ok(())
}
