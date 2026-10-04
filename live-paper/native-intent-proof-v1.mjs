import {verifyNativeDipResult,micros} from './native-dip-proof-v1.mjs';
const need=(v,c)=>{if(!v)throw Error('NATIVE_INTENT_'+c);},count=n=>Number.isSafeInteger(n)&&n>=0,integer=s=>typeof s==='string'&&/^(?:0|[1-9][0-9]*)$/.test(s),instant=s=>{const n=Date.parse(s);need(Number.isSafeInteger(n)&&new Date(n).toISOString()===s,'CLOCK');return n;};
const fields=['instrument','marketId','intentId','action','quantity','limit','decisionAt','minimumRequestStartedAt','expiresAt','decisionSequence','decisionSourceNs','reason'];
export function verifyNativeIntentResult(input){
 const projection=verifyNativeDipResult(input),{result,declaration,universe}=input,e=result.evaluation,cfg=declaration.config,start=instant(declaration.declaredAt),through=instant(e.evaluationThrough),scope=new Map(universe.exchanges.map(x=>[x.exchangeId+':YES',x.marketId])),all=[],pendingMarkets=new Set();
 need(e.intentObservationSchemaVersion===1,'SCHEMA');
 for(const card of e.scorecards){const point=card.point,rows=card.pendingReadIntents;need(Array.isArray(rows)&&count(point.pendingIntents)&&rows.length===point.pendingIntents&&count(point.intents),'COUNT');
  const held=new Map();for(const f of card.fills)held.set(f.instrument,(held.get(f.instrument)||0)+(f.action==='buy'?f.quantity:-f.quantity));
  const instruments=new Set(),ids=new Set(),filled=new Set(card.fills.map(f=>String(f.intentId)));
  for(const row of rows){need(row&&typeof row==='object'&&Object.keys(row).length===fields.length&&fields.every(k=>Object.hasOwn(row,k)),'FIELDS');const market=scope.get(row.instrument);
   need(market===row.marketId&&cfg.marketIds.includes(market)&&!instruments.has(row.instrument),'SCOPE');instruments.add(row.instrument);
   need(integer(row.intentId)&&BigInt(row.intentId)>0n&&BigInt(row.intentId)<=BigInt(point.intents)&&!ids.has(row.intentId)&&!filled.has(row.intentId),'ID');ids.add(row.intentId);
   need(['buy','sell'].includes(row.action)&&Number.isSafeInteger(row.quantity)&&row.quantity>0&&row.quantity<=cfg.maxPositionShares,'QUANTITY');
   if(row.action==='buy')need(row.reason==='ENTRY'&&row.quantity===cfg.orderShares&&(held.get(row.instrument)||0)===0,'ENTRY');else need(['TAKE_PROFIT','STOP_LOSS','HORIZON'].includes(row.reason)&&row.quantity===(held.get(row.instrument)||0),'EXIT');
   const limit=micros(row.limit),upper=row.action==='buy'?1000000n+BigInt(card.scenario.slippageTicks)*5000n:1000000n+micros(card.parameters.takeProfit);need(limit>0n&&limit<=upper&&limit%5000n===0n,'LIMIT');
   const decision=instant(row.decisionAt),earliest=instant(row.minimumRequestStartedAt),expiry=instant(row.expiresAt);need(decision>start&&decision<=through&&earliest===decision+card.scenario.latencyMs&&expiry===decision+cfg.maxDataAgeMs+cfg.cycleIntervalMs*4,'CLOCK');
   need(integer(row.decisionSequence)&&integer(row.decisionSourceNs),'VERSION');const source=BigInt(row.decisionSourceNs);need(source<=BigInt(decision+2000)*1000000n&&source>=BigInt(decision-cfg.maxDataAgeMs)*1000000n,'SOURCE_CLOCK');
   // An old unrefreshed intent remains in frozen accounting. It cannot request
   // an entry confirmation after expiry; inventory remains protected separately.
   const active=expiry>=through;if(active)pendingMarkets.add(market);
   all.push({...row,card:[card.strategy,card.scenario.id,card.exitPolicy].join('/'),active,executionAuthority:false});
  }
 }
 const unique=new Map();for(const row of all){const {card,...identity}=row,key=JSON.stringify(identity);if(!unique.has(key))unique.set(key,{...identity,cards:[]});unique.get(key).cards.push(card);}
 return{...projection,intentObservationSchemaVersion:1,pendingMarketIds:[...pendingMarkets].sort((a,b)=>Number(a)-Number(b)),pendingReadIntents:[...unique.values()],readOnly:true};
}
export function nativeIntentReadFocus(projection,now=Date.now()){
 const held=new Set(projection.heldMarketIds),pending=new Set();for(const row of projection.pendingReadIntents)if(Date.parse(row.expiresAt)>=now&&Date.parse(row.minimumRequestStartedAt)<=now&&row.active===true)pending.add(row.marketId);
 return{heldMarketIds:[...held],pendingMarketIds:[...pending],executionAuthority:false};
}
