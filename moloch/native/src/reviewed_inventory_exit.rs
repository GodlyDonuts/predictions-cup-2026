//! Covered inherited-inventory SELL reservation. Only actual official feedback
//! changes money/inventory. Runtime reviewed capabilities supply authorization;
//! these hashes bind the durable decision, not authenticate an outside caller.
use super::*;
impl Engine {
    pub(super) fn reserve_reviewed_inventory_exit(&mut self,e:&Value,at:i64)->Result<()> {
        if !self.live_mode||e["source"]!="moloch_live_dispatcher"{return Err("REVIEWED_EXIT_SOURCE")}
        let ex=id(&e["exchangeId"])?;let market=id(&e["marketId"])?;let tournament=string(&e["bankScope"]["tournamentId"])?;
        let side=match e["side"].as_str(){Some("YES")=>Side::Yes,Some("NO")=>Side::No,_=>return Err("REVIEWED_EXIT_SIDE")};
        for key in ["originSha256","reviewSha256","bookSha256","expectedPositionSha256","adoptionReceiptHash"]{
            if e[key].as_str().is_none_or(|s|s.len()!=64||!s.bytes().all(|b|b.is_ascii_digit()||(b'a'..=b'f').contains(&b))){return Err("REVIEWED_EXIT_PROOF")}
        }
        let inherited=self.inherited.as_ref().ok_or("REVIEWED_EXIT_NOT_INHERITED")?;
        if e["adoptionReceiptHash"]!=inherited.receipt_hash||!inherited.remaining.contains_key(&(ex.clone(),side))||self.package_protects_exchange(&ex){return Err("REVIEWED_EXIT_ORIGIN")}
        let positions=self.wallet.positions();if hash(&positions[&ex])!=e["expectedPositionSha256"]{return Err("REVIEWED_EXIT_POSITION_CHANGED")}
        let lots=self.wallet.lots.get(&(ex.clone(),side)).ok_or("REVIEWED_EXIT_HOLDINGS")?;
        if lots.is_empty()||lots.iter().any(|l|l.executed_at.is_some())||self.wallet.quantity(&ex,side.other())!=0||self.orders.iter().any(|q|q.exchange==ex&&!q.retired){return Err("REVIEWED_EXIT_UNRESOLVED_OR_MIXED")}
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
        self.writes.push(at);self.count("reviewedInventoryExitReservations");Ok(())
    }
}
