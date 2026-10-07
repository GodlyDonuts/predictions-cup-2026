// Read allocation only. It cannot adopt holdings, authorize trades or make a
// stale book executable. Inventory comes from the current attributed controller.
export function genericFinancialMarketIds(status,universe){
 const c=status?.controller;if(c?.ownerId!=='Codex'||c.tournamentId!==universe.tournamentId||!c.inventory)return[];
 const members=new Map(universe.exchanges.map(x=>[x.exchangeId,x.marketId])),ids=[];
 for(const [exchangeId,p]of Object.entries(c.inventory)){
  if(!Number.isSafeInteger(p.quantity)||p.quantity<0||members.get(exchangeId)!==p.marketId||!/^[a-f0-9]{64}$/.test(p.proofHash??''))throw Error('GENERIC_FINANCIAL_FOCUS_SCOPE');
  if(p.quantity>0)ids.push(p.marketId);
 }
 if(c.intent){if(members.get(c.intent.exchangeId)!==c.intent.marketId)throw Error('GENERIC_FINANCIAL_FOCUS_INTENT_SCOPE');ids.push(c.intent.marketId);}
 return [...new Set(ids)];
}
export function focusGenericFinancialBooks(plan,status,universe,lastAttempts={}){
 const ids=genericFinancialMarketIds(status,universe);if(!ids.length)return plan;
 const budget=plan.reads.length;if(budget<1||budget>4)throw Error('GENERIC_FINANCIAL_FOCUS_BUDGET');
 const priority=[...ids].sort((a,b)=>(lastAttempts[a]??0)-(lastAttempts[b]??0)||Number(a)-Number(b)),byMarket=new Map(plan.reads.map(r=>[r.marketId,r])),selected=[];
 for(const marketId of [...priority,...plan.reads.map(r=>r.marketId)]){if(selected.some(r=>r.marketId===marketId)||selected.length===budget)continue;const member=universe.exchanges.find(x=>x.marketId===marketId);if(!member)throw Error('GENERIC_FINANCIAL_FOCUS_MARKET');selected.push(byMarket.get(marketId)??{...member,path:'supplementalDepth',reason:'attributed_financial_risk'});}
 return{...plan,reads:selected,genericFinancialFocus:{marketIds:ids,budget,executionAuthority:false}};
}

