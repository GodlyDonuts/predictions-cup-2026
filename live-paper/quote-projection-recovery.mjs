import {instant} from '../paper/fixed.mjs';
import {payloadHash} from '../execution/ledger.mjs';
const need=(ok,c)=>{if(!ok)throw Error('QUOTE_RECOVERY_'+c);};
const id=x=>typeof x==='string'&&/^[1-9]\d*$/.test(x)&&Number.isSafeInteger(Number(x));
export function validateQuoteRecoveryRequest(r,scope,now){
 need(r&&Object.keys(r).sort().join('|')==='exchangeId|from|key|kind|marketId|requiredCheckpoint|schemaVersion|to','SHAPE');
 need(r.schemaVersion===1&&r.kind==='expired_paper_quote_coverage'&&id(r.marketId)&&id(r.exchangeId)&&scope.exchanges.some(x=>x.marketId===r.marketId&&x.exchangeId===r.exchangeId),'SCOPE');
 const from=instant(r.from),to=instant(r.to),at=typeof now==='string'?instant(now):now;
 need(Number.isSafeInteger(at)&&from<to&&to<=at&&to-from<=120000&&at-from<=7*86400000,'WINDOW');
 need(typeof r.requiredCheckpoint==='string'&&/^(0|[1-9]\d*)$/.test(r.requiredCheckpoint)&&Number.isSafeInteger(Number(r.requiredCheckpoint)),'CHECKPOINT');
 const {key,...body}=r;need(key===payloadHash(body),'HASH');return r;
}
export function quoteProjectionRecoveryRequests(state,bank,now){
 const at=instant(now),requests=new Map();
 for(const w of Object.values(state.wallets))for(const q of Object.values(w.quotes)){
  if(q.resolvedAt||q.coverageBarrierSequence===null||instant(q.expiresAt)>at||instant(q.activationAt)>=instant(q.expiresAt))continue;
  const body={schemaVersion:1,kind:'expired_paper_quote_coverage',marketId:q.marketId,exchangeId:q.exchangeId,from:q.activationAt,to:q.expiresAt,requiredCheckpoint:q.coverageBarrierSequence},request={...body,key:payloadHash(body)};
  try{validateQuoteRecoveryRequest(request,{exchanges:bank.legs},at);requests.set(request.key,request);}catch{}
 }
 return [...requests.values()].sort((a,b)=>instant(a.from)-instant(b.from)||Number(a.marketId)-Number(b.marketId)).slice(0,4);
}
