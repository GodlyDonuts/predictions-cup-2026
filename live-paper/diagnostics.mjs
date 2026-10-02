import fs from 'node:fs';
import {fileURLToPath} from 'node:url';
import {replayJournal} from './replay.mjs';
import {initialState,normalizeBook,observe,invalidate} from './model.mjs';
import {amount,fee,TICK} from '../paper/fixed.mjs';

function distribution(values){
  const sorted=values.filter(Number.isFinite).sort((a,b)=>a-b);
  const quantile=p=>sorted.length?sorted[Math.floor((sorted.length-1)*p)]:null;
  return {count:sorted.length,min:quantile(0),median:quantile(.5),p95:quantile(.95),max:quantile(1)};
}

// No fitting, order submission or credential access. The replay first validates
// the complete captured prefix; diagnostics then follow the same causal model.
export function diagnoseJournal(raw,c){
  const replay=replayJournal(raw,c);
  let state=null;
  const markets={},skipReasons={},samples={latencyMs:[],sourceAgeMs:[]};
  const increment=(map,key)=>map[key]=(map[key]||0)+1;
  for(const line of raw.trimEnd().split('\n')){
    const r=JSON.parse(line),event=r.event,clock=event.clockAt||r.at;
    if(event.type==='account_read_outage'){if(state)invalidate(state,null,clock,event.reason);continue;}
    if(event.type!=='cycle')continue;
    const excluded=new Set(event.accountExclusions);
    for(const [index,e] of event.sourceSnapshots.entries()){
      if(e.skipped){increment(skipReasons,e.skipped);if(state)invalidate(state,e.marketId,e.at||r.state?.skips?.[e.marketId]?.at||clock,e.skipped);continue;}
      const match=/^\/markets\/(\d+)\/orderbook\?/.exec(e.endpoint||'');
      if(!match||!c.marketIds.includes(match[1]))continue;
      state??=initialState(c,'diagnostics',e.requestStartedAt);
      let b;
      try{b=normalizeBook(e.data,match[1],c.tournamentId,e.receivedAt,e.requestStartedAt,c.maxDataAgeMs,excluded);}
      catch(err){
        // The collector appends a raw successful-HTTP response followed by its
        // validation skip. That adjacent event is the authoritative skip count.
        const next=event.sourceSnapshots[index+1];
        if(!next?.skipped||String(next.marketId)!==match[1])increment(skipReasons,err.message);
        invalidate(state,match[1],e.receivedAt,err.message);continue;
      }
      const m=markets[b.marketId]??={accepted:0,duplicates:0,uniqueVersions:0,gateCounts:{},latencyMs:[],sourceAgeMs:[],samplingGapMs:[],spread:[],zScore:[],entryEdge:[],usableAskShares:[],lastAt:null};
      const prev=state.books[b.instrument],priorIntents=state.strategies.mean_reversion.intents;
      const history=(state.histories[b.instrument]||[]).slice(-c.lookback);
      state=observe(state,b,c);m.accepted++;
      const latency=Date.parse(b.receivedAt)-Date.parse(b.requestStartedAt),age=Date.parse(b.receivedAt)-Date.parse(b.sourceAt);
      m.latencyMs.push(latency);m.sourceAgeMs.push(age);samples.latencyMs.push(latency);samples.sourceAgeMs.push(age);
      if(m.lastAt)m.samplingGapMs.push(Date.parse(b.receivedAt)-Date.parse(m.lastAt));m.lastAt=b.receivedAt;
      if(prev&&prev.sequence===b.sequence&&prev.versionKey===state.books[b.instrument].versionKey){m.duplicates++;continue;}
      m.uniqueVersions++;
      const signal=state.signals[b.instrument],st=state.strategies.mean_reversion;
      m.spread.push(Number(amount(b.asks[0].price)-amount(b.bids[0].price))/1e6);
      if(signal.zScore!==null)m.zScore.push(signal.zScore);
      const ask=amount(b.asks[0].price)+TICK*BigInt(c.slippageTicks);
      const mean=history.length?history.reduce((n,x)=>n+x,0)/history.length:(Number(amount(b.bids[0].price))+Number(amount(b.asks[0].price)))/2;
      const exactEdge=mean-Number(ask)-Number(fee(ask,1,{feeBps:c.feeBps,feePerShare:'0.000000'}));
      m.entryEdge.push(exactEdge/1e6);
      const capacity=Math.floor(b.asks[0].quantity*c.depthFractionBps/10000);m.usableAskShares.push(capacity);
      const warm=signal.warmupRemaining===0,variance=signal.zScore!==null,z=variance&&signal.zScore>=c.entryZ,edge=exactEdge>=Number(amount(c.minEdge));
      if(!warm)increment(m.gateCounts,'warmup');
      else{
        increment(m.gateCounts,'postWarmup');
        if(!variance)increment(m.gateCounts,'zeroVariance');
        if(!z)increment(m.gateCounts,'zBelowThreshold');
        if(!edge)increment(m.gateCounts,'edgeBelowThreshold');
        if(z)increment(m.gateCounts,'zPass');
        if(edge)increment(m.gateCounts,'edgePass');
        if(z&&edge)increment(m.gateCounts,'jointSignalPass');
      }
      if(!capacity)increment(m.gateCounts,'zeroTopAskCapacity');
      if(st.intents>priorIntents)increment(m.gateCounts,'actualNewIntents');
    }
  }
  for(const m of Object.values(markets))for(const key of ['latencyMs','sourceAgeMs','samplingGapMs','spread','zScore','entryEdge','usableAskShares'])m[key]=distribution(m[key]);
  return {schemaVersion:1,sourceHash:replay.replay.sourceHash,configHash:replay.configHash,sourceRecords:replay.replay.sourceRecords,finalEventClock:replay.replay.finalEventClock,observations:replay.observations,markets,skipReasons,latencyMs:distribution(samples.latencyMs),sourceAgeMs:distribution(samples.sourceAgeMs),interpretation:{networkRequests:0,parametersChanged:false,gateCountsOverlap:true,edgeUnits:'SUSQies per share; lagged mean less ask, adverse tick and assumed entry fee',scope:'Exploratory coverage and entry diagnostics, not profitability evidence',limitations:['Four-market sequential REST sampling misses intracycle changes','Entry edge excludes subsequent exit costs and is an uncalibrated hypothesis','Depth capacity is current top ask only; it is not a future fill guarantee','Duplicate versions are accepted for freshness but do not extend signal history']}};
}

if(process.argv[1]&&fileURLToPath(import.meta.url)===process.argv[1]){
  const arg=name=>{const i=process.argv.indexOf(name);if(i<0||!process.argv[i+1])throw Error('Required '+name);return process.argv[i+1];};
  const result=diagnoseJournal(fs.readFileSync(arg('--journal'),'utf8'),JSON.parse(fs.readFileSync(arg('--config'),'utf8')));
  console.log(JSON.stringify(result,null,2));
}
