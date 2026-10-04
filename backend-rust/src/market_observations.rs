// Included inside publication: reuse the authoritative public book validator.
// A quote is a feature, never a source of executable quantity.
use crate::types::{Book, Level};
use std::{collections::HashMap, sync::Arc};
use serde::Deserialize;
#[derive(Clone)]
pub enum MarketEvent {
    Book(Arc<Book>), Boundary(Option<String>),
    Quotes(Vec<(String, Option<i128>)>, i64),
    Eligibility { expires: i64, excluded: HashSet<String> },
}
#[derive(Clone)]
pub struct AvailableEvent { pub at: i64, pub rank: u8, pub event: MarketEvent }
#[derive(Clone, Deserialize, serde::Serialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct SourceBinding { pub bytes: u64, pub hash: String }
#[derive(Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct MarketSource {
    pub path: String, pub bytes: u64, pub hash: Option<String>,
    #[serde(default)] pub checkpoints: Vec<SourceBinding>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MarketSources { pub books: MarketSource, pub quotes: MarketSource, pub eligibility: MarketSource }
pub struct MarketCapture { pub events: Vec<AvailableEvent>, pub meta: Value, pub scope: Value }
fn digest_ok(s:&str)->bool{s.len()==64&&s.bytes().all(|b|b.is_ascii_digit()||(b'a'..=b'f').contains(&b))}
fn lines<F>(s:&MarketSource, mut visit:F)->Result<Value,String> where F:FnMut(&[u8],usize)->Result<i64,String>{
    need(s.bytes<=SAFE as u64&&s.hash.as_ref().is_none_or(|h|digest_ok(h)),"MARKET_SOURCE_BINDING")?;
    let mut checkpoints=HashMap::new();
    for c in &s.checkpoints{need(c.bytes<=s.bytes&&digest_ok(&c.hash)&&checkpoints.insert(c.bytes,c.hash.clone()).is_none(),"MARKET_SOURCE_CHECKPOINT")?;}
    let mut hash=Sha256::new();let mut bytes=0u64;let mut records=0usize;let mut first=None;let mut through=None;let mut verified=Vec::new();
    if let Some(h)=checkpoints.remove(&0){need(h==format!("{:x}",hash.clone().finalize()),"MARKET_SOURCE_PREFIX_CHANGED")?;verified.push(json!({"bytes":0,"hash":h,"through":null,"records":0}));}
    let file=File::open(&s.path).map_err(|_|"MARKET_SOURCE_OPEN")?;
    need(file.metadata().map_err(|_|"MARKET_SOURCE_METADATA")?.len()>=s.bytes,"MARKET_SOURCE_SHORT")?;
    let mut reader=BufReader::with_capacity(1024*1024,file.take(s.bytes));let mut line=Vec::new();
    loop{line.clear();let n=(&mut reader).take((MAX_RECORD+1) as u64).read_until(b'\n',&mut line).map_err(|_|"MARKET_SOURCE_READ")?;
        if n==0{break;}need(n<=MAX_RECORD&&line.last()==Some(&b'\n'),"MARKET_SOURCE_PARTIAL")?;
        records+=1;let at=visit(&line[..n-1],records)?;
        need(through.is_none_or(|old|at>=old),"MARKET_SOURCE_CLOCK_ROLLBACK")?;first.get_or_insert(at);through=Some(at);
        bytes+=n as u64;hash.update(&line);
        if let Some(h)=checkpoints.remove(&bytes){need(h==format!("{:x}",hash.clone().finalize()),"MARKET_SOURCE_PREFIX_CHANGED")?;verified.push(json!({"bytes":bytes,"hash":h,"through":at,"records":records}));}
    }
    let sha=format!("{:x}",hash.finalize());
    need(bytes==s.bytes&&s.hash.as_ref().is_none_or(|h|*h==sha)&&checkpoints.is_empty(),"MARKET_SOURCE_HASH")?;
    Ok(json!({"bytes":bytes,"hash":sha,"records":records,"firstAt":first,"through":through,"verifiedCheckpoints":verified}))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawMarketRow<'a>{sequence:i64,previous:String,at:String,#[serde(borrow)]event:&'a serde_json::value::RawValue,#[serde(borrow)]state:&'a serde_json::value::RawValue,hash:String}
fn journal_row(line:&[u8],seq:usize,previous:&mut String)->Result<(i64,Value),String>{
    let raw=std::str::from_utf8(line).map_err(|_|"MARKET_SOURCE_UTF8")?;
    let r:RawMarketRow=serde_json::from_str(raw).map_err(|_|"MARKET_SOURCE_ROW")?;
    need(r.sequence==seq as i64&&r.previous==*previous&&r.state.get()=="null"&&digest_ok(&r.hash),"MARKET_SOURCE_CHAIN")?;
    let suffix=format!(",\"hash\":\"{}\"}}",r.hash);let body=raw.strip_suffix(&suffix).ok_or("MARKET_SOURCE_HASH_ENCODING")?;
    let mut h=Sha256::new();h.update(body.as_bytes());h.update(b"}");need(format!("{:x}",h.finalize())==r.hash,"MARKET_SOURCE_CHAIN")?;
    let at=clock(&r.at,false)?.0;*previous=r.hash;
    Ok((at,serde_json::from_str(r.event.get()).map_err(|_|"MARKET_SOURCE_EVENT")?))
}
pub fn read_market_sources(s:&MarketSources,input:&Value)->Result<MarketCapture,String>{
    let started=Instant::now();let mut publication=Publication::new(input)?;
    let members:HashMap<String,String>=publication.scope["exchanges"].as_array().unwrap().iter().map(|r|(r["exchangeId"].as_str().unwrap().to_owned(),r["marketId"].as_str().unwrap().to_owned())).collect();
    let markets:HashSet<_>=members.values().cloned().collect();let universe=text(input,"hash")?;need(digest_ok(universe),"MARKET_UNIVERSE_HASH")?;
    let mut events=Vec::new();
    let books=lines(&s.books,|line,_|{
        let r:Value=serde_json::from_slice(line).map_err(|_|"PUBLIC_BOOK_JSON_INVALID")?;publication.apply(&r)?;let at=clock(text(&r,"at")?,false)?.0;
        let event=if r["kind"]=="book"{let b=&r["payload"]["book"];
            let levels=|key:&str|->Result<Vec<Level>,String>{b[key].as_array().unwrap().iter().map(|l|Ok(Level{price:price(text(l,"price")?)?,quantity:integer(&l["quantity"])?})).collect()};
            MarketEvent::Book(Arc::new(Book{market_id:text(b,"marketId")?.into(),exchange_id:text(b,"exchangeId")?.into(),instrument:text(b,"instrument")?.into(),
                received_at:clock(text(b,"receivedAt")?,false)?.0,request_started_at:clock(text(b,"requestStartedAt")?,false)?.0,
                source_ns:i64::try_from(clock(text(b,"sourceAt")?,true)?.1).map_err(|_|"MARKET_SOURCE_NS")?,sequence:sequence(text(b,"sequence")?)? as u64,bids:levels("bids")?,asks:levels("asks")?}))
        }else{MarketEvent::Boundary(r["payload"]["marketId"].as_str().map(str::to_owned))};
        events.push(AvailableEvent{at,rank:0,event});Ok(at)
    })?;
    let mut previous=String::new();let mut last_receipt=0;
    let quotes=lines(&s.quotes,|line,seq|{
        let(at,e)=journal_row(line,seq,&mut previous)?;exact(&e,&["type","snapshot"])?;need(e["type"]=="market_price_observation","MARKET_QUOTE_TYPE")?;
        let q=&e["snapshot"];exact(q,&["tournamentId","universeHash","requestStartedAt","receivedAt","sourceTimestamp","depthAvailable","executable","quotes","missingIds"])?;
        let received=clock(text(q,"receivedAt")?,false)?.0;let request=clock(text(q,"requestStartedAt")?,false)?.0;
        need(q["tournamentId"]==publication.tournament&&q["universeHash"]==universe&&q["sourceTimestamp"].is_null()&&q["depthAvailable"]==false&&q["executable"]==false&&request<=received&&received<=at&&received>=last_receipt,"MARKET_QUOTE_SCOPE_CLOCK")?;last_receipt=received;
        let rows=q["quotes"].as_array().ok_or("MARKET_QUOTE_ROWS")?;let missing=q["missingIds"].as_array().ok_or("MARKET_QUOTE_ROWS")?;
        need(!rows.is_empty()||!missing.is_empty(),"MARKET_QUOTE_ROWS")?;need(rows.len()+missing.len()<=100,"MARKET_QUOTE_ROWS")?;
        let mut seen=HashSet::new();let mut observations=Vec::new();
        for row in rows{exact(row,&["exchangeId","marketId","option","latestPrice","bestBid","bestAsk","spread","crossed"])?;let ex=text(row,"exchangeId")?;
            need(members.get(ex).is_some_and(|m|row["marketId"]==*m)&&row["option"]=="YES"&&seen.insert(ex.to_owned()),"MARKET_QUOTE_IDENTITY")?;
            for key in ["latestPrice","bestBid","bestAsk","spread"]{need(row[key].is_null()||row[key].as_f64().is_some_and(|n|n.is_finite()&&n<=1.0&&n>=if key=="spread"{-1.0}else{0.0}),"MARKET_QUOTE_PRICE")?;}
            let bid=row["bestBid"].as_f64();let ask=row["bestAsk"].as_f64();let crossed=bid.zip(ask).is_some_and(|(b,a)|b>=a);
            need(row["crossed"]==crossed,"MARKET_QUOTE_CROSSED")?;
            let usable=bid.zip(ask).filter(|(b,a)|!crossed&&*b>=0.005&&*a<=0.995).map(|(b,_)|(b*1_000_000.0).round() as i128);
            observations.push((members[ex].clone(),usable));
        }
        for row in missing{let ex=row.as_str().ok_or("MARKET_QUOTE_MISSING")?;need(members.contains_key(ex)&&seen.insert(ex.to_owned()),"MARKET_QUOTE_MISSING")?;observations.push((members[ex].clone(),None));}
        events.push(AvailableEvent{at,rank:2,event:MarketEvent::Quotes(observations,received)});Ok(at)
    })?;
    previous.clear();let mut prior_positions=0;let mut prior_orders=0;
    let eligibility=lines(&s.eligibility,|line,seq|{
        let(at,e)=journal_row(line,seq,&mut previous)?;exact(&e,&["type","snapshot"])?;need(e["type"]=="market_account_eligibility","MARKET_ELIGIBILITY_TYPE")?;
        let q=&e["snapshot"];exact(q,&["tournamentId","universeHash","positionsAt","ordersAt","expiresAt","excludedMarketIds","complete"])?;
        let p=clock(text(q,"positionsAt")?,false)?.0;let o=clock(text(q,"ordersAt")?,false)?.0;let expires=clock(text(q,"expiresAt")?,false)?.0;
        need(q["tournamentId"]==publication.tournament&&q["universeHash"]==universe&&q["complete"]==true&&p<=o&&o<=at&&expires==p+20000&&p>=prior_positions&&o>=prior_orders,"MARKET_ELIGIBILITY_SCOPE_CLOCK")?;prior_positions=p;prior_orders=o;
        let mut excluded=HashSet::new();for m in q["excludedMarketIds"].as_array().ok_or("MARKET_ELIGIBILITY_ROWS")?{let m=m.as_str().ok_or("MARKET_ELIGIBILITY_ROWS")?;need(markets.contains(m)&&excluded.insert(m.into()),"MARKET_ELIGIBILITY_ROWS")?;}
        events.push(AvailableEvent{at,rank:1,event:MarketEvent::Eligibility{expires,excluded}});Ok(at)
    })?;
    // Stable ordering retains publication sequence. Quotes/eligibility sharing
    // a book's millisecond are unavailable for that decision.
    events.sort_by_key(|e|(e.at,e.rank));
    let first=events.first().map(|e|e.at);let through=events.last().map(|e|e.at);
    Ok(MarketCapture{meta:json!({"mode":"native_compact_market_sources_v1","engineHash":env!("BAAL_ENGINE_HASH"),"sources":{"books":books,"quotes":quotes,"eligibility":eligibility},"firstAt":first,"through":through,"events":events.len(),"verifyMs":started.elapsed().as_secs_f64()*1000.0,"networkRequests":0,"competitionOrders":0}),events,scope:input.clone()})
}
