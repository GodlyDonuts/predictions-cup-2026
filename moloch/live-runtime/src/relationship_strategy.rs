//! Offline adapter from an already received canonical census to exact packages.
//! No files, network, account mutation or execution authority.
use crate::relationship_census::{Availability,Census};
use baal_transport_service::{Receipt,Scope,decimal_units};
use baal_transport_service::relationships::{self,DirectionKind,NodeRole,Relationship,RelationshipStatus,RelationshipType};
use moloch_relationship_arbitrage::{self as arb,Book,Contract,Declaration,Instrument,Logic,Plan,Settlement,Side,Source};
use serde::{Deserialize,Serialize};
use serde_json::{Value,json};
use sha2::{Digest,Sha256};
use std::collections::{BTreeMap,BTreeSet};

type Result<T> = std::result::Result<T,&'static str>;
fn need(ok:bool,code:&'static str)->Result<()>{if ok{Ok(())}else{Err(code)}}
fn hash<T:Serialize>(v:&T)->Result<String>{Ok(format!("{:x}",Sha256::digest(serde_json::to_vec(v).map_err(|_|"STRATEGY_SERIALIZATION")?)))}
fn clock(v:&str)->Result<u64>{u64::try_from(chrono::DateTime::parse_from_rfc3339(v).map_err(|_|"STRATEGY_CLOCK")?.timestamp_millis()).map_err(|_|"STRATEGY_CLOCK")}
fn fresh(receipt:&Receipt,now:u64,max_age:u64)->Result<Source>{
 let received=clock(&receipt.received_at)?;let started=clock(&receipt.request_started_at)?;
 need(receipt.status==200&&started<=received&&received<=now&&now-received<=max_age,"STRATEGY_RECEIPT_STALE")?;
 Ok(Source{sha256:hash(receipt)?,received_ms:received})
}
fn exact(v:&str)->Result<i128>{decimal_units(v).map_err(|_|"STRATEGY_DECIMAL")}
fn money(v:i128)->String{let sign=if v<0{"-"}else{""};let v=v.unsigned_abs();format!("{sign}{}.{:06}",v/1_000_000,v%1_000_000)}
fn source_value(s:&Source)->Value{json!({"receiptSha256":s.sha256,"receivedAtMs":s.received_ms})}

/// An explicit externally reviewed interpretation of official terminal rules.
/// Its hash must be in the separately supplied trusted certificate allowlist.
/// An API relationship type or `active` flag never creates this attestation.
#[derive(Debug,Clone,Serialize,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct SettlementCertificate {
 pub version:u32,pub tournament_id:String,
 pub relationship_id:String,pub relationship_version:u64,
 pub canonical_relationship_sha256:String,pub relationship_receipt_sha256:String,
 pub issued_at_ms:u64,pub expires_at_ms:u64,pub verifier_id:String,
 pub logic:CertifiedLogic,pub contracts:Vec<ContractCertificate>,
}
#[derive(Debug,Clone,Serialize,Deserialize)]
#[serde(tag="kind",rename_all="snake_case",deny_unknown_fields)]
pub enum CertifiedLogic {MutuallyExclusive{exhaustive:bool},Complement}
#[derive(Debug,Clone,Serialize,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct ContractCertificate {
 pub exchange_id:String,pub market_id:String,pub contract_id:String,
 /// Exact canonical node outcome label; meaning reviewed by the verifier.
 pub canonical_outcome:String,
 pub rule_receipt_sha256:String,pub rule_endpoint:String,
 /// V2 binds every field of the reviewed rule body, independently of receipt
 /// transport clocks. None preserves the exact V1 serialization/hash.
 #[serde(default,skip_serializing_if="Option::is_none")]
 pub rule_data_sha256:Option<String>,
 pub ordinary_yes_payout:String,pub ordinary_no_payout:String,
 pub ordinary_no_yes_payout:String,pub ordinary_no_no_payout:String,
 pub terminal_rules_complete:bool,pub exceptions:Vec<ExceptionalPayout>,
}
#[derive(Debug,Clone,Serialize,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct ExceptionalPayout {pub yes_payout:String,pub no_payout:String}

