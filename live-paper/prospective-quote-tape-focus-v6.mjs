import {prospectiveQuoteTapeFocusCoreV2} from './prospective-quote-tape-focus-core-v2.mjs';
import {amount,price} from '../paper/fixed.mjs';
export function prospectiveQuoteTapeFocusWithInventoryV1(input){
 const base=prospectiveQuoteTapeFocusCoreV2(input),risk=new Set(base.riskMarketIds),inventory=new Set(),currentQuotes=[];
 for(const c of input.result.cohorts)for(const q of c.unresolvedQuotes){const end=Math.min(q.expiresAt,q.cancelEffectiveAt??Infinity);if(end>(input.now??Date.now()))currentQuotes.push({marketId:q.marketId,until:end});}
 for(const c of input.result.cohorts)for(const [exchangeId,p]of Object.entries(c.report?.positions??{})){
  const member=input.universe.exchanges.find(x=>x.exchangeId===exchangeId);
  if(!member||!['YES','NO'].includes(p.side)||!Number.isSafeInteger(p.quantity)||p.quantity<1||!Array.isArray(p.lots)||p.lots.some(l=>!Number.isSafeInteger(l.quantity)||l.quantity<1)||p.lots.reduce((n,l)=>n+l.quantity,0)!==p.quantity||p.lots.reduce((n,l)=>n+price(l.price)*BigInt(l.quantity),0n)!==amount(p.cost))throw Error('PROSPECTIVE_TAPE_OWNED_INVENTORY');
  risk.add(member.marketId);inventory.add(member.marketId);
 }
 return{...base,riskMarketIds:[...risk],inventoryMarketIds:[...inventory],currentQuotes};
}
export function createProspectiveQuoteTapeFocusV6({universe}){
 const known=new Map();return{update(entries,{now=Date.now()}={}){
  const unavailable=[],seen=new Set();for(const e of entries){if(!/^[a-z0-9-]{1,80}$/.test(e.id)||seen.has(e.id))throw Error('MULTI_TAPE_COHORT_ID');seen.add(e.id);try{known.set(e.id,prospectiveQuoteTapeFocusWithInventoryV1({bank:e.bank,result:e.result,universe,now}));}catch{unavailable.push(e.id);}}
  const allStates=[...known.values()],offset=allStates.length?Math.floor(now/60000)%allStates.length:0,states=[...allStates.slice(offset),...allStates.slice(0,offset)],risk=[...new Set(states.flatMap(s=>s.riskMarketIds))],windows=[],keys=new Set();for(let i=0;i<4&&windows.length<4;i++)for(const s of states){const r=s.recoveryWindows[i];if(r&&!keys.has(r.key)){keys.add(r.key);windows.push(r);if(windows.length===4)break;}}
  return{riskMarketIds:risk,inventoryMarketIds:[...new Set(states.flatMap(s=>s.inventoryMarketIds))],currentMarketIds:[...new Set(states.flatMap(s=>[...s.inventoryMarketIds,...s.currentQuotes.filter(q=>q.until>now).map(q=>q.marketId)]))],recoveryWindows:windows,cohortsTracked:known.size,unavailableCohorts:unavailable,knownLiabilitiesRetained:unavailable.some(id=>known.has(id)),maximumRecoveryWindows:4,cohortRotationOffset:offset,extraReadSlots:0,executionAuthority:false};
 }};
}
