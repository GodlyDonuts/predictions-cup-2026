import fs from 'node:fs';import path from 'node:path';import {fileURLToPath} from 'node:url';
import {verifySource} from './ml-shadow.mjs';import {digest,initialState,normalizeBook,invalidate,stepDip,report,sampleEquity,assertState,validateConfig} from './model.mjs';import {amount,TICK} from '../paper/fixed.mjs';
import {validateUniverse} from './breadth.mjs';import {historicalReference} from './history.mjs';import {atomicJson,lock} from './runtime.mjs';
import {dipPerformance} from './dip-performance.mjs';
export function validateDipPolicy(p){
 if(p?.name!=='diversified-dip-v1'||p.schemaVersion!==1||p.feeBps!==0)throw Error('Invalid dip policy');
 for(const k of ['lookbackMs','historyCacheAgeMs','minimumSamples','minimumSpanMs','lastHistoricalObservationAgeMs','maximumHoldMs','cooldownMs','maxOpenMarkets','maxPerState','maxPerOffice','maxUnknownIdentity'])if(!Number.isSafeInteger(p[k])||p[k]<1)throw Error('Invalid dip limit');
 if(p.profile!==undefined&&p.profile!=='scale')throw Error('Unknown dip profile');const scaled=p.profile==='scale';
 if(p.executionPolicy!==undefined&&p.executionPolicy!=='responsive_v2')throw Error('Unknown dip execution policy');
 if(p.executionPolicy==='responsive_v2'&&(p.maxOpenMarkets>3||p.requireScheduledEntry!==true))throw Error('Responsive cohort exceeds collection capacity');
 if(p.minimumSamples<8||p.minimumSpanMs<1800000||p.maxOpenMarkets>(scaled?100:20)||p.maxPerState>(scaled?4:3)||p.maxPerOffice>(scaled?50:6)||p.maxUnknownIdentity!==1||p.lookbackMs>86400000||p.maximumHoldMs>21600000||amount(p.maxMarketCost)>amount(scaled?'1000.000000':'50.000000'))throw Error('Unsafe dip limits');
 for(const k of ['minimumDip','minimumRelativeDip','maximumDip','minimumZ','maximumSpread','minimumPrice','maximumPrice'])if(!Number.isFinite(p[k])||p[k]<=0)throw Error('Invalid dip signal');
 if(p.minimumDip>=p.maximumDip||p.minimumPrice>=p.maximumPrice||p.maximumPrice>=1||p.maximumSpread>.05||amount(p.takeProfit)<=0n||amount(p.stopLoss)<=0n||amount(p.maxMarketCost)<=0n)throw Error('Invalid dip costs');return p;
}
function appendBook(s,b,p){const list=s.bookHistory[b.exchangeId]||[];if(list.at(-1)?.versionKey===b.versionKey)return;const mid=(Number(amount(b.bids[0].price))+Number(amount(b.asks[0].price)))/2e6;s.bookHistory[b.exchangeId]=[...list,{at:b.sourceAt,receivedAt:b.receivedAt,mid,versionKey:b.versionKey}].filter(x=>Date.parse(x.at)>=Date.parse(b.receivedAt)-p.lookbackMs).slice(-512);}
export function dipSignal(reference,ask,bid,p){
 if(!reference.ready)return {admitted:false,reason:reference.reason,...reference};
 const discount=reference.center-ask,z=discount/reference.robustDeviation;
 const admitted=ask>=p.minimumPrice&&ask<=p.maximumPrice&&ask>bid&&ask-bid<=p.maximumSpread&&discount>=p.minimumDip&&discount/Math.max(reference.center,.001)>=p.minimumRelativeDip&&discount<=p.maximumDip&&z>=p.minimumZ;
 return {...reference,discount,relativeDiscount:discount/Math.max(reference.center,.001),z,ask,bid,admitted,reason:admitted?'historical_discount':'discount_spread_or_regime_gate'};
}
export function diversification(s,bank,marketId){
 const st=s.wallet.strategies.mean_reversion,ids=new Set([...Object.keys(st.positions),...Object.entries(st.pending).filter(([id,p])=>p.action==='buy'&&s.wallet.books[id]?.marketId!==marketId).map(([id])=>id)]),markets=[...ids].map(id=>s.wallet.books[id]?.marketId);
 const current=bank.identities[marketId]||{},meta=markets.map(id=>bank.identities[id]||{}),p=bank.policy;
 if(markets.length>=p.maxOpenMarkets)return {allowed:false,reason:'maximum_open_markets'};
 if(!current.state||!current.office){if(meta.filter(x=>!x.state||!x.office).length>=p.maxUnknownIdentity)return {allowed:false,reason:'unknown_identity_concentration'};}
 if(current.state&&meta.filter(x=>x.state===current.state).length>=p.maxPerState)return {allowed:false,reason:'state_concentration'};
 if(current.office&&meta.filter(x=>x.office===current.office).length>=p.maxPerOffice)return {allowed:false,reason:'office_concentration'};
 return {allowed:true,reason:'within_diversification_limits'};
}
function invalidateDip(s,market,at,reason){invalidate(s.wallet,market,at,reason);for(const [id,p]of Object.entries(s.wallet.strategies.mean_reversion.pending))if(p.action==='buy'&&(market===null||s.wallet.books[id]?.marketId===market)){delete s.wallet.strategies.mean_reversion.pending[id];s.wallet.strategies.mean_reversion.expiredIntents++;}}
export function createDip(raw,c,universe,policy,identities){
 validateConfig(c);validateDipPolicy(policy);const u=validateUniverse(universe,c.tournamentId);if(c.strategyUniverse!==(policy.profile==='scale'?'diversified-dip-scale-v1':'diversified-dip-v1')||c.feeBps!==0||u.exchanges.some(x=>!c.marketIds.includes(x.marketId)||x.option!=='YES'))throw Error('Dip universe scope mismatch');
 const records=verifySource(raw);if(!records?.length)throw Error('Complete history source required');const cutoff=records.at(-1).event.clockAt||records.at(-1).at;
 const bank={schemaVersion:1,name:policy.name,config:c,universe:u,policy,identities,cutoff,sourceRecords:records.length,sourceHash:records.prefixHashes.at(-1)};
 let state={schemaVersion:1,bankHash:digest(bank),sourceSequence:0,lastRecordHash:null,bookHistory:{},histories:{},signals:{},cooldowns:{},wallet:initialState(c,policy.name,cutoff),coverage:null};
 state=advanceDip(state,records,bank);return {bank,state};
}
export function advanceDip(input,records,bank){
 const s=structuredClone(input),c=bank.config,p=bank.policy;validateDipPolicy(p);if(s.bankHash!==digest(bank)||records?.prefixHashes?.[bank.sourceRecords-1]!==bank.sourceHash||s.sourceSequence>records.length||(s.sourceSequence&&records[s.sourceSequence-1]?.hash!==s.lastRecordHash))throw Error('Dip source/bank changed');assertState(s.wallet,c);
 const universe=new Map(bank.universe.exchanges.map(x=>[x.marketId,x]));
 for(const r of records){if(r.sequence<=s.sourceSequence)continue;s.sourceSequence=r.sequence;s.lastRecordHash=r.hash;const e=r.event,at=e.clockAt||r.at,prospective=Date.parse(at)>Date.parse(bank.cutoff);
  if(e.type==='market_history'){const h=e.history;if(h?.tournamentId!==c.tournamentId||universe.get(h.marketId)?.exchangeId!==h.exchangeId||Date.parse(h.receivedAt)>Date.parse(at))throw Error('Historical capture scope mismatch');s.histories[h.exchangeId]=h;continue;}
  if(e.type==='account_read_outage'){if(prospective)invalidateDip(s,null,at,'account_outage');continue;}
  if(e.type!=='cycle')continue;const acceptedFresh=new Set();
  for(const snap of [...(e.sourceSnapshots||[]),...(e.supplementalDepth||[])]){
   if(snap.skipped){if(prospective&&!(snap.skipped==='Missing/stale engine version'&&acceptedFresh.has(snap.marketId)))invalidateDip(s,snap.marketId,snap.at||at,snap.skipped);continue;}
   const id=/^\/markets\/(\d+)\/orderbook\?/.exec(snap.endpoint||'')?.[1],spec=universe.get(id);if(!spec)continue;let b;
   try{if(!Array.isArray(e.accountExclusions))throw Error('Missing reconciled exclusion list');b=normalizeBook(snap.data,id,c.tournamentId,snap.receivedAt,snap.requestStartedAt,c.maxDataAgeMs,new Set(e.accountExclusions),'authoritative_receipt_v1');if(b.exchangeId!==spec.exchangeId)throw Error('Identity changed');acceptedFresh.add(id);}catch{if(prospective)invalidateDip(s,id,snap.receivedAt,'invalid_book');continue;}
   if(prospective&&Date.parse(b.requestStartedAt)>Date.parse(bank.cutoff)){
    const reference=historicalReference(s.histories[b.exchangeId],s.bookHistory[b.exchangeId],b.requestStartedAt,p),ask=Number(amount(b.asks[0].price)+TICK*BigInt(c.slippageTicks))/1e6,bid=Number(amount(b.bids[0].price)-TICK*BigInt(c.slippageTicks))/1e6;
    const signal=dipSignal(reference,ask,bid,p),risk=diversification(s,bank,id),st=s.wallet.strategies.mean_reversion,wasHeld=Boolean(st.positions[b.instrument]),cooldown=Date.parse(s.cooldowns[id]||'')>Date.parse(b.receivedAt);
    s.signals[b.exchangeId]={...signal,marketId:id,exchangeId:b.exchangeId,at:b.receivedAt,risk,cooldown};
    const scheduled=!p.requireScheduledEntry||(Array.isArray(e.entryMarketIds)&&e.entryMarketIds.includes(id));
    s.wallet=stepDip(s.wallet,b,c,{reference:reference.ready?reference.center-Number(TICK*BigInt(c.slippageTicks))/1e6:null,allowEntry:signal.admitted&&risk.allowed&&!cooldown&&scheduled,minimumEdge:p.minimumDip,takeProfit:p.takeProfit,horizonMs:p.maximumHoldMs,maxMarketCost:p.maxMarketCost,executionPolicy:p.executionPolicy});
    if(wasHeld&&!s.wallet.strategies.mean_reversion.positions[b.instrument])s.cooldowns[id]=new Date(Date.parse(b.receivedAt)+p.cooldownMs).toISOString();
   }
   appendBook(s,b,p);
  }
  if(e.bulkPrices)s.coverage={...e.bulkPrices,quotes:e.bulkPrices.quotes};if(prospective)sampleEquity(s.wallet,c,at);
 }
 assertState(s.wallet,c);return s;
}
export function dipDepthPlan(s,bank,coverage,now){
 const priority=new Map(),st=s.wallet.strategies.mean_reversion;
 for(const id of new Set([...Object.keys(st.positions),...Object.keys(st.pending)])){const b=s.wallet.books[id];if(b)priority.set(b.marketId,{marketId:b.marketId,priority:1e6+(now-Date.parse(b.receivedAt))/1000,reason:'position_or_pending_intent'});}
 for(const q of Object.values(coverage?.quotes||{})){if(q.option!=='YES'||q.bestAsk===null||q.bestBid===null||q.crossed||now-Date.parse(q.receivedAt)>600000)continue;const ref=historicalReference(s.histories[q.exchangeId],s.bookHistory[q.exchangeId],new Date(now).toISOString(),bank.policy),sig=dipSignal(ref,q.bestAsk,q.bestBid,bank.policy);if(sig.admitted&&!priority.has(q.marketId)&&diversification(s,bank,q.marketId).allowed)priority.set(q.marketId,{marketId:q.marketId,priority:sig.discount,reason:'historical_discount_lead',exchangeId:q.exchangeId});}
 return [...priority.values()].sort((a,b)=>b.priority-a.priority||a.marketId.localeCompare(b.marketId)).slice(0,2);
}
export function dipReport(s,bank,at,coverage){
 const r=report(s.wallet,bank.config,at),v=r.strategies.mean_reversion;for(const pos of v.positions)pos.label=bank.identities[s.wallet.books[pos.instrument]?.marketId]?.title||'Market '+s.wallet.books[pos.instrument]?.marketId;
 const label=id=>bank.identities[s.wallet.books[id]?.marketId]?.title||'Market '+s.wallet.books[id]?.marketId;
 v.pending=Object.entries(s.wallet.strategies.mean_reversion.pending).map(([instrument,p])=>({...p,instrument,label:label(instrument)}));
 v.recentFills=v.recentFills.map(f=>({...f,label:label(f.instrument)}));
 return {mode:'diversified_historical_dip_paper',updatedAt:at,feeBps:0,bankHash:s.bankHash,cutoff:bank.cutoff,sourceSequence:s.sourceSequence,sourceHash:s.lastRecordHash,observations:r.observations,universeSize:bank.universe.exchanges.length,historiesAvailable:Object.keys(s.histories).length,bookHistoriesAvailable:Object.keys(s.bookHistory).length,signals:s.signals,depthPlan:dipDepthPlan(s,bank,coverage,Date.parse(at)),strategy:v,performance:dipPerformance(s.wallet.strategies.mean_reversion.fills,v.positions,at),policy:bank.policy,config:bank.config,networkRequests:0,competitionOrders:0,profitabilityEstablished:false};
}
if(process.argv[1]&&fileURLToPath(import.meta.url)===process.argv[1]){
 const argIndex=process.argv.indexOf('--name'),name=argIndex<0?'dip':process.argv[argIndex+1];if(!['dip','dip-scale','dip-responsive'].includes(name))throw Error('Unknown dip worker');
 const dir=process.env.SIG_PAPER_DIR||path.join(process.env.LOCALAPPDATA,'SigTradingCup','paper-runtime'),workspace=path.dirname(path.dirname(fileURLToPath(import.meta.url))),release=lock(path.join(dir,name+'-shadow.lock'));process.on('exit',release);
 const bank=JSON.parse(fs.readFileSync(path.join(dir,name+'-bank.json'))),file=path.join(dir,name+'-state.json');let s=JSON.parse(fs.readFileSync(file)),partials=0;
 const stopped=()=>['STOP','DIP_STOP'].some(f=>fs.existsSync(path.join(dir,f)))||fs.existsSync(path.join(workspace,'STOP'));
 const publish=status=>{let coverage;try{coverage=JSON.parse(fs.readFileSync(path.join(dir,'price-coverage.json')));}catch{}atomicJson(path.join(dir,name+'-stats.json'),{...dipReport(s,bank,new Date().toISOString(),coverage),status});};
 try{do{if(stopped())break;const records=verifySource(fs.readFileSync(path.join(dir,'journal.jsonl'),'utf8'));if(!records){if(++partials>3)throw Error('Incomplete source');publish('awaiting_capture');}else{partials=0;s=advanceDip(s,records,bank);atomicJson(file,s);publish('running');}if(process.argv.includes('--once'))break;for(let i=0;i<75;i++){if(stopped())break;await new Promise(r=>setTimeout(r,200));}}while(!stopped());if(stopped())publish('stopped');}catch{publish('failed');process.exitCode=1;}
}
