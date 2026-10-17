//! Covered inherited-inventory SELL reservation. Only actual official feedback
//! changes money/inventory. Runtime reviewed capabilities supply authorization;
//! these hashes bind the durable decision, not authenticate an outside caller.
use super::*;
impl Engine {
    fn verify_reviewed_cohort_exit_origin(&self,e:&Value,ex:&str,market:&str,side:Side)->Result<()> {
        let p=&e["cohortOrigin"];if p["version"]!=1||p["originSha256"]!=e["originSha256"]||p["positionsSha256"]!=e["expectedPositionSha256"]{return Err("REVIEWED_COHORT_ORIGIN")}
        let entries=p["entries"].as_array().filter(|a|!a.is_empty()&&a.len()<=16).ok_or("REVIEWED_COHORT_ENTRIES")?;
        let exits=p["exits"].as_array().filter(|a|a.len()<=4096).ok_or("REVIEWED_COHORT_EXITS")?;
        let mut ids=BTreeMap::new();
        for(buy,rows)in [(true,entries),(false,exits)]{let mut previous=0;for row in rows{
            let id=row["qid"].as_u64().filter(|n|*n>previous).ok_or("REVIEWED_COHORT_ORDER")?;previous=id;
            for key in ["bindingSha256","closedDispatchSha256"]{if row[key].as_str().is_none_or(|s|s.len()!=64||!s.bytes().all(|b|b.is_ascii_digit()||(b'a'..=b'f').contains(&b))){return Err("REVIEWED_COHORT_PROOF")}}
            let q=self.orders.iter().find(|q|q.id==id).ok_or("REVIEWED_COHORT_ORDER")?;
            if q.exchange!=ex||q.market!=market||q.side!=side||q.buy!=buy||!q.retired||self.is_package_order(id)||self.live_orders.get(&id).is_none_or(|actual|row["actualOrderId"]!=*actual)||row["filledQuantity"].as_u64()!=Some(q.filled)||ids.insert(id,(buy,q.filled)).is_some(){return Err("REVIEWED_COHORT_ORDER")}
        }}
        let mut lots:Vec<Value>=Vec::new();let mut counts:BTreeMap<u64,u64>=BTreeMap::new();let mut started=false;
        for f in &self.fills{if f["exchangeId"]!=ex{continue}let id=f["orderId"].as_u64().ok_or("REVIEWED_COHORT_FILL")?;
            if !started{if !ids.get(&id).is_some_and(|(buy,_)|*buy){continue}started=true;}
            let(buy,_)=ids.get(&id).ok_or("REVIEWED_COHORT_FOREIGN_FILL")?;
            let mut n=f["quantity"].as_u64().filter(|n|*n>0).ok_or("REVIEWED_COHORT_FILL")?;
            if f["counterfactual"]!=false||f["marketId"]!=market||f["side"]!=label(side)||f["action"]!=(if *buy{"buy"}else{"sell"})||self.live_orders.get(&id).is_none_or(|actual|f["actualOrderId"]!=*actual)||!f["executedAt"].is_string(){return Err("REVIEWED_COHORT_FILL")}
            let total=counts.entry(id).or_default();*total=total.checked_add(n).ok_or("REVIEWED_COHORT_RANGE")?;
            if *buy{lots.push(json!({"quantity":n,"price":f["price"],"executedAt":f["executedAt"]}));}else{while n>0{let first=lots.first_mut().ok_or("REVIEWED_COHORT_FIFO")?;let held=first["quantity"].as_u64().ok_or("REVIEWED_COHORT_FIFO")?;let take=n.min(held);n-=take;if take==held{lots.remove(0);}else{first["quantity"]=json!(held-take);}}}
        }
        for(id,(_,filled))in ids{if counts.get(&id).copied().unwrap_or(0)!=filled{return Err("REVIEWED_COHORT_FILL_TOTAL")}}
        let positions=self.wallet.positions();let held=self.wallet.quantity(ex,side);
        if !started||held==0||self.wallet.quantity(ex,side.other())!=0||positions[ex][label(side)]!=json!(lots)||hash(&positions[ex])!=e["expectedPositionSha256"]||p["heldQuantity"].as_u64()!=Some(held)||dec(&p["heldCost"])?!=self.wallet.market_cost(ex){return Err("REVIEWED_COHORT_FIFO")}
        if self.inherited.as_ref().is_some_and(|i|i.remaining.iter().any(|((x,_),lots)|x==ex&&!lots.is_empty())){return Err("REVIEWED_COHORT_INHERITED_MIX")}Ok(())
    }
    pub(super) fn reserve_reviewed_inventory_exit(&mut self,e:&Value,at:i64)->Result<()> {
        if !self.live_mode||e["source"]!="moloch_live_dispatcher"{return Err("REVIEWED_EXIT_SOURCE")}
        let ex=id(&e["exchangeId"])?;let market=id(&e["marketId"])?;let tournament=string(&e["bankScope"]["tournamentId"])?;
        let side=match e["side"].as_str(){Some("YES")=>Side::Yes,Some("NO")=>Side::No,_=>return Err("REVIEWED_EXIT_SIDE")};
        let cohort=e["type"]=="reviewed_cohort_exit_reserved_v1";
        for key in ["originSha256","reviewSha256","bookSha256","expectedPositionSha256"]{
            if e[key].as_str().is_none_or(|s|s.len()!=64||!s.bytes().all(|b|b.is_ascii_digit()||(b'a'..=b'f').contains(&b))){return Err("REVIEWED_EXIT_PROOF")}
        }
        if cohort{if !self.managed_inventory_planning_excludes(&ex)||e.get("adoptionRef").is_some()||e.get("adoptionReceiptHash").is_some(){return Err("REVIEWED_COHORT_MODE_OR_ORIGIN")};self.verify_reviewed_cohort_exit_origin(e,&ex,&market,side)?;}else{
            if e["adoptionReceiptHash"].as_str().is_none_or(|s|s.len()!=64||!s.bytes().all(|b|b.is_ascii_digit()||(b'a'..=b'f').contains(&b))){return Err("REVIEWED_EXIT_PROOF")}
            let inherited=self.inherited.as_ref().ok_or("REVIEWED_EXIT_NOT_INHERITED")?;
            if e["adoptionReceiptHash"]!=inherited.receipt_hash||!inherited.remaining.contains_key(&(ex.clone(),side)){return Err("REVIEWED_EXIT_ORIGIN")}
        }
        if self.package_protects_exchange(&ex)||(cohort&&self.package_blocks_submissions()){return Err("REVIEWED_EXIT_ORIGIN")}
        let positions=self.wallet.positions();if hash(&positions[&ex])!=e["expectedPositionSha256"]{return Err("REVIEWED_EXIT_POSITION_CHANGED")}
        let lots=self.wallet.lots.get(&(ex.clone(),side)).ok_or("REVIEWED_EXIT_HOLDINGS")?;
        if lots.is_empty()||(!cohort&&lots.iter().any(|l|l.executed_at.is_some()))||self.wallet.quantity(&ex,side.other())!=0||self.orders.iter().any(|q|q.exchange==ex&&!q.retired){return Err("REVIEWED_EXIT_UNRESOLVED_OR_MIXED")}
        let n=e["quantity"].as_u64().filter(|n|*n>0&&*n<=self.policy.quantity&&*n<=self.wallet.quantity(&ex,side)).ok_or("REVIEWED_EXIT_QUANTITY")?;
        let limit=price(dec(&e["limitPrice"])?)?;let payout=dec(&e["expectedPayout"])?;
        if !(0..=SCALE).contains(&payout){return Err("REVIEWED_EXIT_VALUE")}
        match e["reason"].as_str(){
            Some("refreshed_value_exit") if limit>=payout&&e["reviewCurrentUntilMs"].as_i64().is_some_and(|t|t>at)=>{},
            Some("reviewed_source_revocation") if e["revoked"]==true&&e["reviewCurrentUntilMs"].as_i64().is_some_and(|t|t>at)=>{},
            Some("reviewed_holding_deadline") if e["holdingDeadlineMs"].as_i64().is_some_and(|t|t<=at)=>{},
            _=>return Err("REVIEWED_EXIT_REASON"),
        }
        let b=self.books.get(&ex).ok_or("REVIEWED_EXIT_BOOK")?;
        if b.market!=market||b.tournament!=tournament||!b.fresh(&self.policy,at)||hash(&b.raw)!=e["bookSha256"]{return Err("REVIEWED_EXIT_BOOK")}
        let mut left=n;let mut marginal=None;
        for (yes,quantity) in if side==Side::Yes{&b.bids}else{&b.asks}{let px=if side==Side::Yes{*yes}else{SCALE-*yes};let take=left.min(*quantity);if take>0{left-=take;marginal=Some(px)}if left==0{break}}
        if left!=0||marginal!=Some(limit){return Err("REVIEWED_EXIT_EXECUTABLE_DEPTH")}
        let expires=ms(&e["expiresAt"])?;
        if expires<=at.checked_add(self.policy.latency).ok_or("REVIEWED_EXIT_CLOCK")?||expires-at>self.policy.lifetime{return Err("REVIEWED_EXIT_CLOCK")}
        self.writes.retain(|t|at-*t<60000);
        if self.writes.len()>=self.policy.writes||self.writes.last().is_some_and(|t|at-*t<self.policy.write_gap){return Err("REVIEWED_EXIT_WRITE_BUDGET")}
        let qid=self.prior_orders+self.orders.len() as u64+1;let (same,better)=Self::visible(b,side,false,limit);
        self.orders.push(Order{id:qid,market,exchange:ex,tournament,side,buy:false,quantity:n,filled:0,limit,created:at,activation:at+self.policy.latency,expires,cancel:None,retired:false,credit:0,barrier:None,same,better,visible_same:same,visible_better:better,touch_remainder:0,aon:false,age_reason:false,through:false,participation:100,conditional:false,touch_bps:0});
        self.reviewed_inventory_exits.insert(qid,json!({"originSha256":e["originSha256"],"reviewSha256":e["reviewSha256"],"reason":e["reason"],"bookSha256":e["bookSha256"],"adoptionReceiptHash":e["adoptionReceiptHash"]}));
        if cohort{let record=self.reviewed_inventory_exits.get_mut(&qid).ok_or("REVIEWED_COHORT_RECORD")?;record.as_object_mut().unwrap().remove("adoptionReceiptHash");record["originKind"]=json!("original_entry_cohort");record["cohortOriginSha256"]=json!(hash(&e["cohortOrigin"]));}
        self.writes.push(at);self.count("reviewedInventoryExitReservations");Ok(())
    }
}
