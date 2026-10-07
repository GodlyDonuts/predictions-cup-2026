import {tradeTapeFocus} from './trade-tape-focus.mjs';
// Rotating matched-book groups use the existing tape slot, never add requests.
export function matchedTradeTapeFocusV2(universe,{activeBanks=[],riskMarketIds=[],recoveryWindows=[],publication=null,financialMarketIds=[],now=Date.now()}={}){
 const members=new Map(universe.exchanges.map(x=>[x.marketId,x.exchangeId]));const lacksDefault=!activeBanks.length&&!riskMarketIds.length&&!recoveryWindows.length&&!['363','297','381','382'].some(id=>members.has(id)),base=tradeTapeFocus(universe,{activeBanks,riskMarketIds:lacksDefault?[universe.exchanges[0]?.marketId]:riskMarketIds,recoveryWindows});for(const id of financialMarketIds)if(!members.has(id))throw Error('MATCHED_TAPE_FINANCIAL_SCOPE');
 const recent=[];if(publication&&!publication.closed&&publication.tournamentId===universe.tournamentId)for(const b of Object.values(publication.books??{})){const start=Date.parse(b.requestStartedAt),end=Date.parse(b.receivedAt);if(members.get(b.marketId)!==b.exchangeId)throw Error('MATCHED_TAPE_BOOK_SCOPE');if(Number.isFinite(start)&&start<=end&&end<=now&&now-start<=120000)recent.push(b.marketId);}
 const available=[...new Set(recent)].sort((a,b)=>Number(a)-Number(b));if(!available.length)return{...base,matchedBookSelection:{enabled:false,reason:'NO_FRESH_BOOKS',extraRequests:0}};
 const phase=Math.floor(now/600000),offset=(phase*4)%available.length,group=Array.from({length:Math.min(4,available.length)},(_,i)=>available[(offset+i)%available.length]);
 const urgent=[...new Set([...riskMarketIds,...recoveryWindows.map(x=>x.marketId)])],active=activeBanks.flatMap(b=>b.legs.map(l=>l.marketId));
 return{marketIds:[...new Set([...urgent,...active,...available,...financialMarketIds])],priorityMarketIds:[...new Set([...urgent,...group])],recoveryWindows,matchedBookSelection:{enabled:true,phase,group,freshBookMarkets:available.length,financialMarkets:financialMarketIds.length,extraRequests:0,maximumNewFocusMarkets:4}};
}