pub fn certificate_sha256(c:&SettlementCertificate)->Result<String>{hash(c)}
pub fn canonical_relationship_sha256(r:&Relationship)->Result<String>{hash(r)}
/// V2 semantic digest removes ONLY dynamic node.currentPrice. Version, status,
/// timestamps, direction, contracts, predicates and all other typed fields stay.
pub fn relationship_semantics_sha256(r:&Relationship)->Result<String>{
 let mut v=serde_json::to_value(r).map_err(|_|"STRATEGY_SERIALIZATION")?;
 for n in v["nodes"].as_array_mut().ok_or("STRATEGY_CANONICAL_SCHEMA")?{n.as_object_mut().ok_or("STRATEGY_CANONICAL_SCHEMA")?.remove("currentPrice");}
 hash(&v)
}
pub fn rule_data_sha256(r:&Receipt)->Result<String>{hash(&r.data)}
pub fn receipt_sha256(r:&Receipt)->Result<String>{hash(r)}

pub struct Certificates<'a> {
 pub values:&'a [SettlementCertificate],
 /// Independently approved by the existing owner/verifier. Merely loading a
 /// supplied certificate must not add its own digest to this set.
 pub trusted_sha256:&'a BTreeSet<String>,
 /// Already received official rule receipts AND original reviewed relationship
 /// attestation receipts. These are evidence, never URLs to fetch. V2 keeps the
 /// immutable original attestation while independently validating current data.
 pub rule_receipts:&'a [Receipt],
}
#[derive(Debug,Clone)]
pub struct Capital {
 pub gross_account_cash:i128,pub attributed_bank_cash:i128,
 pub reserved_cash:i128,pub maximum_committed_remaining:i128,
 pub package_cost_cap:i128,pub orphan_cash_loss_cap:i128,
}
impl Capital {
 fn limits(&self,p:&Policy)->Result<arb::Limits>{
  need([self.gross_account_cash,self.attributed_bank_cash,self.reserved_cash,self.maximum_committed_remaining,self.package_cost_cap,self.orphan_cash_loss_cap].iter().all(|n|*n>=0),"STRATEGY_CAPITAL_NEGATIVE")?;
  let cash=self.gross_account_cash.min(self.attributed_bank_cash).checked_sub(self.reserved_cash).ok_or("STRATEGY_CAPITAL_RANGE")?;
  need(cash>=0,"STRATEGY_RESERVATIONS_EXCEED_CASH")?;
  Ok(arb::Limits{now_ms:p.now_ms,max_book_age_ms:p.max_book_age_ms,max_book_skew_ms:p.max_book_skew_ms,cash_available:cash,max_package_cost:self.maximum_committed_remaining.min(self.package_cost_cap),max_orphan_cash_loss:self.orphan_cash_loss_cap,fee_per_share_per_leg:p.entry_fee_per_share_per_leg,buffer_per_share:p.buffer_per_share,minimum_surplus:p.minimum_surplus,max_quantity:p.maximum_quantity})
 }
}
#[derive(Debug,Clone)]
pub struct Policy {
 pub now_ms:u64,pub max_relationship_age_ms:u64,pub max_rule_age_ms:u64,
 pub max_book_age_ms:u64,pub max_book_skew_ms:u64,
 pub entry_fee_per_share_per_leg:i128,pub buffer_per_share:i128,
 pub minimum_surplus:i128,pub maximum_quantity:u64,pub quantities:Vec<u64>,
 pub maximum_retained_candidates:usize,
}
#[derive(Debug,Clone)]
pub struct OwnedPosition {pub exchange_id:String,pub side:Side,pub quantity:u64}
#[derive(Debug,Clone,Serialize)]
#[serde(rename_all="camelCase")]
pub struct PublicDiagnostic {
 pub version:u32,pub at_ms:u64,pub canonical_feed_state:String,pub canonical_relationships:usize,
 pub supported_active_relationships:usize,pub certified_components:usize,
 pub positive_candidates:usize,pub retained_candidates:usize,pub blocked_reason_counts:BTreeMap<String,usize>,
 pub execution_authority:bool,
}
/// No Serialize implementation: holdings, candidate prices and source references
/// must not enter public status by accidentally serializing the analysis object.
pub struct Analysis {pub public:PublicDiagnostic,pub candidates:Vec<PrivateCandidate>}
pub struct PrivateCandidate {
 pub relationship_id:String,pub relationship_version:u64,
 pub certificate_sha256:String,pub contracts:Vec<(String,String,String)>,
 pub existing_component_floor:i128,pub marginal_floors:Vec<i128>,pub plan:Plan,
}
impl PrivateCandidate {
 pub fn private_json(&self)->Value{
  let legs:Vec<_>=self.plan.instruments.iter().enumerate().map(|(j,i)|{
   let c=&self.contracts[i.contract];json!({"exchangeId":c.0,"marketId":c.1,"contractId":c.2,"side":match i.side{Side::Yes=>"YES",Side::No=>"NO"},"quantity":self.plan.quantity,"fullDepthCost":money(self.plan.leg_costs[j]),"marginalExistingPortfolioFloor":money(self.marginal_floors[j]),"bookSource":source_value(&self.plan.book_sources[j]),"bookObservedAtMs":self.plan.book_observed_ms[j]})
  }).collect();
  json!({"version":1,"relationshipId":self.relationship_id,"relationshipVersion":self.relationship_version,"certificateSha256":self.certificate_sha256,"legs":legs,"acquisitionCost":money(self.plan.acquisition_cost),"entryFees":money(self.plan.fees),"safetyBuffer":money(self.plan.buffer),"minimumTerminalPayout":money(self.plan.terminal_floor),"guaranteedSurplusIfCompleted":money(self.plan.guaranteed_surplus),"hardOrphanCashLossBound":money(self.plan.worst_orphan_cash_loss),"observedFullLegSubsetUnwindLoss":self.plan.observed_orphan_unwind_loss.map(money),"existingComponentMinimumPayout":money(self.existing_component_floor),"proofSources":self.plan.proof_sources.iter().map(source_value).collect::<Vec<_>>(),"executionAuthority":false,"dispatchBlockedBy":["PACKAGE_AUTHORITY_AND_DURABLE_EXECUTOR_NOT_INTEGRATED"],"actualFills":false,"realizedProfit":false})
 }
}
fn block(a:&mut Analysis,code:&str){*a.public.blocked_reason_counts.entry(code.to_owned()).or_default()+=1;}

