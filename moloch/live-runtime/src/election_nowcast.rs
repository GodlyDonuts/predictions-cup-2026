//! Reweight joint election particles using genuine cumulative result vintages.
//! Geography AND vote mode are distinct units. Exchangeability inside a unit is
//! an explicit reviewed model assumption, not inferred from reporting order.
//! Outputs concern ordinary two-party race outcomes, not SIG payoff guarantees.
use crate::feedback::Result;
use serde::{Deserialize,Serialize};
use sha2::{Digest,Sha256};
use std::collections::{BTreeMap,BTreeSet};
fn need(v:bool,s:&'static str)->Result<()>{if v{Ok(())}else{Err(s.into())}}
fn hex(s:&str)->bool{s.len()==64&&s.bytes().all(|x|x.is_ascii_hexdigit()&&!x.is_ascii_uppercase())}
#[derive(Clone,Debug,Serialize,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct Unit{pub id:String,pub race_id:String,pub vote_mode:String,pub definition_source_sha256:String,pub exchangeable_within_unit_mode:bool}
#[derive(Clone,Debug,Serialize,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct UnitPrediction{pub unit_id:String,pub total_two_party_votes:u64,pub democratic_share:f64,pub concentration:f64}
#[derive(Clone,Debug,Serialize,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct Particle{pub id:String,pub prior_weight_micros:u64,pub units:Vec<UnitPrediction>}
#[derive(Clone,Debug,Serialize,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct Model{pub version:u32,pub fitted_through_ms:u64,pub created_at_ms:u64,pub expires_at_ms:u64,pub training_source_sha256:String,pub maximum_result_age_ms:u64,pub units:Vec<Unit>,pub particles:Vec<Particle>,pub assumption_label:String}
impl Model{
 pub fn sha256(&self)->Result<String>{Ok(format!("{:x}",Sha256::digest(serde_json::to_vec(self)?)))}
 fn check(&self,trusted:&str,now:u64)->Result<()>{
  need(self.version==1&&self.sha256()?==trusted&&hex(trusted)&&hex(&self.training_source_sha256)
   &&self.fitted_through_ms<=self.created_at_ms&&self.created_at_ms<=now&&now<self.expires_at_ms
   &&self.maximum_result_age_ms>0&&!self.assumption_label.trim().is_empty()&&self.assumption_label.len()<=4096
   &&!self.units.is_empty()&&self.units.len()<=10000&&!self.particles.is_empty()&&self.particles.len()<=8192,"NOWCAST_MODEL")?;
  let mut ids=BTreeSet::new();for u in &self.units{need(!u.id.is_empty()&&!u.race_id.is_empty()&&!u.vote_mode.is_empty()&&hex(&u.definition_source_sha256)&&u.exchangeable_within_unit_mode&&ids.insert(u.id.as_str()),"NOWCAST_UNIT_MODEL")?;}
  let mut particles=BTreeSet::new();let mut sum=0u64;
  for p in &self.particles{need(!p.id.is_empty()&&particles.insert(&p.id)&&p.prior_weight_micros>0&&p.units.len()==self.units.len(),"NOWCAST_PARTICLE")?;sum=sum.checked_add(p.prior_weight_micros).ok_or("NOWCAST_WEIGHT_RANGE")?;let mut seen=BTreeSet::new();
   for u in &p.units{need(ids.contains(u.unit_id.as_str())&&seen.insert(&u.unit_id)&&u.total_two_party_votes<=1_000_000_000
    &&u.democratic_share.is_finite()&&u.democratic_share>0.&&u.democratic_share<1.&&u.concentration.is_finite()&&u.concentration>=2.&&u.concentration<=1_000_000.,"NOWCAST_UNIT_PREDICTION")?;}
  }need(sum==1_000_000,"NOWCAST_NORMALIZED_PRIOR")
 }
}
#[derive(Clone,Debug,Serialize,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct Vintage{
 pub unit_id:String,pub revision:u64,pub packet_sha256:String,pub original_source_sha256:String,
 pub published_at_ms:u64,pub received_at_ms:u64,pub genuine_first_available_vintage:bool,
 pub cumulative_snapshot:bool,pub democratic_votes:u64,pub republican_votes:u64,pub other_votes:u64,
 pub unit_complete:bool,pub corrects_packet_sha256:Option<String>,
}
#[derive(Clone,Debug,Serialize)]
#[serde(rename_all="camelCase")]
pub struct RacePosterior{pub ordinary_two_party_democratic_win_probability:f64,pub expected_two_party_margin_votes:f64}
#[derive(Clone,Debug,Serialize)]
#[serde(rename_all="camelCase")]
pub struct WeightedParticle{pub id:String,pub posterior_weight:f64,pub races:BTreeMap<String,RacePosterior>}
#[derive(Debug,Serialize)]
#[serde(rename_all="camelCase")]
pub struct Posterior{pub model_sha256:String,pub generated_at_ms:u64,pub source_packet_sha256:Vec<String>,pub effective_particles:f64,pub particles:Vec<WeightedParticle>,pub races:BTreeMap<String,RacePosterior>,pub residual_distribution:String,pub exceptional_party_payoffs_modeled:bool}
// Lanczos log-gamma; all callers have positive alpha/beta/count arguments.
fn lgamma(z:f64)->f64{
 const C:[f64;8]=[676.5203681218851,-1259.1392167224028,771.3234287776531,-176.6150291621406,12.507343278686905,-0.13857109526572012,9.984369578019572e-6,1.5056327351493116e-7];
 if z<0.5{return std::f64::consts::PI.ln()-(std::f64::consts::PI*z).sin().ln()-lgamma(1.-z)}
 let z=z-1.;let mut x=0.9999999999998099;for(i,c)in C.iter().enumerate(){x+=c/(z+i as f64+1.);}let t=z+7.5;0.5*(2.*std::f64::consts::PI).ln()+(z+0.5)*t.ln()-t+x.ln()
}
fn lbeta(a:f64,b:f64)->f64{lgamma(a)+lgamma(b)-lgamma(a+b)}
fn cdf(z:f64)->f64{let x=z.abs();let t=1./(1.+0.2316419*x);let tail=(-0.5*x*x).exp()/(2.*std::f64::consts::PI).sqrt()*t*(0.319381530+t*(-0.356563782+t*(1.781477937+t*(-1.821255978+t*1.330274429))));if z>=0.{1.-tail}else{tail}}
fn latest<'a>(m:&Model,events:&'a [Vintage],now:u64)->Result<BTreeMap<&'a str,&'a Vintage>>{
 let ids:BTreeSet<_>=m.units.iter().map(|u|u.id.as_str()).collect();let mut out:BTreeMap<&str,&Vintage>=BTreeMap::new();let mut received=0;
 for e in events{
  need(ids.contains(e.unit_id.as_str())&&hex(&e.packet_sha256)&&hex(&e.original_source_sha256)&&e.revision>0
   &&e.genuine_first_available_vintage&&e.cumulative_snapshot&&e.published_at_ms<=e.received_at_ms&&e.received_at_ms<=now
   &&m.fitted_through_ms<e.received_at_ms&&received<=e.received_at_ms,"NOWCAST_VINTAGE_CAUSALITY")?;received=e.received_at_ms;
  need(e.democratic_votes.checked_add(e.republican_votes).and_then(|v|v.checked_add(e.other_votes)).is_some_and(|v|v<=1_000_000_000),"NOWCAST_VOTE_RANGE")?;
  if let Some(old)=out.get(e.unit_id.as_str()){
   if old.revision==e.revision{need(serde_json::to_vec(*old)?==serde_json::to_vec(e)?,"NOWCAST_CHANGED_DUPLICATE")?;continue}
   need(e.revision>old.revision&&e.received_at_ms>=old.received_at_ms,"NOWCAST_REVISION")?;
   if let Some(c)=&e.corrects_packet_sha256{need(c==&old.packet_sha256,"NOWCAST_CORRECTION_CHAIN")?;}
   else{need(e.democratic_votes>=old.democratic_votes&&e.republican_votes>=old.republican_votes&&e.other_votes>=old.other_votes&&(!old.unit_complete||e.unit_complete),"NOWCAST_UNDECLARED_CORRECTION")?;}
  }else{need(e.corrects_packet_sha256.is_none(),"NOWCAST_MISSING_CORRECTION_PREFIX")?;}
  out.insert(&e.unit_id,e);
 }
 need(!out.is_empty(),"NOWCAST_NO_GENUINE_VINTAGE")?;
 // Old precinct counts remain known information. Require a genuinely fresh
 // update somewhere, rather than demanding every completed precinct republish.
 need(out.values().any(|v|now-v.received_at_ms<=m.maximum_result_age_ms),"NOWCAST_RESULTS_STALE")?;Ok(out)
}
/// Each calculation uses only the LATEST cumulative unit snapshot. Earlier
/// snapshots establish revision/correction provenance but are never multiplied
/// into likelihood again. National/state dependence survives in joint particles.
pub fn update(model:&Model,trusted_model_sha:&str,vintages:&[Vintage],now:u64)->Result<Posterior>{
 model.check(trusted_model_sha,now)?;let observations=latest(model,vintages,now)?;let units:BTreeMap<_,_>=model.units.iter().map(|u|(u.id.as_str(),u)).collect();let mut raw=Vec::new();
 for p in &model.particles{
  let mut logweight=(p.prior_weight_micros as f64/1_000_000.).ln();let mut moments:BTreeMap<String,(f64,f64)>=BTreeMap::new();
  for u in &p.units{
   let alpha=u.democratic_share*u.concentration;let beta=(1.-u.democratic_share)*u.concentration;
   let(d,r,complete)=observations.get(u.unit_id.as_str()).map_or((0,0,false),|o|(o.democratic_votes,o.republican_votes,o.unit_complete));let n=d+r;
   if n>u.total_two_party_votes&&!complete{logweight=f64::NEG_INFINITY;break}
   if n>0{logweight+=lbeta(alpha+d as f64,beta+r as f64)-lbeta(alpha,beta);}
   let remaining=if complete{0}else{u.total_two_party_votes-n};let a=alpha+d as f64;let b=beta+r as f64;let share=a/(a+b);let rem=remaining as f64;
   let mean=d as f64-r as f64+rem*(2.*share-1.);let variance=4.*rem*share*(1.-share)*(rem+a+b)/(a+b+1.);
   let total=moments.entry(units[u.unit_id.as_str()].race_id.clone()).or_default();total.0+=mean;total.1+=variance;
  }
  raw.push((p.id.clone(),logweight,moments));
 }
 let max=raw.iter().map(|r|r.1).fold(f64::NEG_INFINITY,f64::max);need(max.is_finite(),"NOWCAST_NO_SUPPORTED_TURNOUT_SCENARIO")?;
 let sum=raw.iter().map(|r|(r.1-max).exp()).sum::<f64>();need(sum.is_finite()&&sum>0.,"NOWCAST_WEIGHT_RANGE")?;
 let mut marginal:BTreeMap<String,RacePosterior>=BTreeMap::new();let mut particles=Vec::new();let mut squares=0.;
 for(id,logweight,moments)in raw{let weight=(logweight-max).exp()/sum;if weight==0.{continue}squares+=weight*weight;let mut races=BTreeMap::new();
  for(race,(mean,var))in moments{let win=if var>0.{cdf(mean/var.sqrt()).clamp(0.,1.)}else if mean>0.{1.}else if mean<0.{0.}else{0.5};let aggregate=marginal.entry(race.clone()).or_insert(RacePosterior{ordinary_two_party_democratic_win_probability:0.,expected_two_party_margin_votes:0.});aggregate.ordinary_two_party_democratic_win_probability+=weight*win;aggregate.expected_two_party_margin_votes+=weight*mean;races.insert(race,RacePosterior{ordinary_two_party_democratic_win_probability:win,expected_two_party_margin_votes:mean});}
  particles.push(WeightedParticle{id,posterior_weight:weight,races});
 }
 Ok(Posterior{model_sha256:trusted_model_sha.into(),generated_at_ms:now,source_packet_sha256:observations.values().map(|v|v.packet_sha256.clone()).collect(),effective_particles:1./squares,particles,races:marginal,residual_distribution:"beta_binomial_remaining_votes_normal_margin_approximation".into(),exceptional_party_payoffs_modeled:false})
}
#[cfg(test)]mod tests{
 use super::*;
 fn fixture()->Model{let units=vec![Unit{id:"county-mail".into(),race_id:"A".into(),vote_mode:"mail".into(),definition_source_sha256:"a".repeat(64),exchangeable_within_unit_mode:true},Unit{id:"other-race".into(),race_id:"B".into(),vote_mode:"in-person".into(),definition_source_sha256:"b".repeat(64),exchangeable_within_unit_mode:true}];Model{version:1,fitted_through_ms:10,created_at_ms:20,expires_at_ms:10000,training_source_sha256:"c".repeat(64),maximum_result_age_ms:1000,units,particles:vec![Particle{id:"D-wave".into(),prior_weight_micros:500000,units:vec![UnitPrediction{unit_id:"county-mail".into(),total_two_party_votes:1000,democratic_share:0.7,concentration:40.},UnitPrediction{unit_id:"other-race".into(),total_two_party_votes:1000,democratic_share:0.7,concentration:40.}]},Particle{id:"R-wave".into(),prior_weight_micros:500000,units:vec![UnitPrediction{unit_id:"county-mail".into(),total_two_party_votes:1000,democratic_share:0.3,concentration:40.},UnitPrediction{unit_id:"other-race".into(),total_two_party_votes:1000,democratic_share:0.3,concentration:40.}]}],assumption_label:"synthetic explicitly exchangeable unit-mode particle fixture".into()}}
 fn vintage(rev:u64,d:u64,r:u64)->Vintage{Vintage{unit_id:"county-mail".into(),revision:rev,packet_sha256:format!("{rev:064x}"),original_source_sha256:"d".repeat(64),published_at_ms:100+rev,received_at_ms:101+rev,genuine_first_available_vintage:true,cumulative_snapshot:true,democratic_votes:d,republican_votes:r,other_votes:0,unit_complete:false,corrects_packet_sha256:None}}
 #[test]fn a_result_updates_related_races_through_joint_states(){let m=fixture();let p=update(&m,&m.sha256().unwrap(),&[vintage(1,80,20)],200).unwrap();assert!(p.races["B"].ordinary_two_party_democratic_win_probability>0.9);assert!(p.effective_particles<1.2);assert!(!p.exceptional_party_payoffs_modeled);}
 #[test]fn cumulative_reports_are_not_reused_as_independent_samples(){let m=fixture();let h=m.sha256().unwrap();let a=vintage(1,40,10);let b=vintage(2,80,20);let p=update(&m,&h,&[a,b.clone()],200).unwrap();let q=update(&m,&h,&[b.clone(),b],200).unwrap();assert_eq!(p.effective_particles,q.effective_particles);assert_eq!(p.races["A"].expected_two_party_margin_votes,q.races["A"].expected_two_party_margin_votes);}
 #[test]fn corrections_require_original_chain_and_can_reduce_reported_counts(){let m=fixture();let h=m.sha256().unwrap();let a=vintage(1,80,20);let mut b=vintage(2,20,20);assert!(update(&m,&h,&[a.clone(),b.clone()],200).is_err());b.corrects_packet_sha256=Some(a.packet_sha256.clone());assert!(update(&m,&h,&[a,b.clone()],200).is_ok());assert!(update(&m,&h,&[b],200).is_err());}
 #[test]fn hindsight_totals_future_training_and_changed_revisions_do_not_become_live_evidence(){let mut m=fixture();let mut e=vintage(1,80,20);e.genuine_first_available_vintage=false;assert!(update(&m,&m.sha256().unwrap(),&[e.clone()],200).is_err());e.genuine_first_available_vintage=true;m.fitted_through_ms=150;m.created_at_ms=151;assert!(update(&m,&m.sha256().unwrap(),&[e.clone()],200).is_err());m=fixture();let mut changed=e.clone();changed.democratic_votes=81;assert!(update(&m,&m.sha256().unwrap(),&[e,changed],200).is_err());}
 #[test]fn reported_votes_beyond_every_turnout_particle_and_stale_results_are_rejected(){let m=fixture();let h=m.sha256().unwrap();assert!(update(&m,&h,&[vintage(1,900,900)],200).is_err());assert!(update(&m,&h,&[vintage(1,80,20)],2000).is_err());assert!(update(&m,&h,&[],200).is_err());}
}
