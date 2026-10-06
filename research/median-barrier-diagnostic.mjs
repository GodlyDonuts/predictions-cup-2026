import fs from 'node:fs';
import readline from 'node:readline';
import {createHash} from 'node:crypto';
import {validatePriceObservation} from '../live-paper/market-price-publication.mjs';

const HOUR=3600000,HOLD=7200000,COOLDOWN=900000,GAP=120000;
const need=(x,c)=>{if(!x)throw Error(c);};
const tick=x=>typeof x==='number'&&Number.isFinite(x)&&Math.abs(x*200-Math.round(x*200))<1e-8?Math.round(x*200):null;
const upper=a=>[...a].sort((x,y)=>x-y)[Math.floor(a.length/2)];

// Quote-path diagnosis only: no order depth, executions, wallet or profit claim.
export function createBarrierDiagnostic(universe){
 const members=new Map(universe.exchanges.map(x=>[x.exchangeId,x]));
 const markets=new Map(),events=[],counts={candidates:0,target:0,stop:0,horizon:0,data_gap:0,missing_quote:0,unfinished:0};
 let seq=0,previous='',lastKnown=-Infinity;
 function finish(m,reason,at,bid=null){const e=m.active;counts[reason]++;events.push({...e,reason,endAt:at,observedBid:bid,quoteDifferenceTicks:bid===null?null:bid-e.ask});m.active=null;m.cooldown=at+COOLDOWN;}
 function accept(row){
  const {hash,...body}=row;need(row.sequence===seq+1&&row.previous===previous&&createHash('sha256').update(JSON.stringify(body)).digest('hex')===hash,'PRICE_CHAIN');
  need(row.state===null&&row.event?.type==='market_price_observation','PRICE_ROW');
  const s=validatePriceObservation(row.event.snapshot,universe),at=Date.parse(row.at),request=Date.parse(s.requestStartedAt);
  need(Number.isFinite(at)&&at>=lastKnown&&Date.parse(s.receivedAt)<=at,'PRICE_CLOCK');
  for(const id of s.missingIds){const m=markets.get(id);if(m?.active)finish(m,'missing_quote',at);}
  for(const q of s.quotes){
   const member=members.get(q.exchangeId);need(member?.marketId===q.marketId&&q.option==='YES','SCOPE');
   const bid=tick(q.bestBid),ask=tick(q.bestAsk);let m=markets.get(q.exchangeId);
   if(!m){m={points:[],head:0,active:null,cooldown:0};markets.set(q.exchangeId,m);}
   if(bid===null||ask===null||bid<=0||ask>=200||bid>=ask){if(m.active)finish(m,'missing_quote',at);continue;}
   if(m.active){
    const e=m.active;
    if(at-e.lastAt>GAP)finish(m,'data_gap',at);
    else if(at>=e.at+HOLD)finish(m,'horizon',at,bid);
    else if(request>=e.at+3000){if(bid<=e.ask-4)finish(m,'stop',at,bid);else if(bid>=e.ask+1)finish(m,'target',at,bid);}
    if(m.active)m.active.lastAt=at;
   }
   while(m.head<m.points.length&&m.points[m.head].at<at-HOUR)m.head++;
   if(m.head>256&&m.head*2>m.points.length){m.points=m.points.slice(m.head);m.head=0;}
   if(!m.active&&at>=m.cooldown){
    const prior=m.points.slice(m.head).filter(p=>p.at<request);prior.pop();
    if(prior.length>=20&&prior.at(-1).at-prior[0].at>=1800000){
     const median=upper(prior.map(p=>p.bid));
     if(median-ask>=1){
      const trend=split=>{const older=prior.filter(p=>p.at<at-split),recent=prior.filter(p=>p.at>=at-split);return older.length>=5&&recent.length>=5?upper(recent.map(p=>p.bid))-upper(older.map(p=>p.bid)):null;};
      counts.candidates++;m.active={marketId:q.marketId,exchangeId:q.exchangeId,at,lastAt:at,ask,bid,reference:median,trend30Ticks:trend(1800000),trend15Ticks:trend(900000),sourceSequence:row.sequence,sourceHash:hash,historyPoints:prior.length};
     }
    }
   }
   m.points.push({at,bid});
  }
  seq=row.sequence;previous=hash;lastKnown=at;
 }
 return{accept,report(){const unfinished=[...markets.values()].filter(m=>m.active).map(m=>({...m.active,reason:'unfinished'}));return{schemaVersion:1,mode:'causal_bulk_quote_barrier_diagnostic',counts:{...counts,unfinished:unfinished.length},events,unfinished,source:{sequence:seq,hash:previous,lastKnownAt:Number.isFinite(lastKnown)?new Date(lastKnown).toISOString():null},thresholds:{historyMs:HOUR,minimumHistoryPoints:20,minimumHistorySpanMs:1800000,dropNewest:true,targetTicks:1,stopTicks:4,maximumHoldMs:HOLD,cooldownMs:COOLDOWN,maximumQuoteGapMs:GAP},fills:0,orders:0,profitabilityEstablished:false,limitations:['Quoted bid paths are not executable fills or wallet returns.','Entry is the observed bulk ask, not a simulated delayed purchase.','Gaps and unfinished paths remain explicit; markets/episodes are correlated.']};}};
}

export async function diagnosePrefix({file,bytes,sha256,universe}){
 need(Number.isSafeInteger(bytes)&&bytes>0&&fs.statSync(file).size>=bytes,'PREFIX_BYTES');
 const h=createHash('sha256'),d=createBarrierDiagnostic(universe),stream=fs.createReadStream(file,{start:0,end:bytes-1});stream.on('data',chunk=>h.update(chunk));
 const lines=readline.createInterface({input:stream,crlfDelay:Infinity});let consumed=0;
 for await(const line of lines){consumed+=Buffer.byteLength(line)+1;d.accept(JSON.parse(line));}
 need(consumed===bytes&&h.digest('hex')===sha256,'PREFIX_HASH_OR_BOUNDARY');
 return d.report();
}