fn canonical_source(census:&Census,receipt:&Receipt,p:&Policy)->Result<Source>{
 need(!census.report.transport_guard_observed,"STRATEGY_TRANSPORT_GUARD")?;
 need(census.report.tournament_id==census.cache.scope.tournament_id&&census.report.scope_sha256==hash(&census.cache.scope)?,"STRATEGY_CENSUS_SCOPE")?;
 need(matches!(census.report.relationships.availability,Availability::Available|Availability::Empty),"STRATEGY_RELATIONSHIPS_UNAVAILABLE")?;
 let evidence=census.report.relationships.source.as_ref().ok_or("STRATEGY_RELATIONSHIP_SOURCE_MISSING")?;
 let source=fresh(receipt,p.now_ms,p.max_relationship_age_ms)?;
 need(source.sha256==evidence.receipt_sha256&&receipt.endpoint.split('?').next()==Some("/relationships")&&receipt.endpoint.split('?').nth(1).is_some_and(|q|q.split('&').any(|v|v==format!("tournamentId={}",census.cache.scope.tournament_id))),"STRATEGY_RELATIONSHIP_RECEIPT_BINDING")?;
 let rows=relationships::parse_list(&receipt.data,&census.cache.scope).map_err(|_|"STRATEGY_CANONICAL_SCHEMA")?;
 need(!rows.pagination.has_more,"STRATEGY_RELATIONSHIPS_INCOMPLETE")?;
 let cached=census.cache.relationships.as_ref().ok_or("STRATEGY_CANONICAL_CACHE_MISSING")?;
 need(serde_json::to_value(&rows).ok()==serde_json::to_value(cached).ok(),"STRATEGY_CANONICAL_CACHE_CHANGED")?;
 need(census.report.relationships.row_count==Some(rows.data.len()),"STRATEGY_CANONICAL_COUNT_MISMATCH")?;
 Ok(source)
}

