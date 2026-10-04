// Re-read only the already validated original quote prefix, with its exact SHA.
// Keep ask/bid evidence for price diagnostics; it supplies no executable size.
#[derive(Clone)]
pub struct QuoteTick {
    pub market:String,pub at:i64,pub request:i64,pub received:i64,
    pub bid:Option<i128>,pub ask:Option<i128>,pub sequence:usize,
}
pub fn read_study_quotes(s:&MarketSource,verified_hash:&str,input:&Value)->Result<Vec<QuoteTick>,String>{
    let pinned=MarketSource{path:s.path.clone(),bytes:s.bytes,hash:Some(verified_hash.to_owned()),checkpoints:vec![]};
    let members:HashMap<_,_>=input["exchanges"].as_array().ok_or("STUDY_SCOPE")?.iter().map(|r|(r["exchangeId"].as_str().unwrap().to_owned(),r["marketId"].as_str().unwrap().to_owned())).collect();
    let mut previous=String::new();let mut ticks=Vec::new();
    lines(&pinned,|line,seq|{
        let(at,e)=journal_row(line,seq,&mut previous)?;let s=&e["snapshot"];
        let request=clock(text(s,"requestStartedAt")?,false)?.0;let received=clock(text(s,"receivedAt")?,false)?.0;
        for q in s["quotes"].as_array().ok_or("STUDY_QUOTES")?{
            let bid=q["bestBid"].as_f64();let ask=q["bestAsk"].as_f64();
            let valid=bid.zip(ask).is_some_and(|(b,a)|b>=0.005&&a<=0.995&&b<a)&&at-received<=120000;
            ticks.push(QuoteTick{market:text(q,"marketId")?.into(),at,request,received,sequence:seq,
                bid:if valid{bid.map(|x|(x*1000000.0).round()as i128)}else{None},
                ask:if valid{ask.map(|x|(x*1000000.0).round()as i128)}else{None}});
        }
        for id in s["missingIds"].as_array().ok_or("STUDY_QUOTES")?{let ex=id.as_str().ok_or("STUDY_QUOTES")?;
            ticks.push(QuoteTick{market:members.get(ex).ok_or("STUDY_QUOTES")?.clone(),at,request,received,sequence:seq,bid:None,ask:None});}
        Ok(at)
    })?;Ok(ticks)
}
