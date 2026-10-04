import {planBookReads} from './book-scheduler.mjs';
const need=(ok,c)=>{if(!ok)throw Error('NATIVE_DISCOVERY_'+c);},id=x=>typeof x==='string'&&/^[1-9][0-9]*$/.test(x);
export function recordNativeDiscoveryAttempt(state,marketId){need(id(marketId),'MARKET');const n=state.nativeDiscoveryAttempts??0;need(Number.isSafeInteger(n)&&n>=0&&Number.isSafeInteger(n+1),'LEDGER');return{...state,nativeDiscoveryAttempts:n+1};}
export function planNativeDiscoveryBooks(state,scope,inputs,{screenedMarketIds=[],excludedMarketIds=[],ownedRiskMarketIds=[]}={}){
 const members=new Map(scope.universe.exchanges.map(x=>[x.marketId,x])),blocked=new Set(excludedMarketIds);need(Array.isArray(screenedMarketIds)&&screenedMarketIds.every(x=>members.has(x))&&Array.isArray(excludedMarketIds)&&excludedMarketIds.every(x=>members.has(x)),'SCOPE');const ordinal=state.nativeDiscoveryAttempts??0;need(Number.isSafeInteger(ordinal)&&ordinal>=0&&Number.isSafeInteger(ordinal+1),'LEDGER');
 const held=new Set([...(inputs.heldMarketIds||[]),...(inputs.priorityManagedMarketIds||[])]);need(Array.isArray(ownedRiskMarketIds)&&ownedRiskMarketIds.every(x=>members.has(x)&&held.has(x)),'OWNED_RISK');const reduceOnly=new Set(ownedRiskMarketIds),readInputs={...inputs,excludedMarketIds:(inputs.excludedMarketIds||[]).filter(x=>!reduceOnly.has(x))};
 const plan=planBookReads(state,scope,readInputs,{comparisonSlots:0}),reads=plan.reads.map(x=>({...x})),optional=reads.findIndex(x=>x.reason==='discovery');
 if(optional<0)return{...plan,nativeDiscovery:{mode:'native_market_discovery_v1',reason:'protected_budget_full',requestsAdded:0,executionAuthority:false}};
 const reservedFair=(ordinal+1)%4===0,others=new Set(reads.filter((_,i)=>i!==optional).map(x=>x.marketId));
 const eligible=[...new Set(screenedMarketIds)].filter(x=>!blocked.has(x)&&!others.has(x)).sort((a,b)=>(state.bookLastAttempt?.[a]||0)-(state.bookLastAttempt?.[b]||0)||screenedMarketIds.indexOf(a)-screenedMarketIds.indexOf(b));
 if(!reservedFair&&eligible.length){const marketId=eligible[0],spec=members.get(marketId);reads[optional]={...spec,path:scope.coreMarketIds.includes(marketId)?'sourceSnapshots':'supplementalDepth',reason:'discovery'};}
 reads[optional].nativeDiscovery=true;
 return{...plan,reads,nativeDiscovery:{mode:'native_market_discovery_v1',reason:!reservedFair&&eligible.length?'screened_depth_candidate':'persisted_fair_discovery',ordinal:ordinal+1,marketId:reads[optional].marketId,screenedCandidates:eligible.length,reservedFair,requestsAdded:0,executionAuthority:false}};
}
