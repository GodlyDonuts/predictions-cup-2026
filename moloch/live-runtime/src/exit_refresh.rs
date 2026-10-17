//! Read-only expired-exit refresh planning. Never execution authority or renewal.
use baal_transport_service::{Receipt, Scope, decimal_units};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use crate::feedback::Result;

fn need(ok: bool, code: &'static str) -> Result<()> { if ok {Ok(())} else {Err(code.into())} }
fn hash(v: &Value) -> Result<String> {Ok(format!("{:x}",Sha256::digest(serde_json::to_vec(v)?)))}
fn clock(s: &str) -> Result<u64> {
    Ok(u64::try_from(chrono::DateTime::parse_from_rfc3339(s)?.timestamp_millis())?)
}
fn number(q: &Value, key: &str) -> Result<u64> {q[key].as_u64().ok_or("EXIT_REFRESH_QUOTE_CLOCK_OR_SIZE".into())}
fn canceled(q: &Value) -> bool {q.get("cancelEffectiveAt").is_some_and(|v|!v.is_null())}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadKind { MaximumAge, Profit }
impl ReadKind {
    pub fn from_quote(q:&Value)->Option<Self>{
        if q["action"]!="sell"||q["executionStyle"]!="inventoryAON"{return None}
        if q["inventoryExitReason"]=="maximumAge"{Some(Self::MaximumAge)}
        else if q.get("inventoryExitReason").is_none_or(Value::is_null){Some(Self::Profit)}else{None}
    }
}
fn identity(q: &Value) -> Result<ReadKind> {
    need(ReadKind::from_quote(q).is_some()
        && matches!(q["side"].as_str(),Some("YES"|"NO")) && q["filled"].as_u64()==Some(0)
        && q["retired"].is_boolean() && !canceled(q), "EXIT_REFRESH_NOT_INVENTORY_AON_SELL")?;
    need(number(q,"id")?>0 && number(q,"quantity")?>0, "EXIT_REFRESH_QUOTE_ID_OR_SIZE")?;
    let price=decimal_units(q["price"].as_str().ok_or("EXIT_REFRESH_PRICE")?)?;
    need(price>0 && price<1_000_000,"EXIT_REFRESH_PRICE")?;
    let (created,activation,expiry)=(number(q,"createdAt")?,number(q,"activationAt")?,number(q,"expiresAt")?);
    need(created<=activation && activation<=created.saturating_add(3000) && activation<expiry,"EXIT_REFRESH_NATIVE_WINDOW")?;
    Ok(ReadKind::from_quote(q).unwrap())
}

/// Necessary profit check for an actual current native quote, never a permit.
/// Matches native profitable_age/depth rules: worst remaining lot and rounded
/// one-share fees on both acquisition basis and exit. No age-loss substitution.
pub fn current_profit_qualified(bank:&moloch_replay::session::Session,q:&Value)->Result<bool>{
    if identity(q)?!=ReadKind::Profit{return Ok(false)}
    let ex=q["exchangeId"].as_str().ok_or("EXIT_REFRESH_SCOPE")?;
    let side=q["side"].as_str().ok_or("EXIT_REFRESH_SCOPE")?;
    let positions=bank.engine().wallet.positions();
    let lots=positions.get(ex).and_then(|p|p.get(side)).and_then(Value::as_array).ok_or("EXIT_REFRESH_PROFIT_LOTS")?;
    let mut worst=0i128;let mut held=0u64;
    for lot in lots{
        let n=lot["quantity"].as_u64().filter(|n|*n>0).ok_or("EXIT_REFRESH_PROFIT_LOTS")?;
        held=held.checked_add(n).ok_or("EXIT_REFRESH_PROFIT_RANGE")?;
        worst=worst.max(decimal_units(lot["price"].as_str().ok_or("EXIT_REFRESH_PROFIT_LOTS")?)?);
    }
    if held<number(q,"quantity")?{return Ok(false)}
    let policy=bank.engine().policy();let limit=decimal_units(q["price"].as_str().ok_or("EXIT_REFRESH_PRICE")?)?;
    let bps=u32::try_from(policy["feeBps"].as_u64().ok_or("EXIT_REFRESH_PROFIT_FEES")?)?;
    let per=decimal_units(policy["feePerShare"].as_str().ok_or("EXIT_REFRESH_PROFIT_FEES")?)?;
    let minimum=policy.get("inventoryMinimumGain").map(|v|->Result<i128>{Ok(decimal_units(v.as_str().ok_or("EXIT_REFRESH_PROFIT_MINIMUM")?)?)}).transpose()?.unwrap_or(0);
    Ok(limit-worst-moloch_replay::fee(worst,1,bps,per)?-moloch_replay::fee(limit,1,bps,per)? >= minimum)
}

/// Supply CURRENT authoritative journal/queue/control observations on EVERY gate.
/// reserved_sell_quantity includes other native/physical same-side SELL liability,
/// excluding this never-attempted candidate. Unknown reservations deny the flow.
#[derive(Clone, Copy)]
pub struct Safety {
    pub never_attempted: bool, pub has_intent: bool, pub unresolved_attempts: bool,
    pub global_unknown: bool, pub stop: bool, pub stream_healthy: bool,
    pub owned_quantity: u64, pub reserved_sell_quantity: u64,
    pub same_exchange_pending_physical_order: bool,
    /// Hash of bank identity/policy, owned economic lifecycle and dispatch head.
    /// Exclude mere clock ticks, include every ownership/liability transition.
    pub lifecycle_fingerprint: [u8;32],
    /// Exact committed native head after accepted book; any later native event
    /// invalidates this short-lived plan, including otherwise harmless ticks.
    pub native_head_fingerprint: [u8;32],
}
impl Safety {
    fn check(self) -> Result<u64> {
        need(self.never_attempted && !self.has_intent && !self.unresolved_attempts && !self.global_unknown
            && !self.stop && self.stream_healthy && !self.same_exchange_pending_physical_order
            && self.lifecycle_fingerprint.iter().any(|b|*b!=0) && self.native_head_fingerprint.iter().any(|b|*b!=0),"EXIT_REFRESH_SAFETY")?;
        let available=self.owned_quantity.checked_sub(self.reserved_sell_quantity).ok_or("EXIT_REFRESH_RESERVED_COVERAGE")?;
        need(available>0,"EXIT_REFRESH_NO_AVAILABLE_HOLDING")?;Ok(available)
    }
}
#[derive(Clone)]
pub struct ReadHint { old_qid: u64, market: String, exchange: String, side: String, kind: ReadKind }
impl ReadHint {
    /// Retired is permitted ONLY for this expired, never-attempted read hint.
    /// An existing intent, even with no recorded attempt, is not refreshable.
    pub fn from_expired(q: &Value, safety: Safety, now_ms: u64) -> Result<Self> {
        Self::from_expired_kind(q,safety,now_ms,ReadKind::MaximumAge)
    }
    pub fn from_expired_kind(q:&Value,safety:Safety,now_ms:u64,kind:ReadKind)->Result<Self>{
        safety.check()?;need(identity(q)?==kind,"EXIT_REFRESH_HINT_KIND")?;
        need(number(q,"expiresAt")?<=now_ms,"EXIT_REFRESH_HINT_NOT_EXPIRED")?;
        let market=q["marketId"].as_str().filter(|s|!s.is_empty()).ok_or("EXIT_REFRESH_SCOPE")?.to_owned();
        let exchange=q["exchangeId"].as_str().filter(|s|!s.is_empty()).ok_or("EXIT_REFRESH_SCOPE")?.to_owned();
        Ok(Self{old_qid:number(q,"id")?,market,exchange,side:q["side"].as_str().unwrap().into(),kind})
    }
    /// A live unused profit quote nominates READS only. This does not retire it,
    /// renew its window or authorize its replacement; the caller must exclude
    /// reviewed/protected scopes and independently prove every dispatch fact.
    pub fn from_live_profit(q:&Value,safety:Safety,now_ms:u64)->Result<Self>{
        safety.check()?;need(identity(q)?==ReadKind::Profit,"EXIT_REFRESH_HINT_KIND")?;
        need(q["retired"]==false && number(q,"createdAt")?<=now_ms && now_ms<number(q,"expiresAt")?,"EXIT_REFRESH_HINT_NOT_LIVE")?;
        let market=q["marketId"].as_str().filter(|s|!s.is_empty()).ok_or("EXIT_REFRESH_SCOPE")?.to_owned();
        let exchange=q["exchangeId"].as_str().filter(|s|!s.is_empty()).ok_or("EXIT_REFRESH_SCOPE")?.to_owned();
        Ok(Self{old_qid:number(q,"id")?,market,exchange,side:q["side"].as_str().unwrap().into(),kind:ReadKind::Profit})
    }
    pub fn kind(&self)->ReadKind{self.kind}
    pub fn old_qid(&self) -> u64 {self.old_qid}
    pub fn side(&self) -> &str {&self.side}
    pub fn market(&self) -> &str {&self.market}
    pub fn exchange(&self) -> &str {&self.exchange}
}

/// Construct ONLY from the actual normalized event successfully durably applied
/// to this Session, not a separately normalized receipt. The bool/head are trusted
/// caller evidence; this helper cannot inspect another component's private state.
pub struct BookAcceptance { started_ms:u64, completed_ms:u64, native_head_fingerprint:[u8;32], facts_hash:String }
impl BookAcceptance {
    /// Recovery may commit an invalid book followed by its valid replacement.
    /// Check the complete committed emission, including any final invalidation.
    pub fn from_events(facts:&RefreshFacts, events:&[Value], source_committed:bool, native_head_fingerprint:[u8;32], started_ms:u64, completed_ms:u64)->Result<Self>{
        let mut candidate=None;let mut final_valid=false;
        for event in events{
            if event["type"]=="gap"&&event["exchangeId"]==facts.hint.exchange{final_valid=false;}
            if event["type"]!="book"||event["book"]["exchangeId"]!=facts.hint.exchange||event["book"]["marketId"]!=facts.hint.market{continue}
            final_valid=event["book"]["invalid"]!=true;
            if final_valid{need(candidate.is_none(),"EXIT_REFRESH_AMBIGUOUS_ADMITTED_BOOK")?;candidate=Some(event);}
        }
        need(final_valid,"EXIT_REFRESH_FINAL_BOOK_INVALID")?;
        Self::new(facts,candidate.ok_or("EXIT_REFRESH_NO_VALID_BOOK")?,source_committed,native_head_fingerprint,started_ms,completed_ms)
    }
    pub fn new(facts:&RefreshFacts, admitted_event:&Value, source_committed:bool, native_head_fingerprint:[u8;32], started_ms:u64, completed_ms:u64)->Result<Self> {
        need(source_committed && native_head_fingerprint.iter().any(|b|*b!=0) && started_ms<=completed_ms
            && clock(&facts.book().received_at)?<=started_ms,"EXIT_REFRESH_BOOK_NOT_COMMITTED")?;
        let r=facts.book();
        let wrapped=json!({"source":"official_sig_api_v1","kind":"book","tournamentId":facts.scope.tournament_id,"marketId":facts.hint.market,"exchangeId":facts.hint.exchange,"endpoint":r.endpoint,"status":r.status,"requestStartedAt":r.request_started_at,"receivedAt":r.received_at,"data":r.data});
        let expected=moloch_replay::source::receipt_events(&wrapped)?;
        let expected=expected.iter().find(|e|e["type"]=="book").ok_or("EXIT_REFRESH_NO_VALID_BOOK")?;
        need(admitted_event["type"]=="book" && admitted_event["book"]["invalid"]!=true,"EXIT_REFRESH_INVALID_BOOK")?;
        let mut actual_book=admitted_event["book"].clone();
        if let Some(object)=actual_book.as_object_mut(){object.remove("invalid");}
        need(actual_book==expected["book"],"EXIT_REFRESH_ADMITTED_BOOK_MISMATCH")?;
        facts.fresh(completed_ms)?;
        Ok(Self{started_ms,completed_ms,native_head_fingerprint,facts_hash:facts.digest.clone()})
    }
}

/// Separate post-admission planning observation, never a source-clock renewal.
/// The caller records real wall clocks around normal native planning and supplies
/// its exact held-head fingerprint, as for BookAcceptance. The actual Session
/// head and normalized current book are checked again at selection. No native
/// event, retirement, tick or physical permission is created by this object.
pub struct PostBookPlanning {
    started_ms:u64, completed_ms:u64, head:Value, book:Value,
    native_head_fingerprint:[u8;32], facts_hash:String,
    book_started_ms:u64, book_completed_ms:u64,
}
fn current_original_book(facts:&RefreshFacts,bank:&moloch_replay::session::Session)->Result<Value>{
    let r=facts.book();
    let wrapped=json!({"source":"official_sig_api_v1","kind":"book","tournamentId":facts.scope.tournament_id,"marketId":facts.hint.market,"exchangeId":facts.hint.exchange,"endpoint":r.endpoint,"status":r.status,"requestStartedAt":r.request_started_at,"receivedAt":r.received_at,"data":r.data});
    let events=moloch_replay::source::receipt_events(&wrapped)?;
    let expected=&events.iter().find(|e|e["type"]=="book").ok_or("EXIT_REFRESH_NO_VALID_BOOK")?["book"];
    let mut found=None;
    for mut book in bank.engine().live_package_books(){
        if book["exchangeId"]!=facts.hint.exchange{continue}
        need(found.is_none(),"EXIT_REFRESH_AMBIGUOUS_ADMITTED_BOOK")?;
        if let Some(o)=book.as_object_mut(){o.remove("invalid");}
        need(book==*expected,"EXIT_REFRESH_CURRENT_BOOK_CHANGED")?;found=Some(book);
    }
    found.ok_or("EXIT_REFRESH_NO_CURRENT_BOOK".into())
}
fn ordinary_profit_scope(bank:&moloch_replay::session::Session,exchange:&str)->bool{
    if bank.engine().managed_inventory_planning_excludes(exchange)||bank.engine().ordinary_entry_exclusions().contains(exchange){return false}
    // Public probabilistic summaries omit non-wire protected components.
    !bank.engine().package_records().as_array().is_none_or(|rows|rows.iter().any(|p|{
        p["phase"]!="no_execution" && p["protectionReleased"]!=true &&
        (p.get("probabilistic").is_some_and(|v|!v.is_null()) ||
         p["legs"].as_array().is_some_and(|xs|xs.iter().any(|l|l["exchangeId"]==exchange)) ||
         p["portfolio"]["componentExchanges"].as_array().is_some_and(|xs|xs.iter().any(|e|e==exchange)))
    }))
}
impl PostBookPlanning {
    pub fn from_session(facts:&RefreshFacts,acceptance:&BookAcceptance,bank:&moloch_replay::session::Session,
        planning_started_ms:u64,planning_completed_ms:u64,native_head_fingerprint:[u8;32])->Result<Self>{
        need(facts.hint.kind==ReadKind::Profit && acceptance.facts_hash==facts.digest &&
            acceptance.completed_ms<=planning_started_ms && planning_started_ms<=planning_completed_ms &&
            native_head_fingerprint.iter().any(|b|*b!=0),"EXIT_REFRESH_PLANNING_BINDING")?;
        facts.fresh(planning_completed_ms)?;
        need(ordinary_profit_scope(bank,&facts.hint.exchange),"EXIT_REFRESH_PROTECTED_PROFIT_SCOPE")?;
        let mut head_bytes=serde_json::to_vec(&bank.durable_head())?;head_bytes.push(b'\n');
        let actual_head:[u8;32]=Sha256::digest(&head_bytes).into();
        need(native_head_fingerprint==actual_head,"EXIT_REFRESH_PLANNING_HEAD")?;
        let book=current_original_book(facts,bank)?;
        Ok(Self{started_ms:planning_started_ms,completed_ms:planning_completed_ms,head:bank.durable_head(),book,
            native_head_fingerprint,facts_hash:facts.digest.clone(),book_started_ms:acceptance.started_ms,book_completed_ms:acceptance.completed_ms})
    }
    pub fn planning_started_ms(&self)->u64{self.started_ms}
    pub fn planning_completed_ms(&self)->u64{self.completed_ms}
    pub fn native_head(&self)->&Value{&self.head}
    pub fn book_acceptance_started_ms(&self)->u64{self.book_started_ms}
    pub fn book_acceptance_completed_ms(&self)->u64{self.book_completed_ms}
}

/// Original complete receipts, ordered tournament,positions,openOrders,exactCash,
/// market,nodes,book. No synthetic timestamp, refreshed slot or receipt substitution.
pub struct RefreshFacts { scope: Scope, hint: ReadHint, receipts: [Receipt;7], digest: String }
impl RefreshFacts {
    pub fn new(scope: &Scope, hint: &ReadHint, receipts: [Receipt;7], now_ms: u64) -> Result<Self> {
        need(scope.exchanges.get(&hint.exchange)==Some(&hint.market),"EXIT_REFRESH_SCOPE")?;
        let endpoints=[format!("/tournaments/{}",scope.tournament_slug),format!("/tournaments/{}/portfolio/positions",scope.tournament_slug),
            format!("/orders?status=open&limit=200&tournamentId={}",scope.tournament_id),format!("/account?tournamentId={}",scope.tournament_id),
            format!("/markets/{}?tournamentId={}",hint.market,scope.tournament_id),format!("/markets/{}/nodes?tournamentId={}",hint.market,scope.tournament_id),
            format!("/markets/{}/orderbook?tournamentId={}&depth=20",hint.market,scope.tournament_id)];
        let mut previous=0;
        for (r,endpoint) in receipts.iter().zip(endpoints.iter()) {
            let start=clock(&r.request_started_at)?;let received=clock(&r.received_at)?;
            need(r.endpoint==*endpoint && r.status==200 && previous<=start && start<=received && received<=now_ms,"EXIT_REFRESH_RECEIPT_SCOPE_OR_ORDER")?;
            previous=received;
        }
        need(receipts[0].data["id"]==scope.tournament_id && receipts[0].data["slug"]==scope.tournament_slug && receipts[0].data["status"]=="active"
            && receipts[1].data["positions"].is_array() && receipts[2].data["coverage"]["complete"]==true
            && receipts[2].data["pagination"]["hasMore"]==false,"EXIT_REFRESH_ACCOUNT_COVERAGE")?;
        need(receipts[4].data["id"]==hint.market && receipts[5].data["market_id"]==hint.market,"EXIT_REFRESH_MARKET_SCOPE")?;
        for index in [4,5,6] {
            let contexts=receipts[index].data["contexts"].as_array().ok_or("EXIT_REFRESH_CONTEXT")?;
            need(contexts.len()==1 && contexts[0]["type"]=="tournament" && contexts[0]["tournament"]["id"]==scope.tournament_id,"EXIT_REFRESH_CONTEXT")?;
        }
        let rows=receipts[6].data["contexts"][0]["orderbook"]["exchanges"].as_array().ok_or("EXIT_REFRESH_BOOK_SCOPE")?;
        need(rows.iter().filter(|r|r["exchangeId"]==hint.exchange).count()==1,"EXIT_REFRESH_BOOK_SCOPE")?;
        let digest=hash(&json!({"scope":scope,"oldQid":hint.old_qid,"market":hint.market,"exchange":hint.exchange,"side":hint.side,"kind":format!("{:?}",hint.kind),"receipts":receipts}))?;
        let facts=Self{scope:scope.clone(),hint:hint.clone(),receipts,digest};facts.fresh(now_ms)?;Ok(facts)
    }
    pub fn digest(&self) -> &str {&self.digest}
    pub fn book(&self) -> &Receipt {&self.receipts[6]}
    fn fresh(&self, now_ms: u64) -> Result<()> {
        for (i,r) in self.receipts.iter().enumerate() {
            let maximum=if i<4 {20_000} else {10_000};
            for t in [clock(&r.request_started_at)?,clock(&r.received_at)?] {
                need(t<=now_ms && now_ms-t<=maximum,"EXIT_REFRESH_FACTS_STALE")?;
            }
        }
        let row=self.receipts[6].data["contexts"][0]["orderbook"]["exchanges"].as_array().unwrap().iter().find(|r|r["exchangeId"]==self.hint.exchange).unwrap();
        let at=clock(row["asOf"]["at"].as_str().ok_or("EXIT_REFRESH_BOOK_CLOCK")?)?;
        need(row["asOf"]["sequence"].is_u64() && at<=clock(&self.book().received_at)? && at<=now_ms && now_ms-at<=10_000,"EXIT_REFRESH_BOOK_STALE")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefreshStage { WaitUntil(u64), Ready }
/// A private, in-memory source binding. Do not serialize it as a reusable permit.
pub struct RefreshPlan { quote: Value, quote_hash: String, facts_hash: String, scope_hash: String, lifecycle_fingerprint: [u8;32], native_head_fingerprint:[u8;32] }
impl RefreshPlan {
    /// Candidates MUST be freshly reread native records after normal source/book
    /// admission. Capture maxqid BEFORE ordinary cleanup begins the whole flow;
    /// its successor may be created by cleanup and retained by the fresh book.
    pub fn select(
        facts: &RefreshFacts, acceptance:&BookAcceptance, candidates: &[Value], maximum_qid_before: u64,
        flow_started_ms: u64, accept_completed_ms: u64, now_ms: u64,
        mut safety: impl FnMut(u64) -> Safety,
    ) -> Result<Self> {
        facts.fresh(now_ms)?;
        need(acceptance.facts_hash==facts.digest && flow_started_ms<=acceptance.started_ms && acceptance.completed_ms==accept_completed_ms && accept_completed_ms<=now_ms,"EXIT_REFRESH_ACCEPT_CLOCK")?;
        let mut found=None;
        for q in candidates {
            if q["marketId"]!=facts.hint.market || q["exchangeId"]!=facts.hint.exchange || q["side"]!=facts.hint.side || q["action"]!="sell" {continue;}
            let qid=number(q,"id")?;
            if qid<=maximum_qid_before || qid==facts.hint.old_qid || q["retired"]!=false {continue;}
            if identity(q)?!=facts.hint.kind{continue}
            let created=number(q,"createdAt")?;
            need(created>=flow_started_ms && created<=accept_completed_ms,"EXIT_REFRESH_NOT_CREATED_IN_FLOW")?;
            let current_safety=safety(qid);
            need(number(q,"quantity")?<=current_safety.check()?,"EXIT_REFRESH_UNCOVERED")?;
            need(found.is_none(),"EXIT_REFRESH_AMBIGUOUS_CANDIDATES")?;found=Some((q.clone(),current_safety.lifecycle_fingerprint));
        }
        let (quote,lifecycle_fingerprint)=found.ok_or("EXIT_REFRESH_NO_FRESH_CANDIDATE")?;
        let plan=Self{quote_hash:hash(&quote)?,quote,facts_hash:facts.digest.clone(),scope_hash:hash(&serde_json::to_value(&facts.scope)?)?,lifecycle_fingerprint,native_head_fingerprint:acceptance.native_head_fingerprint};
        let _=plan.stage(&plan.quote,facts,&facts.scope,safety(number(&plan.quote,"id")?),now_ms)?;Ok(plan)
    }
    /// Select only a genuine current native profit successor after final original
    /// book admission and normal planning. A missing successor is not evidence of
    /// write-budget denial and gives no permission to mutate or extend a quote.
    pub fn select_after_planning(facts:&RefreshFacts,planning:&PostBookPlanning,bank:&moloch_replay::session::Session,
        maximum_qid_before:u64,now_ms:u64,mut safety:impl FnMut(u64)->Safety)->Result<Self>{
        facts.fresh(now_ms)?;
        need(planning.facts_hash==facts.digest && planning.completed_ms<=now_ms && bank.durable_head()==planning.head,
            "EXIT_REFRESH_PLANNING_CHANGED")?;
        need(current_original_book(facts,bank)?==planning.book,"EXIT_REFRESH_CURRENT_BOOK_CHANGED")?;
        need(ordinary_profit_scope(bank,&facts.hint.exchange),"EXIT_REFRESH_PROTECTED_PROFIT_SCOPE")?;
        let mut found=None;
        for q in bank.engine().order_records(){
            if q["marketId"]!=facts.hint.market || q["exchangeId"]!=facts.hint.exchange || q["side"]!=facts.hint.side || q["action"]!="sell"{continue}
            let qid=number(&q,"id")?;
            if qid<=maximum_qid_before || qid==facts.hint.old_qid || q["retired"]!=false{continue}
            if identity(&q)?!=ReadKind::Profit{continue}
            let created=number(&q,"createdAt")?;
            need(created>=planning.started_ms && created<=planning.completed_ms,"EXIT_REFRESH_NOT_CREATED_IN_FLOW")?;
            need(!bank.engine().is_package_order(qid) && !bank.engine().live_order_bindings().contains_key(&qid) && current_profit_qualified(bank,&q)?,"EXIT_REFRESH_NOT_CURRENT_PROFIT")?;
            let s=safety(qid);need(number(&q,"quantity")?<=s.check()?,"EXIT_REFRESH_UNCOVERED")?;
            need(s.native_head_fingerprint==planning.native_head_fingerprint,"EXIT_REFRESH_BINDING_CHANGED")?;
            need(found.is_none(),"EXIT_REFRESH_AMBIGUOUS_CANDIDATES")?;found=Some((q,s.lifecycle_fingerprint));
        }
        let(quote,lifecycle_fingerprint)=found.ok_or("EXIT_REFRESH_NO_FRESH_CANDIDATE")?;
        let plan=Self{quote_hash:hash(&quote)?,quote,facts_hash:facts.digest.clone(),scope_hash:hash(&serde_json::to_value(&facts.scope)?)?,lifecycle_fingerprint,native_head_fingerprint:planning.native_head_fingerprint};
        let _=plan.stage(&plan.quote,facts,&facts.scope,safety(number(&plan.quote,"id")?),now_ms)?;Ok(plan)
    }
    pub fn quote(&self) -> &Value {&self.quote}
    pub fn facts_digest(&self) -> &str {&self.facts_hash}
    /// Necessary timing/source/liability gate only. ProductionAuthority, durable
    /// intent and physical queue/network authorization remain mandatory afterward.
    pub fn stage(&self,current: &Value,facts: &RefreshFacts,scope: &Scope,safety: Safety,now_ms:u64) -> Result<RefreshStage> {
        need(hash(current)?==self.quote_hash && facts.digest==self.facts_hash && hash(&serde_json::to_value(scope)?)?==self.scope_hash
            && safety.lifecycle_fingerprint==self.lifecycle_fingerprint && safety.native_head_fingerprint==self.native_head_fingerprint,"EXIT_REFRESH_BINDING_CHANGED")?;
        facts.fresh(now_ms)?;need(identity(current)?==facts.hint.kind,"EXIT_REFRESH_KIND_CHANGED")?;
        need(current["retired"]==false && number(current,"quantity")?<=safety.check()?,"EXIT_REFRESH_UNCOVERED_OR_RETIRED")?;
        let activation=number(current,"activationAt")?;let expiry=number(current,"expiresAt")?;
        need(expiry.saturating_sub(now_ms)>3000,"EXIT_REFRESH_EXPIRY_HEADROOM")?;
        if activation>now_ms {
            need(activation-now_ms<=3000,"EXIT_REFRESH_ACTIVATION_WAIT")?;Ok(RefreshStage::WaitUntil(activation))
        } else {Ok(RefreshStage::Ready)}
    }
}

#[cfg(test)]
mod tests {
    use super::*;use std::collections::BTreeMap;
    fn iso(t:u64)->String {chrono::DateTime::from_timestamp_millis(t as i64).unwrap().to_rfc3339_opts(chrono::SecondsFormat::Millis,true)}
    fn safe()->Safety {Safety{never_attempted:true,has_intent:false,unresolved_attempts:false,global_unknown:false,stop:false,stream_healthy:true,owned_quantity:300,reserved_sell_quantity:0,same_exchange_pending_physical_order:false,lifecycle_fingerprint:[1;32],native_head_fingerprint:[3;32]}}
    fn q(id:u64,created:u64)->Value {json!({"id":id,"marketId":"525","exchangeId":"1012","side":"NO","action":"sell","executionStyle":"inventoryAON","inventoryExitReason":"maximumAge","filled":0,"quantity":60,"price":"0.685000","retired":false,"createdAt":created,"activationAt":created+3000,"expiresAt":created+13000})}
    fn fixture()->(Scope,ReadHint,RefreshFacts,Value) {
        let scope=Scope{tournament_id:"fixture-tournament".into(),tournament_slug:"fixture".into(),exchanges:BTreeMap::from([("1012".into(),"525".into())])};
        let hint=ReadHint::from_expired(&q(1,0),safe(),20_000).unwrap();
        let endpoints=["/tournaments/fixture","/tournaments/fixture/portfolio/positions","/orders?status=open&limit=200&tournamentId=fixture-tournament","/account?tournamentId=fixture-tournament","/markets/525?tournamentId=fixture-tournament","/markets/525/nodes?tournamentId=fixture-tournament","/markets/525/orderbook?tournamentId=fixture-tournament&depth=20"];
        let contexts=json!([{"type":"tournament","tournament":{"id":"fixture-tournament"}}]);
        let data=[json!({"id":"fixture-tournament","slug":"fixture","status":"active"}),json!({"positions":[]}),json!({"coverage":{"complete":true},"pagination":{"hasMore":false}}),json!({"grossCash":"100.000000"}),json!({"id":"525","contexts":contexts}),json!({"market_id":"525","contexts":contexts}),json!({"contexts":[{"type":"tournament","tournament":{"id":"fixture-tournament"},"orderbook":{"exchanges":[{"exchangeId":"1012","asOf":{"sequence":7,"at":iso(26_000)},"bids":[{"price":0.31,"quantity":300}],"asks":[{"price":0.315,"quantity":300}]}]}}]})];
        let receipts=std::array::from_fn(|i|Receipt{endpoint:endpoints[i].into(),request_started_at:iso(20_000+i as u64*1000),received_at:iso(20_500+i as u64*1000),status:200,data:data[i].clone()});
        let facts=RefreshFacts::new(&scope,&hint,receipts,27_000).unwrap();(scope,hint,facts,q(2,27_000))
    }
    fn admitted(f:&RefreshFacts)->Value {let r=f.book();moloch_replay::source::receipt_events(&json!({"source":"official_sig_api_v1","kind":"book","tournamentId":f.scope.tournament_id,"marketId":f.hint.market,"exchangeId":f.hint.exchange,"endpoint":r.endpoint,"status":r.status,"requestStartedAt":r.request_started_at,"receivedAt":r.received_at,"data":r.data})).unwrap().remove(0)}
    fn acceptance(f:&RefreshFacts)->BookAcceptance {BookAcceptance::new(f,&admitted(f),true,[3;32],26_900,27_100).unwrap()}
    fn plan(facts:&RefreshFacts,q:&Value)->Result<RefreshPlan>{RefreshPlan::select(facts,&acceptance(facts),&[q.clone()],1,26_900,27_100,27_100,|_|safe())}
    #[test]fn profit_hint_is_explicit_and_cannot_select_an_age_loss_successor(){
        let(s,_,mut f,mut fresh)=fixture();let mut old=q(1,0);old.as_object_mut().unwrap().remove("inventoryExitReason");
        assert!(ReadHint::from_expired(&old,safe(),20_000).is_err());
        let hint=ReadHint::from_expired_kind(&old,safe(),20_000,ReadKind::Profit).unwrap();
        f=RefreshFacts::new(&s,&hint,f.receipts.clone(),27_000).unwrap();
        assert!(plan(&f,&fresh).is_err(),"maximum-age successor cannot satisfy a profit hint");
        fresh.as_object_mut().unwrap().remove("inventoryExitReason");let p=plan(&f,&fresh).unwrap();
        let original=fresh.clone();fresh["inventoryExitReason"]=json!("maximumAge");assert!(p.stage(&fresh,&f,&s,safe(),30_000).is_err());assert_eq!(p.quote(),&original);
    }
    #[test]fn new_quote_waits_original_activation_and_then_ready(){let(s,_,f,q)=fixture();let p=plan(&f,&q).unwrap();assert_eq!(p.stage(&q,&f,&s,safe(),27_100).unwrap(),RefreshStage::WaitUntil(30_000));assert_eq!(p.stage(&q,&f,&s,safe(),30_000).unwrap(),RefreshStage::Ready);assert_eq!(p.quote()["expiresAt"],40_000);}
    #[test]fn old_qid_never_becomes_new_intent(){let(_,_,f,_)=fixture();assert!(plan(&f,&q(1,27_000)).is_err());assert!(RefreshPlan::select(&f,&acceptance(&f),&[q(2,27_000)],2,26_900,27_100,27_100,|_|safe()).is_err());}
    #[test]fn changed_price_and_size_are_bound_to_new_quote_only(){let(s,_,f,mut q)=fixture();q["price"]=json!("0.650000");q["quantity"]=json!(169);let p=plan(&f,&q).unwrap();assert_eq!(p.quote()["quantity"],169);assert_eq!(p.quote()["price"],"0.650000");q["price"]=json!("0.655000");assert!(p.stage(&q,&f,&s,safe(),30_000).is_err());}
    #[test]fn reduced_holdings_or_other_sell_reservations_deny(){let(s,_,f,q)=fixture();let p=plan(&f,&q).unwrap();for safety in [Safety{owned_quantity:59,..safe()},Safety{reserved_sell_quantity:241,..safe()},Safety{reserved_sell_quantity:301,..safe()}]{assert!(p.stage(&q,&f,&s,safety,30_000).is_err());}}
    #[test]fn attempted_intent_unknown_stop_stream_and_unresolved_deny(){let(s,_,f,q)=fixture();let p=plan(&f,&q).unwrap();for safety in [Safety{never_attempted:false,..safe()},Safety{has_intent:true,..safe()},Safety{global_unknown:true,..safe()},Safety{stop:true,..safe()},Safety{stream_healthy:false,..safe()},Safety{unresolved_attempts:true,..safe()}]{assert!(p.stage(&q,&f,&s,safety,30_000).is_err());assert!(ReadHint::from_expired(&q,safety,50_000).is_err());}}
    #[test]fn canceled_retired_or_filled_new_candidate_deny(){for field in ["cancelEffectiveAt","retired","filled"]{let(_,_,f,mut q)=fixture();q[field]=match field{"retired"=>json!(true),"filled"=>json!(1),_=>json!(30_001)};assert!(plan(&f,&q).is_err());}}
    #[test]fn hint_is_expired_read_only_and_retired_allowed(){let(_,_,_,mut old)=fixture();assert!(ReadHint::from_expired(&old,safe(),27_100).is_err());old["retired"]=json!(true);assert!(ReadHint::from_expired(&old,safe(),50_000).is_ok());old["cancelEffectiveAt"]=json!(49_000);assert!(ReadHint::from_expired(&old,safe(),50_000).is_err());}
    #[test]fn fresh_candidate_must_match_scope_side_and_acceptance_interval(){for field in ["marketId","exchangeId","side","action","createdAt"]{let(_,_,f,mut q)=fixture();q[field]=match field{"side"=>json!("YES"),"action"=>json!("buy"),"createdAt"=>json!(26_000),_=>json!("other")};assert!(plan(&f,&q).is_err());}}
    #[test]fn two_new_candidates_are_ambiguous(){let(_,_,f,q)=fixture();let mut other=q.clone();other["id"]=json!(3);assert!(RefreshPlan::select(&f,&acceptance(&f),&[q,other],1,26_900,27_100,27_100,|_|safe()).is_err());}
    #[test]fn stale_book_after_wait_and_future_clock_deny(){let(s,_,f,q)=fixture();let p=plan(&f,&q).unwrap();assert!(p.stage(&q,&f,&s,safe(),36_001).is_err());assert!(p.stage(&q,&f,&s,safe(),25_000).is_err());}
    #[test]fn account_age_rechecked_after_wait_even_if_book_fresh(){let(s,h,f,q)=fixture();let mut receipts=f.receipts.clone();receipts[0].request_started_at=iso(9_500);receipts[0].received_at=iso(9_600);let f=RefreshFacts::new(&s,&h,receipts,27_000).unwrap();let p=plan(&f,&q).unwrap();assert!(p.stage(&q,&f,&s,safe(),30_000).is_err());}
    #[test]fn new_scope_or_replaced_facts_deny_existing_plan(){let(mut s,h,f,q)=fixture();let p=plan(&f,&q).unwrap();let mut receipts=f.receipts.clone();receipts[3].data["grossCash"]=json!("101.000000");let f2=RefreshFacts::new(&s,&h,receipts,27_000).unwrap();assert!(p.stage(&q,&f2,&s,safe(),30_000).is_err());s.tournament_slug="other".into();assert!(p.stage(&q,&f,&s,safe(),30_000).is_err());}
    #[test]fn missing_coverage_wrong_endpoint_or_nonserial_reads_deny(){for mode in 0..4{let(s,h,f,_)=fixture();let mut r=f.receipts.clone();match mode{0=>r[2].data["coverage"]["complete"]=json!(false),1=>r[6].endpoint="/other".into(),2=>r[6].request_started_at=iso(24_000),_=>r[1].status=503};assert!(RefreshFacts::new(&s,&h,r,27_000).is_err());}}
    #[test]fn no_expiry_extension_or_extra_activation_delay(){let(_,_,f,mut q)=fixture();q["activationAt"]=json!(30_001);assert!(plan(&f,&q).is_err());let(s,_,f,q)=fixture();let p=plan(&f,&q).unwrap();assert!(p.stage(&q,&f,&s,safe(),37_000).is_err());}
    #[test]fn native_aon_does_not_clear_partially_filled_or_resting_physical_liability(){let(s,_,f,q)=fixture();let p=plan(&f,&q).unwrap();assert!(p.stage(&q,&f,&s,Safety{same_exchange_pending_physical_order:true,..safe()},30_000).is_err());assert!(RefreshPlan::select(&f,&acceptance(&f),&[q],1,26_900,27_100,27_100,|_|Safety{same_exchange_pending_physical_order:true,..safe()}).is_err());}
    #[test]fn ownership_policy_bank_or_dispatch_lifecycle_change_invalidates_plan(){let(s,_,f,q)=fixture();let p=plan(&f,&q).unwrap();assert!(p.stage(&q,&f,&s,Safety{lifecycle_fingerprint:[2;32],..safe()},30_000).is_err());assert!(p.stage(&q,&f,&s,Safety{lifecycle_fingerprint:[0;32],..safe()},30_000).is_err());}
    #[test]fn cleanup_created_successor_can_be_revalidated_by_actual_fresh_book(){let(_,_,f,_)=fixture();let q=q(2,26_600);assert!(RefreshPlan::select(&f,&acceptance(&f),&[q],1,26_000,27_100,27_100,|_|safe()).is_ok());}
    #[test]fn invalid_uncommitted_or_cached_different_book_cannot_certify_flow(){let(_,_,f,_)=fixture();let e=admitted(&f);assert!(BookAcceptance::new(&f,&e,false,[3;32],26_900,27_100).is_err());for field in ["invalid","sourceAt","sequence","requestStartedAt","bids"]{let mut e=e.clone();e["book"][field]=match field{"invalid"=>json!(true),"bids"=>json!([]),"sourceAt"|"requestStartedAt"=>json!(iso(25_000)),_=>json!("6")};assert!(BookAcceptance::new(&f,&e,true,[3;32],26_900,27_100).is_err());}}
    #[test]fn intervening_native_event_invalidates_original_admission_head(){let(s,_,f,q)=fixture();let p=plan(&f,&q).unwrap();assert!(p.stage(&q,&f,&s,Safety{native_head_fingerprint:[4;32],..safe()},30_000).is_err());}
    #[test]fn admission_certificate_cannot_be_reused_with_replaced_account_facts(){let(s,h,f,q)=fixture();let proof=acceptance(&f);let mut receipts=f.receipts.clone();receipts[3].data["grossCash"]=json!("101.000000");let replaced=RefreshFacts::new(&s,&h,receipts,27_000).unwrap();assert!(RefreshPlan::select(&replaced,&proof,&[q],1,26_900,27_100,27_100,|_|safe()).is_err());}
}
