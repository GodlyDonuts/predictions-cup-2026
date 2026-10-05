import fs from 'node:fs';
import path from 'node:path';
import readline from 'node:readline';
import {createHash} from 'node:crypto';
import {createMicropriceDataset,MICROPRICE_DATA_POLICY} from '../live-paper/realtime-microprice-dataset.mjs';

// One pass over the same sealed sources; no account or network client.
const root=path.join(process.env.LOCALAPPDATA,'SigTradingCup/paper-runtime');
const sources=JSON.parse(fs.readFileSync('data/realtime-spread-economics-20261005.json')).sources;
const horizons=[5,30,120],started=performance.now();
const cohorts=horizons.map(minutes=>{
 const out=`data/realtime-horizon-${minutes}m-dataset-20261005.json`;
 if(fs.existsSync(out))throw Error('FROZEN_DATASET_EXISTS:'+out);
 const rows=[],policy={...MICROPRICE_DATA_POLICY,horizonMs:minutes*60000,endpointResearchAcrossGaps:true};
 return{minutes,out,rows,builder:createMicropriceDataset({policy,onSample:r=>rows.push(r)})};
});
for(const s of sources){
 const h=createHash('sha256'),stream=fs.createReadStream(path.join(root,s.file));stream.on('data',b=>h.update(b));
 for await(const line of readline.createInterface({input:stream,crlfDelay:Infinity}))if(line){const row=JSON.parse(line);for(const c of cohorts)c.builder.consume(row,{sourceId:s.file});}
 if(h.digest('hex')!==s.sha256)throw Error('SOURCE_HASH_CHANGED:'+s.file);
 console.log(JSON.stringify({file:s.file,labels:cohorts.map(c=>({minutes:c.minutes,rows:c.rows.length})),elapsedMs:Math.round(performance.now()-started)}));
}
const summaries=[];
for(const c of cohorts){
 const record={schemaVersion:2,frozenAt:new Date().toISOString(),mode:'causal_endpoint_horizon_research',...c.builder.report(),rows:c.rows,sources,elapsedMs:performance.now()-started,networkRequests:0,competitionOrders:0,profitabilityEstablished:false,
  limitations:['Independently observed entry and liquidation prices are endpoint forecasting labels; fills and path-dependent risk are not established.','Missing paths are flagged, never silently treated as continuous. Unlabelled/censored decisions are excluded from forecast metrics, so labelled subsets may have selection bias.','Only fixed chronological purged splits may select models; this is not a funded wallet or a prospective profit result.']};
 fs.writeFileSync(c.out,JSON.stringify(record)+'\n',{flag:'wx'});
 summaries.push({...record,rows:record.rows.length,sources:record.sources.length,file:c.out,sha256:createHash('sha256').update(fs.readFileSync(c.out)).digest('hex')});
}
fs.writeFileSync('data/realtime-horizons-dataset-summary-20261005.json',JSON.stringify({schemaVersion:1,frozenAt:new Date().toISOString(),horizons:summaries,networkRequests:0,competitionOrders:0},null,2)+'\n',{flag:'wx'});
console.log(JSON.stringify(summaries.map(({policy,counts,rows,unresolved,file})=>({policy,counts,rows,unresolved,file})),null,2));
