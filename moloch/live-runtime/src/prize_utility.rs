//! Offline, conditional tournament utility. No reservation or execution authority.
//! Whole actual portfolio only: a sleeve expectation cannot stand in for wealth.
use crate::{feedback::Result,semantic_states::{self,Compiled,PayoutRule,Side,UNIT},value_surface::BoundSurface,probabilistic_portfolio::{self,Plan,Policy,Leg},collateral_feedback::{RetainedReceipt,ReceiptStore},dispatch::DispatchLog};
use baal_transport_service::{Scope,decimal_units,relationships};
use moloch_replay::session::Session;
use serde::{Serialize,Deserialize};
use serde_json::{Value,json};
use sha2::{Digest,Sha256};
use std::collections::{BTreeMap,BTreeSet};
fn need(v:bool,e:&'static str)->Result<()>{if v{Ok(())}else{Err(e.into())}}
fn add(a:i128,b:i128)->Result<i128>{a.checked_add(b).ok_or_else(||"PRIZE_RANGE".into())}
fn mul(a:i128,b:i128)->Result<i128>{a.checked_mul(b).ok_or_else(||"PRIZE_RANGE".into())}
fn sha(b:&[u8])->String{format!("{:x}",Sha256::digest(b))}
fn hex(s:&str)->bool{s.len()==64&&s.bytes().all(|b|b.is_ascii_digit()||(b'a'..=b'f').contains(&b))}
fn atom(s:&str)->bool{!s.is_empty()&&s.len()<=128&&!s.chars().any(char::is_control)}
fn gcd(mut a:i128,mut b:i128)->i128{while b!=0{let r=a%b;a=b;b=r;}a}
/// Exact rational in micro-prize units; no float/tie-rounding convention implied.
#[derive(Clone,Debug,PartialEq,Eq,Serialize)]#[serde(rename_all="camelCase")]
pub struct Exact{pub numerator:i128,pub denominator:i128}
impl Exact{
 fn new(n:i128,d:i128)->Result<Self>{need(d>0,"PRIZE_DENOMINATOR")?;let g=gcd(n.checked_abs().ok_or("PRIZE_RANGE")?,d);Ok(Self{numerator:n/g,denominator:d/g})}
 fn plus(&self,b:&Self)->Result<Self>{let g=gcd(self.denominator,b.denominator);Self::new(add(mul(self.numerator,b.denominator/g)?,mul(b.numerator,self.denominator/g)?)?,mul(self.denominator/g,b.denominator)?)}
 fn minus(&self,b:&Self)->Result<Self>{self.plus(&Self::new(b.numerator.checked_neg().ok_or("PRIZE_RANGE")?,b.denominator)?)}
 fn less(&self,b:&Self)->Result<bool>{let g=gcd(self.denominator,b.denominator);Ok(mul(self.numerator,b.denominator/g)?<mul(b.numerator,self.denominator/g)?)}
}
#[derive(Clone,Copy,Debug,Serialize,Deserialize)]#[serde(rename_all="camelCase")]
pub enum TieRule{SplitOccupiedPrizes,LoseTies}
#[derive(Clone,Debug,Serialize,Deserialize)]#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct Atom{pub id:String,pub state_id:String,pub weight_ppm:u32,/// Joint terminal wealth, not today's balances or independent marginal draws.
 pub opponent_wealth:Vec<String>}
