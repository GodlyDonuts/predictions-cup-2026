//! A reviewed exhaustive-state certificate is deliberately distinct from a
//! finite probabilistic scenario table. This pure module has no account or
//! dispatch authority. Guarantees are conditional on the separately approved
//! completeness proof and original official settlement/fee semantics.
use serde::{Deserialize,Serialize};
use serde_json::Value;
use sha2::{Digest,Sha256};
use std::collections::{BTreeMap,BTreeSet};
pub const UNIT:i128=1_000_000;
pub type Result<T>=std::result::Result<T,&'static str>;
fn need(v:bool,e:&'static str)->Result<()>{if v{Ok(())}else{Err(e)}}
fn hash(b:&[u8])->String{format!("{:x}",Sha256::digest(b))}
fn json_hash<T:Serialize>(v:&T)->Result<String>{Ok(hash(&serde_json::to_vec(v).map_err(|_|"FLOOR_JSON")?))}
fn hex(s:&str)->bool{s.len()==64&&s.bytes().all(|c|c.is_ascii_digit()||(b'a'..=b'f').contains(&c))}
fn atom(s:&str)->bool{!s.is_empty()&&s.len()<=128&&s.bytes().all(|b|b.is_ascii_alphanumeric()||b"-_:".contains(&b))}
fn clock(v:&Value)->Result<u64>{let t=chrono::DateTime::parse_from_rfc3339(v.as_str().ok_or("FLOOR_CLOCK")?).map_err(|_|"FLOOR_CLOCK")?.timestamp_millis();u64::try_from(t).map_err(|_|"FLOOR_CLOCK")}
fn units(v:&Value)->Result<i128>{let s=if let Some(s)=v.as_str(){s.to_owned()}else if v.is_number(){v.to_string()}else{return Err("FLOOR_DECIMAL")};crate::amount(&s)}
fn add(a:i128,b:i128)->Result<i128>{a.checked_add(b).ok_or("FLOOR_OVERFLOW")}
fn mul(a:i128,b:i128)->Result<i128>{a.checked_mul(b).ok_or("FLOOR_OVERFLOW")}
fn fresh(r:&Value,endpoint:&str,now:u64,age:u64)->Result<u64>{need(r["status"]==200&&r["endpoint"]==endpoint,"FLOOR_RECEIPT_ENDPOINT")?;let start=clock(&r["requestStartedAt"])?;let end=clock(&r["receivedAt"])?;need(start<=end&&end<=now&&now-start<=age,"FLOOR_RECEIPT_STALE")?;start.checked_add(age).ok_or("FLOOR_OVERFLOW")}
fn node_scope(data:&Value,tid:&str)->Result<()>{need(data["contexts"].as_array().is_some_and(|rows|rows.iter().filter(|c|c["type"]=="tournament"&&c["tournament"]["id"]==tid).count()==1),"FLOOR_NODE_TOURNAMENT")}

#[derive(Clone,Copy,Debug,Serialize,Deserialize,PartialEq,Eq,PartialOrd,Ord)]
#[serde(rename_all="UPPERCASE")]
pub enum Side{Yes,No}
#[derive(Clone,Copy,Debug,Serialize,Deserialize,PartialEq,Eq)]
#[serde(rename_all="camelCase")]
pub enum Payout{ResolvedYes,ResolvedNo,RefundPrincipal}
#[derive(Clone,Debug,Serialize,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct Contract{pub market_id:String,pub exchange_id:String,pub contract_id:String,pub official_root_sha256:String,pub original_node_sha256:String}
#[derive(Clone,Debug,Serialize,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct State{pub id:String,pub outcomes:Vec<Payout>}
#[derive(Clone,Debug,Serialize,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct Source{pub sha256:String,pub observed_at_ms:u64,pub maximum_age_ms:u64}
#[derive(Clone,Debug,Serialize,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct ExceptionCoverage{
 pub family:String,pub source_sha256:String,
 /// Either nonempty certified states or an independently approved impossibility
 /// proof is required. A zero probability never makes a state impossible.
 pub state_ids:Vec<String>,pub impossible:bool,pub explanation:String,
}
#[derive(Clone,Debug,Serialize,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct CertificateInput{
 pub version:u32,pub tournament_id:String,pub tournament_slug:String,
 pub reviewed_at_ms:u64,pub valid_until_ms:u64,pub contracts:Vec<Contract>,
 pub states:Vec<State>,pub sources:Vec<Source>,pub exceptions:Vec<ExceptionCoverage>,
 pub completeness_proof_sha256:String,pub fee_source_sha256:String,
 pub maximum_fee_per_share_units:i128,pub unresolved_obligations:Vec<String>,
}
#[derive(Deserialize)]#[serde(rename_all="camelCase",deny_unknown_fields)]
struct PartitionProof{kind:String,reviewer_id:String,statement:String,rule_hashes:Vec<String>,state_ids:Vec<String>,state_payoffs_sha256:String,exception_families:Vec<String>,pairwise_disjoint:bool,collectively_exhaustive:bool,fees_are_proven_bounded:bool,fee_source_sha256:String,maximum_fee_per_share_units:i128,unresolved_obligations:Vec<String>}
pub struct OriginalSource<'a>{pub sha256:&'a str,pub bytes:&'a[u8]}
/// Neither Deserialize nor public fields: a finite model/table cannot mint this.
pub struct ExhaustivePayoffCertificate{input:CertificateInput,sha:String,until:u64}
impl ExhaustivePayoffCertificate{
 pub fn sha256(&self)->&str{&self.sha}
 pub fn valid_until_ms(&self)->u64{self.until}
 pub fn contracts(&self)->&[Contract]{&self.input.contracts}
 /// The two approval sets are operator-reviewed ORIGINAL artifact digests. They
 /// are not populated by loading a finite model or by this verifier itself.
 pub fn verify(bytes:&[u8],approved_certificates:&BTreeSet<String>,approved_completeness_proofs:&BTreeSet<String>,originals:&[OriginalSource<'_>],now:u64)->Result<Self>{
  let sha=hash(bytes);need(bytes.len()<=2*1024*1024&&approved_certificates.contains(&sha),"FLOOR_CERTIFICATE_NOT_APPROVED")?;
  let total_bytes=originals.iter().try_fold(0usize,|n,o|n.checked_add(o.bytes.len()).ok_or("FLOOR_SOURCE_BOUND"))?;need(total_bytes<=32*1024*1024,"FLOOR_SOURCE_BOUND")?;
  let r:CertificateInput=serde_json::from_slice(bytes).map_err(|_|"FLOOR_CERTIFICATE_SCHEMA")?;
  need(r.version==1&&atom(&r.tournament_id)&&atom(&r.tournament_slug)&&r.reviewed_at_ms<=now&&now<r.valid_until_ms&&r.valid_until_ms-r.reviewed_at_ms<=366*86_400_000&&(1..=10).contains(&r.contracts.len())&&(1..=4096).contains(&r.states.len())&&(1..=64).contains(&r.sources.len())&&r.sources.len()==originals.len()&&r.unresolved_obligations.is_empty()&&r.maximum_fee_per_share_units>=0&&r.maximum_fee_per_share_units<=UNIT,"FLOOR_CERTIFICATE_SHAPE")?;
  let mut sources=BTreeMap::new();let mut until=r.valid_until_ms;
  for s in &r.sources{
   need(hex(&s.sha256)&&s.observed_at_ms<=r.reviewed_at_ms&&s.maximum_age_ms>0&&s.maximum_age_ms<=366*86_400_000,"FLOOR_SOURCE_CLOCK")?;
   let matches=originals.iter().filter(|o|o.sha256==s.sha256).collect::<Vec<_>>();need(matches.len()==1&&!matches[0].bytes.is_empty()&&matches[0].bytes.len()<=8*1024*1024&&hash(matches[0].bytes)==s.sha256&&sources.insert(s.sha256.clone(),matches[0].bytes).is_none(),"FLOOR_ORIGINAL_SOURCE")?;
   until=until.min(s.observed_at_ms.checked_add(s.maximum_age_ms).ok_or("FLOOR_OVERFLOW")?);
  }
  need(sources.contains_key(&r.fee_source_sha256)&&approved_completeness_proofs.contains(&r.completeness_proof_sha256),"FLOOR_INDEPENDENT_PROOF_REQUIRED")?;
  let proof:PartitionProof=serde_json::from_slice(sources.get(&r.completeness_proof_sha256).ok_or("FLOOR_COMPLETENESS_SOURCE")?).map_err(|_|"FLOOR_COMPLETENESS_SCHEMA")?;
  need(proof.kind=="reviewed_exhaustive_partition_v1"&&atom(&proof.reviewer_id)&&!proof.statement.trim().is_empty()&&proof.statement.len()<=8192&&proof.pairwise_disjoint&&proof.collectively_exhaustive&&proof.unresolved_obligations.is_empty()&&proof.fees_are_proven_bounded&&proof.fee_source_sha256==r.fee_source_sha256&&proof.maximum_fee_per_share_units==r.maximum_fee_per_share_units&&proof.state_payoffs_sha256==json_hash(&r.states)?,"FLOOR_COMPLETENESS_UNPROVEN")?;
  let mut exchanges=BTreeSet::new();let mut roots=BTreeSet::new();
  for c in &r.contracts{
   need(atom(&c.market_id)&&atom(&c.exchange_id)&&atom(&c.contract_id)&&hex(&c.official_root_sha256)&&exchanges.insert(c.exchange_id.clone())&&roots.insert(c.official_root_sha256.clone()),"FLOOR_CONTRACT_SCOPE")?;
   let raw=sources.get(&c.original_node_sha256).ok_or("FLOOR_NODE_SOURCE")?;let node:Value=serde_json::from_slice(raw).map_err(|_|"FLOOR_NODE_JSON")?;
   let end=clock(&node["receivedAt"])?;fresh(&node,&format!("/markets/{}/nodes?tournamentId={}",c.market_id,r.tournament_id),end,u64::MAX-end)?;
   node_scope(&node["data"],&r.tournament_id)?;need(node["data"]["market_id"]==c.market_id&&node["data"]["root"]["node_type"]=="contract"&&node["data"]["root"]["node_id"]==c.contract_id&&node["data"]["root"]["contract_id"]==c.contract_id&&json_hash(&node["data"]["root"])?==c.official_root_sha256,"FLOOR_RULE_CHANGED")?;
   let source=r.sources.iter().find(|s|s.sha256==c.original_node_sha256).ok_or("FLOOR_NODE_SOURCE")?;need(end<=source.observed_at_ms,"FLOOR_NODE_CLOCK")?;
  }
  let state_ids=r.states.iter().map(|s|s.id.clone()).collect::<BTreeSet<_>>();need(state_ids.len()==r.states.len()&&r.states.iter().all(|s|atom(&s.id)&&s.outcomes.len()==r.contracts.len()),"FLOOR_STATE_MATRIX")?;
  let required=["ordinary","fusion","independent","replacement","runoff","notApplicable","cancelled","otherAdmissibleOutcomes"].into_iter().map(str::to_owned).collect::<BTreeSet<_>>();
  let families=r.exceptions.iter().map(|c|c.family.clone()).collect::<BTreeSet<_>>();need(families==required&&families.len()==r.exceptions.len(),"FLOOR_EXCEPTION_COVERAGE")?;
  for e in &r.exceptions{need(sources.contains_key(&e.source_sha256)&&!e.explanation.trim().is_empty()&&e.explanation.len()<=8192,"FLOOR_EXCEPTION_SOURCE")?;if e.impossible{need(e.state_ids.is_empty()&&approved_completeness_proofs.contains(&e.source_sha256),"FLOOR_IMPOSSIBILITY_UNPROVEN")?}else{need(!e.state_ids.is_empty()&&e.state_ids.iter().all(|id|state_ids.contains(id)),"FLOOR_EXCEPTION_STATE")?}}
  need(proof.rule_hashes.iter().cloned().collect::<BTreeSet<_>>()==roots&&proof.rule_hashes.len()==roots.len()&&proof.state_ids.iter().cloned().collect::<BTreeSet<_>>()==state_ids&&proof.state_ids.len()==state_ids.len()&&proof.exception_families.iter().cloned().collect::<BTreeSet<_>>()==families&&proof.exception_families.len()==families.len(),"FLOOR_PROOF_BINDING")?;
  need(now<until,"FLOOR_CERTIFICATE_EXPIRED")?;Ok(Self{input:r,sha,until})
 }
}

#[derive(Clone,Debug,Serialize,Deserialize)]#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct Leg{pub exchange_id:String,pub side:Side,pub quantity:u32,pub limit_price_units:i128}
#[derive(Clone,Debug,Serialize,Deserialize)]#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct Candidate{pub id:String,pub legs:Vec<Leg>}
/// These are original receipts from the sole transport. Hashes authenticate
/// retained identity, not the server. The existing owner/lease/physical authority
/// must supply genuine originals and still revalidate immediately before POST.
pub struct Context<'a>{pub tournament_id:&'a str,pub tournament_slug:&'a str,pub run_id:&'a str,pub native_head_sha256:&'a str,pub unencumbered_cash_units:i128,pub cash_receipt:&'a[u8],pub node_receipts:&'a[&'a[u8]],pub book_receipts:&'a[&'a[u8]],pub now_ms:u64,pub maximum_book_age_ms:u64,pub maximum_plan_age_ms:u64,pub minimum_net_surplus_units:i128}
#[derive(Debug,Serialize)]#[serde(rename_all="camelCase")]
pub struct AtomicProposal{execution_mode:&'static str,tournament_id:String,candidate:Candidate,gross_limit_funding_units:i128,observed_depth_cost_units:i128,maximum_fees_units:i128,minimum_state_net_units:i128,state_net_units:BTreeMap<String,i128>,certificate_sha256:String,native_head_sha256:String,run_id:String,cash_receipt_sha256:String,node_receipt_sha256:Vec<String>,book_receipt_sha256:Vec<String>,valid_until_ms:u64}
impl AtomicProposal{
 pub fn execution_mode(&self)->&'static str{self.execution_mode}pub fn tournament_id(&self)->&str{&self.tournament_id}pub fn candidate(&self)->&Candidate{&self.candidate}
 pub fn gross_limit_funding_units(&self)->i128{self.gross_limit_funding_units}pub fn observed_depth_cost_units(&self)->i128{self.observed_depth_cost_units}pub fn maximum_fees_units(&self)->i128{self.maximum_fees_units}
 pub fn minimum_state_net_units(&self)->i128{self.minimum_state_net_units}pub fn state_net_units(&self)->&BTreeMap<String,i128>{&self.state_net_units}pub fn valid_until_ms(&self)->u64{self.valid_until_ms}
 pub fn certificate_sha256(&self)->&str{&self.certificate_sha256}pub fn native_head_sha256(&self)->&str{&self.native_head_sha256}pub fn run_id(&self)->&str{&self.run_id}
 pub fn cash_receipt_sha256(&self)->&str{&self.cash_receipt_sha256}pub fn node_receipt_sha256(&self)->&[String]{&self.node_receipt_sha256}pub fn book_receipt_sha256(&self)->&[String]{&self.book_receipt_sha256}
}
#[derive(Debug,Serialize)]#[serde(tag="kind",content="evidence",rename_all="camelCase")]
pub enum Decision{Uncertified{reason:&'static str},NoOpportunity{evaluated:usize},GuaranteedFloor(AtomicProposal)}
fn receipt(bytes:&[u8])->Result<Value>{need(!bytes.is_empty()&&bytes.len()<=8*1024*1024,"FLOOR_RECEIPT_BOUND")?;serde_json::from_slice(bytes).map_err(|_|"FLOOR_RECEIPT_JSON")}
fn parse_levels(v:&Value,ascending:bool)->Result<Vec<(i128,u64)>>{
 let rows=v.as_array().ok_or("FLOOR_DEPTH")?;need(rows.len()<=200,"FLOOR_DEPTH_BOUND")?;let mut out=Vec::new();let mut last=None;
 for l in rows{let p=units(&l["price"])?;let q=units(&l["quantity"])?;need((0..=UNIT).contains(&p)&&p%5000==0&&q>0&&q%UNIT==0&&q/UNIT<=u32::MAX as i128,"FLOOR_DEPTH_LEVEL")?;if let Some(prev)=last{need(if ascending{p>prev}else{p<prev},"FLOOR_DEPTH_ORDER")?;}last=Some(p);out.push((p,(q/UNIT)as u64));}Ok(out)
}
pub fn solve(certificate:Option<&ExhaustivePayoffCertificate>,context:&Context<'_>,candidates:&[Candidate])->Decision{
 match solve_checked(certificate,context,candidates){Ok(v)=>v,Err(reason)=>Decision::Uncertified{reason}}
}
fn solve_checked(cert:Option<&ExhaustivePayoffCertificate>,c:&Context<'_>,candidates:&[Candidate])->Result<Decision>{
 let cert=cert.ok_or("FLOOR_NO_EXHAUSTIVE_CERTIFICATE")?;
 need(c.now_ms>=cert.input.reviewed_at_ms&&c.now_ms<cert.until&&c.tournament_id==cert.input.tournament_id&&c.tournament_slug==cert.input.tournament_slug&&atom(c.run_id)&&hex(c.native_head_sha256)&&c.unencumbered_cash_units>=0&&c.minimum_net_surplus_units>0&&(1..=10_000).contains(&c.maximum_book_age_ms)&&(1..=10_000).contains(&c.maximum_plan_age_ms)&&candidates.len()<=4096&&c.book_receipts.len()<=10,"FLOOR_CONTEXT")?;
 let cash=receipt(c.cash_receipt)?;let mut until=cert.until.min(fresh(&cash,&format!("/tournaments/{}",c.tournament_slug),c.now_ms,20_000)?).min(c.now_ms.checked_add(c.maximum_plan_age_ms).ok_or("FLOOR_OVERFLOW")?);
 need(cash["data"]["id"]==c.tournament_id&&cash["data"]["slug"]==c.tournament_slug&&units(&cash["data"]["myBalance"])? >=c.unencumbered_cash_units,"FLOOR_CASH_SCOPE")?;
 need(c.node_receipts.len()==cert.input.contracts.len(),"FLOOR_CURRENT_RULES_REQUIRED")?;let mut nodes=BTreeSet::new();let mut node_hashes=Vec::new();
 for raw in c.node_receipts{let r=receipt(raw)?;let contract=cert.input.contracts.iter().find(|v|r["endpoint"]==format!("/markets/{}/nodes?tournamentId={}",v.market_id,c.tournament_id)).ok_or("FLOOR_CURRENT_RULE_SCOPE")?;until=until.min(fresh(&r,r["endpoint"].as_str().ok_or("FLOOR_NODE_ENDPOINT")?,c.now_ms,c.maximum_book_age_ms)?);node_scope(&r["data"],c.tournament_id)?;need(nodes.insert(contract.exchange_id.clone())&&r["data"]["market_id"]==contract.market_id&&json_hash(&r["data"]["root"])?==contract.official_root_sha256,"FLOOR_CURRENT_RULE_CHANGED")?;node_hashes.push(hash(raw));}
 let mut books=BTreeMap::new();let mut book_hashes=Vec::new();
 for raw in c.book_receipts{
  let r=receipt(raw)?;let mut found=false;
  for contract in &cert.input.contracts{
   if r["endpoint"]!=format!("/markets/{}/orderbook?tournamentId={}&depth=20",contract.market_id,c.tournament_id)&&r["endpoint"]!=format!("/markets/{}/orderbook?tournamentId={}&depth=200",contract.market_id,c.tournament_id){continue}
   until=until.min(fresh(&r,r["endpoint"].as_str().ok_or("FLOOR_BOOK_ENDPOINT")?,c.now_ms,c.maximum_book_age_ms)?);
   let contexts=r["data"]["contexts"].as_array().ok_or("FLOOR_BOOK_CONTEXT")?;let matched=contexts.iter().filter(|v|v["type"]=="tournament"&&v["tournament"]["id"]==c.tournament_id).collect::<Vec<_>>();need(matched.len()==1,"FLOOR_BOOK_CONTEXT")?;
   let ex=matched[0]["orderbook"]["exchanges"].as_array().ok_or("FLOOR_BOOK_EXCHANGES")?.iter().filter(|v|v["exchangeId"]==contract.exchange_id).collect::<Vec<_>>();need(ex.len()==1,"FLOOR_BOOK_EXCHANGE")?;
   let source=clock(&ex[0]["asOf"]["at"])?;need(source<=clock(&r["receivedAt"])?&&source<=c.now_ms&&c.now_ms-source<=c.maximum_book_age_ms,"FLOOR_BOOK_SOURCE_STALE")?;until=until.min(source.checked_add(c.maximum_book_age_ms).ok_or("FLOOR_OVERFLOW")?);
   let bids=parse_levels(&ex[0]["bids"],false)?;let asks=parse_levels(&ex[0]["asks"],true)?;need(!bids.is_empty()&&!asks.is_empty()&&bids[0].0<asks[0].0&&books.insert(contract.exchange_id.clone(),(bids,asks)).is_none(),"FLOOR_BOOK_DUPLICATE_OR_CROSSED")?;found=true;
  }need(found,"FLOOR_FOREIGN_BOOK")?;book_hashes.push(hash(raw));
 }
 need(c.now_ms<until,"FLOOR_NO_REMAINING_LIFETIME")?;let mut best=None;let mut ids=BTreeSet::new();
 for candidate in candidates{
  need(atom(&candidate.id)&&ids.insert(&candidate.id)&&!candidate.legs.is_empty()&&candidate.legs.len()<=10,"FLOOR_CANDIDATE_BOUND")?;
  let mut seen=BTreeSet::new();let mut gross=0;let mut depth_cost=0;let mut fee=0;let mut complete=true;
  for l in &candidate.legs{
   need(l.quantity>0&&l.quantity<=i32::MAX as u32&&(5000..=995000).contains(&l.limit_price_units)&&l.limit_price_units%5000==0&&seen.insert((l.exchange_id.clone(),l.side)),"FLOOR_LEG")?;
   need(cert.input.contracts.iter().any(|v|v.exchange_id==l.exchange_id),"FLOOR_FOREIGN_LEG")?;
   gross=add(gross,mul(l.quantity as i128,l.limit_price_units)?)?;fee=add(fee,mul(l.quantity as i128,cert.input.maximum_fee_per_share_units)?)?;
   let(bids,asks)=books.get(&l.exchange_id).ok_or("FLOOR_MISSING_BOOK")?;let levels=if l.side==Side::Yes{asks.clone()}else{bids.iter().map(|(p,q)|(UNIT-p,*q)).collect()};let mut left=l.quantity as u64;
   for(p,q)in levels{if p>l.limit_price_units{break}let n=left.min(q);depth_cost=add(depth_cost,mul(n as i128,p)?)?;left-=n;if left==0{break}}
   if left!=0{complete=false}
  }
  if !complete||add(gross,fee)?>c.unencumbered_cash_units{continue}
  let mut net=BTreeMap::new();
  for state in &cert.input.states{
   let mut value=-fee;
   for l in &candidate.legs{
    let index=cert.input.contracts.iter().position(|v|v.exchange_id==l.exchange_id).ok_or("FLOOR_CONTRACT")?;
    // Principal refunds cancel the corresponding actual acquisition debit.
    // They are NEVER a universal $1 payout and do not refund fees.
    let increment=match state.outcomes[index]{Payout::RefundPrincipal=>0,payout=>{let wins=matches!((payout,l.side),(Payout::ResolvedYes,Side::Yes)|(Payout::ResolvedNo,Side::No));mul(l.quantity as i128,(if wins{UNIT}else{0})-l.limit_price_units)?}};
    value=add(value,increment)?;
   }net.insert(state.id.clone(),value);
  }
  let minimum=*net.values().min().ok_or("FLOOR_EMPTY_STATES")?;if minimum<c.minimum_net_surplus_units{continue}
  let proposal=AtomicProposal{execution_mode:"allOrNone",tournament_id:c.tournament_id.into(),candidate:candidate.clone(),gross_limit_funding_units:gross,observed_depth_cost_units:depth_cost,maximum_fees_units:fee,minimum_state_net_units:minimum,state_net_units:net,certificate_sha256:cert.sha.clone(),native_head_sha256:c.native_head_sha256.into(),run_id:c.run_id.into(),cash_receipt_sha256:hash(c.cash_receipt),node_receipt_sha256:node_hashes.clone(),book_receipt_sha256:book_hashes.clone(),valid_until_ms:until};
  if best.as_ref().is_none_or(|b:&AtomicProposal|proposal.minimum_state_net_units>b.minimum_state_net_units||(proposal.minimum_state_net_units==b.minimum_state_net_units&&proposal.candidate.id<b.candidate.id)){best=Some(proposal)}
 }
 Ok(match best{Some(p)=>Decision::GuaranteedFloor(p),None=>Decision::NoOpportunity{evaluated:candidates.len()}})
}

#[cfg(test)]mod tests{
 use super::*;use serde_json::json;
 const NOW:u64=100_000;
 fn iso(ms:u64)->String{chrono::DateTime::from_timestamp_millis(ms as i64).unwrap().to_rfc3339_opts(chrono::SecondsFormat::Millis,true)}
 fn bytes(v:Value)->Vec<u8>{serde_json::to_vec(&v).unwrap()}
 struct Fixture{review:Vec<u8>,sources:Vec<(String,Vec<u8>)>,approved:BTreeSet<String>,proofs:BTreeSet<String>,cash:Vec<u8>,books:Vec<Vec<u8>>}
 impl Fixture{
  // Explicitly SYNTHETIC proof: tests exercise enforcement, not venue evidence.
  fn new(refund:bool,fee:i128)->Self{
   let mut sources=Vec::new();let mut contracts=Vec::new();
   for i in 1..=2{let market=i.to_string();let root=json!({"node_type":"contract","node_id":format!("10{i}"),"contract_id":format!("10{i}"),"contract_details":{"synthetic":true}});let node=bytes(json!({"status":200,"endpoint":format!("/markets/{market}/nodes?tournamentId=cup"),"requestStartedAt":iso(90000),"receivedAt":iso(90001),"data":{"market_id":market,"root":root,"contexts":[{"type":"tournament","tournament":{"id":"cup"}}]}}));let h=hash(&node);contracts.push(Contract{market_id:i.to_string(),exchange_id:format!("1{i}"),contract_id:format!("10{i}"),official_root_sha256:json_hash(&root).unwrap(),original_node_sha256:h.clone()});sources.push((h,node));}
   let mut states=vec![State{id:"left".into(),outcomes:vec![Payout::ResolvedYes,Payout::ResolvedNo]},State{id:"right".into(),outcomes:vec![Payout::ResolvedNo,Payout::ResolvedYes]}];if refund{states.push(State{id:"refund".into(),outcomes:vec![Payout::RefundPrincipal,Payout::RefundPrincipal]});}
   let families=["ordinary","fusion","independent","replacement","runoff","notApplicable","cancelled","otherAdmissibleOutcomes"];
   let fees=bytes(json!({"SYNTHETIC_fee_cap":fee}));let fh=hash(&fees);let proof=bytes(json!({"kind":"reviewed_exhaustive_partition_v1","reviewerId":"SYNTHETIC_TEST_ONLY","statement":"Synthetic binary fixture only, not real election exhaustiveness.","ruleHashes":contracts.iter().map(|c|c.official_root_sha256.clone()).collect::<Vec<_>>(),"stateIds":states.iter().map(|s|s.id.clone()).collect::<Vec<_>>(),"statePayoffsSha256":json_hash(&states).unwrap(),"feesAreProvenBounded":true,"feeSourceSha256":fh,"maximumFeePerShareUnits":fee,"exceptionFamilies":families,"pairwiseDisjoint":true,"collectivelyExhaustive":true,"unresolvedObligations":[]}));let ph=hash(&proof);sources.push((ph.clone(),proof));sources.push((fh.clone(),fees));
   let exceptions=families.iter().map(|family|{let covered=*family=="ordinary"||refund&&["notApplicable","cancelled"].contains(family);ExceptionCoverage{family:(*family).into(),source_sha256:ph.clone(),state_ids:if *family=="ordinary"{vec!["left".into(),"right".into()]}else if covered{vec!["refund".into()]}else{vec![]},impossible:!covered,explanation:"Synthetic fixture assumption; not an actual venue certificate.".into()}}).collect();
   let r=CertificateInput{version:1,tournament_id:"cup".into(),tournament_slug:"cup".into(),reviewed_at_ms:95000,valid_until_ms:200000,contracts,states,sources:sources.iter().map(|(h,_)|Source{sha256:h.clone(),observed_at_ms:90001,maximum_age_ms:200000}).collect(),exceptions,completeness_proof_sha256:ph.clone(),fee_source_sha256:fh,maximum_fee_per_share_units:fee,unresolved_obligations:vec![]};let review=serde_json::to_vec(&r).unwrap();let approved=[hash(&review)].into();
   let cash=bytes(json!({"status":200,"endpoint":"/tournaments/cup","requestStartedAt":iso(99000),"receivedAt":iso(99001),"data":{"id":"cup","slug":"cup","myBalance":"100.000000"}}));
   let books=(1..=2).map(|i|bytes(json!({"status":200,"endpoint":format!("/markets/{i}/orderbook?tournamentId=cup&depth=20"),"requestStartedAt":iso(99000),"receivedAt":iso(99001),"data":{"contexts":[{"type":"tournament","tournament":{"id":"cup"},"orderbook":{"exchanges":[{"exchangeId":format!("1{i}"),"asOf":{"at":iso(99000),"sequence":1},"bids":[{"price":"0.300000","quantity":10}],"asks":[{"price":"0.400000","quantity":2},{"price":"0.450000","quantity":8}]}]}}]}}))).collect();Self{review,sources,approved,proofs:[ph].into(),cash,books}
  }
  fn cert(&self)->Result<ExhaustivePayoffCertificate>{ExhaustivePayoffCertificate::verify(&self.review,&self.approved,&self.proofs,&self.sources.iter().map(|(h,b)|OriginalSource{sha256:h,bytes:b}).collect::<Vec<_>>(),NOW)}
  fn edit_review(&mut self,f:impl FnOnce(&mut Value)){let mut v:Value=serde_json::from_slice(&self.review).unwrap();f(&mut v);self.review=bytes(v);self.approved=[hash(&self.review)].into();}
  fn solve(&self,certificate:Option<&ExhaustivePayoffCertificate>,candidate:Candidate,cash:i128)->Decision{let refs=self.books.iter().map(Vec::as_slice).collect::<Vec<_>>();let nodes=self.sources.iter().take(2).map(|(_,b)|{let mut r:Value=serde_json::from_slice(b).unwrap();r["requestStartedAt"]=json!(iso(99000));r["receivedAt"]=json!(iso(99001));bytes(r)}).collect::<Vec<_>>();let node_refs=nodes.iter().map(Vec::as_slice).collect::<Vec<_>>();solve(certificate,&Context{tournament_id:"cup",tournament_slug:"cup",run_id:"fixture",native_head_sha256:&"a".repeat(64),unencumbered_cash_units:cash,cash_receipt:&self.cash,node_receipts:&node_refs,book_receipts:&refs,now_ms:NOW,maximum_book_age_ms:10000,maximum_plan_age_ms:3000,minimum_net_surplus_units:1},&[candidate])}
 }
 fn pair(q:u32,limit:i128)->Candidate{Candidate{id:"pair".into(),legs:(1..=2).map(|i|Leg{exchange_id:format!("1{i}"),side:Side::Yes,quantity:q,limit_price_units:limit}).collect()}}
 #[test]fn finite_model_without_separate_reviewed_completeness_never_certifies(){let mut f=Fixture::new(false,0);f.proofs.clear();assert!(matches!(f.cert(),Err("FLOOR_INDEPENDENT_PROOF_REQUIRED")));assert!(matches!(f.solve(None,pair(5,450000),100*UNIT),Decision::Uncertified{reason:"FLOOR_NO_EXHAUSTIVE_CERTIFICATE"}));}
 #[test]fn full_depth_limit_funding_and_minimum_net_are_separate(){let f=Fixture::new(false,0);let cert=f.cert().unwrap();let Decision::GuaranteedFloor(p)=f.solve(Some(&cert),pair(5,450000),100*UNIT)else{panic!("expected synthetic certified floor")};assert_eq!(p.minimum_state_net_units,500000);assert_eq!(p.gross_limit_funding_units,4500000);assert_eq!(p.observed_depth_cost_units,4300000);assert_eq!(p.execution_mode,"allOrNone");assert_eq!(p.state_net_units.len(),2);assert_eq!(p.valid_until_ms,103000);}
 #[test]fn refund_is_principal_not_dollar_and_fees_remain_costs(){for fee in [0,1000]{let f=Fixture::new(true,fee);let cert=f.cert().unwrap();assert!(matches!(f.solve(Some(&cert),pair(5,450000),100*UNIT),Decision::NoOpportunity{..}));}}
 #[test]fn known_fees_can_remove_an_apparent_floor(){let f=Fixture::new(false,60000);let cert=f.cert().unwrap();assert!(matches!(f.solve(Some(&cert),pair(5,450000),100*UNIT),Decision::NoOpportunity{..}));}
 #[test]fn depth_exhaustion_and_insufficient_full_limit_cash_do_not_become_opportunities(){let f=Fixture::new(false,0);let cert=f.cert().unwrap();for(q,p,cash)in[(3,400000,100*UNIT),(11,450000,100*UNIT),(5,450000,4400000)]{assert!(matches!(f.solve(Some(&cert),pair(q,p),cash),Decision::NoOpportunity{..}));}}
 #[test]fn positive_expected_single_leg_is_not_positive_minimum(){let f=Fixture::new(false,0);let cert=f.cert().unwrap();let mut c=pair(1,400000);c.legs.truncate(1);assert!(matches!(f.solve(Some(&cert),c,100*UNIT),Decision::NoOpportunity{..}));}
 #[test]fn no_side_uses_complementary_bid_depth_and_not_yes_ask(){let f=Fixture::new(false,0);let cert=f.cert().unwrap();let mut c=pair(1,400000);for l in &mut c.legs{l.side=Side::No;}assert!(matches!(f.solve(Some(&cert),c,100*UNIT),Decision::NoOpportunity{..}));}
 #[test]fn current_receipt_cannot_refresh_old_source_clock(){let mut f=Fixture::new(false,0);let cert=f.cert().unwrap();let mut b:Value=serde_json::from_slice(&f.books[0]).unwrap();b["data"]["contexts"][0]["orderbook"]["exchanges"][0]["asOf"]["at"]=json!(iso(1));f.books[0]=bytes(b);assert!(matches!(f.solve(Some(&cert),pair(5,450000),100*UNIT),Decision::Uncertified{reason:"FLOOR_BOOK_SOURCE_STALE"}));}
 #[test]fn omitted_exceptions_unresolved_obligations_and_wrong_matrix_deny(){for case in 0..3{let mut f=Fixture::new(false,0);f.edit_review(|v|match case{0=>{v["exceptions"].as_array_mut().unwrap().pop();},1=>v["unresolvedObligations"]=json!(["unknown refund"]),_=>v["states"][0]["outcomes"]=json!(["resolvedYes"])});assert!(f.cert().is_err());}}
 #[test]fn original_source_mutation_or_current_foreign_scope_deny(){let mut f=Fixture::new(false,0);f.sources[0].1.push(b' ');assert!(matches!(f.cert(),Err("FLOOR_ORIGINAL_SOURCE")));let f=Fixture::new(false,0);let cert=f.cert().unwrap();let mut c=pair(1,400000);c.legs[0].exchange_id="999".into();assert!(matches!(f.solve(Some(&cert),c,100*UNIT),Decision::Uncertified{reason:"FLOOR_FOREIGN_LEG"}));}
 #[test]fn candidate_leg_bound_and_duplicate_consumption_deny(){let f=Fixture::new(false,0);let cert=f.cert().unwrap();let mut c=pair(1,400000);c.legs=vec![c.legs[0].clone();11];assert!(matches!(f.solve(Some(&cert),c,100*UNIT),Decision::Uncertified{reason:"FLOOR_CANDIDATE_BOUND"}));let mut c=pair(1,400000);c.legs[1]=c.legs[0].clone();assert!(matches!(f.solve(Some(&cert),c,100*UNIT),Decision::Uncertified{reason:"FLOOR_LEG"}));}
 #[test]fn separately_approved_proof_pins_matrix_and_fee_bound_not_only_state_names(){for changed in ["payout","fee"]{let mut f=Fixture::new(false,0);f.edit_review(|v|if changed=="payout"{v["states"][0]["outcomes"][0]=json!("resolvedNo")}else{v["maximumFeePerShareUnits"]=json!(1)});assert!(matches!(f.cert(),Err("FLOOR_COMPLETENESS_UNPROVEN")));}}
 #[test]fn fresh_current_node_with_changed_rule_cannot_use_old_certificate(){let mut f=Fixture::new(false,0);let cert=f.cert().unwrap();let mut changed:Value=serde_json::from_slice(&f.sources[0].1).unwrap();changed["data"]["root"]["contract_details"]["changed"]=json!(true);f.sources[0].1=bytes(changed);assert!(matches!(f.solve(Some(&cert),pair(5,450000),100*UNIT),Decision::Uncertified{reason:"FLOOR_CURRENT_RULE_CHANGED"}));}
}