fn relationship_endpoint(endpoint:&str,scope:&Scope)->bool{
 let mut pieces=endpoint.split('?');if pieces.next()!=Some("/relationships"){return false}
 let Some(query)=pieces.next() else{return false};if pieces.next().is_some(){return false}
 let ids:Vec<_>=query.split('&').filter_map(|v|v.strip_prefix("tournamentId=")).collect();
 ids.len()==1&&ids[0]==scope.tournament_id
}
fn rule_scope(r:&Receipt,c:&ContractCertificate,scope:&Scope,v2:bool)->Result<()>{
 let market=r.data.get("market_id").or_else(||r.data.get("id")).and_then(|v|v.as_str().map(str::to_owned).or_else(||v.as_u64().map(|n|n.to_string())));
 need(market.as_ref()==Some(&c.market_id),"STRATEGY_RULE_MARKET_SCOPE")?;
 if !v2{return need(r.endpoint==c.rule_endpoint&&(r.endpoint==format!("/markets/{}/nodes",c.market_id)||r.endpoint==format!("/markets/{}",c.market_id)),"STRATEGY_RULE_ENDPOINT")}
 need(scope.exchanges.get(&c.exchange_id)==Some(&c.market_id),"STRATEGY_RULE_MARKET_SCOPE")?;
 let mut parts=r.endpoint.split('?');let base=parts.next().unwrap_or("");
 need(base==format!("/markets/{}/nodes",c.market_id)||base==format!("/markets/{}",c.market_id),"STRATEGY_RULE_ENDPOINT")?;
 if let Some(query)=parts.next(){need(parts.next().is_none()&&query==format!("tournamentId={}",scope.tournament_id),"STRATEGY_RULE_TOURNAMENT_SCOPE")?;}
 for key in ["tournamentId","tournament_id"]{if let Some(t)=r.data.get(key){need(t.as_str()==Some(scope.tournament_id.as_str()),"STRATEGY_RULE_TOURNAMENT_SCOPE")?;}}
 Ok(())
}
/// Selects verified CURRENT rule evidence, suitable for admission expiry.
/// V1 preserves exact original freshness. V2 compares every rule-body field and
/// never lets older matching evidence hide a newer changed/unsuccessful receipt.
pub fn validated_rule_receipt<'a>(cert:&SettlementCertificate,c:&ContractCertificate,certificates:&Certificates<'a>,scope:&Scope,p:&Policy)->Result<&'a Receipt>{
 need(matches!(cert.version,1|2),"STRATEGY_CERTIFICATE_BINDING")?;
 let originals:Vec<_>=certificates.rule_receipts.iter().filter(|r|hash(*r).ok().as_ref()==Some(&c.rule_receipt_sha256)).collect();
 need(originals.len()==1,"STRATEGY_RULE_RECEIPT_MISSING_OR_AMBIGUOUS")?;let original=originals[0];
 rule_scope(original,c,scope,cert.version==2)?;
 if cert.version==1{fresh(original,p.now_ms,p.max_rule_age_ms)?;return Ok(original)}
 need(original.endpoint==c.rule_endpoint&&original.status==200&&clock(&original.request_started_at)?<=clock(&original.received_at)?&&clock(&original.received_at)?<=cert.issued_at_ms,"STRATEGY_RULE_ATTESTATION_BINDING")?;
 let expected=c.rule_data_sha256.as_ref().ok_or("STRATEGY_RULE_DATA_HASH_REQUIRED")?;
 need(rule_data_sha256(original)?==*expected,"STRATEGY_RULE_DATA_CHANGED")?;
 let base=original.endpoint.split('?').next().unwrap_or("");let mut candidates=Vec::new();
 for r in certificates.rule_receipts{
  if r.endpoint.split('?').next()==Some(base){rule_scope(r,c,scope,true)?;candidates.push((clock(&r.received_at)?,clock(&r.request_started_at)?,r));}
 }
 let latest=candidates.iter().map(|(received,started,_)|(*received,*started)).max().ok_or("STRATEGY_RULE_RECEIPT_MISSING_OR_AMBIGUOUS")?;
 let current:Vec<_>=candidates.iter().filter(|(r,s,_)|(*r,*s)==latest).map(|(_,_,r)|*r).collect();
 need(current.len()==1,"STRATEGY_RULE_RECEIPT_MISSING_OR_AMBIGUOUS")?;let r=current[0];
 fresh(r,p.now_ms,p.max_rule_age_ms)?;need(rule_data_sha256(r)?==*expected,"STRATEGY_RULE_DATA_CHANGED")?;Ok(r)
}
fn validate_v2_attestation(row:&Relationship,cert:&SettlementCertificate,certificates:&Certificates<'_>,scope:&Scope)->Result<()>{
 let originals:Vec<_>=certificates.rule_receipts.iter().filter(|r|hash(*r).ok().as_ref()==Some(&cert.relationship_receipt_sha256)).collect();
 need(originals.len()==1,"STRATEGY_RELATIONSHIP_ATTESTATION_MISSING_OR_AMBIGUOUS")?;let r=originals[0];
 need(r.status==200&&relationship_endpoint(&r.endpoint,scope)&&clock(&r.request_started_at)?<=clock(&r.received_at)?&&clock(&r.received_at)?<=cert.issued_at_ms,"STRATEGY_RELATIONSHIP_ATTESTATION_BINDING")?;
 let list=relationships::parse_list(&r.data,scope).map_err(|_|"STRATEGY_CANONICAL_SCHEMA")?;need(!list.pagination.has_more,"STRATEGY_RELATIONSHIPS_INCOMPLETE")?;
 let matches:Vec<_>=list.data.iter().filter(|old|old.id==row.id).collect();need(matches.len()==1,"STRATEGY_RELATIONSHIP_ATTESTATION_BINDING")?;let old=matches[0];
 need(old.status==RelationshipStatus::Active&&old.version==cert.relationship_version&&relationship_semantics_sha256(old)?==cert.canonical_relationship_sha256&&relationship_semantics_sha256(row)?==cert.canonical_relationship_sha256,"STRATEGY_CERTIFICATE_BINDING")
}

