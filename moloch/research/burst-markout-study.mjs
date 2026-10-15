import fs from 'node:fs';import path from 'node:path';import {createHash} from 'node:crypto';
import {extractRows} from '../adverse-model.mjs';import {payloadHash} from '../../execution/ledger.mjs';

const output=process.argv[2];if(!output)throw Error('new output required');
const phases=[61,65,66,67,71,75,76],sources=[],samples=[],seen=new Set();
for(const phase of phases){
 const directory=`runtime/moloch/live-sig-20261014-${phase}/capture/stream`,read=f=>JSON.parse(fs.readFileSync(path.join(directory,f))),stats=read('stats.json');
 if(stats.status!=='completed'||stats.failure)throw Error('BURST_STUDY_TERMINAL');
 const bytes=fs.readFileSync(path.join(directory,'events.jsonl')),rows=bytes.toString('utf8').split('\n').slice(0,stats.sequence).map(JSON.parse);
 const labels=extractRows(rows);if(labels.eventCount!==stats.sequence||labels.finalHash!==stats.previous)throw Error('BURST_STUDY_PREFIX');
 const trades=new Map(),facts=new Map();
 for(const {event:e} of rows){if(e.kind!=='broadcast'||e.event!=='market_batch'||e.duplicate||e.gap||e.payload.resyncRequired)continue;
  for(const t of e.payload.trades){if(t.id===null||t.sequence===null)continue;const key=t.exchangeId+':'+t.id,hash=payloadHash(t);if(facts.has(key)){if(facts.get(key)!==hash)throw Error('BURST_STUDY_FACT');continue;}facts.set(key,hash);const list=trades.get(t.exchangeId)??[];list.push({received:Date.parse(e.receivedAt),executed:Date.parse(t.executedAt),quantity:t.quantity});trades.set(t.exchangeId,list);}
 }
 const cache=new Map();
 for(const s of labels.samples){
  const id=s.exchangeId+':'+s.tradeId;if(seen.has(id))continue;seen.add(id);
  const cutoff=Date.parse(s.quoteAt),key=s.exchangeId+':'+cutoff;let profile=cache.get(key);
  if(!profile){const past=(trades.get(s.exchangeId)??[]).filter(t=>t.received<=cutoff&&t.executed<=cutoff&&t.executed>=cutoff-180000);let total=0n;const buckets=new Map();
   for(const t of past){if(!Number.isSafeInteger(t.quantity)||t.quantity<=0)throw Error('BURST_STUDY_QUANTITY');const q=BigInt(t.quantity),second=Math.floor(t.executed/1000);total+=q;buckets.set(second,(buckets.get(second)??0n)+q);}
   let largest=0n;for(const q of buckets.values())if(q>largest)largest=q;
   profile={priorTrades:past.length,largestSecondFraction:total?Number(largest)/Number(total):null};cache.set(key,profile);
  }
  samples.push({phase,exchangeId:s.exchangeId,side:s.side,executedAt:s.executedAt,delta:s.delta,label:s.label,...profile});
 }
 sources.push({phase,sequence:stats.sequence,previous:stats.previous,sha256:createHash('sha256').update(bytes).digest('hex'),labeled:labels.samples.length,censored:labels.censored});
}
function summary(rows){
 const groups=new Map();for(const s of rows){const key=s.phase+':'+s.exchangeId+':'+s.side+':'+Math.floor(Date.parse(s.executedAt)/60000);const list=groups.get(key)??[];list.push(s.delta);groups.set(key,list);}
 return{samples:rows.length,markets:new Set(rows.map(s=>s.exchangeId)).size,minuteSideClusters:groups.size,adverseRate:rows.length?rows.reduce((n,s)=>n+s.label,0)/rows.length:null,meanOneShareDelta:rows.length?rows.reduce((n,s)=>n+s.delta,0)/rows.length:null,equalClusterMeanDelta:groups.size?[...groups.values()].reduce((n,x)=>n+x.reduce((a,b)=>a+b,0)/x.length,0)/groups.size:null};
}
const result={schemaVersion:1,at:new Date().toISOString(),sources,fixedDiagnostic:{windowMs:180000,minimumPriorTrades:10,maximumLargestSecondFraction:.5},groups:['earlier','later'].map(name=>{const all=samples.filter(s=>(s.phase<=66?'earlier':'later')===name),eligible=all.filter(s=>s.priorTrades>=10),retained=eligible.filter(s=>s.largestSecondFraction<=.5);return{name,all:summary(all),enoughHistory:summary(eligible),retained:summary(retained),rejected:summary(eligible.filter(s=>s.largestSecondFraction>.5))};}),contestOrders:0,profitabilityEstablished:false,limitations:['Previously inspected captures; chronological grouping is retrospective, not a pristine holdout.','Labels are later executable one-share marks conditional on public prints, not attributed maker fills or funded trading returns.','Equal minute/market/side cluster averages reduce print-batch dominance but do not supply independent observations.','Fixed concentration gate is not fitted or automatically enabled.']};
fs.writeFileSync(output,JSON.stringify(result,null,2)+'\n',{flag:'wx'});console.log(JSON.stringify(result.groups,null,2));
