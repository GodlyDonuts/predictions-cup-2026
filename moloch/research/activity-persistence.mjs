import fs from 'node:fs';import path from 'node:path';import {pathToFileURL} from 'node:url';
import {payloadHash} from '../../execution/ledger.mjs';import {instantNs} from '../../feed/contract.mjs';
import {streamActivity} from './stream-activity.mjs';
const iso=n=>new Date(n).toISOString(),ms=s=>Number(instantNs(s)/1000000n);
export function futureActivity(rows,ids,start,end,pairs=[]){
 const selected=new Set(ids),selectedMarkets=new Set(pairs.filter(p=>selected.has(p.exchangeId)).map(p=>p.marketId)),seen=new Set(),facts=new Map();let shares=0n,trades=0,invalid=false;
 for(const {event:e} of rows)for(const t of e.payload?.trades??[]){if(!selected.has(String(t.exchangeId))||t.id===null)continue;const key=String(t.exchangeId)+':'+t.id,hash=payloadHash(t);if(facts.has(key)&&facts.get(key)!==hash)throw Error('PERSISTENCE_TRADE_CONFLICT');facts.set(key,hash);}
 for(const {event:e} of rows){
  const received=ms(e.receivedAt);if(received<start-3000||received>end+30000||!e.payload)continue;
  if(e.kind!=='broadcast')continue;
  const scoped=selectedMarkets.has(e.marketId)||(e.payload.books??[]).some(b=>selected.has(String(b.exchangeId)))||(e.payload.trades??[]).some(t=>selected.has(String(t.exchangeId)));
  if(scoped&&(e.gap||e.payload.resyncRequired||e.event!=='market_batch'))invalid=true;
  if(e.event!=='market_batch'||e.duplicate||e.gap||e.payload.resyncRequired)continue;
  for(const t of e.payload.trades){
   const id=String(t.exchangeId),at=ms(t.executedAt),key=id+':'+t.id;
   if(!selected.has(id)||t.id===null||t.sequence===null||seen.has(key)||at<start||at>=end)continue;
   seen.add(key);if(!Number.isSafeInteger(t.quantity)||t.quantity<=0)throw Error('PERSISTENCE_QUANTITY');shares+=BigInt(t.quantity);trades++;
  }
 }
 return{observedFutureTrades:trades,observedFutureShares:shares.toString(),futureGapDetected:invalid};
}
function readStream(directory){
 const stats=JSON.parse(fs.readFileSync(path.join(directory,'stats.json'))),manifest=JSON.parse(fs.readFileSync(path.join(directory,'manifest.json')));
 if(stats.status!=='completed'||stats.failure)throw Error('PERSISTENCE_TERMINAL_STREAM');
 const rows=fs.readFileSync(path.join(directory,'events.jsonl'),'utf8').split('\n').slice(0,stats.sequence).map(JSON.parse);
 let sequence=0,previous='';for(const row of rows){const {hash,...body}=row;if(hash!==payloadHash(body)||body.sequence!==++sequence||body.previous!==previous)throw Error('PERSISTENCE_CHAIN');previous=hash;}
 if(sequence!==stats.sequence||previous!==stats.previous)throw Error('PERSISTENCE_TERMINAL_HASH');
 return{stats,manifest,rows};
}
export function persistenceStudy(rows,pairs){
 const times=rows.map(r=>ms(r.event.receivedAt)),start=Math.min(...times),end=Math.max(...times),results=[];
 for(let cut=start+60000;cut+63000+30000<=end;cut+=30000){
  const prefix=rows.filter(r=>ms(r.event.receivedAt)<=cut),candidates=streamActivity(prefix,iso(cut),pairs).filter(c=>c.eligible&&c.recentTrades>=2);
  for(const method of ['queueTurnover','recentShares','recentTrades']){
   const score=c=>method==='queueTurnover'?c.activityScore:method==='recentShares'?Number(c.recentShares):c.recentTrades;
   const selected=[...candidates].sort((a,b)=>score(b)-score(a)||a.exchangeId.localeCompare(b.exchangeId)).slice(0,2).map(c=>c.exchangeId);
   if(!selected.length)continue;
   for(const delay of [0,30000,120000]){
    // Actual activation delay plus a thirty-second label delivery allowance.
    const first=cut+delay+3000,last=cut+delay+63000;if(last+30000>end)continue;
    const label=futureActivity(rows,selected,first,last,pairs);
    results.push({cutoff:iso(cut),method,startupDelayMs:delay,selected,activationAt:iso(first),horizonEnd:iso(last),...label});
   }
  }
 }
 return results;
}
if(process.argv[1]&&import.meta.url===pathToFileURL(path.resolve(process.argv[1])).href){
 const [output,...directories]=process.argv.slice(2);if(!directories.length)throw Error('Usage: new-output stream-directory ...');
 const sources=directories.map(directory=>({directory,...readStream(directory)}));
 const cases=sources.flatMap(s=>persistenceStudy(s.rows,s.manifest.pairs).map(c=>({source:s.directory,...c}))),summary=[];
 for(const method of ['queueTurnover','recentShares','recentTrades'])for(const delay of [0,30000,120000]){
  const all=cases.filter(c=>c.method===method&&c.startupDelayMs===delay),valid=all.filter(c=>!c.futureGapDetected);
  summary.push({method,startupDelayMs:delay,cases:all.length,gapExcluded:all.length-valid.length,evaluable:valid.length,withFutureTrades:valid.filter(c=>c.observedFutureTrades>0).length,totalObservedFutureTrades:valid.reduce((n,c)=>n+c.observedFutureTrades,0),totalObservedFutureShares:valid.reduce((n,c)=>n+BigInt(c.observedFutureShares),0n).toString()});
 }
 const document={schemaVersion:1,at:new Date().toISOString(),sources:sources.map(s=>({directory:s.directory,sequence:s.stats.sequence,previous:s.stats.previous})),summary,cases,contestOrders:0,networkRequests:0,profitabilityEstablished:false,
  limitations:['Retrospective diagnostic on previously inspected captures, not untouched strategy validation.','Overlapping cutoffs are dependent; totals are repeated opportunity measurements and must not be treated as unique traded volume.','Future observed activity is not a fill, a profit, or a complete tape guarantee.','Only facts received by each cutoff enter its ranking; pre-activation executions never count as future opportunity.','Selected-market future gaps exclude scoring; there is no synthetic gap repair.']};
 fs.writeFileSync(output,JSON.stringify(document,null,2)+'\n',{flag:'wx'});console.log(JSON.stringify(summary,null,2));
}
