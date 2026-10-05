import fs from 'node:fs';import path from 'node:path';import readline from 'node:readline';import {createHash} from 'node:crypto';
import {createRealtimeSpreadEconomics,REALTIME_ECONOMICS_POLICIES} from '../live-paper/realtime-spread-economics.mjs';
import {MICROPRICE_FEATURES} from '../live-paper/realtime-microprice-dataset.mjs';
const root=path.join(process.env.LOCALAPPDATA,'SigTradingCup/paper-runtime'),sources=JSON.parse(fs.readFileSync('data/realtime-spread-economics-20261005.json')).sources;
const policies=REALTIME_ECONOMICS_POLICIES.filter(p=>p.latencyMs===3000),rows=[],quotes=[];
const study=createRealtimeSpreadEconomics({policies,onQuote:q=>quotes.push(q),onOutcome:o=>rows.push(o)}),started=performance.now();
const out='data/realtime-quote-dataset-20261005.json';if(fs.existsSync(out))throw Error('FROZEN_DATASET_EXISTS');
for(const s of sources){const h=createHash('sha256'),stream=fs.createReadStream(path.join(root,s.file));stream.on('data',b=>h.update(b));for await(const line of readline.createInterface({input:stream,crlfDelay:Infinity}))if(line)study.consume(JSON.parse(line),{sourceId:s.file});if(h.digest('hex')!==s.sha256)throw Error('SOURCE_HASH_CHANGED');console.log(JSON.stringify({file:s.file,quotes:quotes.length,rows:rows.length,elapsedMs:Math.round(performance.now()-started)}));}
const result={schemaVersion:1,frozenAt:new Date().toISOString(),mode:'causal_quote_outcome_training_dataset',features:MICROPRICE_FEATURES,policies,sources,quotes,rows,
 report:study.report(),elapsedMs:performance.now()-started,networkRequests:0,competitionOrders:0,profitabilityEstablished:false,
 limitations:['Strict-through anonymous prints are hypothetical fills; queue position and actual execution are not proven.','Censored outcomes have unknown economic value, never a zero-profit training target.','Frozen decision features contain no later fills or prices. Overlapping outcomes are not funded P&L.','Development captures were previously examined; independent subsequent replay and prospective wallet accounting remain necessary.']};
// The report is a compact summary; rows already preserve each exact outcome.
result.report.policies=result.report.policies.map(({outcomes,...summary})=>summary);
fs.writeFileSync(out,JSON.stringify(result)+'\n',{flag:'wx'});
const {quotes:q,rows:r,...summary}=result;summary.quotes=q.length;summary.rows=r.length;summary.file=out;summary.sha256=createHash('sha256').update(fs.readFileSync(out)).digest('hex');
fs.writeFileSync('data/realtime-quote-dataset-summary-20261005.json',JSON.stringify(summary,null,2)+'\n',{flag:'wx'});console.log(JSON.stringify(summary,null,2));
