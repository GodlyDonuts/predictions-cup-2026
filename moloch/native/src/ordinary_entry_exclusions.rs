//! Durable ordinary-planning exclusions for the separately reviewed manager.
//! Audit hashes bind provenance; runtime must authenticate CURRENT control and
//! approved review set. This event grants no BUY, cancel, fill or cash authority.
use super::*;
fn digest(v:&Value)->Result<String>{v.as_str().filter(|s|s.len()==64&&s.bytes().all(|b|b.is_ascii_digit()||(b'a'..=b'f').contains(&b))).map(str::to_owned).ok_or("ORDINARY_EXCLUSION_HASH")}
impl Engine{
 pub fn ordinary_entry_exclusion_record(&self)->Option<Value>{self.ordinary_entry_exclusions.clone()}
 pub fn ordinary_entry_exclusions(&self)->BTreeSet<String>{self.ordinary_entry_exclusions.as_ref().and_then(|v|v["managedExchangeIds"].as_array()).map(|rows|rows.iter().filter_map(|x|x.as_str().map(str::to_owned)).collect()).unwrap_or_default()}
 pub(super) fn ordinary_entry_excluded(&self,exchange:&str)->bool{self.ordinary_entry_exclusions.as_ref().is_some_and(|v|v["managedExchangeIds"].as_array().is_some_and(|xs|xs.iter().any(|e|e==exchange)))}
 pub(super) fn apply_ordinary_entry_exclusions(&mut self,e:&Value,at:i64)->Result<()>{
  if !self.live_mode||e["source"]!="moloch_live_dispatcher"{return Err("ORDINARY_EXCLUSION_SOURCE")}
  if ms(&e["processedAt"])?!=at||ms(&e["receivedAt"])? >at{return Err("ORDINARY_EXCLUSION_CLOCK")}
  let scope=&e["bankScope"];let scope_fields=scope.as_object().ok_or("ORDINARY_EXCLUSION_SCOPE")?;
  if scope_fields.len()!=3||scope["runId"].as_str().is_none_or(|s|s.is_empty()||s.len()>128)||scope["tournamentId"].as_str().is_none_or(|s|s.is_empty())||digest(&scope["bankManifestHash"]).is_err(){return Err("ORDINARY_EXCLUSION_SCOPE")}
  digest(&e["approvedReviewSetSha256"])?;digest(&e["controlSha256"])?;
  let ids=e["managedExchangeIds"].as_array().filter(|a|a.len()<=256).ok_or("ORDINARY_EXCLUSION_IDS")?;let mut previous:Option<String>=None;
  for raw in ids{let ex=id(raw)?;if raw.as_str()!=Some(ex.as_str())||previous.as_ref().is_some_and(|p|p>=&ex){return Err("ORDINARY_EXCLUSION_SORTED_UNIQUE_IDS")};previous=Some(ex);}
  let previous_hash=self.ordinary_entry_exclusions.as_ref().map_or(Value::Null,|v|v["sha256"].clone());
  if e.get("expectedPreviousExclusionSha256")!=Some(&previous_hash){return Err("ORDINARY_EXCLUSION_PREVIOUS")}
  if let Some(old)=&self.ordinary_entry_exclusions{
   if old["bankScope"]!=*scope{return Err("ORDINARY_EXCLUSION_SCOPE_CHANGED")}
   // Repeated same-set installation does not rewrite historical audit sources.
   // The execution owner normally compares the public getter and emits no row.
   if old["managedExchangeIds"]==e["managedExchangeIds"]{return Ok(())}
  }
  let revision=self.ordinary_entry_exclusions.as_ref().map_or(Some(1),|v|v["revision"].as_u64().and_then(|n|n.checked_add(1))).ok_or("ORDINARY_EXCLUSION_REVISION")?;
  let mut record=json!({"version":1,"revision":revision,"bankScope":scope,"managedExchangeIds":ids,"approvedReviewSetSha256":e["approvedReviewSetSha256"],"controlSha256":e["controlSha256"],"installedAtMs":at,"previousSha256":previous_hash});record["sha256"]=json!(hash(&record));self.ordinary_entry_exclusions=Some(record);Ok(())
 }
}

