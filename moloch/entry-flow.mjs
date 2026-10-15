import {amount,money} from '../paper/fixed.mjs';import {instantNs} from '../feed/contract.mjs';
const ms=x=>Number(instantNs(x)/1000000n),canonical=x=>money(amount(x));
export function observeEntryFlow(state,event,duplicate=false){
 if(!state.policy.requireCurrentFlow||duplicate)return;
 state.entryFlow??={};
 const id=event.book?.exchangeId??event.exchangeId;
 if(event.type==='gap'){for(const ex of event.exchangeIds??[id])delete state.entryFlow[ex];return;}
 if(!['book','trade'].includes(event.type))return;
 const m=state.entryFlow[id]??={history:[],flow:[]};
 if(event.type==='book'){
  const b=event.book;if(b.invalid){m.history=[];m.flow=[];return;}
  const bid=canonical(b.bids[0].price),ask=canonical(b.asks[0].price),old=m.history.at(-1),received=ms(event.receivedAt);
  m.history.push({bid,ask,sourceAt:b.sourceAt,received,bidSince:old?.bid===bid?old.bidSince:received,askSince:old?.ask===ask?old.askSince:received});
  m.history=m.history.slice(-128);return;
 }
 if(event.idNamespace!=='sig_engine_match')return;
 const executed=instantNs(event.executedAt),at=ms(event.executedAt),prior=[...m.history].reverse().find(b=>instantNs(b.sourceAt)<executed&&b.received<=at&&at-ms(b.sourceAt)<=10000);
 if(!prior)return;
 const value=amount(event.yesPrice),side=value<=amount(prior.bid)?'bid':value>=amount(prior.ask)?'ask':null;
 if(!side||at-prior[side+'Since']<state.policy.latencyMs)return;
 m.flow.push({at,side,price:prior[side]});m.flow=m.flow.filter(t=>t.at>=at-180000).slice(-2048);
}
export function currentFlowReady(state,book,at){
 if(!state.policy.requireCurrentFlow)return true;
 const m=state.entryFlow?.[book.exchangeId],latest=m?.history.at(-1);if(!latest)return false;
 const bid=canonical(book.bids[0].price),ask=canonical(book.asks[0].price);
 if(latest.bid!==bid||latest.ask!==ask)return false;
 const valid=t=>t.at>=at-180000&&t.at<=at&&t.at>=latest[t.side+'Since']&&t.price===(t.side==='bid'?bid:ask);
 return m.flow.some(t=>t.side==='bid'&&valid(t))&&m.flow.some(t=>t.side==='ask'&&valid(t));
}
