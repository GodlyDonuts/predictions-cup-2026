//! Causal information models, separate from economic contract equivalence.
//! A model's coefficients must come from its retained training artifact. This
//! module never creates an order, review, payout guarantee or execution permit.
use serde::{Deserialize,Serialize};
use sha2::{Digest,Sha256};
use crate::feedback::Result;
fn need(v:bool,s:&'static str)->Result<()>{if v{Ok(())}else{Err(s.into())}}
fn hex(s:&str)->bool{s.len()==64&&s.bytes().all(|x|x.is_ascii_hexdigit()&&!x.is_ascii_uppercase())}
fn logit(p:f64)->f64{let p=p.clamp(0.000001,0.999999);(p/(1.-p)).ln()}
fn logistic(x:f64)->f64{if x>=0.{1./(1.+(-x).exp())}else{let e=x.exp();e/(1.+e)}}
#[derive(Clone,Debug,Serialize,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct Observation{
 pub source:String,pub contract_root_sha256:String,pub packet_sha256:String,
 pub revision:u64,pub published_at_ms:u64,pub received_at_ms:u64,pub probability:f64,
}
impl Observation{
 fn check(&self,now:u64)->Result<()>{need(!self.source.is_empty()&&hex(&self.contract_root_sha256)&&hex(&self.packet_sha256)
  &&self.revision>0&&self.published_at_ms<=self.received_at_ms&&self.received_at_ms<=now
  &&self.probability.is_finite()&&(0. ..=1.).contains(&self.probability),"INNOVATION_OBSERVATION")}
}
#[derive(Clone,Copy,Debug,Serialize,Deserialize,PartialEq,Eq)]
#[serde(rename_all="camelCase")]
pub enum Kind{ExternalCatchUp,SigLocalDislocation}
#[derive(Clone,Debug,Serialize,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct Model{
 pub version:u32,pub kind:Kind,pub source:String,pub source_contract_root_sha256:String,
 pub target_exchange_id:String,pub target_contract_root_sha256:String,
 pub training_source_sha256:String,pub training_end_ms:u64,pub validation_end_ms:u64,
 pub created_at_ms:u64,pub expires_at_ms:u64,pub maximum_source_age_ms:u64,
 pub maximum_baseline_age_ms:u64,pub half_life_ms:u64,pub prediction_horizon_ms:u64,pub minimum_independent_clusters:u64,
 pub independent_clusters:u64,pub source_common_beta:f64,pub target_common_beta:f64,
 pub response_coefficient:f64,pub response_standard_error:f64,pub residual_standard_deviation:f64,
 pub risk_z:f64,pub minimum_residual:f64,pub assumption_label:String,
}
impl Model{
 pub fn sha256(&self)->Result<String>{Ok(format!("{:x}",Sha256::digest(serde_json::to_vec(self)?)))}
 fn check(&self,trusted:&str,now:u64)->Result<()>{
  need(self.version==1&&self.sha256()?==trusted&&hex(trusted)&&hex(&self.training_source_sha256)
   &&hex(&self.source_contract_root_sha256)&&hex(&self.target_contract_root_sha256)
   &&!self.source.is_empty()&&!self.target_exchange_id.is_empty()&&!self.assumption_label.trim().is_empty()
   &&self.training_end_ms<self.validation_end_ms&&self.validation_end_ms<=self.created_at_ms
   &&self.created_at_ms<=now&&now<self.expires_at_ms&&self.half_life_ms>0
   &&self.maximum_source_age_ms>0&&self.maximum_baseline_age_ms>0&&self.prediction_horizon_ms>0
   &&self.minimum_independent_clusters>=2&&self.independent_clusters>=self.minimum_independent_clusters,"INNOVATION_MODEL_BINDING")?;
  need([self.source_common_beta,self.target_common_beta,self.response_coefficient,self.response_standard_error,self.residual_standard_deviation,self.risk_z,self.minimum_residual].iter().all(|v|v.is_finite())
   &&self.response_standard_error>=0.&&self.residual_standard_deviation>0.&&self.risk_z>0.&&self.minimum_residual>=0.,"INNOVATION_MODEL_ECONOMICS")
 }
}
#[derive(Clone,Debug,Serialize)]
#[serde(rename_all="camelCase")]
pub struct Signal{
 pub kind:Kind,pub exchange_id:String,pub side:String,pub expected_value:f64,pub conservative_value:f64,
 pub residual_log_odds:f64,pub uncertainty_log_odds:f64,pub elapsed_ms:u64,pub half_life_ms:u64,
 pub model_sha256:String,pub source_packet_sha256:String,pub target_packet_sha256:String,
 pub target_contract_root_sha256:String,pub generated_at_ms:u64,
 pub first_available_at_ms:u64,pub expires_at_ms:u64,pub horizon_ms:u64,pub training_source_sha256:String,
}
impl Signal{pub fn is_current(&self,now:u64)->bool{self.first_available_at_ms<=self.generated_at_ms&&self.generated_at_ms<=now&&now<self.expires_at_ms&&self.horizon_ms>0}}
/// SIG baseline must have been available BEFORE the external innovation.
/// The current observation must be at least as recent as that innovation.
/// Common-factor deltas must come from the same causal interval. They are
/// model features, never an assertion that two contracts have equal payoffs.
pub fn evaluate(model:&Model,trusted_sha:&str,external_before:&Observation,external_after:&Observation,
 sig_before:&Observation,sig_current:&Observation,common_delta:f64,now:u64)->Result<Option<Signal>>{
 model.check(trusted_sha,now)?;
 for o in [external_before,external_after,sig_before,sig_current]{o.check(now)?;}
 need(common_delta.is_finite()&&external_before.source==model.source&&external_after.source==model.source
  &&external_before.contract_root_sha256==model.source_contract_root_sha256&&external_after.contract_root_sha256==model.source_contract_root_sha256
  &&sig_before.contract_root_sha256==model.target_contract_root_sha256&&sig_current.contract_root_sha256==model.target_contract_root_sha256
  &&sig_before.source==sig_current.source,"INNOVATION_SCOPE")?;
 need(external_before.revision<external_after.revision&&external_before.received_at_ms<external_after.received_at_ms
  &&external_before.published_at_ms<=external_after.published_at_ms&&external_before.packet_sha256!=external_after.packet_sha256
  &&sig_before.received_at_ms<=external_before.received_at_ms&&sig_current.received_at_ms>=external_after.received_at_ms
  &&model.validation_end_ms<external_before.received_at_ms,"INNOVATION_CAUSALITY")?;
 let elapsed=now-external_after.received_at_ms;
 if elapsed>model.maximum_source_age_ms||now-sig_current.received_at_ms>model.maximum_source_age_ms
  ||external_after.received_at_ms-external_before.received_at_ms>model.maximum_baseline_age_ms
  ||external_after.received_at_ms-sig_before.received_at_ms>model.maximum_baseline_age_ms{return Ok(None)}
 let innovation=logit(external_after.probability)-logit(external_before.probability)-model.source_common_beta*common_delta;
 let sig_change=logit(sig_current.probability)-logit(sig_before.probability)-model.target_common_beta*common_delta;
 let decay=(-std::f64::consts::LN_2*elapsed as f64/model.half_life_ms as f64).exp();
 let residual=(model.response_coefficient*innovation-sig_change)*decay;
 let uncertainty=model.risk_z*(model.residual_standard_deviation.powi(2)+(model.response_standard_error*innovation).powi(2)).sqrt();
 need(innovation.is_finite()&&sig_change.is_finite()&&residual.is_finite()&&uncertainty.is_finite(),"INNOVATION_NONFINITE_MODEL_RESULT")?;
 // Common-factor catch-up belongs to the election curve, not a spurious
 // claim that this external publisher supplied an independent innovation.
 if model.kind==Kind::ExternalCatchUp&&innovation.abs()<=uncertainty{return Ok(None)}
 if residual.abs()<=model.minimum_residual+uncertainty{return Ok(None)}
 let expected=logistic(logit(sig_current.probability)+residual);
 let yes=residual>0.;let conservative=logistic(logit(sig_current.probability)+residual-if yes{uncertainty}else{-uncertainty});
 Ok(Some(Signal{kind:model.kind,exchange_id:model.target_exchange_id.clone(),side:if yes{"YES"}else{"NO"}.into(),
  expected_value:if yes{expected}else{1.-expected},conservative_value:if yes{conservative}else{1.-conservative},
  residual_log_odds:residual,uncertainty_log_odds:uncertainty,elapsed_ms:elapsed,half_life_ms:model.half_life_ms,
  model_sha256:trusted_sha.into(),source_packet_sha256:external_after.packet_sha256.clone(),target_packet_sha256:sig_current.packet_sha256.clone(),
  target_contract_root_sha256:model.target_contract_root_sha256.clone(),generated_at_ms:now,
  first_available_at_ms:external_after.received_at_ms,expires_at_ms:model.expires_at_ms.min(external_after.received_at_ms.saturating_add(model.maximum_source_age_ms)).min(sig_current.received_at_ms.saturating_add(model.maximum_source_age_ms)),horizon_ms:model.prediction_horizon_ms,training_source_sha256:model.training_source_sha256.clone()}))
}
#[cfg(test)]mod tests{
 use super::*;
 fn obs(source:&str,root:char,revision:u64,at:u64,p:f64)->Observation{Observation{source:source.into(),contract_root_sha256:root.to_string().repeat(64),packet_sha256:format!("{revision:064x}"),revision,published_at_ms:at-1,received_at_ms:at,probability:p}}
 fn model()->Model{Model{version:1,kind:Kind::ExternalCatchUp,source:"public".into(),source_contract_root_sha256:"a".repeat(64),target_exchange_id:"966".into(),target_contract_root_sha256:"b".repeat(64),training_source_sha256:"c".repeat(64),training_end_ms:10,validation_end_ms:20,created_at_ms:21,expires_at_ms:100000,maximum_source_age_ms:2000,maximum_baseline_age_ms:10000,half_life_ms:1000,prediction_horizon_ms:1000,minimum_independent_clusters:3,independent_clusters:8,source_common_beta:1.,target_common_beta:1.,response_coefficient:1.,response_standard_error:0.01,residual_standard_deviation:0.01,risk_z:2.,minimum_residual:0.01,assumption_label:"synthetic causal regression fixture".into()}}
 #[test]fn information_already_in_sig_and_common_election_moves_do_not_create_alpha(){let m=model();let h=m.sha256().unwrap();let a=obs("public",'a',1,100,0.5);let b=obs("public",'a',2,200,0.7);let s=obs("SIG",'b',1,90,0.5);let t=obs("SIG",'b',2,200,0.7);assert!(evaluate(&m,&h,&a,&b,&s,&t,0.,200).unwrap().is_none());let flat=obs("SIG",'b',2,200,0.5);assert!(evaluate(&m,&h,&a,&b,&s,&flat,logit(0.7),200).unwrap().is_none());}
 #[test]fn a_causal_unpriced_innovation_has_side_specific_uncertainty_and_decays(){let m=model();let h=m.sha256().unwrap();let a=obs("public",'a',1,100,0.5);let b=obs("public",'a',2,200,0.7);let s=obs("SIG",'b',1,90,0.5);let t=obs("SIG",'b',2,200,0.5);let x=evaluate(&m,&h,&a,&b,&s,&t,0.,200).unwrap().unwrap();assert_eq!(x.side,"YES");assert!(x.conservative_value>0.5&&x.conservative_value<x.expected_value);let mut later=t.clone();later.received_at_ms=1200;let y=evaluate(&m,&h,&a,&b,&s,&later,0.,1200).unwrap().unwrap();assert!(y.expected_value<x.expected_value);assert!(evaluate(&m,&h,&a,&b,&s,&t,0.,3000).unwrap().is_none());}
 #[test]fn future_training_shuffled_availability_wrong_root_and_same_revision_are_rejected(){let m=model();let h=m.sha256().unwrap();let a=obs("public",'a',1,100,0.5);let b=obs("public",'a',2,200,0.7);let s=obs("SIG",'b',1,90,0.5);let t=obs("SIG",'b',2,200,0.5);let mut bad=b.clone();bad.received_at_ms=80;assert!(evaluate(&m,&h,&a,&bad,&s,&t,0.,200).is_err());bad=b.clone();bad.revision=1;assert!(evaluate(&m,&h,&a,&bad,&s,&t,0.,200).is_err());bad=b.clone();bad.contract_root_sha256="d".repeat(64);assert!(evaluate(&m,&h,&a,&bad,&s,&t,0.,200).is_err());let mut future=m.clone();future.validation_end_ms=150;future.created_at_ms=151;assert!(evaluate(&future,&future.sha256().unwrap(),&a,&b,&s,&t,0.,200).is_err());}
}
