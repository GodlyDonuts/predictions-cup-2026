//! Cooperative READ hints from genuine live native owned execution clocks.
//! No order/price/quantity authority, age guarantee or source completeness.
use crate::feedback::Result;
use baal_transport_service::Scope;
use moloch_replay::session::Session;
use serde_json::Value;
use std::collections::{BTreeMap,BTreeSet};
pub const RESAMPLE_MS:u64=30_000;
pub struct ReadHint { exchange:String,market:String,oldest:u64,due:u64,at:u64,serial:u64,head:Value,reason:&'static str,qid:Option<u64> }
impl ReadHint {
    pub fn exchange_id(&self)->&str{&self.exchange}
    pub fn market_id(&self)->&str{&self.market}
    pub fn oldest_execution_ms(&self)->u64{self.oldest}
    pub fn due_at_ms(&self)->u64{self.due}
    pub fn native_head(&self)->&Value{&self.head}
    /// For covered native exits the timing fields are original quote creation
    /// and activation, NOT an invented inherited execution/loss clock.
    pub fn reason(&self)->&'static str{self.reason}
    pub fn native_qid(&self)->Option<u64>{self.qid}
}
pub struct Scheduler {scope:Scope,completed:BTreeMap<String,u64>,cursor:Option<String>,
    last_clock:Option<u64>,alternate:bool,serial:u64,pending:Option<(u64,String)>,manifest:Option<Value>}
fn numeric(s:&str)->bool{!s.is_empty()&&!s.starts_with('0')&&s.bytes().all(|b|b.is_ascii_digit())}
fn clock(v:&Value)->Result<u64>{let n=chrono::DateTime::parse_from_rfc3339(v.as_str().ok_or("OWNED_AGE_CLOCK")?)?.timestamp_millis();u64::try_from(n).map_err(|_|"OWNED_AGE_CLOCK".into())}
impl Scheduler {
    pub fn new(scope:&Scope)->Result<Self>{
        if scope.tournament_id.is_empty()||scope.exchanges.len()>4096||scope.exchanges.iter().any(|(e,m)|!numeric(e)||!numeric(m)){
            return Err("OWNED_AGE_SCOPE".into());
        }
        Ok(Self{scope:scope.clone(),completed:BTreeMap::new(),cursor:None,last_clock:None,
            alternate:false,serial:0,pending:None,manifest:None})
    }
    fn time(&mut self,now:u64)->Result<()>{
        if self.last_clock.is_some_and(|t|now<t){return Err("OWNED_AGE_CLOCK_REGRESSION".into())}
        self.last_clock=Some(now);Ok(())
    }
    /// Called only AFTER the sole worker's actual ordinary source() succeeds.
    /// This completion bookkeeping is not independent proof of an HTTP call;
    /// all original source/tape/financial authority remains with that caller.
    pub fn record_source_read(&mut self,exchange:&str,started:u64,completed:u64)->Result<()>{
        if !self.scope.exchanges.contains_key(exchange){return Err("OWNED_AGE_READ_SCOPE".into())}
        if started>completed||self.last_clock.is_some_and(|t|completed<t)
            ||self.completed.get(exchange).is_some_and(|old|completed<*old){
            return Err("OWNED_AGE_READ_CLOCK".into());
        }
        self.time(completed)?;self.completed.insert(exchange.into(),completed);Ok(())
    }
    pub fn complete_read(&mut self,hint:&ReadHint,started:u64,completed:u64)->Result<()>{
        if self.pending.as_ref()!=Some(&(hint.serial,hint.exchange.clone()))||started<hint.at
            ||self.scope.exchanges.get(&hint.exchange)!=Some(&hint.market){
            return Err("OWNED_AGE_READ_HINT".into());
        }
        self.record_source_read(&hint.exchange,started,completed)?;self.pending=None;Ok(())
    }
    /// Call at most once per regular selected-scope iteration, after ordinary
    /// owned reconciliation and before dispatch. A nomination consumes no read
    /// completion or financial permission. Alternate iterations stay regular;
    /// eligible overdue scopes rotate, and completed reads resample after30s.
    pub fn nominate(&mut self,bank:&Session,scope:&Scope,managed:&BTreeSet<String>,current:&str,now:u64)->Result<Option<ReadHint>>{
        if scope.tournament_id!=self.scope.tournament_id||scope.exchanges!=self.scope.exchanges
            ||!self.scope.exchanges.contains_key(current)||managed.iter().any(|e|!self.scope.exchanges.contains_key(e)){
            return Err("OWNED_AGE_SCOPE_CHANGED".into());
        }
        if !bank.engine().is_live()||bank.live_account_seed().is_none(){return Err("OWNED_AGE_LIVE_BANK".into())}
        if self.manifest.as_ref().is_some_and(|m|m!=bank.manifest_hash()){
            return Err("OWNED_AGE_BANK_CHANGED".into());
        }
        self.time(now)?;self.manifest=Some(bank.manifest_hash().clone());
        let policy=bank.engine().policy();
        let age=if policy["inventoryAgeClock"]=="execution"{policy["maximumInventoryAgeMs"].as_u64().filter(|n|*n>0)}else{None};
        if self.alternate{self.alternate=false;return Ok(None)}
        let packages=bank.engine().package_records();let mut protected=BTreeSet::new();
        for p in packages.as_array().ok_or("OWNED_AGE_PACKAGES")?{
            if p["phase"]=="no_execution"||p["protectionReleased"]==true{continue}
            for leg in p["legs"].as_array().ok_or("OWNED_AGE_PACKAGE_LEGS")?{
                protected.insert(leg["exchangeId"].as_str().ok_or("OWNED_AGE_PACKAGE_SCOPE")?.to_owned());
            }
            if let Some(xs)=p["portfolio"]["componentExchanges"].as_array(){for ex in xs{
                protected.insert(ex.as_str().ok_or("OWNED_AGE_PACKAGE_SCOPE")?.to_owned());
            }}
        }
        let positions=bank.engine().wallet.positions();let mut due=BTreeMap::new();
        for(ex,sides)in positions.as_object().ok_or("OWNED_AGE_POSITIONS")?{
            if !self.scope.exchanges.contains_key(ex){return Err("OWNED_AGE_POSITION_SCOPE".into())}
            if ex==current||managed.contains(ex)||protected.contains(ex){continue}
            if self.completed.get(ex).is_some_and(|t|now.saturating_sub(*t)<RESAMPLE_MS){continue}
            let Some(age)=age else{continue};let mut oldest=None;
            for side in ["YES","NO"]{for lot in sides[side].as_array().ok_or("OWNED_AGE_LOTS")?{
                let quantity=lot["quantity"].as_u64().ok_or("OWNED_AGE_QUANTITY")?;
                if quantity==0{continue}
                // Adopted original lots have no executedAt; their historical
                // openedAt must never start a new-entry loss timer.
                if let Some(executed)=lot.get("executedAt"){
                    let at=clock(executed)?;if at>now{return Err("OWNED_AGE_FUTURE_EXECUTION".into())}
                    oldest=Some(oldest.map_or(at,|old:u64|old.min(at)));
                }
            }}
            if let Some(oldest)=oldest{
                let end=oldest.checked_add(age).ok_or("OWNED_AGE_CLOCK_RANGE")?;
                if end<=now{due.insert(ex.clone(),(oldest,end,"new_lot_maximum_age",None));}
            }
        }
        // A genuine native profitable-age exit can cover inherited inventory
        // without a post-adoption executedAt. READ its scope while this exact
        // native order is alive; never invent a replacement, price or SELL.
        for q in bank.engine().order_records(){
            if q["action"]!="sell"||q["executionStyle"]!="inventoryAON"||q["retired"]!=false||q.get("cancelEffectiveAt").is_some_and(|v|!v.is_null()){continue}
            let ex=q["exchangeId"].as_str().ok_or("OWNED_AGE_QUOTE_SCOPE")?;
            let market=self.scope.exchanges.get(ex).ok_or("OWNED_AGE_QUOTE_SCOPE")?;
            if q["marketId"]!=*market{return Err("OWNED_AGE_QUOTE_SCOPE".into())}
            if ex==current||managed.contains(ex)||protected.contains(ex)||self.completed.get(ex).is_some_and(|t|now.saturating_sub(*t)<RESAMPLE_MS){continue}
            let qid=q["id"].as_u64().ok_or("OWNED_AGE_NATIVE_QUOTE")?;
            if bank.engine().is_package_order(qid){continue}
            let created=q["createdAt"].as_u64().ok_or("OWNED_AGE_NATIVE_QUOTE_CLOCK")?;
            let activation=q["activationAt"].as_u64().ok_or("OWNED_AGE_NATIVE_QUOTE_CLOCK")?;
            let expires=q["expiresAt"].as_u64().ok_or("OWNED_AGE_NATIVE_QUOTE_CLOCK")?;
            if created>activation||activation>=expires{return Err("OWNED_AGE_NATIVE_QUOTE_CLOCK".into())}
            if activation>now||expires<=now{continue}
            let quantity=q["quantity"].as_u64().ok_or("OWNED_AGE_NATIVE_QUOTE_QUANTITY")?;
            let filled=q["filled"].as_u64().ok_or("OWNED_AGE_NATIVE_QUOTE_QUANTITY")?;
            let remaining=quantity.checked_sub(filled).ok_or("OWNED_AGE_NATIVE_QUOTE_QUANTITY")?;
            let side=match q["side"].as_str(){Some("YES")=>moloch_replay::Side::Yes,Some("NO")=>moloch_replay::Side::No,_=>return Err("OWNED_AGE_NATIVE_QUOTE_SIDE".into())};
            if remaining==0||bank.engine().wallet.quantity(ex,side)<remaining{continue}
            // Prefer this stronger existing covered-exit provenance on the
            // same scope; global round-robin/alternation remains unchanged.
            due.insert(ex.into(),(created,activation,"sourcebound_covered_exit",Some(qid)));
        }
        let key=due.keys().find(|ex|self.cursor.as_ref().is_none_or(|c|*ex>c))
            .or_else(||due.keys().next()).cloned();
        let Some(exchange)=key else{return Ok(None)};let(oldest,end,reason,qid)=due[&exchange];
        self.serial=self.serial.checked_add(1).ok_or("OWNED_AGE_SERIAL")?;
        self.cursor=Some(exchange.clone());self.pending=Some((self.serial,exchange.clone()));self.alternate=true;
        Ok(Some(ReadHint{market:self.scope.exchanges[&exchange].clone(),exchange,oldest,due:end,at:now,
            serial:self.serial,head:bank.durable_head(),reason,qid}))
    }
}
