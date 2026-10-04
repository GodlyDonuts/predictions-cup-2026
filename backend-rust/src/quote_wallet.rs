// Companion path for the separate quote-history cohort; frozen engine.rs stays
// byte-identical. Publication availability is the intent's decision clock.
impl Wallet {
    pub fn cancel_quote_buys(&mut self,market:Option<&str>){
        let keys:Vec<_>=self.pending.iter().filter(|(id,p)|p.buy&&market.is_none_or(|m|self.books.get(*id).is_some_and(|b|b.market_id==m))).map(|(id,_)|id.clone()).collect();
        for id in keys{self.pending.remove(&id);self.expired+=1;}
    }
    pub fn step_quote_dip(&mut self,b:Arc<Book>,c:&Config,edge:Option<f64>,horizon:i64,margin:f64,take_profit:i128,available:i64,eligible:bool,max_markets:usize)->Result<bool,String>{
        if available<b.received_at||available-b.request_started_at>c.max_data_age_ms{
            self.invalidate(Some(&b.market_id),available);return Ok(false);
        }
        if !eligible{self.cancel_quote_buys(Some(&b.market_id));}
        let managed=self.positions.keys().chain(self.pending.keys()).collect::<HashSet<_>>().len();
        let allowed=eligible&&(self.positions.contains_key(&b.instrument)||self.pending.contains_key(&b.instrument)||managed<max_markets);
        let before=self.intents;
        let result=self.step_dip_policy(b.clone(),c,if allowed{edge}else{None},horizon,margin,take_profit,DipExitPolicy::ProfitFloorStopBuffer(2))?;
        if self.intents>before{
            let p=self.pending.get_mut(&b.instrument).ok_or("QUOTE_INTENT_MISSING")?;
            p.at=available;p.audit.as_mut().ok_or("QUOTE_INTENT_AUDIT")?.decision_at=available;
        }
        Ok(result)
    }
}
