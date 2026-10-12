import {matchedTradeTapeFocusV2} from './matched-trade-tape-focus-v2.mjs';
// Watches replace optional rotation, never existing inventory/recovery priority.
export function matchedTradeTapeFocusV3(universe,input={}){
 const base=matchedTradeTapeFocusV2(universe,input),members=new Map(universe.exchanges.map(x=>[x.marketId,x.exchangeId])),watch=input.watchMarketIds??[];
 if(!Array.isArray(watch)||watch.length>2||new Set(watch).size!==watch.length||watch.some(id=>!members.has(id)))throw Error('MATCHED_TAPE_WATCH_SCOPE');
 const fresh=watch.filter(id=>Object.values(input.publication?.books??{}).some(b=>b.marketId===id&&b.exchangeId===members.get(id)&&Date.parse(b.requestStartedAt)<=Date.parse(b.receivedAt)&&Date.parse(b.receivedAt)<=input.now&&input.now-Date.parse(b.requestStartedAt)<=120000));
 if(!fresh.length)return base;
 const urgent=[...new Set([...(input.riskMarketIds??[]),...(input.recoveryWindows??[]).map(x=>x.marketId),...(input.financialMarketIds??[])])];
 return{...base,marketIds:[...new Set([...base.marketIds,...fresh])],priorityMarketIds:[...new Set([...urgent,...fresh])],matchedBookSelection:{...base.matchedBookSelection,watchMarketIds:fresh,group:fresh,extraRequests:0,existingRiskRetained:true}};
}
