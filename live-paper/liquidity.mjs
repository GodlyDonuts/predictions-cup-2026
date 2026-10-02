import fs from 'node:fs';
import {fileURLToPath} from 'node:url';
import {captureRows} from './ml-bank.mjs';
import {roundTripLabel,ROUND_TRIP} from './roundtrip-target.mjs';
import {amount,fee,TICK} from '../paper/fixed.mjs';
import {ISOLATED} from './history-policy.mjs';
const number=x=>Number(amount(x))/1e6;
export function distribution(values){const a=values.filter(Number.isFinite).sort((a,b)=>a-b),q=p=>a.length?a[Math.floor((a.length-1)*p)]:null;return {count:a.length,min:q(0),median:q(.5),p95:q(.95),max:q(1)};}
export function decisionAudit(rows,c,cutoff,finalSegments,{horizons=[60,180,600]}={}){
 if(!horizons.length||horizons.some(x=>!Number.isSafeInteger(x)||x<1)||new Set(horizons).size!==horizons.length)throw Error('Invalid diagnostic horizons');
 const markets={},previous={};
 for(const r of rows){
  const b=r.book,m=markets[b.marketId]??={uniqueDecisions:0,warmup:0,latencyMs:[],sourceAgeMs:[],gapMs:[],spread:[],askCapacity:[],bidCapacity:[],quoteUnchanged:0,continuousPairs:0,segmentBreaks:0,askDepthChange:[],bidDepthChange:[],nextBidChange:[],nextEntryCostChange:[],horizons:{}};
  m.uniqueDecisions++;if(r.historyLength<6)m.warmup++;
  m.latencyMs.push(Date.parse(b.receivedAt)-Date.parse(b.requestStartedAt));m.sourceAgeMs.push(Date.parse(b.receivedAt)-Date.parse(b.sourceAt));m.spread.push(number(b.asks[0].price)-number(b.bids[0].price));
  const depth=levels=>levels.reduce((n,l)=>n+Math.floor(l.quantity*c.depthFractionBps/10000),0),entry=book=>{const a=amount(book.asks[0].price)+TICK*BigInt(c.slippageTicks);return Number(a+fee(a,1,{feeBps:c.feeBps,feePerShare:'0.000000'}))/1e6;};
  m.askCapacity.push(depth(b.asks));m.bidCapacity.push(depth(b.bids));
  const old=previous[r.instrument];if(old){m.gapMs.push(Date.parse(r.at)-Date.parse(old.at));if(old.segment===r.segment){m.continuousPairs++;if(old.book.asks[0].price===b.asks[0].price&&old.book.bids[0].price===b.bids[0].price)m.quoteUnchanged++;m.askDepthChange.push(depth(b.asks)-depth(old.book.asks));m.bidDepthChange.push(depth(b.bids)-depth(old.book.bids));m.nextBidChange.push(number(b.bids[0].price)-number(old.book.bids[0].price));m.nextEntryCostChange.push(entry(b)-entry(old.book));}else m.segmentBreaks++;}previous[r.instrument]=r;
 }
 for(const horizon of horizons)for(let i=0;i<rows.length;i++){
  const r=rows[i],m=markets[r.book.marketId],h=m.horizons[horizon]??={decisions:0,reasons:{},enteredDecisions:0,openDecisions:0,missingLiquidation:0,missingReference:0,realizedPnl:[],cashDelta:[],fees:[],referencePnl:[],liquidationPnl:[],outcomesByReason:{}};
  const {outcome:o}=roundTripLabel(rows,i,c,horizon*1000,{includeOutcome:true,asOf:cutoff,terminalInvalidated:r.segment!==finalSegments[r.book.marketId]});
  h.decisions++;h.reasons[o.reason]=(h.reasons[o.reason]||0)+1;if(o.enteredQuantity)h.enteredDecisions++;if(o.openQuantity)h.openDecisions++;if(o.liquidationPnl===null)h.missingLiquidation++;if(o.referencePnl===null)h.missingReference++;
  const group=h.outcomesByReason[o.reason]??={count:0,openDecisions:0,missingLiquidation:0,realizedPnl:[],liquidationPnl:[],fees:[]};group.count++;if(o.openQuantity)group.openDecisions++;if(o.liquidationPnl===null)group.missingLiquidation++;
  for(const key of ['realizedPnl','cashDelta','fees','referencePnl','liquidationPnl'])if(o[key]!==null)h[key].push(o[key]);for(const key of ['realizedPnl','liquidationPnl','fees'])if(o[key]!==null)group[key].push(o[key]);
 }
 for(const m of Object.values(markets)){for(const key of ['latencyMs','sourceAgeMs','gapMs','spread','askCapacity','bidCapacity','askDepthChange','bidDepthChange','nextBidChange','nextEntryCostChange'])m[key]=distribution(m[key]);for(const h of Object.values(m.horizons)){for(const key of ['realizedPnl','cashDelta','fees','referencePnl','liquidationPnl'])h[key]=distribution(h[key]);for(const group of Object.values(h.outcomesByReason))for(const key of ['realizedPnl','liquidationPnl','fees'])group[key]=distribution(group[key]);}}
 return markets;
}
export function auditLiquidity(raw,c,{since=null,horizons=[60,180,600]}={}){
 const source=captureRows(raw,c,{historyPolicy:ISOLATED,since}),markets=decisionAudit(source.rows,c,source.cutoff,source.finalSegments,{horizons}),captureHealth={accountReadOutages:0,scopedSkips:{}};
 for(const line of raw.trimEnd().split('\n')){const r=JSON.parse(line),e=r.event;if(since&&Date.parse(e.clockAt||r.at)<Date.parse(since))continue;if(e.type==='account_read_outage')captureHealth.accountReadOutages++;for(const s of [...(e.sourceSnapshots||[]),...(e.supplementalDepth||[])])if(s.skipped&&c.marketIds.includes(String(s.marketId)))captureHealth.scopedSkips[s.skipped]=(captureHealth.scopedSkips[s.skipped]||0)+1;}
 return {schemaVersion:1,createdAt:new Date().toISOString(),sourceHash:source.sourceHash,sourceRecords:source.sourceRecords,captureThrough:source.cutoff,windowStart:since,targetPolicy:ROUND_TRIP,decisionCount:source.rows.length,captureHealth,markets,interpretation:{networkRequests:0,liveWalletsChanged:false,profitabilityEstablished:false,decisionUnit:'Unique accepted scoped book; warmup and every outcome retained',counterfactual:'Independent flat-wallet entry probes using the preserved v1 label admission; not actual ML signals or a portfolio',quoteUnchanged:'Top bid/ask prices unchanged across adjacent accepted versions in one uninterrupted segment; not continuous persistence',depthChange:'Observed capacity difference at sampled levels; not proven replenishment or queue evidence',nextPriceChange:'Next sampled book change with corresponding gap distribution; not signal-conditioned execution quality',limitations:['Overlapping probes are correlated and cannot be summed as portfolio PnL or independent trades','Open inventory and missing liquidation are retained; zero realized PnL on an open probe does not mean zero loss','Segment boundaries/outages suppress executable marks; stale or insufficient depth liquidation is unavailable','REST snapshots do not establish maker fills or effective independent sample size','Historical STOP-affected capture evidence still requires per-boundary audit; this report is exploratory']}};
}
if(process.argv[1]&&fileURLToPath(import.meta.url)===process.argv[1]){const arg=n=>{const i=process.argv.indexOf(n);if(i<0||!process.argv[i+1])throw Error('Required '+n);return process.argv[i+1];};console.log(JSON.stringify(auditLiquidity(fs.readFileSync(arg('--journal'),'utf8'),JSON.parse(fs.readFileSync(arg('--config'),'utf8')),{since:process.argv.includes('--since')?arg('--since'):null}),null,2));}

