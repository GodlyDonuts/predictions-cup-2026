//! Finite human-authorized UTC entry windows; this configuration alone grants no dispatch.
use crate::{authority::Activation,feedback::Result};
use serde::{Serialize,Deserialize};
use serde_json::{Value,json};
pub const DAY_MS:u64=86_400_000;
#[derive(Clone,Debug,Serialize,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct DailyEntryWindows {pub authorization_id:String,pub approved_at:String,pub anchor_ms:u64,pub campaign_end_ms:u64}
pub fn clock(s:&str)->Result<u64>{Ok(u64::try_from(chrono::DateTime::parse_from_rfc3339(s)?.timestamp_millis())?)}
pub fn iso(t:u64)->Result<String>{Ok(chrono::DateTime::from_timestamp_millis(i64::try_from(t)?).ok_or("DAILY_CLOCK_RANGE")?.to_rfc3339_opts(chrono::SecondsFormat::Millis,true))}
impl DailyEntryWindows {
 pub fn validate(&self,a:&Activation,now:u64)->Result<()>{
  if self.authorization_id.is_empty()||self.authorization_id.len()>128||!self.authorization_id.bytes().all(|b|b.is_ascii_alphanumeric()||b==b'-'||b==b'_')||clock(&self.approved_at)?>now||self.anchor_ms!=a.entry_until_ms||self.campaign_end_ms<=self.anchor_ms||self.campaign_end_ms-self.anchor_ms>31*DAY_MS{return Err("DAILY_MANDATE_INVALID".into())}iso(self.campaign_end_ms)?;Ok(())
 }
 pub fn end_at(&self,at:u64)->Result<u64>{
  if at<self.anchor_ms{return Ok(self.anchor_ms)}
  if at>=self.campaign_end_ms{return Ok(self.campaign_end_ms)}
  Ok(self.anchor_ms.checked_add(((at-self.anchor_ms)/DAY_MS+1).checked_mul(DAY_MS).ok_or("DAILY_CLOCK_OVERFLOW")?).ok_or("DAILY_CLOCK_OVERFLOW")?.min(self.campaign_end_ms))
 }
 pub fn mandate(&self,a:&Activation)->Value{json!({"authorizationId":self.authorization_id,"approvedAt":self.approved_at,"anchorMs":self.anchor_ms,"campaignEndMs":self.campaign_end_ms,"intervalMs":DAY_MS,"maximumCommitted":a.maximum_committed,"maximumMarketCost":a.maximum_market_cost,"maximumQuantity":a.maximum_quantity})}
}
#[cfg(test)]mod tests{
 use super::*;
 fn cfg()->DailyEntryWindows{DailyEntryWindows{authorization_id:"human-20261015".into(),approved_at:"1970-01-01T00:00:00Z".into(),anchor_ms:1000,campaign_end_ms:1000+2*DAY_MS+2000}}
 #[test]fn half_open_utc_windows_and_final_partial_window(){let c=cfg();for(at,end)in [(999,1000),(1000,1000+DAY_MS),(1000+DAY_MS-1,1000+DAY_MS),(1000+DAY_MS,1000+2*DAY_MS),(1000+2*DAY_MS,c.campaign_end_ms),(c.campaign_end_ms,c.campaign_end_ms),(u64::MAX,c.campaign_end_ms)]{assert_eq!(c.end_at(at).unwrap(),end)}}
 #[test]fn invalid_authorizations_are_rejected(){
  let a=Activation{run_id:"test".into(),entry_until_ms:1000,maximum_committed:"40000.000000".into(),maximum_market_cost:"10000.000000".into(),maximum_quantity:10000,tournament_id:"cup".into(),tournament_slug:"cup".into(),binary_sha256:"a".repeat(64)};
  cfg().validate(&a,1000).unwrap();
  for case in 0..5{let mut c=cfg();match case{0=>c.anchor_ms=999,1=>c.campaign_end_ms=c.anchor_ms,2=>c.campaign_end_ms=c.anchor_ms+31*DAY_MS+1,3=>c.approved_at="2026-10-15T00:00:00Z".into(),_=>c.authorization_id="bad/key".into()}assert!(c.validate(&a,1000).is_err())}
 }
 #[test]fn timezone_independent_and_strict_schema(){assert_eq!(clock("2026-11-01T01:00:00-07:00").unwrap()+3_600_000,clock("2026-11-01T01:00:00-08:00").unwrap());let mut v=serde_json::to_value(cfg()).unwrap();v["maximumCommitted"]=json!(999999);assert!(serde_json::from_value::<DailyEntryWindows>(v).is_err())}
}
