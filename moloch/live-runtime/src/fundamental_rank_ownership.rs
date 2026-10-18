//! Ownership-only continuation for a separately approved rank-risk SELL.
//! This is not a fundamental-value decision and imposes no accounting-profit
//! floor. Original cohorts, lots, entry bindings and prior fills remain intact.
use super::*;

/// Constructed only after the held original Cohorts writer registers the exact
/// SELL. It cannot be deserialized from a nomination or an approval JSON file.
pub struct RankInventoryOwnership{
    qid:u64,quote:Value,scope_sha:String,spec_sha:String,permit_sha:String,
    ledger_sha:String,head:Value,tag:Value,minted:u64,until:u64,
    sidecar:FilePin,marker:FilePin,ordinary:File,ordinary_path:PathBuf,
    ordinary_identity:Value,_owner:Arc<ExclusiveLock>,
}
impl RankInventoryOwnership{
    pub fn native_head(&self)->&Value{&self.head}
    pub fn expires_at_ms(&self)->u64{self.until}
    pub fn durable_tag(&self)->&Value{&self.tag}
    pub fn validate_frozen(&self,q:&Value,ledger:&Value,qid:u64,at:u64)->Result<()>{
        need(at>=self.minted&&at<self.until&&qid==self.qid&&q==&self.quote
            &&digest(ledger)?==self.ledger_sha,"RANK_COHORT_FROZEN_CHANGED")
    }
    pub fn authorize(&self,scope:&Scope,spec:&RequestSpec,permit:&Permit,at:u64)->Result<()>{
        need(at>=self.minted&&at<self.until&&digest(scope)?==self.scope_sha
            &&digest(spec)?==self.spec_sha&&digest(permit)?==self.permit_sha,"RANK_COHORT_REQUEST_CHANGED")?;
        self.sidecar.validate()?;self.marker.validate()?;
        need(identity(&self.ordinary)?==self.ordinary_identity,"RANK_COHORT_DISPATCH_CHANGED")?;
        path_pin(&self.ordinary_path,&self.ordinary_identity)?;self._owner.descriptor_identity()?;Ok(())
    }
}

