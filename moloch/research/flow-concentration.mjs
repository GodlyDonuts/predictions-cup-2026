import fs from 'node:fs';import path from 'node:path';import {pathToFileURL} from 'node:url';
import {payloadHash} from '../../execution/ledger.mjs';import {instantNs} from '../../feed/contract.mjs';
import {streamActivity} from './stream-activity.mjs';

// Execution-time concentration is a diagnostic, not aggressor identification.
// Data received after the decision clock cannot improve the selection score.
export function flowConcentration(rows,referenceAt,pairs){
 const cutoff=instantNs(referenceAt),start=cutoff-180000000000n,known=new Map(pairs.map(p=>[p.exchangeId,p.marketId])),markets=new Map(),seen=new Map();
 for(const {event:e} of rows){
  const received=instantNs(e.receivedAt);if(received>cutoff)continue;
  if(e.kind!=='broadcast'||e.event!=='market_batch')continue;
  if(e.source!=='official_sig_realtime')throw Error('CONCENTRATION_SOURCE');
  if(e.duplicate||e.gap||e.payload.resyncRequired)continue;
  for(const t of e.payload.trades){
   const id=String(t.exchangeId);if(known.get(id)!==e.marketId)throw Error('CONCENTRATION_SCOPE');
   if(t.id===null||t.sequence===null)continue;
   const at=instantNs(t.executedAt);if(at>received||!Number.isSafeInteger(t.quantity)||t.quantity<=0)throw Error('CONCENTRATION_TRADE');
   const key=id+':'+t.id,hash=payloadHash(t);if(seen.has(key)){if(seen.get(key)!==hash)throw Error('CONCENTRATION_CONFLICT');continue;}seen.set(key,hash);
   if(at<start)continue;
   let m=markets.get(id);if(!m){m={shares:0n,trades:0,buckets:new Map(),activeBins:new Set()};markets.set(id,m);}
   const second=at/1000000000n,bin=(at-start)/30000000000n;const quantity=BigInt(t.quantity);
   m.shares+=quantity;m.trades++;m.buckets.set(second,(m.buckets.get(second)??0n)+quantity);m.activeBins.add(bin.toString());
  }
 }
 return [...markets].map(([exchangeId,m])=>{let largest=0n,squares=0n;for(const v of m.buckets.values()){if(v>largest)largest=v;squares+=v*v;}
  return{exchangeId,marketId:known.get(exchangeId),observedTrades:m.trades,observedShares:m.shares.toString(),largestSecondShares:largest.toString(),largestSecondFraction:Number(largest)/Number(m.shares),activeThirtySecondBins:m.activeBins.size,effectiveActiveSeconds:Number(m.shares*m.shares)/Number(squares)};
 });
}

if(process.argv[1]&&import.meta.url===pathToFileURL(path.resolve(process.argv[1])).href){
 const [directory,output]=process.argv.slice(2);if(!output)throw Error('Usage: completed-stream new-output');
 const read=file=>JSON.parse(fs.readFileSync(path.join(directory,file))),stats=read('stats.json'),manifest=read('manifest.json');
 if(stats.status!=='completed'||stats.failure)throw Error('CONCENTRATION_TERMINAL');
 const rows=fs.readFileSync(path.join(directory,'events.jsonl'),'utf8').split('\n').slice(0,stats.sequence).map(JSON.parse);let sequence=0,previous='';
 for(const row of rows){const{hash,...body}=row;if(hash!==payloadHash(body)||body.sequence!==++sequence||body.previous!==previous)throw Error('CONCENTRATION_JOURNAL');previous=hash;}
 if(sequence!==stats.sequence||previous!==stats.previous)throw Error('CONCENTRATION_PREFIX');
 const concentration=new Map(flowConcentration(rows,stats.at,manifest.pairs).map(c=>[c.exchangeId,c]));
 const candidates=streamActivity(rows,stats.at,manifest.pairs).filter(c=>c.eligible&&c.balancedFlowScore>0).map(c=>({...c,...concentration.get(c.exchangeId)})).sort((a,b)=>b.balancedFlowScore-a.balancedFlowScore);
 const result={schemaVersion:1,at:stats.at,sourceDirectory:path.resolve(directory),sourceSequence:sequence,sourcePrevious:previous,candidates,contestOrders:0,profitabilityEstablished:false,limitations:['Retrospective warmup diagnostic; no threshold is validated or automatically applied.','A burst can be legitimate trading; lower concentration does not establish positive expected maker return.','Observed volume and price-relative direction do not prove our queue priority or fills.']};
 fs.writeFileSync(output,JSON.stringify(result,null,2)+'\n',{flag:'wx'});console.log(JSON.stringify(candidates.slice(0,6).map(c=>({exchangeId:c.exchangeId,score:c.balancedFlowScore,largestSecondFraction:c.largestSecondFraction,bins:c.activeThirtySecondBins,effectiveSeconds:c.effectiveActiveSeconds})),null,2));
}
