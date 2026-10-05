import {amount,money,SCALE,TICK} from '../paper/fixed.mjs';
const need=(v,c)=>{if(!v)throw Error('FUNDED_QUOTE_'+c);};
const key=q=>q.policyId+':'+q.marketId+':'+q.createdAt;
// One local counterfactual wallet. No network, account or order endpoint exists.
export function createFundedQuoteWallet({policyId,startAt,selector=()=>true,initialCash='100.000000',maximumCommitted='5.000000',maximumCumulativePurchases='25.000000',maximumMarkets=3}={}){
 const initial=amount(initialCash),cap=amount(maximumCommitted),lifetime=amount(maximumCumulativePurchases),start=Date.parse(startAt);
 need(typeof policyId==='string'&&Number.isFinite(start)&&initial>0n&&cap>0n&&lifetime>0n&&Number.isSafeInteger(maximumMarkets)&&maximumMarkets>0,'CONFIG');
 let cash=initial,reserved=0n,basis=0n,cumulative=0n,realized=0n,closed=0,buys=0,sells=0,paired=0;
 const active=new Map(),fills=[],outcomes=[],counts={selected:0,selectorRejected:0,capitalRejected:0,censored:0};
 function decideQuote(q){
  if(q.policyId!==policyId||Date.parse(q.createdAt)<start)return false;
  need(Array.isArray(q.limitsTicks)&&q.limitsTicks.length===2&&q.limitsTicks.every(v=>Number.isSafeInteger(v)&&v>0&&v<200),'LIMITS');
  need(Date.parse(q.createdAt)<Date.parse(q.activationAt)&&Date.parse(q.activationAt)<Date.parse(q.expiresAt),'CAUSALITY');
  if(selector(structuredClone(q))!==true){counts.selectorRejected++;return false;}
  const cost=q.limitsTicks.reduce((n,v)=>n+BigInt(v)*TICK,0n);
  if(active.size>=maximumMarkets||[...active.values()].some(a=>a.marketId===q.marketId)||cash-reserved<cost||basis+reserved+cost>cap||cumulative+reserved+cost>lifetime){counts.capitalRejected++;return false;}
  const id=key(q);need(!active.has(id),'DUPLICATE_INTENT');
  active.set(id,{...structuredClone(q),costs:q.limitsTicks.map(v=>BigInt(v)*TICK),filled:[null,null],censored:false});reserved+=cost;counts.selected++;return true;
 }
 function onFill(event){
  if(event.policyId!==policyId)return;
  const q=active.get(key(event));need(q,'UNKNOWN_INTENT');const side=event.side==='YES'?0:event.side==='NO'?1:-1;need(side>=0,'SIDE');
  if(q.filled[side]){need(q.filled[side].source.id===event.source.id,'CONFLICTING_FILL');return;}
  need(!q.censored&&event.limitTicks===q.limitsTicks[side]&&Date.parse(event.learnedAt)>=Date.parse(q.activationAt)&&Date.parse(event.learnedAt)<Date.parse(q.expiresAt),'FILL_WINDOW');
  const cost=q.costs[side];need(reserved>=cost&&cash>=cost,'INSUFFICIENT_CASH');reserved-=cost;cash-=cost;basis+=cost;cumulative+=cost;buys++;
  q.filled[side]=structuredClone(event);fills.push({...event,action:'buy',price:money(cost),quantity:1,model:'hypothetical_strict_trade_through_not_proven_execution'});
 }
 function onOutcome(event){
  if(event.policyId!==policyId)return;
  const id=key(event),q=active.get(id);need(q,'UNKNOWN_OUTCOME');const filled=q.filled.filter(Boolean).length;
  if(event.kind==='censored'){
   if(!q.censored){counts.censored++;q.censored=true;outcomes.push({key:id,at:event.resolvedAt,kind:'unknown_execution_path',knownFills:filled});}
   // Missing prints can hide fills on either leg. Keep both known basis and
   // every unresolved reservation; do not release cash or invent a zero result.
   return;
  }
  need(!q.censored&&!event.gapExposed,'UNKNOWN_PATH');
  if(event.kind==='unfilled')need(filled===0&&event.tradeSources.length===0,'UNFILLED_CONFLICT');
  else if(event.kind==='paired'){
   need(filled===2&&event.counterfactualNetTicks===200-q.limitsTicks[0]-q.limitsTicks[1],'PAIR_ACCOUNTING');
   cash+=SCALE;basis-=q.costs[0]+q.costs[1];realized+=SCALE-q.costs[0]-q.costs[1];paired++;closed++;
  }else if(event.kind==='marked'){
   need(filled===1&&Number.isSafeInteger(event.counterfactualNetTicks),'MARK_ACCOUNTING');const side=q.filled[0]?0:1;
   const proceeds=q.costs[side]+BigInt(event.counterfactualNetTicks)*TICK;need(proceeds>=0n&&proceeds<=SCALE,'EXIT_PRICE');
   cash+=proceeds;basis-=q.costs[side];realized+=proceeds-q.costs[side];sells++;closed++;
   fills.push({action:'sell',at:event.resolvedAt,marketId:q.marketId,side:side===0?'YES':'NO',price:money(proceeds),quantity:1,model:'hypothetical_later_depth_markout_not_proven_execution'});
  }else need(false,'OUTCOME_KIND');
  for(let side=0;side<2;side++)if(!q.filled[side])reserved-=q.costs[side];
  active.delete(id);need(cash>=reserved&&basis>=0n&&reserved>=0n,'BALANCE');
  need(cash+basis===initial+realized,'CONSERVATION');
  outcomes.push({key:id,at:event.resolvedAt,kind:event.kind,realizedPnl:money(realized)});
 }
 return{decideQuote,onFill,onOutcome,report:()=>({schemaVersion:1,mode:'funded_strict_through_quote_counterfactual',policyId,startAt,
  initialCash:money(initial),cash:money(cash),reserved:money(reserved),inventoryCost:money(basis),realizedPnl:money(realized),
  fees:'0.000000',cumulativePurchases:money(cumulative),buyFills:buys,sellFills:sells,pairedRedemptions:paired,closedTrades:closed,
  counts,activeMarkets:[...active.values()].map(q=>({marketId:q.marketId,createdAt:q.createdAt,knownSides:q.filled.map((f,i)=>f?(i===0?'YES':'NO'):null).filter(Boolean),uncertain:q.censored})),
  completeEconomicObservation:active.size===0,fills,outcomes,networkRequests:0,competitionOrders:0,executionAuthority:false,profitabilityEstablished:false,
  limitations:['One funded wallet enforces shared cash, reservations and exact one-share basis; counterfactual fills still do not prove actual queue execution.','Censored quote paths retain worst-case reservations and known holdings; their economic outcomes remain unknown.','A final realized result does not include unresolved inventory value and is not prospective competition profitability.']})};
}