#[derive(Clone,Debug,Serialize,Deserialize)]#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct ThresholdModel{pub id:String,pub atoms:Vec<Atom>}
#[derive(Clone,Debug,Serialize,Deserialize)]#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct Source{pub kind:String,pub sha256:String,pub observed_at_ms:u64,pub maximum_age_ms:u64}
#[derive(Clone,Debug,Serialize,Deserialize)]#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct ReviewInput{
 pub version:u32,pub run_id:String,pub bank_manifest_sha256:String,pub tournament_id:String,pub tournament_slug:String,
 pub model_sha256:String,pub semantic_sha256:String,pub reviewed_at_ms:u64,pub valid_until_ms:u64,
 pub prizes:Vec<String>,pub tie_rule:TieRule,pub opponent_count:usize,pub complete_opponent_field:bool,
 pub models:Vec<ThresholdModel>,pub sources:Vec<Source>,pub assumption_label:String,
 /// Nonzero debt requires separately reviewed fixed terminal liability terms.
 pub terminal_debt_equals_observed:bool,
}
/// Original bytes + independent allowlists; a hash cannot authenticate a server.
pub struct Original<'a>{pub sha256:&'a str,pub bytes:&'a [u8]}
pub struct Reviewed{input:ReviewInput,sha:String,until:u64,prizes:Vec<i128>}
impl Reviewed{
 pub fn load(bytes:&[u8],expected:&str,approved_reviews:&BTreeSet<String>,approved_sources:&BTreeSet<String>,originals:&[Original<'_>],now:u64)->Result<Self>{
  need(bytes.len()<=4*1024*1024&&hex(expected)&&sha(bytes)==expected&&approved_reviews.contains(expected),"PRIZE_REVIEW_NOT_APPROVED")?;
  let r:ReviewInput=serde_json::from_slice(bytes)?;
  need(r.version==1&&atom(&r.run_id)&&hex(&r.bank_manifest_sha256)&&hex(&r.model_sha256)&&hex(&r.semantic_sha256)&&!r.tournament_id.is_empty()&&!r.tournament_slug.is_empty()
   &&r.reviewed_at_ms<r.valid_until_ms&&(1..=128).contains(&r.opponent_count)&&r.complete_opponent_field&&(1..=32).contains(&r.models.len())&&(1..=129).contains(&r.prizes.len())&&r.prizes.len()<=r.opponent_count+1
   &&!r.assumption_label.trim().is_empty()&&r.assumption_label.len()<=4096&&(3..=32).contains(&r.sources.len())&&originals.len()==r.sources.len(),"PRIZE_REVIEW_SHAPE")?;
  let prizes=r.prizes.iter().map(|x|decimal_units(x).map_err(Into::into)).collect::<Result<Vec<_>>>()?;need(prizes.iter().all(|p|*p>=0)&&prizes.windows(2).all(|w|w[0]>=w[1]),"PRIZE_LADDER_ORDER")?;
  let mut until=r.valid_until_ms;let mut ids=BTreeSet::new();let mut kinds=BTreeSet::new();let mut total_bytes=0usize;
  for s in &r.sources{need(hex(&s.sha256)&&approved_sources.contains(&s.sha256)&&ids.insert(&s.sha256)&&s.observed_at_ms<=r.reviewed_at_ms&&s.maximum_age_ms>0&&s.maximum_age_ms<=86_400_000,"PRIZE_SOURCE")?;
   let originals=originals.iter().filter(|o|o.sha256==s.sha256).collect::<Vec<_>>();need(originals.len()==1&&!originals[0].bytes.is_empty()&&sha(originals[0].bytes)==s.sha256,"PRIZE_ORIGINAL_SOURCE")?;
   total_bytes=total_bytes.checked_add(originals[0].bytes.len()).ok_or("PRIZE_RANGE")?;need(total_bytes<=32*1024*1024,"PRIZE_SOURCE_SIZE")?;
   until=until.min(s.observed_at_ms.checked_add(s.maximum_age_ms).ok_or("PRIZE_RANGE")?);kinds.insert(s.kind.as_str());
  }
  need(["prize_rules","leaderboard_observation","opponent_projection"].iter().all(|k|kinds.contains(k)),"PRIZE_SOURCE_KINDS")?;
  if r.terminal_debt_equals_observed{need(kinds.contains("terminal_liability_terms"),"PRIZE_DEBT_TERMS")?;}
  let mut model_ids=BTreeSet::new();let mut work=0usize;
  for m in &r.models{need(atom(&m.id)&&model_ids.insert(&m.id)&&(1..=4096).contains(&m.atoms.len()),"PRIZE_MODELS")?;let mut atoms=BTreeSet::new();let mut mass=0u64;
   for a in &m.atoms{need(atom(&a.id)&&atom(&a.state_id)&&atoms.insert(&a.id)&&a.weight_ppm>0&&a.opponent_wealth.len()==r.opponent_count,"PRIZE_ATOM")?;
    for w in &a.opponent_wealth{decimal_units(w)?;}mass=mass.checked_add(a.weight_ppm as u64).ok_or("PRIZE_RANGE")?;work=work.checked_add(a.opponent_wealth.len()).ok_or("PRIZE_RANGE")?;
   }need(mass==1_000_000&&work<=1_000_000,"PRIZE_MASS_OR_WORK")?;
  }
  let out=Self{input:r,sha:expected.into(),until,prizes};out.current(now)?;Ok(out)
 }
 fn current(&self,now:u64)->Result<()>{need(self.input.reviewed_at_ms<=now&&now<self.until,"PRIZE_STALE_SOURCE")}
 pub fn sha256(&self)->&str{&self.sha}
 fn context(&self,run:&str,manifest:&str,scope:&Scope,model:&str,semantic:&str)->Result<()>{let r=&self.input;need(r.run_id==run&&r.bank_manifest_sha256==manifest&&r.tournament_id==scope.tournament_id&&r.tournament_slug==scope.tournament_slug&&r.model_sha256==model&&r.semantic_sha256==semantic,"PRIZE_SCOPE_OR_MODEL")}
}
#[derive(Clone)]struct Held{exchange:String,side:Side,lots:Vec<(u64,i128)>}
fn quantity(h:&Held)->Result<u64>{h.lots.iter().try_fold(0u64,|a,l|a.checked_add(l.0).ok_or_else(||"PRIZE_RANGE".into()))}
fn principal(h:&Held)->Result<i128>{h.lots.iter().try_fold(0,|a,l|add(a,mul(l.0 as i128,l.1)?))}
fn inventory(v:&Value)->Result<Vec<Held>>{
 let mut held=vec![];for(ex,sides)in v.as_object().ok_or("PRIZE_POSITIONS")?{for(label,side)in [("YES",Side::Yes),("NO",Side::No)]{
  let rows=sides[label].as_array().ok_or("PRIZE_LOTS")?;need(rows.len()<=65536,"PRIZE_LOT_BOUND")?;let mut lots=vec![];
  for row in rows{let n=row["quantity"].as_u64().filter(|n|*n>0).ok_or("PRIZE_LOT")?;let p=decimal_units(row["price"].as_str().ok_or("PRIZE_BASIS")?)?;need((0..=UNIT).contains(&p),"PRIZE_BASIS")?;lots.push((n,p));}
  if !lots.is_empty(){held.push(Held{exchange:ex.clone(),side,lots});}
 }}Ok(held)
}
struct State{id:String,weight:u32,payouts:Vec<PayoutRule>}
fn states(surface:&BoundSurface,c:&Compiled)->Result<Vec<State>>{
 need(surface.source_sha256()==c.scenario_source_sha256()&&surface.contracts().len()==c.contracts().len(),"PRIZE_MODEL_BINDING")?;
 for(a,b)in surface.contracts().iter().zip(c.contracts()){need(a.exchange_id==b.exchange_id&&a.market_id==b.market_id&&a.contract_id==b.contract_id&&a.official_root_sha256==b.official_root_sha256,"PRIZE_CONTRACT_BINDING")?;}
 let exceptional=c.exception_weights_ppm();let exception_mass=exceptional.values().map(|w|*w as u64).sum::<u64>();need(exception_mass<1_000_000,"PRIZE_MASS")?;
 let weights=probabilistic_portfolio::scaled_ordinary_weights(&surface.scenarios().iter().map(|s|(s.id.clone(),s.weight_ppm)).collect::<Vec<_>>(),1_000_000-exception_mass as u32)?;
 let mut out=vec![];let mut used=BTreeSet::new();
 for s in surface.scenarios(){let row=c.states().iter().find(|r|r.id==s.id).ok_or("PRIZE_STATE_MISSING")?;need(row.kind==semantic_states::StateKind::PartyWinner&&used.insert(&row.id)&&!exceptional.contains_key(&row.id)&&s.payout_micros.len()==c.contracts().len(),"PRIZE_STATE")?;
  for(i,b)in surface.contracts().iter().enumerate(){let side=if b.side==moloch_replay::Side::Yes{Side::Yes}else{Side::No};need(row.outcomes[i].payout(side,1,0)?==s.payout_micros[i],"PRIZE_MODEL_PAYOUT")?;}out.push(State{id:row.id.clone(),weight:weights[&row.id],payouts:row.outcomes.clone()});
 }
 for(id,w)in exceptional{let row=c.states().iter().find(|r|&r.id==id).ok_or("PRIZE_STATE_MISSING")?;need(used.insert(&row.id),"PRIZE_DUPLICATE_STATE")?;out.push(State{id:id.clone(),weight:*w,payouts:row.outcomes.clone()});}
 need(used.len()==c.states().len(),"PRIZE_UNWEIGHTED_STATE")?;Ok(out)
}
/// Original retained account evidence; no HTTP request or JSON receipt minting.
pub struct AccountEvidence<'a>{pub root:&'a std::path::Path,pub account:&'a RetainedReceipt,pub positions:&'a RetainedReceipt,pub open_orders:&'a RetainedReceipt,pub collateral:&'a RetainedReceipt}
fn profile_uuid(s:&str)->bool{s.len()==36&&s.bytes().enumerate().all(|(i,b)|if [8,13,18,23].contains(&i){b==b'-'}else{b.is_ascii_hexdigit()})}
fn instant(s:&str)->Result<i64>{chrono::DateTime::parse_from_rfc3339(s)?.timestamp_nanos_opt().ok_or_else(||"PRIZE_LOT_CLOCK".into())}
fn curve(lots:&[(u64,i128)])->Result<Vec<(u64,i128)>>{let mut out:Vec<(u64,i128)>=vec![];for(n,p)in lots{if let Some(last)=out.last_mut().filter(|l|l.1==*p){last.0=last.0.checked_add(*n).ok_or("PRIZE_RANGE")?;}else{out.push((*n,*p));}}Ok(out)}
fn official_lots(row:&Value,received:&str,expected:&[(u64,i128)],ids:&mut BTreeSet<String>)->Result<()>{
 let signed=row["quantity"].as_i64().filter(|q|*q!=i64::MIN).ok_or("PRIZE_LOT_QUANTITY")?;let side=if signed<0{"NO"}else{"YES"};let received=instant(received)?;
 let rows=row["lots"].as_array().filter(|r|r.len()<=65536).ok_or("PRIZE_OFFICIAL_LOTS_REQUIRED")?;let mut ordered=vec![];let mut quantity=0u64;
 for l in rows{let id=l["lotId"].as_str().map(str::to_owned).or_else(||l["lotId"].as_u64().map(|id|id.to_string())).ok_or("PRIZE_LOT_ID")?;
  need(!id.is_empty()&&!id.starts_with('0')&&id.bytes().all(|b|b.is_ascii_digit())&&ids.insert(id),"PRIZE_DUPLICATE_LOT_ID")?;let n=l["quantity"].as_u64().filter(|q|*q>0).ok_or("PRIZE_LOT_QUANTITY")?;
  let price=baal_transport_service::prices::official_tick_units(&l["entryPrice"])?;let at=instant(l["openedAt"].as_str().ok_or("PRIZE_LOT_CLOCK")?)?;
  need(l["side"]==side&&at<=received,"PRIZE_LOT_SIDE_OR_CLOCK")?;quantity=quantity.checked_add(n).ok_or("PRIZE_RANGE")?;ordered.push((at,n,price));
 }
 need(quantity==signed.unsigned_abs(),"PRIZE_LOT_QUANTITY")?;ordered.sort_by_key(|l|l.0);
 need(ordered.windows(2).all(|w|w[0].0!=w[1].0||w[0].2==w[1].2),"PRIZE_FIFO_ORDER_AMBIGUOUS")?;
 let official=ordered.iter().map(|(_,n,p)|(*n,*p)).collect::<Vec<_>>();need(curve(&official)?==curve(expected)?,"PRIZE_FIFO_BASIS_MISMATCH")
}
fn account(bank:&Session,scope:&Scope,e:&AccountEvidence<'_>,review:&Reviewed,now:u64)->Result<(i128,i128,Vec<String>)>{
 let paths=[format!("/account?tournamentId={}",scope.tournament_id),format!("/tournaments/{}/portfolio/positions",scope.tournament_slug),format!("/orders?status=open&limit=200&tournamentId={}",scope.tournament_id),format!("/portfolio/collateral?tournamentId={}",scope.tournament_id)];let mut hashes=vec![];let mut min=u64::MAX;let mut max=0;
 for(r,path)in [e.account,e.positions,e.open_orders,e.collateral].iter().zip(paths){r.verify_root(e.root)?;let r=r.receipt();let start=crate::daily_windows::clock(&r.request_started_at)?;let end=crate::daily_windows::clock(&r.received_at)?;
  need(r.status==200&&r.endpoint==path&&start<=end&&end<=now&&now-start<=20_000,"PRIZE_ACCOUNT_SCOPE_OR_CLOCK")?;min=min.min(start);max=max.max(end);hashes.push(crate::feedback::digest(r)?);
 }need(max-min<=20_000,"PRIZE_ACCOUNT_WINDOW")?;
 let a=e.account.receipt();let economics=relationships::parse_buying_power(&a.data,scope)?;let cash=economics.gross_cash_units.ok_or("PRIZE_CASH_UNKNOWN")?;
 need(cash==crate::authority::reconciled_expected_cash(bank)?,"PRIZE_CASH_NOT_RECONCILED")?;
 // Historical identity evidence is bound by the immutable seed, not renewed.
 // Its old receipt is NOT subject to the fresh account packet's20s window.
 let seed=bank.live_account_seed().ok_or("PRIZE_ACCOUNT_SEED")?;let origin=RetainedReceipt::from_recorded_digest(e.root,seed["accountSnapshotHash"].as_str().ok_or("PRIZE_ACCOUNT_SEED")?,ReceiptStore::Ordinary)?;origin.verify_root(e.root)?;let original=origin.receipt();
 let original_start=crate::daily_windows::clock(&original.request_started_at)?;let original_end=crate::daily_windows::clock(&original.received_at)?;let seed_at=crate::daily_windows::clock(seed["observedAt"].as_str().ok_or("PRIZE_ACCOUNT_SEED")?)?;
 need(original.status==200&&original.endpoint==format!("/account?tournamentId={}",scope.tournament_id)&&original_start<=original_end&&original_end==seed_at&&seed_at<=now
  &&original.data["id"].as_str().is_some_and(profile_uuid)&&a.data["id"]==original.data["id"]&&relationships::parse_buying_power(&original.data,scope)?.gross_cash_units==Some(decimal_units(seed["availableCash"].as_str().ok_or("PRIZE_ACCOUNT_SEED")?)?),"PRIZE_PROFILE_ORIGIN")?;
 for receipt in [original,a]{if let Some(tid)=receipt.data.get("tournamentId"){need(tid==&scope.tournament_id,"PRIZE_PROFILE_TOURNAMENT")?;}}
 if let Some(allocation)=bank.engine().owner_capital_allocation_record(){need(allocation["bankScope"]["profileId"]==a.data["id"],"PRIZE_PROFILE_ALLOCATION")?;}
 hashes.push(crate::feedback::digest(original)?);
 let collateral=relationships::parse_collateral(&e.collateral.receipt().data,scope)?;need(collateral.components.iter().all(|x|a.data["id"]==x.profile_id),"PRIZE_PROFILE")?;
 if collateral.outstanding_liability_units>0{need(review.input.terminal_debt_equals_observed,"PRIZE_TERMINAL_LIABILITY_UNMODELED")?;}
 let p=&e.positions.receipt().data;need(p.get("pagination").is_none(),"PRIZE_POSITIONS_INCOMPLETE")?;let rows=p["positions"].as_array().filter(|r|r.len()<=4096).ok_or("PRIZE_POSITIONS")?;let mut seen=BTreeSet::new();let mut lot_ids=BTreeSet::new();let held=inventory(&bank.engine().wallet.positions())?;
 for row in rows{let ex=row["exchangeId"].as_str().ok_or("PRIZE_POSITION_SCOPE")?;need(seen.insert(ex.to_owned())&&scope.exchanges.get(ex)==row["marketId"].as_str().map(|s|s.to_owned()).as_ref(),"PRIZE_POSITION_SCOPE")?;
  let yes=bank.engine().wallet.quantity(ex,moloch_replay::Side::Yes);let no=bank.engine().wallet.quantity(ex,moloch_replay::Side::No);need(yes.min(no)==0&&row["quantity"].as_i64().map(i128::from)==Some(yes as i128-no as i128),"PRIZE_POSITION_MISMATCH")?;
  let expected=held.iter().find(|h|h.exchange==ex&&h.side==if no>0{Side::No}else{Side::Yes}).map(|h|h.lots.as_slice()).unwrap_or(&[]);official_lots(row,&e.positions.receipt().received_at,expected,&mut lot_ids)?;
 }need(held.iter().all(|h|seen.contains(&h.exchange)),"PRIZE_UNATTRIBUTED_HOLDING")?;
 let o=&e.open_orders.receipt().data;need(o["coverage"]["complete"]==true&&o["pagination"]["hasMore"]==false&&o["pagination"].get("nextCursor")==Some(&Value::Null)&&o["data"].as_array().is_some_and(Vec::is_empty)&&bank.engine().wallet.reserved()?==0,"PRIZE_OPEN_LIABILITY")?;
 Ok((cash,collateral.outstanding_liability_units,hashes))
}
fn wealth(cash:i128,debt:i128,held:&[Held],contracts:&[String],state:&State)->Result<i128>{
 need(cash>=0&&debt>=0&&contracts.len()==state.payouts.len(),"PRIZE_WEALTH_SHAPE")?;let mut out=add(cash,-debt)?;let mut seen=BTreeSet::new();
 for h in held{need(seen.insert((&h.exchange,h.side)),"PRIZE_DUPLICATE_INVENTORY")?;let i=contracts.iter().position(|ex|ex==&h.exchange).ok_or("PRIZE_UNMODELED_HOLDING")?;out=add(out,state.payouts[i].payout(h.side,quantity(h)?,principal(h)?)?)?;}Ok(out)
}
fn trade(cash:i128,held:&[Held],legs:&[Leg],fees:i128)->Result<(i128,Vec<Held>,i128,i128)>{
 need(fees>=0&&(1..=10).contains(&legs.len()),"PRIZE_FEE_OR_LEGS")?;let mut after=held.to_vec();let mut seen=BTreeSet::new();let mut buys=0;let mut sells=0;
 for l in legs{need(seen.insert(&l.exchange_id)&&l.quantity>0&&(0..=UNIT).contains(&l.limit_price),"PRIZE_DUPLICATE_OR_INVALID_LEG")?;let value=mul(l.quantity as i128,l.limit_price)?;
  if l.action=="buy"{need(!after.iter().any(|h|h.exchange==l.exchange_id&&h.side!=l.side),"PRIZE_OPPOSITE_NETTING_UNSUPPORTED")?;buys=add(buys,value)?;if let Some(h)=after.iter_mut().find(|h|h.exchange==l.exchange_id&&h.side==l.side){h.lots.push((l.quantity,l.limit_price));}else{after.push(Held{exchange:l.exchange_id.clone(),side:l.side,lots:vec![(l.quantity,l.limit_price)]});}}
  else{need(l.action=="sell","PRIZE_ACTION")?;let h=after.iter_mut().find(|h|h.exchange==l.exchange_id&&h.side==l.side).ok_or("PRIZE_UNCOVERED_SELL")?;let mut left=l.quantity;for lot in &mut h.lots{let n=left.min(lot.0);lot.0-=n;left-=n;if left==0{break}}need(left==0,"PRIZE_UNCOVERED_SELL")?;h.lots.retain(|l|l.0>0);sells=add(sells,value)?;}
 }need(add(buys,fees)?<=cash,"PRIZE_GROSS_FUNDING")?;after.retain(|h|!h.lots.is_empty());Ok((add(add(cash,sells)?,-add(buys,fees)?)?,after,buys,sells))
}
#[derive(Clone,Debug,Serialize)]#[serde(rename_all="camelCase")]
pub struct CaseScore{pub id:String,pub baseline_expected_prize:Exact,pub candidate_expected_prize:Exact,pub incremental_expected_prize:Exact,pub baseline_beats_leader_ppm:u32,pub candidate_beats_leader_ppm:u32,pub baseline_ties_leader_ppm:u32,pub candidate_ties_leader_ppm:u32}
#[derive(Clone,Debug,Serialize)]#[serde(rename_all="camelCase")]
pub struct Score{pub cases:Vec<CaseScore>,pub worst_incremental_expected_prize:Exact,pub robust_positive_in_declared_models:bool,pub modeled_baseline_wealth:Vec<(String,i128)>,pub modeled_candidate_wealth:Vec<(String,i128)>,pub evidence:Value,pub execution_authorized:bool,pub calibrated_probability_claimed:bool}
#[derive(Debug,Serialize)]#[serde(tag="state",rename_all="camelCase")]
pub enum Evaluation{Unavailable{reason:String},Available{score:Score}}
fn prize(wealth:i128,opponents:&[i128],ladder:&[i128],tie:TieRule)->Result<Exact>{
 let above=opponents.iter().filter(|w|**w>wealth).count();let equal=opponents.iter().filter(|w|**w==wealth).count();
 match tie{TieRule::LoseTies=>Exact::new(ladder.get(above+equal).copied().unwrap_or(0),1),TieRule::SplitOccupiedPrizes=>{let total=(above..=above+equal).try_fold(0,|n,i|add(n,ladder.get(i).copied().unwrap_or(0)))?;Exact::new(total,(equal+1)as i128)}}
}
fn score(review:&Reviewed,states:&[State],before:&[(String,i128)],after:&[(String,i128)])->Result<(Vec<CaseScore>,Exact)>{
 let expected:BTreeMap<_,_>=states.iter().map(|s|(s.id.as_str(),s.weight as u64)).collect();need(expected.len()==states.len(),"PRIZE_DUPLICATE_STATE")?;let b:BTreeMap<_,_>=before.iter().map(|(s,w)|(s.as_str(),*w)).collect();let a:BTreeMap<_,_>=after.iter().map(|(s,w)|(s.as_str(),*w)).collect();need(b.len()==states.len()&&a.len()==states.len(),"PRIZE_WEALTH_COVERAGE")?;
 let mut out=vec![];let mut worst:Option<Exact>=None;
 for m in &review.input.models{let mut marginal:BTreeMap<&str,u64>=expected.keys().map(|id|(*id,0)).collect();let(mut epb,mut epa)=(Exact::new(0,1)?,Exact::new(0,1)?);let(mut bb,mut ab,mut bt,mut at)=(0u32,0u32,0u32,0u32);
  for atom in &m.atoms{let id=atom.state_id.as_str();let n=marginal.get_mut(id).ok_or("PRIZE_UNKNOWN_JOINT_STATE")?;*n=n.checked_add(atom.weight_ppm as u64).ok_or("PRIZE_RANGE")?;let opponents=atom.opponent_wealth.iter().map(|w|decimal_units(w).map_err(Into::into)).collect::<Result<Vec<_>>>()?;let leader=*opponents.iter().max().ok_or("PRIZE_OPPONENTS")?;
   let pb=prize(*b.get(id).ok_or("PRIZE_WEALTH_COVERAGE")?,&opponents,&review.prizes,review.input.tie_rule)?;let pa=prize(*a.get(id).ok_or("PRIZE_WEALTH_COVERAGE")?,&opponents,&review.prizes,review.input.tie_rule)?;
   epb=epb.plus(&Exact::new(mul(pb.numerator,atom.weight_ppm as i128)?,mul(pb.denominator,UNIT)?)?)?;epa=epa.plus(&Exact::new(mul(pa.numerator,atom.weight_ppm as i128)?,mul(pa.denominator,UNIT)?)?)?;
   if b[id]>leader{bb=bb.checked_add(atom.weight_ppm).ok_or("PRIZE_RANGE")?}if a[id]>leader{ab=ab.checked_add(atom.weight_ppm).ok_or("PRIZE_RANGE")?}if b[id]==leader{bt=bt.checked_add(atom.weight_ppm).ok_or("PRIZE_RANGE")?}if a[id]==leader{at=at.checked_add(atom.weight_ppm).ok_or("PRIZE_RANGE")?}
  }need(marginal==expected,"PRIZE_JOINT_MARGINAL_MISMATCH")?;let delta=epa.minus(&epb)?;let lower=match &worst{None=>true,Some(w)=>delta.less(w)?};if lower{worst=Some(delta.clone());}
  out.push(CaseScore{id:m.id.clone(),baseline_expected_prize:epb,candidate_expected_prize:epa,incremental_expected_prize:delta,baseline_beats_leader_ppm:bb,candidate_beats_leader_ppm:ab,baseline_ties_leader_ppm:bt,candidate_ties_leader_ppm:at});
 }Ok((out,worst.ok_or("PRIZE_MODELS_MISSING")?))
}
/// No HTTP, mutation or dispatch. All existing holdings must have joint states;
/// Compiled's current six-contract ceiling can correctly make a large bank unavailable.
pub fn evaluate(bank:&Session,log:&DispatchLog,scope:&Scope,surface:&BoundSurface,c:&Compiled,plan:&Plan,policy:&Policy,e:&AccountEvidence<'_>,review:&Reviewed,now:u64)->Evaluation{
 match evaluate_inner(bank,log,scope,surface,c,plan,policy,e,review,now){Ok(score)=>Evaluation::Available{score},Err(e)=>Evaluation::Unavailable{reason:e.to_string()}}
}
fn evaluate_inner(bank:&Session,log:&DispatchLog,scope:&Scope,surface:&BoundSurface,c:&Compiled,plan:&Plan,policy:&Policy,e:&AccountEvidence<'_>,review:&Reviewed,now:u64)->Result<Score>{
 review.current(now)?;c.validate_time(now)?;surface.validate_current(bank,scope,now)?;plan.validate_current(bank,log,scope,policy,now)?;log.require_resolved_attempts()?;
 review.context(c.run_id(),c.bank_manifest_sha256(),scope,surface.source_sha256(),c.review_sha256())?;
 let r=&review.input;need(bank.engine().is_live()&&bank.live_account_seed().is_some_and(|s|s["runId"]==r.run_id)&&bank.manifest_hash().as_str()==Some(&r.bank_manifest_sha256)&&scope.tournament_id==r.tournament_id&&scope.tournament_slug==r.tournament_slug&&c.run_id()==r.run_id&&c.bank_manifest_sha256()==r.bank_manifest_sha256&&c.tournament_id()==r.tournament_id&&c.tournament_slug()==r.tournament_slug&&surface.source_sha256()==r.model_sha256&&c.review_sha256()==r.semantic_sha256&&plan.model_sha256()==r.model_sha256&&plan.semantic_review_sha256()==r.semantic_sha256&&!bank.engine().package_blocks_submissions(),"PRIZE_SCOPE_OR_MODEL")?;
 let(cash,debt,receipt_hashes)=account(bank,scope,e,review,now)?;let held=inventory(&bank.engine().wallet.positions())?;let contracts=c.contracts().iter().map(|c|c.exchange_id.clone()).collect::<Vec<_>>();need(contracts.iter().collect::<BTreeSet<_>>().len()==contracts.len(),"PRIZE_DUPLICATE_CONTRACT")?;
 for l in plan.legs(){let c=c.contracts().iter().find(|c|c.exchange_id==l.exchange_id).ok_or("PRIZE_UNMODELED_LEG")?;need(c.market_id==l.market_id&&c.contract_id==l.contract_id&&scope.exchanges.get(&l.exchange_id)==Some(&l.market_id),"PRIZE_LEG_SCOPE")?;}
 let(after_cash,after_held,buys,sells)=trade(cash,&held,plan.legs(),plan.metrics().fees)?;need(add(buys,plan.metrics().fees)?==plan.metrics().gross_buy_funding&&sells==plan.metrics().minimum_sell_proceeds,"PRIZE_PLAN_COST_MISMATCH")?;
 let states=states(surface,c)?;let before=states.iter().map(|s|Ok((s.id.clone(),wealth(cash,debt,&held,&contracts,s)?))).collect::<Result<Vec<_>>>()?;let after=states.iter().map(|s|Ok((s.id.clone(),wealth(after_cash,debt,&after_held,&contracts,s)?))).collect::<Result<Vec<_>>>()?;
 let(cases,worst)=score(review,&states,&before,&after)?;Ok(Score{cases,robust_positive_in_declared_models:worst.numerator>0,worst_incremental_expected_prize:worst,modeled_baseline_wealth:before,modeled_candidate_wealth:after,evidence:json!({"bankHead":bank.durable_head(),"reportSha256":semantic_states::digest(&bank.engine().report()?)?,"planSha256":semantic_states::digest(plan.proof())?,"reviewSha256":review.sha,"modelSha256":surface.source_sha256(),"semanticSha256":c.review_sha256(),"receiptSha256":receipt_hashes,"cashUnits":cash,"liabilityUnits":debt,"feesUnits":plan.metrics().fees,"wholeAccountPositionsReconciled":true,"unmodeledHoldingsExcluded":false,"reservedCashSubtractedFromWealth":false,"startingCapitalAddedToCash":false,"probabilityBasis":"declared conditional joint opponent scenarios; not calibrated posterior","finiteStatesExhaustive":false}),execution_authorized:false,calibrated_probability_claimed:false})
}

