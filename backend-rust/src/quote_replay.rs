use crate::{engine::{Wallet,Fill,Point},evaluation::{Scenario,economics},money::{amount,TICK},publication::{AvailableEvent,MarketEvent,MarketCapture,SourceBinding,MarketSources},types::*};
use serde::{Serialize,Deserialize};
use serde_json::{Value,json};
use std::{collections::{HashMap,HashSet,VecDeque},time::Instant};
use rayon::prelude::*;
#[derive(Clone,Serialize,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct QuoteParameters{
    pub lookback_ms:i64,pub minimum_minutes:usize,pub minimum_span_ms:i64,
    pub minimum_edge:String,pub recovery_ticks:u8,pub horizon_ms:i64,pub take_profit:String,
}
#[derive(Serialize,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct QuoteStrategy{pub id:String,pub parameters:QuoteParameters}
#[derive(Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct QuoteDeclaration{
    pub schema_version:u32,pub mode:String,pub cohort:String,pub declared_at:String,
    pub config:Config,pub universe:Value,pub development:HashMap<String,SourceBinding>,
    pub strategies:Vec<QuoteStrategy>,pub scenarios:Vec<Scenario>,pub max_managed_markets:usize,
}
#[derive(Clone)]
struct Minute{minute:i64,bid:i128,receipt:i64,available:i64}
#[derive(Default)]
struct History{rows:HashMap<String,VecDeque<Minute>>,latest:HashMap<String,Option<Minute>>}
impl History{
    fn observe(&mut self,at:i64,received:i64,rows:&[(String,Option<i128>)]){
        for(m,bid)in rows{
            let bid=bid.filter(|_|at-received<=120000);
            self.latest.insert(m.clone(),bid.map(|bid|Minute{minute:received/60000,bid,receipt:received,available:at}));
            if let Some(bid)=bid{
                let queue=self.rows.entry(m.clone()).or_default();
                let row=Minute{minute:received/60000,bid,receipt:received,available:at};
                if queue.back().is_some_and(|old|old.minute==row.minute){*queue.back_mut().unwrap()=row;}else{queue.push_back(row);}
                while queue.front().is_some_and(|old|old.minute<(received-21600000)/60000){queue.pop_front();}
            }
        }
    }
    fn signal(&self,market:&str,at:i64,request:i64,p:&QuoteParameters)->Result<(Option<i128>,&'static str),String>{
        let Some(current)=self.latest.get(market).and_then(|x|x.as_ref())else{return Ok((None,"missingQuote"));};
        if current.available>=request||at-current.receipt>120000{return Ok((None,"staleQuote"));}
        let Some(rows)=self.rows.get(market)else{return Ok((None,"warmup"));};
        // Exclude current and preceding minute, and availability after request.
        let mut values:Vec<_>=rows.iter().filter(|r|r.available<request&&r.receipt>=at-p.lookback_ms&&r.minute<=at/60000-2).collect();
        if values.len()<p.minimum_minutes||values.last().unwrap().receipt-values[0].receipt<p.minimum_span_ms{return Ok((None,"warmup"));}
        let mut recent:Vec<_>=rows.iter().filter(|r|r.available<request&&r.receipt>=at-300000).collect();
        recent.sort_by_key(|r|r.receipt);
        if p.recovery_ticks>0{
            if recent.len()<3{return Ok((None,"recoveryWarmup"));}
            let trough=recent.iter().map(|r|r.bid).min().unwrap();
            if current.bid<trough+TICK*p.recovery_ticks as i128||recent.last().is_some_and(|r|r.bid>current.bid){return Ok((None,"recoveryAbsent"));}
        }
        values.sort_by_key(|r|r.bid);let reference=values[(values.len()-1)/4].bid;
        Ok((Some(reference),"referenceReady"))
    }
}
fn identifier(s:&str)->bool{!s.is_empty()&&s.len()<=100&&s.bytes().all(|b|b.is_ascii_alphanumeric()||b==b'-'||b==b'_')}
impl QuoteDeclaration{
    pub fn validate(&self,source:&MarketCapture,sources:&MarketSources)->Result<i64,String>{
        self.config.validate()?;let fail=||Err("INVALID_QUOTE_DECLARATION".into());let cutoff=instant(&self.declared_at)?;
        if self.schema_version!=1||self.mode!="causal_quote_dip_subsequent_v1"||!identifier(&self.cohort)||self.config.fee_bps!=0||self.config.fill_clock_policy!=FillClockPolicy::IndependentRequestLatencyV2||self.max_managed_markets!=1||self.strategies.is_empty()||self.strategies.len()>64||self.scenarios.is_empty()||self.scenarios.len()>16||source.scope!=self.universe{return fail();}
        let scope:HashSet<_>=self.universe["exchanges"].as_array().ok_or("INVALID_QUOTE_UNIVERSE")?.iter().map(|r|r["marketId"].as_str().unwrap_or("").to_owned()).collect();
        if self.config.market_ids.iter().cloned().collect::<HashSet<_>>()!=scope{return fail();}
        if self.development.len()!=3{return fail();}
        for(name,s)in[("books",&sources.books),("quotes",&sources.quotes),("eligibility",&sources.eligibility)]{
            let d=self.development.get(name).ok_or("QUOTE_DEVELOPMENT_MISSING")?;
            if !s.checkpoints.iter().any(|c|c.bytes==d.bytes&&c.hash==d.hash)||!source.meta["sources"][name]["verifiedCheckpoints"].as_array().unwrap().iter().any(|v|v["bytes"]==d.bytes&&v["hash"]==d.hash&&v["through"].as_i64().is_none_or(|at|at<cutoff)){return fail();}
        }
        // No development event may have a publication clock at/after cutoff.
        // Host also binds the exact frozen prefix; initial replay cannot trade
        // before this future cutoff, even with historical warmup quotes.
        let mut ids=HashSet::new();for s in &self.strategies{let p=&s.parameters;
            if !identifier(&s.id)||!ids.insert(s.id.as_str())||![3600000,10800000,21600000].contains(&p.lookback_ms)||p.minimum_minutes<30||p.minimum_minutes>360||p.minimum_span_ms<1800000||p.minimum_span_ms>=p.lookback_ms||amount(&p.minimum_edge)?<=0||p.recovery_ticks>2||p.horizon_ms<60000||p.horizon_ms>21600000||amount(&p.take_profit)?<=0{return fail();}}
        ids.clear();for s in &self.scenarios{if !identifier(&s.id)||!ids.insert(s.id.as_str()){return fail();}let mut c=self.config.clone();c.latency_ms=s.latency_ms;c.slippage_ticks=s.slippage_ticks;c.depth_fraction_bps=s.depth_fraction_bps;c.validate()?;}
        Ok(cutoff)
    }
}
#[derive(Serialize)]
#[serde(rename_all="camelCase")]
struct Card{strategy:String,parameters:QuoteParameters,scenario:Scenario,point:Point,fills:Vec<Fill>,economics:Value,pending_read_intents:Value,signal_diagnostics:HashMap<String,u64>}
pub fn run(source:&MarketCapture,d:&QuoteDeclaration,sources:&MarketSources,workers:usize)->Result<Value,String>{
    let started=Instant::now();let cutoff=d.validate(source,sources)?;
    if workers<1||workers>32{return Err("INVALID_REPLAY_WORKERS".into());}
    let through=source.events.last().map_or(cutoff,|e|e.at).max(cutoff);
    // Build causal features once per distinct strategy, shared by scenarios.
    let mut history=History::default();let mut eligibility:Option<(i64,i64,HashSet<String>)>=None;
    let mut signals=Vec::new();let mut feature_counts=HashMap::new();
    for e in &source.events{match &e.event{
        MarketEvent::Quotes(rows,received)=>history.observe(e.at,*received,rows),
        MarketEvent::Eligibility{expires,excluded}=>eligibility=Some((e.at,*expires,excluded.clone())),
        MarketEvent::Boundary(m)=>{if m.is_none(){eligibility=None;}},
        MarketEvent::Book(b)=>{
            let eligible=eligibility.as_ref().is_some_and(|(at,expiry,excluded)|*at<b.request_started_at&&b.request_started_at<=*expiry&&!excluded.contains(&b.market_id));
            let references=d.strategies.iter().map(|s|history.signal(&b.market_id,e.at,b.request_started_at,&s.parameters)).collect::<Result<Vec<_>,_>>()?;
            if e.at>cutoff{for(_,reason)in &references{*feature_counts.entry(*reason).or_insert(0u64)+=1;}}
            signals.push((references,eligible));
        }
    }}
    let pool=rayon::ThreadPoolBuilder::new().num_threads(workers).build().map_err(|_|"REPLAY_POOL_FAILED")?;
    let specs:Vec<_>=d.strategies.iter().enumerate().flat_map(|(i,s)|d.scenarios.iter().map(move|scenario|(i,s,scenario))).collect();
    let cards=pool.install(||specs.par_iter().map(|(i,s,scenario)|->Result<Card,String>{
        let mut c=d.config.clone();c.latency_ms=scenario.latency_ms;c.slippage_ticks=scenario.slippage_ticks;c.depth_fraction_bps=scenario.depth_fraction_bps;
        let mut wallet=Wallet::new(&c,cutoff)?;let mut diagnostics=HashMap::new();let mut book_index=0usize;
        for AvailableEvent{at,event,..} in &source.events{
            match event{
                MarketEvent::Book(b)=>{let(refs,eligible)=&signals[book_index];book_index+=1;if *at<=cutoff{continue;}
                    let(reference,reason)=refs[*i];*diagnostics.entry(reason.to_owned()).or_insert(0)+=1;
                    if !eligible{*diagnostics.entry("accountIneligible".into()).or_insert(0)+=1;}
                    let edge=reference.map(|r|r-b.asks[0].price-2*TICK*c.slippage_ticks);
                    if edge.is_some_and(|n|n>=amount(&s.parameters.minimum_edge).unwrap()){*diagnostics.entry("costAdjustedEdgePass".into()).or_insert(0)+=1;}
                    wallet.step_quote_dip(b.clone(),&c,edge.map(|n|n as f64/1000000.0),s.parameters.horizon_ms,amount(&s.parameters.minimum_edge)? as f64/1000000.0,amount(&s.parameters.take_profit)?,*at,*eligible,d.max_managed_markets)?;
                },
                MarketEvent::Boundary(m)if *at>cutoff=>wallet.invalidate(m.as_deref(),*at),
                MarketEvent::Eligibility{excluded,..}if *at>cutoff=>{for m in excluded{wallet.cancel_quote_buys(Some(m));}},
                _=>{}
            }
        }
        wallet.verify()?;let point=wallet.point(&c,through)?;let economics=serde_json::to_value(economics(&wallet.fills,&point)?).map_err(|_|"QUOTE_ECONOMICS")?;
        let pending_read_intents=serde_json::to_value(wallet.pending_read_intents(&c)?).map_err(|_|"QUOTE_INTENTS")?;
        Ok(Card{strategy:s.id.clone(),parameters:s.parameters.clone(),scenario:(*scenario).clone(),point,economics,fills:wallet.fills,pending_read_intents,signal_diagnostics:diagnostics})
    }).collect::<Result<Vec<_>,_>>())?;
    Ok(json!({"schemaVersion":1,"mode":d.mode,"cohort":d.cohort,"evaluationStart":d.declared_at,"evaluationThrough":crate::engine::iso(through),"candidates":cards.len(),"scorecards":cards,
        "featureCounts":feature_counts,"replayMs":started.elapsed().as_secs_f64()*1000.0,"workers":workers,"fees":0,"networkRequests":0,"competitionOrders":0,"profitabilityEstablished":false}))
}
#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn history_is_minute_weighted_available_and_excludes_two_recent_minutes(){
        let mut h=History::default();let p=QuoteParameters{lookback_ms:3600000,minimum_minutes:30,minimum_span_ms:1800000,minimum_edge:"0.020000".into(),recovery_ticks:0,horizon_ms:1800000,take_profit:"0.010000".into()};
        for min in 0..40{let at=60_000*min;h.observe(at,at,&[("1".into(),Some(900000))]);h.observe(at+1,at+1,&[("1".into(),Some(900000))]);}
        h.observe(40*60000,40*60000,&[("1".into(),Some(100000))]);
        assert_eq!(h.rows["1"].len(),41);
        assert_eq!(h.signal("1",40*60000+1000,40*60000+500,&p).unwrap().0,Some(900000));
        assert_eq!(h.signal("1",40*60000,40*60000,&p).unwrap().1,"staleQuote");
        h.observe(41*60000,41*60000,&[("1".into(),None)]);assert_eq!(h.signal("1",41*60000+1000,41*60000+500,&p).unwrap().1,"missingQuote");
    }
}
