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
    /// Conservative acquisition-fee-inclusive cost for the FIRST n remaining
    /// FIFO units. Original fees stay charged exactly once in financial PnL;
    /// ceil(fee/fill quantity) is only a per-unit upper bound for exit pricing.
    pub fn reviewed_fifo_cost(&self,ex:&str,side:Side,entries:&BTreeSet<u64>,exits:&BTreeSet<u64>,n:u64)->Result<Value>{
        let mut lots:Vec<Value>=Vec::new();let mut started=false;
        for f in &self.fills{if f["exchangeId"]!=ex{continue}let id=f["orderId"].as_u64().ok_or("REVIEWED_FIFO_FILL")?;
            if !started{if !entries.contains(&id){continue}started=true;}
            let mut q=f["quantity"].as_u64().filter(|q|*q>0).ok_or("REVIEWED_FIFO_FILL")?;
            if f["side"]!=label(side)||f["counterfactual"]!=false{return Err("REVIEWED_FIFO_FILL")}
            if entries.contains(&id)&&f["action"]=="buy"{let fee=dec(&f["fee"])?;if fee<0{return Err("REVIEWED_FIFO_FEE")}
                let upper=fee.checked_add(q as i128-1).ok_or("REVIEWED_FIFO_RANGE")?/(q as i128);
                let cost=dec(&f["price"])?.checked_add(upper).ok_or("REVIEWED_FIFO_RANGE")?;
                lots.push(json!({"quantity":q,"price":f["price"],"executedAt":f["executedAt"],"sourceId":f["sourceId"],"orderId":id,"entryFeePerUnitUpper":money(upper),"unitCostUpper":money(cost)}));
            }else if exits.contains(&id)&&f["action"]=="sell"{while q>0{let first=lots.first_mut().ok_or("REVIEWED_FIFO_OVERSELL")?;let held=first["quantity"].as_u64().ok_or("REVIEWED_FIFO_FILL")?;let take=q.min(held);q-=take;if take==held{lots.remove(0);}else{first["quantity"]=json!(held-take);}}}
            else{return Err("REVIEWED_FIFO_FOREIGN_FILL")}
        }
        let principal:Vec<Value>=lots.iter().map(|l|json!({"quantity":l["quantity"],"price":l["price"],"executedAt":l["executedAt"]})).collect();
        if !started||json!(principal)!=self.wallet.positions()[ex][label(side)]||n==0{return Err("REVIEWED_FIFO_POSITION")}
        let mut left=n;let mut worst=0;
        for l in &lots{let take=left.min(l["quantity"].as_u64().ok_or("REVIEWED_FIFO_FILL")?);if take>0{worst=worst.max(dec(&l["unitCostUpper"])?);left-=take;}if left==0{break}}
        if left!=0{return Err("REVIEWED_FIFO_QUANTITY")}
        Ok(json!({"lots":lots,"fifoCostSha256":hash(&json!(lots)),"maximumFifoUnitCost":money(worst)}))
    }
    pub(super) fn reserve_reviewed_inventory_exit(&mut self,e:&Value,at:i64)->Result<()> {
        if !self.live_mode||e["source"]!="moloch_live_dispatcher"{return Err("REVIEWED_EXIT_SOURCE")}
        let ex=id(&e["exchangeId"])?;let market=id(&e["marketId"])?;let tournament=string(&e["bankScope"]["tournamentId"])?;
        let side=match e["side"].as_str(){Some("YES")=>Side::Yes,Some("NO")=>Side::No,_=>return Err("REVIEWED_EXIT_SIDE")};
        let cohort=e["type"]=="reviewed_cohort_exit_reserved_v1";
        // Default keeps the historical executable-depth event and record exact.
        // The opted-in limit may rest OR execute partially; no depth is promised.
        let passive=match e.get("executionMode"){None=>false,Some(Value::String(mode)) if mode=="executableDepth"=>false,
            Some(Value::String(mode)) if mode=="passiveValueLimit"=>true,_=>return Err("REVIEWED_EXIT_EXECUTION_MODE")};
        let fifo=match e.get("pricingObjective"){None=>false,Some(v) if passive&&cohort&&v["kind"]=="fifoProfitTurnover"&&v.as_object().is_some_and(|o|o.len()==2)=>true,_=>return Err("REVIEWED_EXIT_PRICING_OBJECTIVE")};
        if passive{
            // Cohort exits continue through the original complete entry/exit
            // FIFO verifier below; this mode never turns wallet presence into
            // an inherited adoption or an authenticated cohort origin.
            if e["reason"]!=(if fifo{"reviewed_fifo_profit_turnover"}else{"refreshed_value_exit"}){return Err("REVIEWED_PASSIVE_EXIT_VALUE_ONLY")}
            // Prevent ordinary planning from creating a competing SELL/hedge
            // on later books or partial fills while this limit remains open.
            if !self.managed_inventory_planning_excludes(&ex){return Err("REVIEWED_PASSIVE_EXIT_MANAGED_PLANNING_REQUIRED")}
            for key in ["modelSha256","sourceSha256"]{if e[key].as_str().is_none_or(|s|s.len()!=64||!s.bytes().all(|b|b.is_ascii_digit()||(b'a'..=b'f').contains(&b))){return Err("REVIEWED_PASSIVE_EXIT_PROOF")}}
        }
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
        let mut net_floor=payout;
        if fifo{
            let gain=dec(&e["pricingObjective"]["minimumNetGain"])?;if gain<5000||gain>SCALE{return Err("REVIEWED_FIFO_MINIMUM_GAIN")}
            let entries=e["cohortOrigin"]["entries"].as_array().unwrap().iter().map(|r|r["qid"].as_u64().unwrap()).collect();
            let exits=e["cohortOrigin"]["exits"].as_array().unwrap().iter().map(|r|r["qid"].as_u64().unwrap()).collect();
            let cost=self.reviewed_fifo_cost(&ex,side,&entries,&exits,n)?;
            net_floor=dec(&cost["maximumFifoUnitCost"])?.checked_add(gain).ok_or("REVIEWED_FIFO_RANGE")?;
            let basis=json!({"pricingPolicy":"improveAskOneTickFifoFloor","bankHead":e["bankHead"],"positionsSha256":e["expectedPositionSha256"],"quantity":n,"bookSha256":e["bookSha256"],"fifoCostSha256":cost["fifoCostSha256"],"maximumFifoUnitCost":cost["maximumFifoUnitCost"],"minimumNetSalePrice":money(net_floor),"pricingObjective":e["pricingObjective"],"forecastExpectedPayout":e["expectedPayout"]});
            if e["pricingBasis"]!=basis||e["minimumNetSalePrice"]!=money(net_floor)||e["forecastExpectedPayout"]!=e["expectedPayout"]{return Err("REVIEWED_FIFO_BASIS")}
        }
        if passive{
            let maximum=e["maximumPassiveQuantity"].as_u64().filter(|n|*n>0&&*n<=1_000_000).ok_or("REVIEWED_PASSIVE_EXIT_SLICE")?;
            // The reviewed payout already restores the margin subtracted by the
            // runtime entry ceiling. Do NOT add the margin a second time.
            // One-share rounded fees upper-bound every possible partial fill.
            if n>maximum{return Err("REVIEWED_PASSIVE_EXIT_SLICE")}
            if limit.checked_sub(self.policy.fee(limit,1)?).ok_or("REVIEWED_PASSIVE_EXIT_RANGE")?<net_floor{return Err("REVIEWED_PASSIVE_EXIT_NET_VALUE")}
        }
        match e["reason"].as_str(){
            Some("reviewed_fifo_profit_turnover") if fifo&&e["reviewCurrentUntilMs"].as_i64().is_some_and(|t|t>at)=>{},
            Some("refreshed_value_exit") if limit>=payout&&e["reviewCurrentUntilMs"].as_i64().is_some_and(|t|t>at)=>{},
            Some("reviewed_source_revocation") if e["revoked"]==true&&e["reviewCurrentUntilMs"].as_i64().is_some_and(|t|t>at)=>{},
            Some("reviewed_holding_deadline") if e["holdingDeadlineMs"].as_i64().is_some_and(|t|t<=at)=>{},
            _=>return Err("REVIEWED_EXIT_REASON"),
        }
        let b=self.books.get(&ex).ok_or("REVIEWED_EXIT_BOOK")?;
        if b.market!=market||b.tournament!=tournament||!b.fresh(&self.policy,at)||hash(&b.raw)!=e["bookSha256"]{return Err("REVIEWED_EXIT_BOOK")}
        if fifo{
            let ask=if side==Side::Yes{b.asks.first().map(|(p,_)|*p)}else{b.bids.first().map(|(p,_)|SCALE-*p)}.ok_or("REVIEWED_FIFO_ASK")?;
            let mut floor_tick=None;for tick in 1..200{let p=tick*5000;if p-self.policy.fee(p,1)?>=net_floor{floor_tick=Some(p);break}}
            if Some(limit)!=floor_tick.map(|p|p.max((ask-5000).max(5000))){return Err("REVIEWED_FIFO_LIMIT")}
        }
        if !passive{
            let mut left=n;let mut marginal=None;
            for (yes,quantity) in if side==Side::Yes{&b.bids}else{&b.asks}{let px=if side==Side::Yes{*yes}else{SCALE-*yes};let take=left.min(*quantity);if take>0{left-=take;marginal=Some(px)}if left==0{break}}
            if left!=0||marginal!=Some(limit){return Err("REVIEWED_EXIT_EXECUTABLE_DEPTH")}
        }
        let expires=ms(&e["expiresAt"])?;
        if expires<=at.checked_add(self.policy.latency).ok_or("REVIEWED_EXIT_CLOCK")?||expires-at>self.policy.lifetime{return Err("REVIEWED_EXIT_CLOCK")}
        if passive&&e["reviewCurrentUntilMs"].as_i64().is_none_or(|until|expires>until){return Err("REVIEWED_PASSIVE_EXIT_REVIEW_EXPIRY")}
        self.writes.retain(|t|at-*t<60000);
        if self.writes.len()>=self.policy.writes||self.writes.last().is_some_and(|t|at-*t<self.policy.write_gap){return Err("REVIEWED_EXIT_WRITE_BUDGET")}
        let qid=self.prior_orders+self.orders.len() as u64+1;let (same,better)=Self::visible(b,side,false,limit);
        self.orders.push(Order{id:qid,market,exchange:ex,tournament,side,buy:false,quantity:n,filled:0,limit,created:at,activation:at+self.policy.latency,expires,cancel:None,retired:false,credit:0,barrier:None,same,better,visible_same:same,visible_better:better,touch_remainder:0,aon:false,age_reason:false,through:false,participation:100,conditional:false,touch_bps:0});
        self.reviewed_inventory_exits.insert(qid,json!({"originSha256":e["originSha256"],"reviewSha256":e["reviewSha256"],"reason":e["reason"],"bookSha256":e["bookSha256"],"adoptionReceiptHash":e["adoptionReceiptHash"]}));
        if passive{let record=self.reviewed_inventory_exits.get_mut(&qid).ok_or("REVIEWED_PASSIVE_EXIT_RECORD")?;
            for key in ["executionMode","maximumPassiveQuantity","modelSha256","sourceSha256"]{record[key]=e[key].clone();}
            if fifo{for key in ["pricingObjective","pricingBasis","minimumNetSalePrice","forecastExpectedPayout"]{record[key]=e[key].clone();}}
            else{record["minimumNetValue"]=json!(money(payout));}
        }
        if cohort{let record=self.reviewed_inventory_exits.get_mut(&qid).ok_or("REVIEWED_COHORT_RECORD")?;record.as_object_mut().unwrap().remove("adoptionReceiptHash");record["originKind"]=json!("original_entry_cohort");record["cohortOriginSha256"]=json!(hash(&e["cohortOrigin"]));}
        self.writes.push(at);self.count("reviewedInventoryExitReservations");Ok(())
    }
}