fn compile_row(row:&Relationship,census:&Census,source:&Source,certificates:&Certificates<'_>,p:&Policy)->Result<(arb::Compiled,String)>{
 need(row.status==RelationshipStatus::Active,"STRATEGY_RELATIONSHIP_INACTIVE")?;
 need((2..=3).contains(&row.nodes.len()),"STRATEGY_COMPONENT_SIZE")?;
 need(matches!(row.r#type,RelationshipType::MutuallyExclusive|RelationshipType::Complementary),"STRATEGY_LOGIC_UNSUPPORTED")?;
 need(row.direction.kind==DirectionKind::Symmetric&&row.nodes.iter().all(|n|n.role==NodeRole::Member),"STRATEGY_DIRECTION_UNVERIFIED")?;
 need(row.trading_semantics_ready(),"STRATEGY_CONTRACT_SCOPE_INCOMPLETE")?;
 let matching:Vec<_>=certificates.values.iter().filter(|c|c.relationship_id==row.id).collect();
 need(matching.len()==1,"STRATEGY_CERTIFICATE_MISSING_OR_AMBIGUOUS")?;
 let cert=matching[0];let digest=certificate_sha256(cert)?;
 need(certificates.trusted_sha256.contains(&digest),"STRATEGY_CERTIFICATE_NOT_APPROVED")?;
 need(matches!(cert.version,1|2)&&cert.tournament_id==census.cache.scope.tournament_id&&cert.relationship_version==row.version,"STRATEGY_CERTIFICATE_BINDING")?;
 if cert.version==1{need(cert.canonical_relationship_sha256==hash(row)?&&cert.relationship_receipt_sha256==source.sha256,"STRATEGY_CERTIFICATE_BINDING")?;}else{validate_v2_attestation(row,cert,certificates,&census.cache.scope)?;}
 need(cert.issued_at_ms<=p.now_ms&&p.now_ms<cert.expires_at_ms&&!cert.verifier_id.is_empty()&&cert.verifier_id.len()<=256&&!cert.verifier_id.chars().any(char::is_control),"STRATEGY_CERTIFICATE_CLOCK_OR_VERIFIER")?;
 need(cert.contracts.len()==row.nodes.len(),"STRATEGY_CERTIFICATE_CONTRACTS")?;
 let logic=match (&cert.logic,row.r#type) {
  (CertifiedLogic::MutuallyExclusive{exhaustive},RelationshipType::MutuallyExclusive)=>{need(*exhaustive==row.is_exhaustive,"STRATEGY_EXHAUSTIVENESS_MISMATCH")?;Logic::MutuallyExclusive{members:(0..row.nodes.len()).collect(),exhaustive:*exhaustive}},
  (CertifiedLogic::Complement,RelationshipType::Complementary)=>{need(row.nodes.len()==2,"STRATEGY_COMPLEMENT_ARITY")?;Logic::Complement{a:0,b:1}},
  _=>return Err("STRATEGY_CERTIFIED_LOGIC_MISMATCH"),
 };
 let mut contracts=vec![];let mut oldest_rule=u64::MAX;
 for (n,c) in row.nodes.iter().zip(&cert.contracts){
  need(n.exchange_id==c.exchange_id&&n.market_id.as_ref()==Some(&c.market_id)&&n.contract_id.as_ref()==Some(&c.contract_id)&&n.outcome.as_ref()==Some(&c.canonical_outcome)&&census.cache.scope.exchanges.get(&c.exchange_id)==Some(&c.market_id),"STRATEGY_CERTIFICATE_CONTRACT_SCOPE")?;
  need(exact(&c.ordinary_yes_payout)?==arb::UNIT&&exact(&c.ordinary_no_payout)?==0&&exact(&c.ordinary_no_yes_payout)?==0&&exact(&c.ordinary_no_no_payout)?==arb::UNIT&&c.terminal_rules_complete,"STRATEGY_ORDINARY_PAYOFF_UNVERIFIED")?;
  let r=validated_rule_receipt(cert,c,certificates,&census.cache.scope,p)?;
  let rule_source=fresh(r,p.now_ms,p.max_rule_age_ms)?;oldest_rule=oldest_rule.min(rule_source.received_ms);
  let exceptions=c.exceptions.iter().map(|e|Ok((exact(&e.yes_payout)?,exact(&e.no_payout)?))).collect::<Result<Vec<_>>>()?;
  contracts.push(Contract{exchange_id:c.exchange_id.clone(),market_id:c.market_id.clone(),contract_id:c.contract_id.clone(),settlement:Settlement::Verified{rules:rule_source,complete:true,exceptions}});
 }
 // The approved certificate binds the complete relation interpretation and all
 // underlying rule hashes. It is not an API receipt or a discovered payout floor.
 // V2 time is revalidation against current evidence, never new approval.
 let proof=Source{sha256:digest.clone(),received_ms:if cert.version==2{source.received_ms.min(oldest_rule)}else{cert.issued_at_ms.min(oldest_rule)}};
 let compiled=arb::compile(contracts,&[Declaration{relationship_id:row.id.clone(),version:row.version,active:true,source:source.clone(),settlement_logic_source:Some(proof),logic}],p.now_ms,p.max_rule_age_ms.max(p.max_relationship_age_ms))?;
 Ok((compiled,digest))
}

pub fn analyze(census:&Census,canonical_receipt:&Receipt,certificates:&Certificates<'_>,books:&BTreeMap<String,Book>,positions:&[OwnedPosition],capital:&Capital,p:&Policy)->Analysis{
 let state=match census.report.relationships.availability{Availability::Available=>"available",Availability::Empty=>"available_empty",Availability::Forbidden=>"forbidden",Availability::Unavailable=>"unavailable",Availability::Invalid=>"invalid",Availability::NotRequested=>"not_requested"};
 let mut a=Analysis{public:PublicDiagnostic{version:1,at_ms:p.now_ms,canonical_feed_state:state.into(),canonical_relationships:0,supported_active_relationships:0,certified_components:0,positive_candidates:0,retained_candidates:0,blocked_reason_counts:BTreeMap::new(),execution_authority:false},candidates:vec![]};
 let source=match canonical_source(census,canonical_receipt,p){Ok(s)=>s,Err(e)=>{block(&mut a,e);return a}};
 let limits=match capital.limits(p){Ok(l)=>l,Err(e)=>{block(&mut a,e);return a}};
 if positions.len()>1024||certificates.values.len()>4096||certificates.rule_receipts.len()>4096||books.len()>1024||!(1..=256).contains(&p.maximum_retained_candidates){block(&mut a,"STRATEGY_INPUT_BUDGET");return a}
 let rows=&census.cache.relationships.as_ref().expect("validated cache").data;
 a.public.canonical_relationships=rows.len();
 if rows.len()>4096{block(&mut a,"STRATEGY_RELATIONSHIP_BUDGET");return a}
 for row in rows{
  if row.status==RelationshipStatus::Active&&matches!(row.r#type,RelationshipType::MutuallyExclusive|RelationshipType::Complementary){a.public.supported_active_relationships+=1;}
  if certificates.values.iter().any(|c|c.relationship_id==row.id&&c.version==2)&&!relationship_endpoint(&canonical_receipt.endpoint,&census.cache.scope){block(&mut a,"STRATEGY_RELATIONSHIP_RECEIPT_BINDING");continue}
  let (compiled,digest)=match compile_row(row,census,&source,certificates,p){Ok(c)=>c,Err(e)=>{block(&mut a,e);continue}};
  a.public.certified_components+=1;
  let normalized:Vec<_>=compiled.contracts().iter().filter_map(|c|books.get(&c.exchange_id).cloned()).collect();
  if normalized.len()!=compiled.contracts().len(){block(&mut a,"STRATEGY_EXECUTABLE_BOOK_MISSING");continue}
  let mut inventory:BTreeMap<Instrument,u64>=BTreeMap::new();let mut inventory_error=false;
  for position in positions{
   if let Some(contract)=compiled.contracts().iter().position(|c|c.exchange_id==position.exchange_id){
    let i=Instrument{contract,side:position.side};let n=inventory.entry(i).or_default();
    if let Some(sum)=n.checked_add(position.quantity){*n=sum}else{inventory_error=true;}
   }
  }
  if inventory_error{block(&mut a,"STRATEGY_INVENTORY_RANGE");continue}
  let held:Vec<_>=inventory.into_iter().collect();
  let existing=match compiled.portfolio_floor(&held){Ok(v)=>v,Err(e)=>{block(&mut a,e);continue}};
  let plans=match compiled.scan(&p.quantities,&normalized,&limits){Ok(v)=>v,Err(e)=>{block(&mut a,e);continue}};
  if plans.is_empty(){block(&mut a,"STRATEGY_NO_EXECUTABLE_QUALIFIED_PACKAGE");}
  for plan in plans{
   let marginal=plan.instruments.iter().map(|i|compiled.marginal_floor(&held,*i,plan.quantity)).collect::<Result<Vec<_>>>();
   let marginal=match marginal{Ok(v)=>v,Err(e)=>{block(&mut a,e);continue}};
   a.candidates.push(PrivateCandidate{relationship_id:row.id.clone(),relationship_version:row.version,certificate_sha256:digest.clone(),contracts:compiled.contracts().iter().map(|c|(c.exchange_id.clone(),c.market_id.clone(),c.contract_id.clone())).collect(),existing_component_floor:existing,marginal_floors:marginal,plan});
   a.public.positive_candidates+=1;
  }
  if a.candidates.len()>p.maximum_retained_candidates*2{rank(&mut a.candidates);a.candidates.truncate(p.maximum_retained_candidates);}
 }
 rank(&mut a.candidates);a.candidates.truncate(p.maximum_retained_candidates);
 a.public.retained_candidates=a.candidates.len();a
}
fn rank(plans:&mut [PrivateCandidate]){plans.sort_by(|a,b|b.plan.guaranteed_surplus.cmp(&a.plan.guaranteed_surplus).then(a.relationship_id.cmp(&b.relationship_id)).then(a.plan.instruments.cmp(&b.plan.instruments)).then(a.plan.quantity.cmp(&b.plan.quantity)));}
/// Verified relationship component for portfolio acquisition/unwind planning.
/// Its constructor revalidates canonical and independently approved complete
/// settlement evidence; a serialized component cannot grant planning authority.
pub struct CertifiedComponent{compiled:arb::Compiled,scope:Scope,relationship_id:String,relationship_version:u64,certificate_sha256:String,canonical_sha256:String,validated_at_ms:u64,expires_at_ms:u64,rule_bindings:BTreeMap<String,Value>}
impl CertifiedComponent{
 pub fn compiled(&self)->&arb::Compiled{&self.compiled}
 pub fn scope(&self)->&Scope{&self.scope}
 pub fn relationship_id(&self)->&str{&self.relationship_id}
 pub fn certificate_sha256(&self)->&str{&self.certificate_sha256}
 pub fn validated_at_ms(&self)->u64{self.validated_at_ms}
 pub fn expires_at_ms(&self)->u64{self.expires_at_ms}
 pub fn source_binding(&self)->Value{json!({"relationshipId":self.relationship_id,"relationshipVersion":self.relationship_version,"certificateSha256":self.certificate_sha256,"canonicalReceiptSha256":self.canonical_sha256,"scope":self.scope,"validatedAtMs":self.validated_at_ms,"expiresAtMs":self.expires_at_ms,"statesSha256":hash(&self.compiled.states()).expect("serializable payoff states"),"contracts":self.compiled.contracts().iter().map(|c|json!({"exchangeId":c.exchange_id,"marketId":c.market_id,"contractId":c.contract_id})).collect::<Vec<_>>(),"ruleBindings":self.rule_bindings})}
}
pub fn certified_component(census:&Census,canonical:&Receipt,certificates:&Certificates<'_>,relationship_id:&str,p:&Policy)->Result<CertifiedComponent>{
 let source=canonical_source(census,canonical,p)?;let rows=&census.cache.relationships.as_ref().ok_or("STRATEGY_CANONICAL_CACHE_MISSING")?.data;let matching:Vec<_>=rows.iter().filter(|r|r.id==relationship_id).collect();need(matching.len()==1,"STRATEGY_RELATIONSHIP_MISSING_OR_AMBIGUOUS")?;let row=matching[0];let(compiled,digest)=compile_row(row,census,&source,certificates,p)?;let cert=certificates.values.iter().find(|c|certificate_sha256(c).ok().as_deref()==Some(&digest)).ok_or("STRATEGY_CERTIFICATE_MISSING_OR_AMBIGUOUS")?;
 let mut rule_bindings=BTreeMap::new();let mut expires=cert.expires_at_ms.min(source.received_ms.checked_add(p.max_relationship_age_ms).ok_or("STRATEGY_CLOCK")?).min(compiled.source_expiry_ms());for c in &cert.contracts{let r=validated_rule_receipt(cert,c,certificates,&census.cache.scope,p)?;let(kind,digest)=if cert.version==1{("exact_receipt_v1",hash(r)?)}else{("complete_rule_body_v2",rule_data_sha256(r)?)};need(rule_bindings.insert(c.exchange_id.clone(),json!({"kind":kind,"endpoint":r.endpoint,"sha256":digest})).is_none(),"STRATEGY_DUPLICATE_RULE_BINDING")?;expires=expires.min(clock(&r.received_at)?.checked_add(p.max_rule_age_ms).ok_or("STRATEGY_CLOCK")?);}need(p.now_ms<expires,"STRATEGY_CERTIFICATE_CLOCK_OR_VERIFIER")?;Ok(CertifiedComponent{compiled,scope:census.cache.scope.clone(),relationship_id:row.id.clone(),relationship_version:row.version,certificate_sha256:digest,canonical_sha256:source.sha256,validated_at_ms:p.now_ms,expires_at_ms:expires,rule_bindings})
}
