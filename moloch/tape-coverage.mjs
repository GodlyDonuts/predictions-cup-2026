import {receiptEvents} from './sig-source.mjs';import {instantNs} from '../feed/contract.mjs';import {payloadHash} from '../execution/ledger.mjs';
const check=(ok,code)=>{if(!ok)throw Error('MOLOCH_TAPE_'+code);};
export class TapeCoverage{
 constructor(){this.pending=new Map();}
 row(r){
  if(r.kind!=='trades')return[];receiptEvents(r);
  const u=new URL(r.endpoint,'https://sig.thesuper.market/api/v1'),cursor=u.searchParams.get('cursor');u.searchParams.delete('cursor');const key=[r.tournamentId,r.marketId,r.exchangeId,u.pathname,u.searchParams.get('from'),u.searchParams.get('to')].join(':');
  const from=u.searchParams.get('from'),to=u.searchParams.get('to');check(from&&to&&instantNs(r.data.from)===instantNs(from)&&instantNs(r.data.to)===instantNs(to),'SERVED_WINDOW');
  const p=r.data.pagination,c=r.data.coverage;check(p&&typeof p.hasMore==='boolean'&&(p.hasMore?typeof p.nextCursor==='string'&&p.nextCursor.length>0:p.nextCursor===null),'PAGE_CURSOR');
  if(!cursor){if(!p.hasMore)return[];if(this.pending.size>=64)this.pending.delete(this.pending.keys().next().value);this.pending.set(key,{next:p.nextCursor,cursors:new Set(),minimumSequence:null,complete:true,pages:0,proofs:[]});}
  const window=this.pending.get(key);if(!window)return[];
  if(cursor)check(cursor===window.next&&!window.cursors.has(cursor),'CURSOR_CHAIN');if(cursor)window.cursors.add(cursor);check(++window.pages<=5,'PAGE_BOUND');
  window.complete&&=c?.complete===true&&Number.isSafeInteger(c.projectedThroughSequence);if(Number.isSafeInteger(c?.projectedThroughSequence))window.minimumSequence=window.minimumSequence===null?c.projectedThroughSequence:Math.min(window.minimumSequence,c.projectedThroughSequence);
  window.proofs.push(payloadHash(r));
  if(p.hasMore){check(!window.cursors.has(p.nextCursor)&&p.nextCursor!==cursor,'CURSOR_LOOP');window.next=p.nextCursor;return[];}
  this.pending.delete(key);if(!window.complete)return[];
  return[{type:'coverage',tournamentId:r.tournamentId,marketId:r.marketId,exchangeId:r.exchangeId,receivedAt:r.receivedAt,from:r.data.from,to:r.data.to,complete:true,projectedThroughSequence:String(window.minimumSequence),sourcePagination:{pages:window.pages,receiptHashes:window.proofs,checkpointUse:'minimum verified page projection sequence'}}];
 }
}
