// Membership belongs to the validated universe, not to the first experiment
// that happened to use the tape. This changes one slot's selection, not its budget.
export function tradeTapeFocus(universe,{activeBanks=[],riskMarketIds=[],recoveryWindows=[]}={}){
 const scope=new Map(universe.exchanges.map(x=>[x.marketId,x.exchangeId]));
 const need=(ok)=>{if(!ok)throw Error('TRADE_TAPE_FOCUS_SCOPE');};
 const active=[];
 for(const bank of activeBanks)for(const leg of bank.legs){need(scope.get(leg.marketId)===leg.exchangeId);active.push(leg.marketId);}
 for(const id of riskMarketIds)need(scope.has(id));
 for(const r of recoveryWindows)need(scope.get(r.marketId)===r.exchangeId);
 const priorityMarketIds=[...new Set(riskMarketIds.length?riskMarketIds:active)];
 let marketIds=[...new Set([...active,...riskMarketIds,...recoveryWindows.map(r=>r.marketId)])];
 if(!marketIds.length)marketIds=['363','297','381','382'].filter(id=>scope.has(id));
 need(marketIds.length>0);
 return{marketIds,priorityMarketIds,recoveryWindows};
}
