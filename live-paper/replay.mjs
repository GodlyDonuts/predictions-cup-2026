import fs from 'node:fs';
import {fileURLToPath} from 'node:url';
import {createHash} from 'node:crypto';
import {initialState,normalizeBook,observe,sampleEquity,report,validateConfig,invalidate} from './model.mjs';

export function replayJournal(raw,c){
  validateConfig(c);if(!raw||!raw.endsWith('\n'))throw Error('Incomplete capture journal');
  const sourceHash=createHash('sha256').update(raw).digest('hex');let previous='',sequence=0,state=null,cycles=0,clock=null;
  for(const line of raw.split('\n').filter(Boolean)){
    const r=JSON.parse(line),{hash,...body}=r;
    if(body.sequence!==++sequence||body.previous!==previous||hash!==createHash('sha256').update(JSON.stringify(body)).digest('hex'))throw Error('Capture integrity failure');previous=hash;
    const eventClock=body.event.clockAt||body.at;
    if(!Number.isFinite(Date.parse(eventClock))||(clock&&Date.parse(eventClock)<Date.parse(clock)))throw Error('Journal clock rollback');clock=eventClock;
    if(body.event.type!=='cycle'){if(state&&body.event.type==='account_read_outage')invalidate(state,null,clock,body.event.reason);continue;}
    const excluded=new Set(body.event.accountExclusions);let cycleAt=null;
    for(const e of body.event.sourceSnapshots){
      if(e.skipped){if(state){const at=e.at||body.state?.skips?.[e.marketId]?.at||clock;state.skips[e.marketId]={at,reason:e.skipped};invalidate(state,e.marketId,at,e.skipped);}continue;}
      if(!e.endpoint)continue;
      const match=/^\/markets\/(\d+)\/orderbook\?/.exec(e.endpoint);if(!match||!c.marketIds.includes(match[1]))continue;
      state??=initialState(c,'replay-'+sourceHash.slice(0,16),e.requestStartedAt);
      let b;try{b=normalizeBook(e.data,match[1],c.tournamentId,e.receivedAt,e.requestStartedAt,c.maxDataAgeMs,excluded);}
      catch(err){state.skips[match[1]]={at:e.receivedAt,reason:err.message};invalidate(state,match[1],e.receivedAt,err.message);continue;}
      state=observe(state,b,c);cycleAt=b.receivedAt;
    }
    if(state)sampleEquity(state,c,clock);cycles++;
  }
  if(!state||!state.observations)throw Error('No accepted real source observations');
  return {...report(state,c,clock),replay:{sourceHash,sourceRecords:sequence,cycles,finalEventClock:clock,scope:'Whole captured path; parameter exploration, not held-out profitability',networkRequests:0}};
}
if(process.argv[1]&&fileURLToPath(import.meta.url)===process.argv[1]){
  if(process.argv.includes('--live'))throw Error('Offline replay only');
  const arg=name=>{const i=process.argv.indexOf(name);if(i===-1||!process.argv[i+1])throw Error('Required '+name);return process.argv[i+1];};
  console.log(JSON.stringify(replayJournal(fs.readFileSync(arg('--journal'),'utf8'),JSON.parse(fs.readFileSync(arg('--config'),'utf8'))),null,2));
}
