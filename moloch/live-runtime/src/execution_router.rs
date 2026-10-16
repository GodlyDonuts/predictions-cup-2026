//! Strategy/horizon-aware proposal comparison. No authorization or lifecycle
//! mutation. Conditional terminal value, owned short-markout estimates, and
//! current information innovations remain separate quantities.
use crate::{alpha_proposal::{self,Mode,ReviewBounds},execution_distribution,feedback::Result,source_innovation::{Kind,Signal},value_surface::{BoundSurface,UNIT}};
use baal_transport_service::Scope;
use moloch_replay::session::Session;
use serde::{Deserialize,Serialize};
use serde_json::{json,Value};
use std::collections::BTreeSet;
fn need(b:bool,e:&'static str)->Result<()>{if b{Ok(())}else{Err(e.into())}}
#[derive(Clone,Copy,Debug,Serialize,PartialEq,Eq)]
#[serde(rename_all="snake_case")]
pub enum Lane{TerminalValue,RelativeValue,InformationShock}
#[derive(Clone,Debug,Serialize,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct Policy{
 /// Exact learned policy/strategy identity, not a generic calibration claim.
 pub strategy_sha256:String,pub order_expiry_ms:u64,
 /// Explicit model-risk/opportunity charges. Separate from a distribution's
 /// existing uncertainty/tail/capital/write terms; never added twice to it.
 pub model_uncertainty_per_share:i128,pub terminal_tail_charge_per_share:i128,
 pub write_charge_per_order:i128,pub capital_charge_per_million_unit_ms:i128,
}
#[derive(Debug,Serialize)]
pub struct RouteCandidate{
 pub mode:Mode,pub action:Value,pub quantity:u64,pub limit_price:i128,pub gross_funding:i128,
 pub immediate_visible_quantity:u64,pub visible_liquidation_notional:i128,
 pub conditional_value_surplus:i128,pub conditional_model_risk_charge:i128,
 pub conditional_terminal_scheduled_capital_charge:i128,pub conditional_net_after_policy_charges:i128,
 pub scheduled_capital_time_unit_ms:i128,
 /// The configured observer's joint quantity/value model, not p(full)*spread.
 pub owned_execution_model:Option<Value>,pub execution_model_reason:Option<String>,
 pub qualified_joint_score_per_requested_share:Option<f64>,pub valid_until_ms:u64,
 pub provenance:Value,
}
#[derive(Debug,Serialize)]
pub struct Routing{
 pub lane:Lane,pub exchange_id:String,pub horizon_ms:u64,pub candidates:Vec<RouteCandidate>,
 pub deferred:Vec<Value>,pub preferred_mode:Option<Mode>,pub selection_basis:&'static str,
 pub prospective_economic_gate_qualified:bool,pub holding_policy_compatible:bool,
 pub execution_authority:bool,
}
fn mul(a:i128,b:i128)->Result<i128>{a.checked_mul(b).ok_or("ROUTER_RANGE".into())}
fn add(a:i128,b:i128)->Result<i128>{a.checked_add(b).ok_or("ROUTER_RANGE".into())}
fn unit_floor(v:f64)->Result<i128>{need(v.is_finite()&&(0. ..=1.).contains(&v),"ROUTER_SIGNAL_VALUE")?;Ok((v*UNIT as f64).floor()as i128)}
/// Terminal route is actionable proposal comparison under the EXISTING exact
/// reviewed ceiling/caps. An unavailable short-horizon observer is descriptive;
/// it does not veto an independently reviewed live fundamental workflow.
pub fn terminal(bank:&Session,scope:&Scope,surface:&BoundSurface,index:usize,p:&Policy,now:u64)->Result<Routing>{route(bank,scope,surface,index,p,None,&BTreeSet::new(),now)}
/// Optional independently fitted innovation context. The caller must have
/// retained its original Model/Observation inputs. Root/book/hash matching here
/// prevents routing another contract or a stale target observation. This does
/// not certify economic equivalence or approve a new short-horizon hold policy.
pub fn with_innovation(bank:&Session,scope:&Scope,surface:&BoundSurface,index:usize,p:&Policy,signal:&Signal,trusted_models:&BTreeSet<String>,now:u64)->Result<Routing>{route(bank,scope,surface,index,p,Some(signal),trusted_models,now)}
fn route(bank:&Session,scope:&Scope,surface:&BoundSurface,index:usize,p:&Policy,signal:Option<&Signal>,trusted_models:&BTreeSet<String>,now:u64)->Result<Routing>{
 surface.validate_current(bank,scope,now)?;let c=surface.contracts().get(index).ok_or("ROUTER_CONTRACT")?;
 need(p.strategy_sha256.len()==64&&p.strategy_sha256.bytes().all(|b|b.is_ascii_hexdigit())&&now<p.order_expiry_ms&&p.order_expiry_ms-now<=bank.engine().policy()["lifetimeMs"].as_u64().ok_or("ROUTER_POLICY")?&&[p.model_uncertainty_per_share,p.terminal_tail_charge_per_share,p.write_charge_per_order,p.capital_charge_per_million_unit_ms].iter().all(|v|(0..=UNIT).contains(v)),"ROUTER_POLICY")?;
 let mut horizon=c.scheduled_settlement_at_ms.checked_sub(now).ok_or("ROUTER_HORIZON")?;let mut lane=Lane::TerminalValue;let mut value=c.expected_payout_micros;let mut until=surface.valid_until_ms();let mut source=surface.source_sha256().to_owned();let mut model=surface.model_sha256().to_owned();let mut context=Value::Null;
 if let Some(s)=signal{
  need(s.is_current(now)&&s.exchange_id==c.exchange_id&&s.side==if c.side==moloch_replay::Side::Yes{"YES"}else{"NO"}&&s.target_contract_root_sha256==c.official_root_sha256&&s.target_packet_sha256==surface.books()[&c.exchange_id].source.sha256&&trusted_models.contains(&s.model_sha256),"ROUTER_SIGNAL_SOURCE_OR_SCOPE")?;
  let expected=unit_floor(s.expected_value)?;let conservative=unit_floor(s.conservative_value)?;need(conservative<=expected&&s.uncertainty_log_odds.is_finite()&&s.uncertainty_log_odds>=0.&&s.half_life_ms>0,"ROUTER_SIGNAL_VALUE")?;
  value=conservative.min(value);horizon=s.horizon_ms;until=until.min(s.expires_at_ms);source=s.source_packet_sha256.clone();model=s.model_sha256.clone();lane=match s.kind{Kind::ExternalCatchUp=>Lane::InformationShock,Kind::SigLocalDislocation=>Lane::RelativeValue};context=serde_json::to_value(s)?;
 }
 need(horizon>0,"ROUTER_HORIZON")?;let ceiling=value-c.margin_per_share;
 if ceiling<5000{return Ok(Routing{lane,exchange_id:c.exchange_id.clone(),horizon_ms:horizon,candidates:vec![],deferred:vec![json!({"reason":"nonpositive_value_after_review_buffer"})],preferred_mode:None,selection_basis:"none",prospective_economic_gate_qualified:false,holding_policy_compatible:signal.is_none(),execution_authority:false})}
 let bounds=ReviewBounds{exchange_id:c.exchange_id.clone(),market_id:c.market_id.clone(),side:c.side,review_sha256:c.review_sha256.clone(),model_sha256:model,source_sha256:source,reviewed_at_ms:c.reviewed_at_ms,valid_until_ms:until,horizon_ms:horizon,expected_payout:value,acquisition_ceiling:ceiling.min(995000),maximum_principal:c.maximum_principal,maximum_quantity:c.maximum_quantity,modes:c.allowed_modes.clone()};
 let comparison=alpha_proposal::compare(bank,scope,&bounds,now)?;let deferred=comparison.deferred.into_iter().map(|d|json!({"mode":d.mode,"reason":d.reason})).collect();let mut candidates=Vec::new();
 for a in comparison.candidates{
  let n=a.quantity();let capital_time=mul(a.gross_funding(),horizon as i128)?;let scheduled_charge=mul(capital_time,p.capital_charge_per_million_unit_ms)?/UNIT;let risk=mul(add(p.model_uncertainty_per_share,p.terminal_tail_charge_per_share)?,n as i128)?;
  let conditional=a.conditional_forecast_surplus();let net=conditional.checked_sub(risk).and_then(|v|v.checked_sub(scheduled_charge)).and_then(|v|v.checked_sub(p.write_charge_per_order)).ok_or("ROUTER_RANGE")?;
  // Even numerically identical durations do not make executable-book markouts
  // terminal payouts. Carry is deliberately absent from the owned observer.
  let estimate=if signal.is_none(){Err("ROUTER_TERMINAL_TARGET_NOT_OWNED_LIQUIDATION")}else{execution_distribution::estimate_runtime_action(bank,scope,a.action(),&p.strategy_sha256,now,p.order_expiry_ms,horizon,now)};
  let(model,reason,joint)=match estimate{Ok(e)=>{let score=e.qualified_descriptive_sample.then_some(e.net_shadow_score_per_requested_share).flatten();(Some(serde_json::to_value(&e)?),None,score)},Err(reason)=>(None,Some(reason.to_owned()),None)};
  candidates.push(RouteCandidate{mode:a.mode(),action:a.action().clone(),quantity:n,limit_price:a.limit_price(),gross_funding:a.gross_funding(),immediate_visible_quantity:a.immediate_acquisition_quantity(),visible_liquidation_notional:a.visible_liquidation_notional(),conditional_value_surplus:conditional,conditional_model_risk_charge:risk,conditional_terminal_scheduled_capital_charge:scheduled_charge,conditional_net_after_policy_charges:net,scheduled_capital_time_unit_ms:capital_time,owned_execution_model:model,execution_model_reason:reason,qualified_joint_score_per_requested_share:joint,valid_until_ms:a.valid_until_ms().min(p.order_expiry_ms),provenance:json!({"alpha":a.provenance(),"surfaceSha256":surface.source_sha256(),"mappingSha256":c.mapping_sha256,"innovation":context,"valueIsConditionalOnFilledQuantity":true,"scheduledCapitalTimeIsNotActualRelease":true,"delayedSettlementCapitalTimeUnbounded":signal.is_none(),"unknownAndOpenAreNotZero":true,"fullPrincipalIsNotLostOnConfirmedZero":true,"modelAndEmpiricalChargesNotSummed":true,"requiresExistingPhysicalAuthority":true})});
 }
 let empirical=candidates.iter().filter_map(|c|c.qualified_joint_score_per_requested_share.map(|s|(s,c.mode))).filter(|(s,_)|s.is_finite()&&*s>0.).max_by(|a,b|a.0.total_cmp(&b.0));
 let compatible=signal.is_none();let (preferred,basis)=if let Some((_,mode))=empirical{(Some(mode),"positive_owned_horizon_aligned_joint_model")}else{(candidates.iter().find(|c|c.conditional_net_after_policy_charges>0).map(|c|c.mode),"reviewed_mode_order_execution_probability_unknown")};
 Ok(Routing{lane,exchange_id:c.exchange_id.clone(),horizon_ms:horizon,candidates,deferred,preferred_mode:preferred,selection_basis:basis,prospective_economic_gate_qualified:empirical.is_some()&&compatible&&!surface.surface().is_research(),holding_policy_compatible:compatible,execution_authority:false})
}
