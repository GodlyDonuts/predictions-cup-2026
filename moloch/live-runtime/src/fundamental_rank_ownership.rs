//! Ownership-only continuation for a separately approved rank-risk SELL.
//! This is not a fundamental-value decision and imposes no accounting-profit
//! floor. Original cohorts, lots, entry bindings and prior fills remain intact.
use super::*;

/// Constructed only after the held original Cohorts writer registers the exact
/// SELL. It cannot be deserialized from a nomination or an approval JSON file.
pub struct RankExitOwnership{
    qid:u64,quote:Value,scope_sha:String,spec_sha:String,permit_sha:String,
    ledger_sha:String,head:Value,tag:Value,minted:u64,until:u64,
    sidecar:FilePin,marker:FilePin,ordinary:File,ordinary_path:PathBuf,
    ordinary_identity:Value,_owner:Arc<ExclusiveLock>,
}
impl RankExitOwnership{
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
impl Cohorts{
    pub fn rank_paused(&self,exchange:&str)->bool{self.rank_exits.values().any(|v|v["exchangeId"]==exchange)}
    /// Original, current ownership input for offline nominations. A caller still
    /// needs independent owner approval and current whole-account facts.
    pub fn rank_exit_state(&self,bank:&Session,log:&DispatchLog,exchange:&str)->Result<Value>{
        self.check(bank,log)?;let c=self.cohorts.get(exchange).ok_or("RANK_COHORT_MISSING")?;
        let r=self.policies.get(exchange).ok_or("RANK_COHORT_REVIEW_MISSING")?;
        let held=self.owned(bank,exchange,c)?;
        Ok(json!({"schemaVersion":1,"exchangeId":exchange,"side":r.side,"generationSha256":self.generation_sha(),
            "reviewSha256":digest(r)?,"cohortSha256":digest(c)?,"cohort":c,"cohortBodyText":String::from_utf8(serde_json::to_vec(c)?)?,"originSha256":c.origin_sha()?,
            "positionsSha256":digest(&bank.engine().wallet.positions()[exchange])?,"heldQuantity":held,
            "exclusionSha256":bank.engine().ordinary_entry_exclusion_record().ok_or("RANK_COHORT_EXCLUSION_MISSING")?["sha256"],
            "rankPaused":self.rank_paused(exchange),"executionAuthority":false}))
    }
    pub fn check_rank_exit_review(&self,bank:&Session,log:&DispatchLog,review:&crate::rank_campaign::Reviewed)->Result<()>{
        self.check(bank,log)?;log.require_resolved_attempts()?;
        let ex=review.exchange_id();let a=review.action();let proof=review.managed_exit().ok_or("RANK_COHORT_HANDOFF_REQUIRED")?;
        let c=self.cohorts.get(ex).ok_or("RANK_COHORT_MISSING")?;let r=self.policies.get(ex).ok_or("RANK_COHORT_REVIEW_MISSING")?;
        need(a["action"]=="SELL"&&a["side"]==r.side&&a["marketId"]==r.market_id
            &&a["quantity"].as_u64().is_some_and(|n|n>0&&self.owned(bank,ex,c).is_ok_and(|held|n<=held)),"RANK_COHORT_COVERED_SELL_REQUIRED")?;
        let existing=bank.engine().order_records().into_iter().find(|q|q["reviewedRankCampaign"]["campaignSha256"]==review.campaign_sha256());
        let registered=existing.as_ref().and_then(|q|q["id"].as_u64()).and_then(|q|self.rank_exits.get(&q));
        let cohort_sha=if let Some(v)=registered{v["managedExit"]["cohortSha256"].clone()}else{json!(digest(c)?)};
        need(proof["schemaVersion"]==1&&proof["exchangeId"]==ex&&proof["cohortSha256"]==cohort_sha
            &&proof["reviewSha256"]==digest(r)?&&proof["generationSha256"]==self.generation_sha()
            &&proof["originSha256"]==c.origin_sha()?
            &&proof["exclusionSha256"]==bank.engine().ordinary_entry_exclusion_record().ok_or("RANK_COHORT_EXCLUSION_MISSING")?["sha256"],"RANK_COHORT_HANDOFF_CHANGED")?;
        Ok(())
    }
    pub(super) fn apply_rank_exit(&mut self,event:&Value)->Result<()>{
        let ex=event["exchangeId"].as_str().ok_or("RANK_COHORT_EXCHANGE")?;
        let b:Binding=serde_json::from_value(event["binding"].clone())?;
        let generation=self.generation_sha().to_owned();let c=self.cohorts.get_mut(ex).ok_or("RANK_COHORT_MISSING")?;
        need(event["managedExit"]["schemaVersion"]==1&&event["managedExit"]["exchangeId"]==ex
            &&event["managedExit"]["cohortSha256"]==digest(c)?
            &&event["managedExit"]["originSha256"]==c.origin_sha()?
            &&event["managedExit"]["generationSha256"]==generation
            &&event["managedExit"]["reviewSha256"]==b.review_sha256&&b.review_sha256==digest(&self.policies[ex])?
            &&event["ownershipOnly"]==true&&event["positiveMonetaryAlphaClaimed"]==false
            &&b.quote["action"]=="sell"&&b.quote["exchangeId"]==ex
            &&b.quote["side"]==self.policies[ex].side&&b.quote["reviewedRankCampaign"]["campaignSha256"]==event["campaignSha256"]
            &&c.exits.len()<128&&!c.exits.contains_key(&b.qid)&&!self.rank_exits.contains_key(&b.qid),"RANK_COHORT_REGISTRATION_CHANGED")?;
        c.exits.insert(b.qid,b.clone());self.rank_exits.insert(b.qid,event.clone());Ok(())
    }
    /// Register before durable dispatch, after current original account/contract
    /// facts and rank evaluation. Reopening replays this same row and binding.
    pub fn bind_rank_exit(&mut self,bank:&Session,log:&DispatchLog,scope:&Scope,review:&crate::rank_campaign::Reviewed,
        qid:u64,spec:&RequestSpec,permit:&Permit,at:u64,until:u64)->Result<RankExitOwnership>{
        self.check_rank_exit_review(bank,log,review)?;
        need(log.never_attempted(qid)&&!log.intents.contains_key(&qid)&&at<until,"RANK_COHORT_ALREADY_ATTEMPTED")?;
        let q=crate::order_snapshot::quote(bank,qid).ok_or("RANK_COHORT_QUOTE")?;
        let ex=review.exchange_id();let r=&self.policies[ex];
        need(q["reviewedRankCampaign"]["campaignSha256"]==review.campaign_sha256()&&q["retired"]==false&&q["filled"]==0
            &&q["action"]=="sell"&&q["side"]==r.side&&q["quantity"]==review.action()["quantity"]
            &&scope.tournament_id==r.tournament_id&&scope.exchanges.get(ex)==Some(&r.market_id),"RANK_COHORT_QUOTE_CHANGED")?;
        request_matches(spec,&q,&self.run,&scope.tournament_id,q["quantity"].as_u64().ok_or("RANK_COHORT_QUANTITY")?)?;
        if let Some(previous)=self.rank_exits.get(&qid){
            let b:Binding=serde_json::from_value(previous["binding"].clone())?;
            need(b.quote==q&&b.spec_sha256==digest(spec)?&&previous["managedExit"]==*review.managed_exit().unwrap(),"RANK_COHORT_ORIGINAL_BINDING_CHANGED")?;
        }else{
            let b=Binding{qid,spec:spec.clone(),spec_sha256:digest(spec)?,quote:q.clone(),review_sha256:digest(r)?,bound_at_ms:at};
            self.append(json!({"kind":"rank_exit","exchangeId":ex,"campaignSha256":review.campaign_sha256(),
                "managedExit":review.managed_exit(),"binding":b,"ownershipOnly":true,"positiveMonetaryAlphaClaimed":false}),at)?;
        }
        self.check_rank_bound(bank,log,scope,qid,spec)?;
        Ok(RankExitOwnership{qid,quote:q,scope_sha:digest(scope)?,spec_sha:digest(spec)?,permit_sha:digest(permit)?,
            ledger_sha:digest(&bank.engine().report()?)?,head:bank.durable_head(),tag:self.durable_tag(qid).ok_or("RANK_COHORT_TAG")?,minted:at,until,
            sidecar:FilePin::from_held(self.file.try_clone()?,self.path.clone(),MAX_LOG,None)?,marker:self.marker.duplicate()?,
            ordinary:log_file(&self.ordinary_path)?,ordinary_path:self.ordinary_path.clone(),ordinary_identity:self.ordinary_identity.clone(),_owner:self._owner.clone()})
    }
    pub fn check_rank_bound(&self,bank:&Session,log:&DispatchLog,scope:&Scope,qid:u64,spec:&RequestSpec)->Result<()>{
        self.check(bank,log)?;let event=self.rank_exits.get(&qid).ok_or("RANK_COHORT_BINDING_REQUIRED")?;
        let b:Binding=serde_json::from_value(event["binding"].clone())?;
        need(b.spec_sha256==digest(spec)?&&scope.exchanges.get(event["exchangeId"].as_str().ok_or("RANK_COHORT_EXCHANGE")?)==b.quote["marketId"].as_str().map(String::from).as_ref()
            &&crate::order_snapshot::quote(bank,qid).is_some_and(|q|q==b.quote),"RANK_COHORT_BOUND_STATE_CHANGED")
    }
}
fn log_file(path:&Path)->Result<File>{Ok(File::open(path)?)}
