import {amount} from '../paper/fixed.mjs';
export const MAX_CONTEXT_AGE_MS=300000;
export function updateContext(input,snapshot,clockAt){
 const state=structuredClone(input||{tournamentId:null,quotes:{}});
 if(!snapshot)return state;
 if(snapshot.executable!==false||snapshot.depthAvailable!==false||!Number.isFinite(Date.parse(snapshot.receivedAt))||!Array.isArray(snapshot.quotes)||!Array.isArray(snapshot.missingIds))throw Error('Invalid breadth feature context');
 if(clockAt&&Date.parse(snapshot.receivedAt)>Date.parse(clockAt))throw Error('Future breadth context');
 if(state.tournamentId!==null&&state.tournamentId!==snapshot.tournamentId)throw Error('Breadth context scope changed');state.tournamentId=snapshot.tournamentId;
 const seen=new Set();
 for(const id of snapshot.missingIds){if(typeof id!=='string'||!/^\d+$/.test(id)||seen.has(id))throw Error('Invalid breadth context IDs');seen.add(id);delete state.quotes[id];}
 for(const q of snapshot.quotes){
  if(typeof q.exchangeId!=='string'||typeof q.marketId!=='string'||!/^\d+$/.test(q.exchangeId)||!/^\d+$/.test(q.marketId)||seen.has(q.exchangeId))throw Error('Invalid breadth context identity');seen.add(q.exchangeId);
  for(const value of [q.bestBid,q.bestAsk])if(value!==null&&(typeof value!=='number'||!Number.isFinite(value)||value<0||value>1))throw Error('Invalid breadth context price');
  const previous=state.quotes[q.exchangeId],at=Date.parse(snapshot.receivedAt);
  if(previous&&at<=Date.parse(previous.receivedAt))throw Error('Breadth context time rollback');
  const mid=q.bestBid!==null&&q.bestAsk!==null&&q.bestBid<q.bestAsk?(q.bestBid+q.bestAsk)/2:null;
  const prior=previous&&at-Date.parse(previous.receivedAt)<=MAX_CONTEXT_AGE_MS?previous:null;
  state.quotes[q.exchangeId]={marketId:q.marketId,mid,spread:mid===null?null:q.bestAsk-q.bestBid,change:mid!==null&&prior?.mid!==null&&prior?.mid!==undefined?mid-prior.mid:null,receivedAt:snapshot.receivedAt};
 }
 return state;
}
export function contextFeatures(state,book,tournamentId){
 if(state?.tournamentId&&state.tournamentId!==tournamentId)throw Error('Breadth feature tournament mismatch');
 const clock=Date.parse(book.requestStartedAt);
 const available=Object.entries(state?.quotes||{}).filter(([,q])=>{const age=clock-Date.parse(q.receivedAt);return age>=0&&age<=MAX_CONTEXT_AGE_MS&&q.mid!==null;});
 const own=available.find(([id,q])=>id===book.exchangeId&&q.marketId===book.marketId)?.[1];
 const depthMid=(Number(amount(book.bids[0].price))+Number(amount(book.asks[0].price)))/2e6;
 const changes=available.map(([,q])=>q.change).filter(x=>x!==null),absoluteChange=changes.length?changes.reduce((n,x)=>n+Math.abs(x),0)/changes.length:0;
 // Collection age is not quote-source age. Missing own context has an explicit
 // indicator; these features never provide execution depth or a fill price.
 return [own?own.mid-depthMid:0,own?.spread||0,absoluteChange,changes.length?changes.filter(x=>x!==0).length/changes.length:0,Math.min(1,available.length/237),own?(clock-Date.parse(own.receivedAt))/MAX_CONTEXT_AGE_MS:1,own?0:1];
}
