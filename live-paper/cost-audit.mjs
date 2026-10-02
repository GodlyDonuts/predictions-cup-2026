import fs from 'node:fs';
import {fileURLToPath} from 'node:url';
import {captureRows} from './ml-bank.mjs';
import {ISOLATED} from './history-policy.mjs';
import {decisionAudit} from './liquidity.mjs';
import {roundTripLabel} from './roundtrip-target.mjs';
import {digest,validateConfig} from './model.mjs';
const rowKeys=['at','marketId','instrument','historyLength','segment','book'];
const bookKeys=['marketId','exchangeId','instrument','receivedAt','requestStartedAt','sourceAt','sequence','versionKey','bids','asks'];
const exact=(v,keys)=>{if(!v||Object.keys(v).sort().join('|')!==[...keys].sort().join('|'))throw Error('Unexpected dataset fields');};
export function validateDataset(d){
 exact(d,['schemaVersion','kind','windowStart','cutoff','finalSegments','rows']);
 if(d.schemaVersion!==1||d.kind!=='derived_normalized_market_decisions_v1'||!Number.isFinite(Date.parse(d.windowStart))||!Number.isFinite(Date.parse(d.cutoff)))throw Error('Invalid dataset metadata');
 let previous=Date.parse(d.windowStart);
 for(const r of d.rows){exact(r,rowKeys);exact(r.book,bookKeys);const b=r.book;
  if(!/^\d+$/.test(r.marketId)||!/^\d+$/.test(b.exchangeId)||b.marketId!==r.marketId||b.instrument!==r.instrument||r.instrument!==b.exchangeId+':YES'||r.at!==b.receivedAt||!Number.isSafeInteger(r.historyLength)||r.historyLength<0||!/^\d+:\d+$/.test(r.segment)||!/^\d+$/.test(b.sequence)||b.versionKey!==b.sequence+':'+b.sourceAt)throw Error('Invalid decision identity');
  const times=[r.at,b.requestStartedAt,b.sourceAt].map(Date.parse);if(times.some(x=>!Number.isFinite(x))||times[0]<previous||times[0]>Date.parse(d.cutoff)||times[1]>times[0])throw Error('Invalid decision clock');previous=times[0];
  for(const levels of [b.bids,b.asks]){if(!levels.length)throw Error('Missing depth');for(const l of levels){exact(l,['price','quantity']);if(!/^(0\.\d{6}|1\.000000)$/.test(l.price)||!Number.isSafeInteger(l.quantity)||l.quantity<1)throw Error('Invalid depth');}}
 }
 for(const [id,segment]of Object.entries(d.finalSegments))if(!/^\d+$/.test(id)||!/^\d+:\d+$/.test(segment))throw Error('Invalid final segment');
 return d;
}
export function exportDataset(raw,c,{since}){
 const source=captureRows(raw,c,{historyPolicy:ISOLATED,since});
 const rows=source.rows.map(r=>Object.fromEntries(rowKeys.map(k=>[k,k==='book'?Object.fromEntries(bookKeys.map(f=>[f,['bids','asks'].includes(f)?r.book[f].map(l=>({price:l.price,quantity:l.quantity})):r.book[f]])):r[k]])));
 return validateDataset({schemaVersion:1,kind:'derived_normalized_market_decisions_v1',windowStart:since,cutoff:source.cutoff,finalSegments:source.finalSegments,rows});
}
export function compareCosts(dataset,c,{horizons=[60]}={}){
 validateDataset(dataset);validateConfig(c);
 if(!horizons.length||horizons.some(h=>!Number.isSafeInteger(h)||h<1)||new Set(horizons).size!==horizons.length)throw Error('Invalid horizons');
 const definitions=[['observed_price_zero_additional_fee',0,0],['fee_10bps_stress',10,0],['extra_tick_stress',0,1],['fee_and_tick_stress',10,1]];
 const scenarios=definitions.map(([name,feeBps,slippageTicks])=>{
  const config={...c,feeBps,slippageTicks},outcomes=[];
  for(const horizon of horizons)for(let i=0;i<dataset.rows.length;i++){
   const r=dataset.rows[i];outcomes.push({horizon,...roundTripLabel(dataset.rows,i,config,horizon*1000,{includeOutcome:true,asOf:dataset.cutoff,terminalInvalidated:r.segment!==dataset.finalSegments[r.marketId]}).outcome});
  }
  return {name,feeBps,slippageTicks,configHash:digest(config),markets:decisionAudit(dataset.rows,config,dataset.cutoff,dataset.finalSegments,{horizons}),outcomes};
 });
 return {schemaVersion:1,datasetContentHash:digest(dataset),baseConfigHash:digest(c),decisionCount:dataset.rows.length,horizons,scenarios,interpretation:{actualWalletTrades:false,profitabilityEstablished:false,baseline:'Observed delayed snapshot execution prices and sampled depth; zero additional explicit fee and zero extra tick penalty. Does not establish all future platform fees.',stress:'10 basis points on each fill and/or one adverse extra tick; explicit assumptions, not inferred Cup fees.',fixed:'Same latency, depth fraction, order quantity, risk limits, warmup, FIFO and outage/censor rules in each scenario.',caution:'Independent overlapping flat-wallet probes, not a portfolio, actual ML signals, held-out model performance or independent trades. Scenario admission and fills can differ near limits. Full quantities and censored outcomes retained. No sum of correlated outcomes is claimed as portfolio PnL.',source:'Derived normalized accepted market decisions; cannot independently verify original private journal or source filtering. Original frozen bank source bindings unchanged.'}};
}
if(process.argv[1]&&fileURLToPath(import.meta.url)===process.argv[1]){
 const arg=n=>{const i=process.argv.indexOf(n);if(i<0||!process.argv[i+1])throw Error('Required '+n);return process.argv[i+1];};
 console.log(JSON.stringify(compareCosts(JSON.parse(fs.readFileSync(arg('--dataset'),'utf8')),JSON.parse(fs.readFileSync(arg('--config'),'utf8'))),null,process.argv.includes('--compact')?undefined:2));
}
