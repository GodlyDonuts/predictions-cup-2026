//! Bounded observational timing. Missing timestamps stay missing; this module
//! cannot authorize, retry, credit a fill, or attribute a price move to latency.
use crate::feedback::Result;
use serde::{Deserialize,Serialize};
#[derive(Clone,Copy,Debug,Serialize,Deserialize,PartialEq,Eq)]
#[serde(rename_all="camelCase")]
pub enum Stage{PreflightRead,PolicyAndRisk,IntentDurability,TransportCall,RecordedRequestService,NativeAckApply,OwnedFillFeedback}
#[derive(Clone,Debug,Serialize,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct Span{pub stage:Stage,pub start_ms:u64,pub end_ms:u64}
#[derive(Clone,Debug,Serialize,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct Trace{
 pub version:u32,pub run_id:String,pub native_qid:u64,pub candidate_created_at_ms:Option<u64>,
 pub book_received_at_ms:Option<u64>,pub permit_issued_at_ms:Option<u64>,pub permit_expires_at_ms:Option<u64>,
 pub ack_received_at_ms:Option<u64>,pub fill_executed_at_ms:Option<u64>,pub fill_observed_at_ms:Option<u64>,
 pub fill_applied_at_ms:Option<u64>,pub started_at_ms:u64,pub finished_at_ms:Option<u64>,pub spans:Vec<Span>,
 pub diagnostic_only:bool,
}
#[derive(Debug,Serialize)]
#[serde(rename_all="camelCase")]
pub struct Summary{pub earliest_recorded_ms:u64,pub latest_recorded_ms:u64,pub recorded_span_union_ms:u64,pub outside_recorded_spans_ms:u64,pub book_age_at_ack_ms:Option<u64>,pub fill_observation_delay_ms:Option<u64>}
fn need(v:bool,s:&'static str)->Result<()>{if v{Ok(())}else{Err(s.into())}}
impl Trace{
 pub fn new(run_id:&str,qid:u64,now:u64)->Result<Self>{need(!run_id.is_empty()&&run_id.len()<=128&&qid>0,"TRACE_ID")?;Ok(Self{version:1,run_id:run_id.into(),native_qid:qid,candidate_created_at_ms:None,book_received_at_ms:None,permit_issued_at_ms:None,permit_expires_at_ms:None,ack_received_at_ms:None,fill_executed_at_ms:None,fill_observed_at_ms:None,fill_applied_at_ms:None,started_at_ms:now,finished_at_ms:None,spans:vec![],diagnostic_only:true})}
 pub fn record(&mut self,stage:Stage,start:u64,end:u64)->Result<()>{need(self.spans.len()<32&&start<=end,"TRACE_SPAN_BOUND_OR_CLOCK")?;self.spans.push(Span{stage,start_ms:start,end_ms:end});Ok(())}
 pub fn finish(&mut self,now:u64)->Result<Summary>{need(now>=self.started_at_ms&&self.spans.iter().all(|s|s.end_ms<=now),"TRACE_FINISH_CLOCK")?;self.finished_at_ms=Some(now);self.summary()}
 pub fn summary(&self)->Result<Summary>{
  let end=self.finished_at_ms.ok_or("TRACE_NOT_FINISHED")?;let mut spans=self.spans.clone();spans.sort_by_key(|s|(s.start_ms,s.end_ms));
  let earliest=spans.first().map_or(self.started_at_ms,|s|s.start_ms.min(self.started_at_ms));let mut union=0u64;let mut current:Option<(u64,u64)>=None;
  for s in spans{need(s.start_ms<=s.end_ms&&s.end_ms<=end,"TRACE_SPAN_CLOCK")?;match current{None=>current=Some((s.start_ms,s.end_ms)),Some((a,b))if s.start_ms<=b=>current=Some((a,b.max(s.end_ms))),Some((a,b))=>{union=union.checked_add(b-a).ok_or("TRACE_OVERFLOW")?;current=Some((s.start_ms,s.end_ms));}}}
  if let Some((a,b))=current{union=union.checked_add(b-a).ok_or("TRACE_OVERFLOW")?;}
  let elapsed=end.checked_sub(earliest).ok_or("TRACE_CLOCK")?;
  Ok(Summary{earliest_recorded_ms:earliest,latest_recorded_ms:end,recorded_span_union_ms:union,outside_recorded_spans_ms:elapsed.checked_sub(union).ok_or("TRACE_UNION")?,book_age_at_ack_ms:self.ack_received_at_ms.zip(self.book_received_at_ms).and_then(|(a,b)|a.checked_sub(b)),fill_observation_delay_ms:self.fill_observed_at_ms.zip(self.fill_executed_at_ms).and_then(|(a,b)|a.checked_sub(b))})
 }
 /// Only AFTER the caller's official own-fill event has committed successfully.
 /// Public prints must not call this method. The owner/qid is checked again.
 pub fn observed_owned_fill(&mut self,qid:u64,executed:u64,observed:u64,applied:u64)->Result<()>{need(qid==self.native_qid&&executed<=observed&&observed<=applied,"TRACE_OWNED_FILL_SCOPE_OR_CLOCK")?;self.fill_executed_at_ms=Some(executed);self.fill_observed_at_ms=Some(observed);self.fill_applied_at_ms=Some(applied);self.record(Stage::OwnedFillFeedback,observed,applied)?;self.finish(applied)?;Ok(())}
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn nested_service_spans_are_not_double_counted_or_labeled_cpu(){let mut t=Trace::new("run",1,100).unwrap();t.record(Stage::PreflightRead,50,80).unwrap();t.record(Stage::PolicyAndRisk,100,130).unwrap();t.record(Stage::TransportCall,140,200).unwrap();t.record(Stage::RecordedRequestService,150,190).unwrap();let s=t.finish(210).unwrap();assert_eq!(s.recorded_span_union_ms,120);assert_eq!(s.outside_recorded_spans_ms,40);assert!(s.book_age_at_ack_ms.is_none());}
 #[test]fn feedback_uses_actual_execution_clock_and_cannot_credit_a_different_order(){let mut t=Trace::new("run",2,100).unwrap();t.finish(130).unwrap();assert!(t.observed_owned_fill(3,140,180,200).is_err());assert!(t.fill_executed_at_ms.is_none());t.observed_owned_fill(2,140,180,200).unwrap();assert_eq!(t.summary().unwrap().fill_observation_delay_ms,Some(40));}
 #[test]fn bounds_and_backward_clocks_fail_only_the_diagnostic(){let mut t=Trace::new("run",1,100).unwrap();assert!(t.record(Stage::PolicyAndRisk,110,100).is_err());for _ in 0..32{t.record(Stage::PreflightRead,100,101).unwrap();}assert!(t.record(Stage::PreflightRead,100,101).is_err());assert!(t.finish(99).is_err());assert!(t.finish(101).is_ok());}
}