#[cfg(test)]mod tests{
 use super::*;
 fn leg(action:&str,n:u64,p:i128)->Leg{Leg{market_id:"201".into(),exchange_id:"101".into(),contract_id:"301".into(),side:Side::Yes,action:action.into(),quantity:n,limit_price:p,book_sha256:"a".repeat(64)}}
 fn held()->Held{Held{exchange:"101".into(),side:Side::Yes,lots:vec![(2,200_000),(2,800_000)]}}
 fn state(id:&str,w:u32,p:PayoutRule)->State{State{id:id.into(),weight:w,payouts:vec![p]}}
 fn atom_(id:&str,s:&str,w:u32,opponent:&str)->Atom{Atom{id:id.into(),state_id:s.into(),weight_ppm:w,opponent_wealth:vec![opponent.into()]}}
 fn fixture_input()->(ReviewInput,Vec<Vec<u8>>){
  let bytes=vec![b"synthetic declared prize rules".to_vec(),b"synthetic retained leaderboard observation".to_vec(),b"synthetic conditional opponent projection".to_vec()];
  let sources=["prize_rules","leaderboard_observation","opponent_projection"].iter().zip(&bytes).map(|(k,b)|Source{kind:(*k).into(),sha256:sha(b),observed_at_ms:100,maximum_age_ms:1000}).collect();
  (ReviewInput{version:1,run_id:"run".into(),bank_manifest_sha256:"b".repeat(64),tournament_id:"cup".into(),tournament_slug:"cup".into(),model_sha256:"c".repeat(64),semantic_sha256:"d".repeat(64),reviewed_at_ms:100,valid_until_ms:2000,prizes:vec!["100.000000".into()],tie_rule:TieRule::SplitOccupiedPrizes,opponent_count:1,complete_opponent_field:true,models:vec![ThresholdModel{id:"base".into(),atoms:vec![atom_("a","win",500000,"900"),atom_("b","lose",500000,"900")]}],sources,assumption_label:"Synthetic joint assumptions, no actual leaderboard/prizes".into(),terminal_debt_equals_observed:false},bytes)
 }
 fn load(r:&ReviewInput,bytes:&[Vec<u8>],now:u64)->Result<Reviewed>{let encoded=serde_json::to_vec(r)?;let digest=sha(&encoded);let refs=bytes.iter().map(|b|Original{sha256:r.sources.iter().find(|s|s.sha256==sha(b)).map(|s|s.sha256.as_str()).unwrap_or(""),bytes:b}).collect::<Vec<_>>();Reviewed::load(&encoded,&digest,&[digest.clone()].into(),&r.sources.iter().map(|s|s.sha256.clone()).collect(),&refs,now)}
 #[test]fn whole_wealth_includes_cash_inventory_and_debt_once_not_cost_twice(){let h=held();let win=state("win",1_000_000,PayoutRule::ResolvedYes);assert_eq!(wealth(10*UNIT,UNIT,&[h.clone()],&["101".into()],&win).unwrap(),13*UNIT);let refund=state("refund",1_000_000,PayoutRule::RefundPrincipal);assert_eq!(wealth(10*UNIT,UNIT,&[h],&["101".into()],&refund).unwrap(),11*UNIT);}
 #[test]fn buy_cost_and_fee_reduce_cash_once_refund_does_not_refund_fee(){let(c,h,b,s)=trade(10*UNIT,&[],&[leg("buy",2,400_000)],100_000).unwrap();assert_eq!((c,b,s),(9_100_000,800_000,0));assert_eq!(wealth(c,0,&h,&["101".into()],&state("refund",1_000_000,PayoutRule::RefundPrincipal)).unwrap(),9_900_000);}
 #[test]fn actual_fifo_partial_sell_preserves_remaining_refundable_principal(){let(c,h,b,s)=trade(10*UNIT,&[held()],&[leg("sell",3,500_000)],100_000).unwrap();assert_eq!((b,s),(0,1_500_000));assert_eq!(principal(&h[0]).unwrap(),800_000);assert_eq!(wealth(c,0,&h,&["101".into()],&state("refund",1_000_000,PayoutRule::RefundPrincipal)).unwrap(),12_200_000);}
 #[test]fn duplicate_inventory_unmodeled_positions_and_duplicate_cost_are_rejected(){let s=state("win",1_000_000,PayoutRule::ResolvedYes);assert!(wealth(UNIT,0,&[held(),held()],&["101".into()],&s).is_err());assert!(wealth(UNIT,0,&[held()],&["999".into()],&s).is_err());assert!(trade(10*UNIT,&[],&[leg("buy",1,100_000),leg("buy",1,100_000)],0).is_err());}
 #[test]fn projected_sell_proceeds_cannot_fund_full_gross_buys(){let mut l=leg("buy",3,500_000);l.exchange_id="102".into();assert!(trade(UNIT,&[held()],&[leg("sell",4,500_000),l],0).is_err());assert!(trade(UNIT,&[],&[leg("buy",1,100_000)],-1).is_err());}
 #[test]fn declared_tie_rules_rank_and_split_exactly(){let p=vec![100*UNIT,30*UNIT,10*UNIT];assert_eq!(prize(10,&[9,8],&p,TieRule::SplitOccupiedPrizes).unwrap(),Exact::new(100*UNIT,1).unwrap());assert_eq!(prize(10,&[11,10,10],&p,TieRule::SplitOccupiedPrizes).unwrap(),Exact::new(40*UNIT,3).unwrap());assert_eq!(prize(10,&[11,10,10],&p,TieRule::LoseTies).unwrap(),Exact::new(0,1).unwrap());}
 #[test]fn prize_objective_can_differ_from_expected_terminal_wealth(){let(r,b)=fixture_input();let r=load(&r,&b,200).unwrap();let states=vec![state("win",500000,PayoutRule::ResolvedYes),state("lose",500000,PayoutRule::ResolvedNo)];let before=vec![("win".into(),600*UNIT),("lose".into(),600*UNIT)];let after=vec![("win".into(),1000*UNIT),("lose".into(),0)];let(cases,worst)=score(&r,&states,&before,&after).unwrap();assert_eq!(worst,Exact::new(50*UNIT,1).unwrap());assert_eq!(cases[0].baseline_beats_leader_ppm,0);assert_eq!(cases[0].candidate_beats_leader_ppm,500000);assert!(500*UNIT<600*UNIT);}
 #[test]fn robust_threshold_cases_are_not_averaged_or_marginally_multiplied(){let(mut r,b)=fixture_input();r.models.push(ThresholdModel{id:"stress".into(),atoms:vec![atom_("x","win",500000,"1100"),atom_("y","lose",500000,"100")]});let r=load(&r,&b,200).unwrap();let states=vec![state("win",500000,PayoutRule::ResolvedYes),state("lose",500000,PayoutRule::ResolvedNo)];let before=vec![("win".into(),600*UNIT),("lose".into(),600*UNIT)];let after=vec![("win".into(),1000*UNIT),("lose".into(),0)];let(cases,worst)=score(&r,&states,&before,&after).unwrap();assert_eq!(cases[0].incremental_expected_prize,Exact::new(50*UNIT,1).unwrap());assert_eq!(worst,Exact::new(-50*UNIT,1).unwrap());}
 #[test]fn joint_atoms_must_preserve_original_own_state_mass(){let(mut r,b)=fixture_input();r.models[0].atoms[0].weight_ppm=600000;r.models[0].atoms[1].weight_ppm=400000;let r=load(&r,&b,200).unwrap();let states=vec![state("win",500000,PayoutRule::ResolvedYes),state("lose",500000,PayoutRule::ResolvedNo)];let w=vec![("win".into(),UNIT),("lose".into(),UNIT)];assert!(score(&r,&states,&w,&w).is_err());}
 #[test]fn no_action_same_states_has_exactly_zero_prize_increment(){let(r,b)=fixture_input();let r=load(&r,&b,200).unwrap();let states=vec![state("win",500000,PayoutRule::ResolvedYes),state("lose",500000,PayoutRule::ResolvedNo)];let w=vec![("win".into(),1000*UNIT),("lose".into(),UNIT)];assert_eq!(score(&r,&states,&w,&w).unwrap().1,Exact::new(0,1).unwrap());}
 #[test]fn stale_missing_source_wrong_scope_and_bad_prize_order_withhold(){let(mut r,b)=fixture_input();assert!(load(&r,&b,1100).is_err());assert!(load(&r,&b[..2],200).is_err());r.prizes=vec!["1".into(),"2".into()];assert!(load(&r,&b,200).is_err());let(r,b)=fixture_input();let r=load(&r,&b,200).unwrap();let scope=Scope{tournament_id:"wrong".into(),tournament_slug:"cup".into(),exchanges:BTreeMap::new()};assert!(r.context("run",&"b".repeat(64),&scope,&"c".repeat(64),&"d".repeat(64)).is_err());}
 #[test]fn actual_live_bank_cash_is_not_added_to_starting_capital_again(){let p=std::env::temp_dir().join(format!("prize-bank-{}-{}",std::process::id(),std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));let seed=json!({"runId":"run","availableCash":"1000.000000","accountSnapshotHash":"a".repeat(64),"observedAt":"2026-10-13T00:00:00Z"});let bank=Session::open_live(&p,&json!({}),&seed).unwrap();let held=inventory(&bank.engine().wallet.positions()).unwrap();let s=State{id:"cash-only".into(),weight:1_000_000,payouts:vec![]};assert_eq!(wealth(bank.engine().wallet.cash,0,&held,&[],&s).unwrap(),1000*UNIT);assert_eq!(bank.engine().wallet.initial,1000*UNIT);drop(bank);std::fs::remove_dir_all(p).unwrap();}
 #[cfg(unix)]#[test]fn original_opaque_account_adapter_rejects_profile_and_fifo_substitution(){
  use baal_transport_service::Receipt;use std::{fs,path::Path,os::unix::fs::PermissionsExt};
  struct Temp(std::path::PathBuf);impl Drop for Temp{fn drop(&mut self){let _=fs::remove_dir_all(&self.0);}}
  fn receipt(path:&str,data:Value,at:u64)->Receipt{Receipt{endpoint:path.into(),status:200,request_started_at:crate::daily_windows::iso(at.saturating_sub(1)).unwrap(),received_at:crate::daily_windows::iso(at).unwrap(),data}}
  fn retain(root:&Path,r:&Receipt)->RetainedReceipt{let digest=crate::feedback::digest(r).unwrap();fs::write(root.join("receipts").join(format!("{digest}.json")),serde_json::to_vec(r).unwrap()).unwrap();RetainedReceipt::open(root,r,ReceiptStore::Ordinary).unwrap()}
  let root=Temp(std::env::temp_dir().join(format!("prize-original-adapter-{}-{}",std::process::id(),std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos())));fs::create_dir(&root.0).unwrap();fs::set_permissions(&root.0,fs::Permissions::from_mode(0o700)).unwrap();fs::create_dir(root.0.join("receipts")).unwrap();
  let scope=Scope{tournament_id:"cup".into(),tournament_slug:"cup".into(),exchanges:[("101".into(),"201".into())].into()};let profile="11111111-1111-1111-1111-111111111111";
  let origin=receipt("/account?tournamentId=cup",json!({"id":profile,"balance":1000,"buyingPower":1000,"availableAdvance":0}),0);let _original=retain(&root.0,&origin);let original_digest=crate::feedback::digest(&origin).unwrap();
  let seed=json!({"runId":"run","availableCash":"1000.000000","accountSnapshotHash":original_digest,"observedAt":origin.received_at});let mut bank=Session::open_live(&root.0.join("bank"),&json!({}),&seed).unwrap();
  let lot=|id,qty,price,at|json!({"lotId":id,"quantity":qty,"entryPrice":price,"side":"YES","openedAt":crate::daily_windows::iso(at).unwrap()});
  let original_positions=receipt("/tournaments/cup/portfolio/positions",json!({"positions":[{"exchangeId":"101","marketId":"201","quantity":4,"costBasis":2,"lots":[lot("101",2,0.2,10000),lot("103",2,0.8,10002)]}],"summary":{"totalMarketValue":2,"totalCostBasis":2,"totalUnrealizedPnl":0}}),30000);
  let adoption=crate::inventory::adoption(&original_positions,bank.engine(),"cup","cup",&scope.exchanges,&crate::daily_windows::iso(30000).unwrap()).unwrap();bank.accept("original-adoption",1,&adoption).unwrap();assert_eq!(bank.engine().wallet.cash,998*UNIT);
  let latest=receipt("/account?tournamentId=cup",origin.data.clone(),30500);let account_receipt=retain(&root.0,&latest);
  let mut valid_positions=original_positions.clone();valid_positions.request_started_at=crate::daily_windows::iso(30499).unwrap();valid_positions.received_at=crate::daily_windows::iso(30500).unwrap();
  valid_positions.data["positions"][0]["lots"]=json!([lot("101",1,0.2,10000),lot("102",1,0.2,10001),lot("103",2,0.8,10002)]);valid_positions.data["positions"][0]["costBasis"]=json!(2.01); // Rounded/reported aggregate is not original FIFO principal.
  let positions=retain(&root.0,&valid_positions);let orders=retain(&root.0,&receipt("/orders?status=open&limit=200&tournamentId=cup",json!({"data":[],"coverage":{"complete":true},"pagination":{"hasMore":false,"nextCursor":null,"limit":200}}),30500));let collateral=retain(&root.0,&receipt("/portfolio/collateral?tournamentId=cup",json!({"availableAdvance":0,"data":[]}),30500));
  let(mut input,bytes)=fixture_input();input.bank_manifest_sha256=bank.manifest_hash().as_str().unwrap().into();input.reviewed_at_ms=30000;input.valid_until_ms=32000;for s in &mut input.sources{s.observed_at_ms=30000;}let review=load(&input,&bytes,30500).unwrap();
  let evidence=AccountEvidence{root:&root.0,account:&account_receipt,positions:&positions,open_orders:&orders,collateral:&collateral};let actual=account(&bank,&scope,&evidence,&review,30500).unwrap();assert_eq!((actual.0,actual.1),(1000*UNIT,0)); // Historical origin0 is intentionally older than20s.
  let mut wrong=latest.clone();wrong.data["id"]=json!("22222222-2222-2222-2222-222222222222");let wrong=retain(&root.0,&wrong);let foreign=AccountEvidence{account:&wrong,..evidence};assert_eq!(account(&bank,&scope,&foreign,&review,30500).unwrap_err().to_string(),"PRIZE_PROFILE_ORIGIN");
  for mode in ["basis","same-principal-different-fifo","ambiguous","duplicate-id"]{
   let mut changed=valid_positions.clone();let lots=&mut changed.data["positions"][0]["lots"];
   match mode{"basis"=>{lots[0]["entryPrice"]=json!(0.3);lots[1]["entryPrice"]=json!(0.3);},"same-principal-different-fifo"=>{lots[0]["entryPrice"]=json!(0.3);lots[1]["entryPrice"]=json!(0.3);lots[2]["entryPrice"]=json!(0.7);},"ambiguous"=>lots[2]["openedAt"]=lots[0]["openedAt"].clone(),_=>lots[2]["lotId"]=lots[0]["lotId"].clone()}
   let changed=retain(&root.0,&changed);let changed=AccountEvidence{positions:&changed,..evidence};let err=account(&bank,&scope,&changed,&review,30500).unwrap_err().to_string();assert!(err.starts_with("PRIZE_FIFO_")||err=="PRIZE_DUPLICATE_LOT_ID","{mode}: {err}");
  }
  fs::remove_file(root.0.join("receipts").join(format!("{original_digest}.json"))).unwrap();assert!(account(&bank,&scope,&evidence,&review,30500).is_err());drop(bank);
 }
}
