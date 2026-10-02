import {amount,fee,TICK} from '../paper/fixed.mjs';
import {digest,initialState,stepForecast,normalizeBook,invalidate,report,validateConfig} from './model.mjs';
import {verifySource} from './ml-shadow.mjs';
import {scopedSnapshots} from './depth-scope.mjs';
const POLICY='source_probability_lower_bound_paper_v1';
const exact=(v,keys)=>{if(!v||Object.keys(v).sort().join('|')!==[...keys].sort().join('|'))throw Error('Unexpected research fields');};
const time=x=>{if(typeof x!=='string'||!Number.isFinite(Date.parse(x)))throw Error('Invalid research time');return Date.parse(x);};
function validatePolicy(p){if(!p||!Number.isSafeInteger(p.maxResearchAgeMs)||p.maxResearchAgeMs<1000||p.maxResearchAgeMs>86400000||!Number.isSafeInteger(p.maxHoldMs)||p.maxHoldMs<1000||p.maxHoldMs>604800000||amount(p.minimumEdge)<=0n)throw Error('Invalid probability paper policy');}
export function validateResearch(r){
 exact(r,['schemaVersion','id','marketId','exchangeId','side','predicateHash','recordedAt','sourceAt','expiresAt','lower','upper','method','settlementReviewed','flags','sources']);
 if(r.schemaVersion!==1||!/^[-a-zA-Z0-9_]{1,80}$/.test(r.id)||!/^\d+$/.test(r.marketId)||!/^\d+$/.test(r.exchangeId)||r.side!=='YES'||!/^[a-f0-9]{64}$/.test(r.predicateHash)||!['subjective_range','calibrated_model'].includes(r.method)||typeof r.settlementReviewed!=='boolean')throw Error('Invalid research identity');
 if(!/^(0\.\d{6}|1\.000000)$/.test(r.lower)||!/^(0\.\d{6}|1\.000000)$/.test(r.upper)||amount(r.lower)>amount(r.upper))throw Error('Invalid probability interval');
 if(time(r.sourceAt)>time(r.recordedAt)||time(r.expiresAt)<=time(r.recordedAt))throw Error('Invalid research chronology');
 if(!Array.isArray(r.flags)||r.flags.some(f=>!['predicate_uncertain','settlement_uncertain','source_conflict','superseded'].includes(f))||new Set(r.flags).size!==r.flags.length||!Array.isArray(r.sources)||!r.sources.length)throw Error('Invalid research evidence');
 for(const s of r.sources){exact(s,['url','contentHash']);let u;try{u=new URL(s.url);}catch{throw Error('Invalid research reference');}if(u.protocol!=='https:'||u.username||u.password||u.search||u.hash||!/^[a-f0-9]{64}$/.test(s.contentHash)||/ace_[a-f0-9]{20,}/i.test(s.url))throw Error('Invalid research reference');}
 return r;
}
export function researchGate(r,b,c,identity,policy){
 validatePolicy(policy);
 if(!r)return {accepted:false,reason:'missing_research'};validateResearch(r);
 if(r.marketId!==b.marketId||r.exchangeId!==b.exchangeId||r.predicateHash!==identity?.predicateHash||identity.exchangeId!==b.exchangeId)return {accepted:false,reason:'contract_mismatch'};
 if(!r.settlementReviewed||r.flags.length)return {accepted:false,reason:'unresolved_evidence'};
 if(time(r.recordedAt)>time(b.requestStartedAt)||time(r.sourceAt)>time(b.requestStartedAt))return {accepted:false,reason:'not_yet_known'};
 if(time(b.receivedAt)>=time(r.expiresAt)||time(b.receivedAt)-time(r.sourceAt)>policy.maxResearchAgeMs)return {accepted:false,reason:'expired_research'};
 if(time(b.receivedAt)-time(b.requestStartedAt)>c.maxDataAgeMs||time(b.receivedAt)-time(b.sourceAt)>c.maxDataAgeMs||time(b.sourceAt)>time(b.receivedAt))return {accepted:false,reason:'stale_book'};
 const price=amount(b.asks[0].price)+TICK*BigInt(c.slippageTicks),entry=price+fee(price,1,{feeBps:c.feeBps,feePerShare:'0.000000'}),edge=amount(r.lower)-entry;
 return {accepted:edge>=amount(policy.minimumEdge),reason:edge>=amount(policy.minimumEdge)?'lower_bound_edge':'insufficient_edge',researchHash:digest(r),edge:edge.toString(),lower:r.lower};
}
export function stepProbability(input,b,c,research,identity,policy){
 const gate=researchGate(research,b,c,identity,policy),s=structuredClone(input),st=s.strategies.mean_reversion,pending=st.pending[b.instrument];
 // Recheck evidence BEFORE a subsequent snapshot can fill the old buy.
 if(pending?.action==='buy'&&(!gate.accepted||s.researchIntents?.[b.instrument]!==gate.researchHash)){delete st.pending[b.instrument];st.expiredIntents++;}
 const next=stepForecast(s,b,c,{netEdge:gate.accepted?Number(gate.edge)/1e6:null,horizonMs:policy.maxHoldMs,minimumEdge:Number(amount(policy.minimumEdge))/1e6});
 next.researchIntents??={};if(next.strategies.mean_reversion.pending[b.instrument]?.action==='buy')next.researchIntents[b.instrument]=gate.researchHash;
 next.researchDecision={at:b.receivedAt,marketId:b.marketId,researchId:research?.id||null,...gate};return next;
}
export function createProbabilityCohort(raw,c,{name,identities,research,policy}){
 validateConfig(c);validatePolicy(policy);if(!/^[-a-z0-9]{1,80}$/.test(name)||!Array.isArray(research))throw Error('Invalid probability paper policy');
 const records=verifySource(raw);if(!records?.length)throw Error('Complete source required');const last=records.at(-1),cutoff=last.event.clockAt||last.at;
 for(const [m,id]of Object.entries(identities)){exact(id,['exchangeId','predicateHash']);if(!c.marketIds.includes(m)||!/^\d+$/.test(id.exchangeId)||!/^[a-f0-9]{64}$/.test(id.predicateHash))throw Error('Invalid contract binding');}
 const seen=new Set();for(const r of research){validateResearch(r);if(seen.has(r.marketId)||time(r.recordedAt)>time(cutoff)||r.predicateHash!==identities[r.marketId]?.predicateHash||r.exchangeId!==identities[r.marketId]?.exchangeId)throw Error('Invalid frozen research binding');seen.add(r.marketId);}
 const bank={schemaVersion:1,policy:POLICY,name,config:structuredClone(c),identities:structuredClone(identities),research:structuredClone(research),parameters:structuredClone(policy),cutoff,sourceRecords:records.length,sourceHash:records.prefixHashes.at(-1)};
 return {bank,state:{schemaVersion:1,bankHash:digest(bank),sourceSequence:records.length,lastRecordHash:last.hash,wallet:initialState(c,name,cutoff),decisions:[],skips:{}}};
}
export function advanceProbability(input,raw,bank){
 const records=verifySource(raw),s=structuredClone(input),c=bank.config;
 if(!records||bank.policy!==POLICY||s.bankHash!==digest(bank)||records.prefixHashes[bank.sourceRecords-1]!==bank.sourceHash||records[s.sourceSequence-1]?.hash!==s.lastRecordHash)throw Error('Probability cohort/source changed');
 for(const r of records){if(r.sequence<=s.sourceSequence)continue;const e=r.event,at=e.clockAt||r.at;s.sourceSequence=r.sequence;s.lastRecordHash=r.hash;
  const revoke=market=>{for(const [id,p]of Object.entries(s.wallet.strategies.mean_reversion.pending))if(p.action==='buy'&&(market===null||s.wallet.books[id]?.marketId===market)){delete s.wallet.strategies.mean_reversion.pending[id];s.wallet.strategies.mean_reversion.expiredIntents++;}};
  if(e.type==='account_read_outage'){invalidate(s.wallet,null,at,'account_outage');revoke(null);continue;}
  if(e.type!=='cycle')continue;
  for(const snap of scopedSnapshots(e,c)){
   if(snap.skipped){invalidate(s.wallet,snap.marketId,snap.at||at,snap.skipped);revoke(snap.marketId);continue;}
   const m=/^\/markets\/(\d+)\/orderbook\?/.exec(snap.endpoint||'');if(!m||!c.marketIds.includes(m[1])||time(snap.requestStartedAt)<=time(bank.cutoff))continue;
   let b;try{b=normalizeBook(snap.data,m[1],c.tournamentId,snap.receivedAt,snap.requestStartedAt,c.maxDataAgeMs,new Set(e.accountExclusions));}catch{invalidate(s.wallet,m[1],snap.receivedAt,'invalid_book');revoke(m[1]);s.skips.invalid_book=(s.skips.invalid_book||0)+1;continue;}
   const signal=bank.research.find(x=>x.marketId===b.marketId);s.wallet=stepProbability(s.wallet,b,c,signal,bank.identities[b.marketId],bank.parameters);s.decisions.push(s.wallet.researchDecision);
  }
 }
 return s;
}
export function probabilityReport(state,bank,at){return {mode:'source_probability_paper_only',bankHash:state.bankHash,sourceSequence:state.sourceSequence,lastRecordHash:state.lastRecordHash,cutoff:bank.cutoff,decisions:state.decisions,skips:state.skips,wallet:report(state.wallet,bank.config,at).strategies.mean_reversion,competitionOrders:0,networkRequests:0,profitabilityEstablished:false,limitations:['Research intervals are supplied estimates, not verified calibrated probabilities','Settlement lower-bound edge is a selection heuristic, not a guaranteed horizon exit price','Forced horizon/risk exits use subsequent sampled depth and FIFO; open inventory retains loss and unavailable liquidation risk','Source references/hashes and settlement review are supplied attestations; this adapter does not authenticate their truth']};}