fn compact_quote(q:&Value)->Value{
 let mut out=serde_json::Map::new();for k in ["id","marketId","exchangeId","side","action","quantity","price","activationAt","expiresAt"]{out.insert(k.into(),q[k].clone());}
 out.insert("reviewedRankCampaign".into(),json!({"campaignSha256":q["reviewedRankCampaign"]["campaignSha256"]}));out.into()
}
fn fold_fill(position:&mut Value,f:&Value)->Result<()>{
 let side=f["side"].as_str().filter(|s|*s=="YES"||*s=="NO").ok_or("RANK_COHORT_FILL_SIDE")?;let opposite=if side=="YES"{"NO"}else{"YES"};
 let mut left=f["quantity"].as_u64().filter(|n|*n>0).ok_or("RANK_COHORT_FILL_QUANTITY")?;
 need(f["counterfactual"]==false&&f["price"].as_str().is_some(),"RANK_COHORT_ACTUAL_FILL_REQUIRED")?;
 if f["action"]=="buy"{
  need(position[opposite].as_array().is_some_and(Vec::is_empty),"RANK_COHORT_UNSUPPORTED_NETTING")?;
  let mut lot=json!({"price":f["price"],"quantity":left});if f["executedAt"].is_string(){lot["executedAt"]=f["executedAt"].clone();}
  position[side].as_array_mut().ok_or("RANK_COHORT_FIFO")?.push(lot);
 }else{
  need(f["action"]=="sell","RANK_COHORT_FILL_ACTION")?;let lots=position[side].as_array_mut().ok_or("RANK_COHORT_FIFO")?;
  while left>0{let first=lots.first_mut().ok_or("RANK_COHORT_UNCOVERED_SELL")?;let n=first["quantity"].as_u64().ok_or("RANK_COHORT_FIFO")?;let take=n.min(left);left-=take;if take==n{lots.remove(0);}else{first["quantity"]=json!(n-take);}}
 }Ok(())
}
impl Cohorts{
 pub fn rank_paused(&self,exchange:&str)->bool{self.rank_bindings.values().any(|v|v["exchangeId"]==exchange)}
 fn ownership_statement(&self,bank:&Session,ex:&str)->Result<Value>{
  let c=self.cohorts.get(ex).ok_or("RANK_COHORT_MISSING")?;let r=self.policies.get(ex).ok_or("RANK_COHORT_REVIEW_MISSING")?;
  Ok(json!({"schemaVersion":1,"producer":"moloch_original_cohort_owner","runId":self.run,"bankManifestHash":self.manifest,
   "exchangeId":ex,"generationSha256":self.generation_sha(),"reviewSha256":digest(r)?,"cohortSha256":digest(c)?,"originSha256":c.origin_sha()?,
   "positionsSha256":digest(&bank.engine().wallet.positions()[ex])?,"handoffEstablished":self.rank_paused(ex),
   "exclusionSha256":bank.engine().ordinary_entry_exclusion_record().ok_or("RANK_COHORT_EXCLUSION_MISSING")?["sha256"],"ownershipOnly":true,"executionAuthority":false}))
 }
 /// Compact ownership source; original bindings remain in the immutable held
 /// sidecar. No model probability, cash credit or physical permission is added.
 pub fn rank_inventory_state(&self,bank:&Session,log:&DispatchLog,ex:&str)->Result<Value>{
  self.check(bank,log)?;self.rank_inventory_state_inner(bank,ex)
 }
 pub fn rank_inventory_states(&self,bank:&Session,log:&DispatchLog)->Result<Vec<Value>>{
  self.check(bank,log)?;let mut states=Vec::new();for ex in self.policies.keys(){if bank.engine().wallet.quantity(ex,Side::Yes)+bank.engine().wallet.quantity(ex,Side::No)>0||self.rank_paused(ex){states.push(self.rank_inventory_state_inner(bank,ex)?);}}Ok(states)
 }
 fn rank_inventory_state_inner(&self,bank:&Session,ex:&str)->Result<Value>{
  let statement=self.ownership_statement(bank,ex)?;let body=serde_json::to_vec(&statement)?;
  let yes=bank.engine().wallet.quantity(ex,Side::Yes);let no=bank.engine().wallet.quantity(ex,Side::No);let mut out=statement;
  out["sourceSha256"]=json!(bytes_sha(&body));out["sourceBodyText"]=json!(String::from_utf8(body)?);
  out["heldQuantity"]=json!(yes+no);out["positions"]=bank.engine().wallet.positions()[ex].clone();
  out["side"]=json!(if yes>0{"YES"}else if no>0{"NO"}else{self.policies[ex].side.as_str()});out["rankPaused"]=json!(self.rank_paused(ex));Ok(out)
 }
 pub fn check_rank_review(&self,bank:&Session,log:&DispatchLog,review:&crate::rank_campaign::Reviewed)->Result<()>{
  self.check(bank,log)?;log.require_resolved_attempts()?;let ex=review.exchange_id();let a=review.action();
  let proof=review.managed_inventory().ok_or("RANK_COHORT_HANDOFF_REQUIRED")?;let r=self.policies.get(ex).ok_or("RANK_COHORT_REVIEW_MISSING")?;
  let side=match a["side"].as_str(){Some("YES")=>Side::Yes,Some("NO")=>Side::No,_=>return Err("RANK_COHORT_SIDE".into())};
  let n=a["quantity"].as_u64().filter(|n|*n>0).ok_or("RANK_COHORT_QUANTITY")?;
  let buy=a["action"]=="BUY";need(buy||a["action"]=="SELL","RANK_COHORT_ACTION")?;
  need(a["marketId"]==r.market_id&&bank.engine().wallet.quantity(ex,if side==Side::Yes{Side::No}else{Side::Yes})==0
   &&if buy{self.rank_paused(ex)}else{n<=bank.engine().wallet.quantity(ex,side)},"RANK_COHORT_FUNDED_ACTION_REQUIRED")?;
  if !self.rank_paused(ex){need(!buy&&a["side"]==r.side,"RANK_COHORT_FIRST_HANDOFF_REQUIRES_ORIGINAL_SELL")?;}
  let registered=bank.engine().order_records().iter().find(|q|q["reviewedRankCampaign"]["campaignSha256"]==review.campaign_sha256()).and_then(|q|q["id"].as_u64()).and_then(|id|self.rank_bindings.get(&id));
  if let Some(old)=registered{need(old["managedInventory"]==*proof,"RANK_COHORT_ORIGINAL_HANDOFF_CHANGED")?;}else{
   let current=self.rank_inventory_state(bank,log,ex)?;
   for k in ["schemaVersion","exchangeId","generationSha256","reviewSha256","cohortSha256","originSha256","exclusionSha256","positionsSha256","handoffEstablished","sourceSha256"]{need(proof[k]==current[k],"RANK_COHORT_HANDOFF_CHANGED")?;}
  }
  need(proof["generationSha256"]==self.generation_sha()&&proof["reviewSha256"]==digest(r)?,"RANK_COHORT_GENERATION_CHANGED")?;Ok(())
 }
 pub(super) fn apply_rank_trade(&mut self,event:&Value)->Result<()>{
  let ex=event["exchangeId"].as_str().ok_or("RANK_COHORT_EXCHANGE")?;let b:Binding=serde_json::from_value(event["binding"].clone())?;
  let generation=self.generation_sha().to_owned();let c=self.cohorts.get_mut(ex).ok_or("RANK_COHORT_MISSING")?;let first=c.rank_trades.is_empty();let proof=&event["managedInventory"];
  need(proof["schemaVersion"]==1&&proof["exchangeId"]==ex&&proof["cohortSha256"]==digest(c)?&&proof["originSha256"]==c.origin_sha()?
   &&proof["generationSha256"]==generation&&proof["reviewSha256"]==b.review_sha256&&b.review_sha256==digest(&self.policies[ex])?
   &&proof["handoffEstablished"]==!first&&event["ownershipOnly"]==true&&event["positiveMonetaryAlphaClaimed"]==false
   &&b.quote["exchangeId"]==ex&&b.quote["reviewedRankCampaign"]["campaignSha256"]==event["campaignSha256"]
   &&c.rank_trades.len()<4096&&!c.rank_trades.contains_key(&b.qid)&&!self.rank_bindings.contains_key(&b.qid),"RANK_COHORT_REGISTRATION_CHANGED")?;
  if first{
   need(b.quote["action"]=="sell"&&b.quote["side"]==self.policies[ex].side,"RANK_COHORT_FIRST_HANDOFF_REQUIRES_ORIGINAL_SELL")?;
   let h=&event["handoff"];let raw=h["originalCohortBodyText"].as_str().ok_or("RANK_COHORT_ORIGINAL_BODY")?;
   need(bytes_sha(raw.as_bytes())==proof["cohortSha256"]&&serde_json::from_str::<Cohort>(raw).is_ok()
    &&digest(&h["positions"])?==proof["positionsSha256"]&&h["firstFillIndex"].is_u64()&&h["priorFillsSha256"].as_str().is_some(),"RANK_COHORT_ORIGINAL_BOUNDARY")?;
  }else{need(event["handoff"].is_null(),"RANK_COHORT_HANDOFF_CANNOT_BE_REPLACED")?;}
  c.rank_trades.insert(b.qid,b.clone());self.rank_bindings.insert(b.qid,event.clone());Ok(())
 }
 /// Reconstruct original FIFO at the transfer, then ONLY registered actual rank
 /// trades. An opposite-side buy is possible only after its old side is empty.
 pub(super) fn rank_owned(&self,bank:&Session,ex:&str,c:&Cohort)->Result<u64>{
  let first=self.rank_bindings.values().find(|v|v["exchangeId"]==ex&&!v["handoff"].is_null()).ok_or("RANK_COHORT_ORIGINAL_HANDOFF_REQUIRED")?;
  let h=&first["handoff"];let end=usize::try_from(h["firstFillIndex"].as_u64().ok_or("RANK_COHORT_FILL_PREFIX")?)?;let fills=bank.engine().fills();
  need(end<=fills.len()&&digest(&fills[..end])?==h["priorFillsSha256"],"RANK_COHORT_FILL_PREFIX_CHANGED")?;
  let original:Cohort=serde_json::from_str(h["originalCohortBodyText"].as_str().ok_or("RANK_COHORT_ORIGINAL_BODY")?)?;
  need(original.rank_trades.is_empty()&&original.first_fill_index<=end&&digest(&fills[..original.first_fill_index])?==original.prior_fills_sha256,"RANK_COHORT_ORIGIN_PREFIX_CHANGED")?;
  let mut position=json!({"YES":[],"NO":[]});let original_side=&self.policies[ex].side;
  if let Some(o)=&original.existing{position[original_side]=o.lots.clone();}
  for f in &fills[original.first_fill_index..end]{if f["exchangeId"]!=ex{continue}let id=f["orderId"].as_u64().ok_or("RANK_COHORT_FILL_ID")?;
   need(f["side"]==*original_side&&(original.is_entry(id)&&f["action"]=="buy"||original.exits.contains_key(&id)&&f["action"]=="sell"),"RANK_COHORT_ORIGINAL_FOREIGN_FILL")?;fold_fill(&mut position,f)?;
  }
  need(position==h["positions"],"RANK_COHORT_ORIGINAL_FIFO_CHANGED")?;
  for f in &fills[end..]{if f["exchangeId"]!=ex{continue}let id=f["orderId"].as_u64().ok_or("RANK_COHORT_FILL_ID")?;let b=c.rank_trades.get(&id).ok_or("RANK_COHORT_FOREIGN_FILL")?;
   need(f["side"]==b.quote["side"]&&f["action"]==b.quote["action"]&&bank.engine().live_order_bindings().get(&id).is_some_and(|a|f["actualOrderId"]==*a),"RANK_COHORT_FILL_BINDING_CHANGED")?;fold_fill(&mut position,f)?;
  }
  need(position==bank.engine().wallet.positions()[ex],"RANK_COHORT_FIFO_CHANGED")?;Ok(bank.engine().wallet.quantity(ex,if original_side=="YES"{Side::Yes}else{Side::No}))
 }
 pub fn bind_rank_trade(&mut self,bank:&Session,log:&DispatchLog,scope:&Scope,review:&crate::rank_campaign::Reviewed,qid:u64,spec:&RequestSpec,permit:&Permit,at:u64,until:u64)->Result<RankInventoryOwnership>{
  self.check_rank_review(bank,log,review)?;need(log.never_attempted(qid)&&!log.intents.contains_key(&qid)&&at<until,"RANK_COHORT_ALREADY_ATTEMPTED")?;
  let q=crate::order_snapshot::quote(bank,qid).ok_or("RANK_COHORT_QUOTE")?;let ex=review.exchange_id();let r=&self.policies[ex];
  need(q["reviewedRankCampaign"]["campaignSha256"]==review.campaign_sha256()&&q["retired"]==false&&q["filled"]==0
   &&q["side"]==review.action()["side"]&&q["action"]==if review.action()["action"]=="BUY"{"buy"}else{"sell"}
   &&q["quantity"]==review.action()["quantity"]&&scope.tournament_id==r.tournament_id&&scope.exchanges.get(ex)==Some(&r.market_id),"RANK_COHORT_QUOTE_CHANGED")?;
  need(log.acknowledgments.keys().all(|id|log.closed.contains(id))&&bank.engine().order_records().iter().all(|v|v["exchangeId"]!=ex||v["retired"]==true||v["id"]==qid),"RANK_COHORT_OTHER_PHYSICAL_OR_DRAFT_LIABILITY")?;
  request_matches(spec,&q,&self.run,&scope.tournament_id,q["quantity"].as_u64().ok_or("RANK_COHORT_QUANTITY")?)?;
  if let Some(previous)=self.rank_bindings.get(&qid){let b:Binding=serde_json::from_value(previous["binding"].clone())?;
   need(b.quote==compact_quote(&q)&&b.spec_sha256==digest(spec)?&&previous["managedInventory"]==*review.managed_inventory().unwrap(),"RANK_COHORT_ORIGINAL_BINDING_CHANGED")?;
  }else{
   let c=&self.cohorts[ex];let handoff=if self.rank_paused(ex){Value::Null}else{json!({"originalCohortBodyText":String::from_utf8(serde_json::to_vec(c)?)?,"positions":bank.engine().wallet.positions()[ex],"firstFillIndex":bank.engine().fills().len(),"priorFillsSha256":digest(bank.engine().fills())?,"nativeHead":bank.durable_head()})};
   let binding=Binding{qid,spec:spec.clone(),spec_sha256:digest(spec)?,quote:compact_quote(&q),review_sha256:digest(r)?,bound_at_ms:at};
   self.append(json!({"kind":"rank_trade","exchangeId":ex,"campaignSha256":review.campaign_sha256(),"managedInventory":review.managed_inventory(),"handoff":handoff,"originalNativeQuoteSha256":digest(&q)?,"binding":binding,"ownershipOnly":true,"positiveMonetaryAlphaClaimed":false}),at)?;
  }
  self.check_rank_bound(bank,log,scope,qid,spec)?;
  Ok(RankInventoryOwnership{qid,quote:q,scope_sha:digest(scope)?,spec_sha:digest(spec)?,permit_sha:digest(permit)?,ledger_sha:digest(&bank.engine().report()?)?,head:bank.durable_head(),tag:self.durable_tag(qid).ok_or("RANK_COHORT_TAG")?,minted:at,until,
   sidecar:FilePin::from_held(self.file.try_clone()?,self.path.clone(),MAX_LOG,None)?,marker:self.marker.duplicate()?,ordinary:File::open(&self.ordinary_path)?,ordinary_path:self.ordinary_path.clone(),ordinary_identity:self.ordinary_identity.clone(),_owner:self._owner.clone()})
 }
 pub fn check_rank_bound(&self,bank:&Session,log:&DispatchLog,scope:&Scope,qid:u64,spec:&RequestSpec)->Result<()>{
  self.check(bank,log)?;let event=self.rank_bindings.get(&qid).ok_or("RANK_COHORT_BINDING_REQUIRED")?;let b:Binding=serde_json::from_value(event["binding"].clone())?;
  need(b.spec_sha256==digest(spec)?&&scope.exchanges.get(event["exchangeId"].as_str().ok_or("RANK_COHORT_EXCHANGE")?)==b.quote["marketId"].as_str().map(String::from).as_ref()
   &&crate::order_snapshot::quote(bank,qid).is_some_and(|q|compact_quote(&q)==b.quote),"RANK_COHORT_BOUND_STATE_CHANGED")
 }
}
