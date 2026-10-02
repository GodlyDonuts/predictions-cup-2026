import fs from 'node:fs';
import {fileURLToPath} from 'node:url';
import {createHash} from 'node:crypto';
import {replayJournal} from './replay.mjs';
import {digest,validateConfig} from './model.mjs';
import {scopedSnapshots} from './depth-scope.mjs';

export function validatePlan(p){
  const created=Date.parse(p.createdAt),start=Date.parse(p.startAt),end=Date.parse(p.endAt);
  if(p.schemaVersion!==1||!Number.isFinite(created)||!Number.isFinite(start)||!Number.isFinite(end)||created>=start||start>=end)throw Error('Plan must be frozen before a valid future window');
  validateConfig(p.baseConfig);
  if(p.baseConfigHash!==digest(p.baseConfig)||!Number.isSafeInteger(p.minimumClosedTrades)||p.minimumClosedTrades<100||!Array.isArray(p.variants)||!p.variants.length)throw Error('Invalid frozen plan');
  const names=new Set();
  for(const v of p.variants){
    if(typeof v.name!=='string'||!v.name||names.has(v.name)||!v.overrides||Object.keys(v.overrides).some(k=>!['feeBps','slippageTicks','latencyMs','depthFractionBps'].includes(k)))throw Error('Invalid sensitivity variant');
    names.add(v.name);const c=validateConfig({...p.baseConfig,...v.overrides});
    if(v.configHash!==digest(c))throw Error('Variant config checksum mismatch');
  }
  return p;
}

// The original complete chain is verified before selecting any observations.
// A derived chain is an explicit window transform, never original source evidence.
export function evaluateJournal(raw,plan){
  validatePlan(plan);
  const original=replayJournal(raw,plan.baseConfig),start=Date.parse(plan.startAt),end=Date.parse(plan.endAt);
  let previous='',sequence=0;const selected=[];let excludedSnapshots=0;
  const append=(event,at)=>{const body={sequence:++sequence,previous,at,event,state:{}},hash=createHash('sha256').update(JSON.stringify(body)).digest('hex');previous=hash;selected.push(JSON.stringify({...body,hash}));};
  for(const line of raw.trimEnd().split('\n')){
    const r=JSON.parse(line),e=r.event,clock=e.clockAt||r.at,t=Date.parse(clock);
    if(t<start)continue;
    if(e.type==='cycle'){
      const scoped=scopedSnapshots(e,plan.baseConfig);
      if(!Array.isArray(e.accountExclusions)||e.accountExclusions.some(id=>typeof id!=='string'||!/^([1-9]\d*)$/.test(id)||!Number.isSafeInteger(Number(id))))throw Error('Incomplete window account exclusions');
      const reconciliation=Date.parse(e.exclusionObservedAt);
      if(!Number.isFinite(reconciliation)||reconciliation<start||reconciliation>=end){excludedSnapshots+=scoped.length;continue;}
      const eligible=s=>{
        const received=Date.parse(s.receivedAt||s.at||clock),requested=Date.parse(s.requestStartedAt||s.at||clock);
        const inside=Number.isFinite(received)&&Number.isFinite(requested)&&requested>=start&&requested>=reconciliation&&received>=requested&&received<=t&&received<end;
        if(!inside)excludedSnapshots++;return inside;
      };
      const kept=new Set(scoped.filter(eligible));
      const sourceSnapshots=(e.sourceSnapshots||[]).filter(s=>kept.has(s));
      const supplementalDepth=(e.supplementalDepth||[]).filter(s=>kept.has(s));
      if(kept.size)append({...e,clockAt:new Date(Math.min(t,end)).toISOString(),sourceSnapshots,supplementalDepth},new Date(Math.min(t,end)).toISOString());
    }else if(e.type==='account_read_outage'&&t<end)append(e,clock);
  }
  const completed=Date.parse(original.replay.finalEventClock)>=end;
  const finalClock=completed?plan.endAt:original.replay.finalEventClock;
  const metadata={planHash:digest(plan),originalSourceHash:original.replay.sourceHash,originalSourceRecords:original.replay.sourceRecords,startAt:plan.startAt,endAt:plan.endAt,captureThrough:original.replay.finalEventClock,windowComplete:completed,excludedSnapshots,networkRequests:0,competitionOrders:0,profitabilityEstablished:false};
  if(!selected.some(s=>JSON.parse(s).event.type==='cycle'))return {...metadata,status:'awaiting_in_window_capture',variants:[]};
  append({type:'evaluation_cutoff',clockAt:finalClock},finalClock);
  const derived=selected.join('\n')+'\n';
  const variants=plan.variants.map(v=>{
    const c={...plan.baseConfig,...v.overrides};let r;
    try{r=replayJournal(derived,c);}catch(err){if(err.message==='No accepted real source observations')return {name:v.name,configHash:v.configHash,observations:0,sampleStatus:'no_accepted_window_books'};throw err;}
    const strategies=Object.fromEntries(Object.entries(r.strategies).map(([name,s])=>[name,{cash:s.cash,realizedPnl:s.realizedPnl,referencePnl:s.referencePnl,liquidationPnl:s.liquidationPnl,fees:s.fees,fills:s.fills,intents:s.intents,closedTrades:s.closedTrades,winRate:s.winRate,openShares:s.positions.reduce((n,p)=>n+p.quantity,0),pendingIntents:s.pending.length,unresolvedPositions:s.positions.map(p=>({instrument:p.instrument,quantity:p.quantity,unsoldShares:p.unsoldShares,observationAgeMs:p.observationAgeMs})),maxObservedDrawdown:s.maxObservedDrawdown}]));
    return {name:v.name,configHash:r.configHash,observations:r.observations,derivedWindowHash:r.replay.sourceHash,strategies,noTradeBaseline:r.noTradeBaseline,sampleStatus:!completed?'window_incomplete':r.strategies.mean_reversion.closedTrades<plan.minimumClosedTrades?'insufficient_closed_trades':'eligible_for_independent_analysis'};
  });
  return {...metadata,status:completed?'window_complete_pending_review':'collecting_frozen_window',variants,limitations:['Synthetic tests verify mechanics only','Fees are unverified scenarios; every listed variant must be reported','Later REST snapshots are hypothetical taker opportunities, not confirmed fills','Unresolved positions are not forcibly liquidated at an invented price','Midpoint returns exclude executable exit costs; liquidation can be unavailable','Trade count alone does not establish independence, calibration or profitability','Original capture history remains exploratory; each window starts flat with in-window warmup']};
}

if(process.argv[1]&&fileURLToPath(import.meta.url)===process.argv[1]){
  const arg=name=>{const i=process.argv.indexOf(name);if(i<0||!process.argv[i+1])throw Error('Required '+name);return process.argv[i+1];};
  console.log(JSON.stringify(evaluateJournal(fs.readFileSync(arg('--journal'),'utf8'),JSON.parse(fs.readFileSync(arg('--plan'),'utf8'))),null,2));
}
