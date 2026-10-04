// Included beside unchanged quote_replay.rs. The active V1 experiment keeps its
// archived engine; this opt-in policy is explicit in every diagnostic result.
#[derive(Default)]
pub struct StudyFeatures { history: History }
impl StudyFeatures {
    pub fn observe(&mut self,market:&str,at:i64,received:i64,bid:Option<i128>){
        self.history.observe(at,received,&[(market.to_owned(),bid)]);
    }
    pub fn signal(&self,market:&str,at:i64,available_before:i64,p:&QuoteParameters,corrected:bool)->Result<(Option<i128>,&'static str),String>{
        let original=self.history.signal(market,at,available_before,p)?;
        if !corrected||p.recovery_ticks==0||original.0.is_none(){return Ok(original);}
        let current=self.history.latest.get(market).and_then(|r|r.as_ref()).ok_or("STUDY_CURRENT_MISSING")?;
        let previous=self.history.rows.get(market).and_then(|rows|rows.iter().rev().find(|r|r.minute<current.minute&&r.available<available_before));
        let Some(previous)=previous else{return Ok((None,"recoveryPriorMinuteMissing"));};
        if current.receipt-previous.receipt>120000{return Ok((None,"recoveryPriorMinuteGap"));}
        if current.bid<previous.bid{return Ok((None,"recoveryBidFalling"));}
        Ok(original)
    }
}
#[cfg(test)]
mod recovery_v2_tests{
    use super::*;
    #[test]
    fn falling_rebound_passes_self_comparison_but_version_two_rejects(){
        let mut f=StudyFeatures::default();let p=QuoteParameters{lookback_ms:3600000,minimum_minutes:30,minimum_span_ms:1800000,minimum_edge:"0.020000".into(),recovery_ticks:1,horizon_ms:1800000,take_profit:"0.010000".into()};
        for minute in 0..40{f.observe("1",minute*60000,minute*60000,Some(900000));}
        for(minute,bid)in[(40,800000),(41,840000),(42,835000)]{f.observe("1",minute*60000,minute*60000,Some(bid));}
        assert_eq!(f.signal("1",42*60000+1,42*60000+1,&p,false).unwrap().0,Some(900000));
        assert_eq!(f.signal("1",42*60000+1,42*60000+1,&p,true).unwrap().1,"recoveryBidFalling");
        f.observe("1",43*60000,43*60000,Some(840000));assert_eq!(f.signal("1",43*60000+1,43*60000+1,&p,true).unwrap().0,Some(900000));
        let mut raw=p.clone();raw.recovery_ticks=0;assert_eq!(f.signal("1",43*60000+1,43*60000+1,&raw,true).unwrap(),f.signal("1",43*60000+1,43*60000+1,&raw,false).unwrap());
    }
}
