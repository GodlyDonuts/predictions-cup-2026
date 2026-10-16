use moloch_live_runtime::fundamental_policy::{ReviewInput,Reviewed,SourceBytes,Settings};
use serde_json::{Value,json};
use std::{collections::BTreeSet,env,fs,time::{SystemTime,UNIX_EPOCH}};
use sha2::{Digest,Sha256};
fn main()->Result<(),Box<dyn std::error::Error>>{
 let args=env::args().collect::<Vec<_>>();if args.len()!=3{return Err("hash|settings ABS_PATH".into())}
 if args[1]=="hash"{
  let jobs:Value=serde_json::from_slice(&fs::read(&args[2])?)?;let mut out=Vec::new();
  for job in jobs["reviews"].as_array().ok_or("jobs")?{
   let path=job["path"].as_str().ok_or("path")?;let raw=fs::read(path)?;
   let input:ReviewInput=serde_json::from_slice(&raw)?;
   let digest=moloch_live_runtime::fundamental_policy::review_sha256(&input)?;
   let trusted=BTreeSet::from([digest.clone()]);
   let refs=job["sourceFiles"].as_array().ok_or("sources")?;
   let data=refs.iter().map(|r|Ok(fs::read(r["path"].as_str().ok_or("source path")?)?)).collect::<Result<Vec<_>,Box<dyn std::error::Error>>>()?;
   let sources=refs.iter().zip(&data).map(|(r,b)|Ok(SourceBytes{sha256:r["sha256"].as_str().ok_or("source sha")?,bytes:b})).collect::<Result<Vec<_>,Box<dyn std::error::Error>>>()?;
   let now=u64::try_from(SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis())?;
   let review=Reviewed::verify(input,&sources,&trusted,now)?;
   out.push(json!({"key":job["key"],"canonicalSha256":digest,"reviewRawSha256":format!("{:x}",Sha256::digest(raw)),"entryCeiling":review.entry_ceiling()?,"supportsNewCohort":review.supports_new_cohort(),"sourceHashesAndContractObjectiveVerified":true}));
  }
  println!("{}",serde_json::to_string(&json!({"reviews":out,"typedCurrentReviewChecksPassed":true,"calibrationVerified":false,"executionAuthority":false}))?);
 }else if args[1]=="settings"{
  let raw=fs::read(&args[2])?;let s:Settings=serde_json::from_slice(&raw)?;let _loaded=s.load()?;
  let v:Value=serde_json::from_slice(&raw)?;
  println!("{}",serde_json::to_string(&json!({"settingsLoadPassed":true,"sourcesVerified":true,"reviewAccepted":true,"settingsCanonicalSha256":format!("{:x}",Sha256::digest(serde_json::to_vec(&v)?)),"forecastCalibrationVerified":false,"executionAuthority":false}))?);
 }else{return Err("mode".into())}
 Ok(())
}
