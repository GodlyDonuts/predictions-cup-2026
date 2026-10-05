import fs from 'node:fs';import path from 'node:path';
import {payloadHash} from '../execution/ledger.mjs';
import {digest} from './model.mjs';

export function readFundamentalEntryDrain(runtimeDir,bank,filesystem=fs){
 const file=path.join(runtimeDir,'FUNDAMENTAL_ENTRY_DRAIN');if(!filesystem.existsSync(file))return null;
 const v=JSON.parse(filesystem.readFileSync(file,'utf8'));
 if(!v||v.schemaVersion!==1||v.owner!=='Codex'||v.cohort!==bank.name||v.bankHash!==digest(bank)
  ||!Number.isFinite(Date.parse(v.at))||!/^[A-Z][A-Z0-9_]{1,79}$/.test(v.reasonCode||'')
  ||!Array.isArray(v.sourceEvidenceHashes)||!v.sourceEvidenceHashes.length||v.sourceEvidenceHashes.some(x=>! /^[a-f0-9]{64}$/.test(x))
  ||Object.keys(v).some(k=>!['schemaVersion','owner','cohort','bankHash','at','reasonCode','sourceEvidenceHashes'].includes(k)))throw Error('FUNDAMENTAL_ENTRY_DRAIN_INVALID');
 return {markerHash:payloadHash(v),reasonCode:v.reasonCode,at:v.at};
}

// Frozen host has no reducer boundary hook. It can continue a single-book exit
// only when *every* latency wallet already owns its one permitted share. On
// divergence/flat state, preserve reservations and require a named continuation.
export function fundamentalDrainAdmission(stats,bank,batch){
 const id=bank.research[0].marketId,wallets=Object.values(stats.wallets||{});
 if(stats.failed!==false||!wallets.length||wallets.some(w=>!Array.isArray(w.positions)||w.positions.length!==1||w.positions[0].marketId!==id||w.positions[0].quantity!==1
  ||!Array.isArray(w.pending)||w.pending.some(p=>p.action!=='sell'||p.marketId!==id)))return {allowed:false,reason:'flat_divergent_or_pending_buy_requires_continuation'};
 const scoped=batch.filter(row=>row.kind==='book'&&row.payload?.book?.marketId===id);
 if(scoped.length>1)return{allowed:false,reason:'multi_book_batch_requires_continuation'};
 return{allowed:true,reason:'held_single_book_exit_only'};
}

export function wrapFundamentalEntryDrain(host,{runtimeDir,now=Date.now,filesystem=fs}={}){
 const source=path.join(runtimeDir,'public-books.jsonl'),journal=path.join(host.dir,'operator-entry-drain.jsonl');let previous=payloadHash({bankHash:digest(host.bank),kind:'fundamental_entry_drain'}),sequence=0,lastMode=null,lastMarker=null;
 if(filesystem.existsSync(journal)){const raw=filesystem.readFileSync(journal,'utf8');if(raw&&!raw.endsWith('\n'))throw Error('FUNDAMENTAL_DRAIN_PARTIAL_JOURNAL');for(const line of raw.trimEnd().split('\n').filter(Boolean)){const{hash,...body}=JSON.parse(line);if(body.sequence!==++sequence||body.previous!==previous||payloadHash(body)!==hash||body.bankHash!==digest(host.bank))throw Error('FUNDAMENTAL_DRAIN_JOURNAL');previous=hash;lastMode=body.mode;lastMarker={markerHash:body.markerHash,reasonCode:body.reasonCode,at:body.markerAt};}}
 let projection={enabled:!!lastMarker,mode:lastMode,continuationRequired:lastMode==='continuation_required',executionAuthority:false};
 const marker=()=>readFundamentalEntryDrain(runtimeDir,host.bank,filesystem)||lastMarker;
 const record=(m,mode,reason,stats)=>{projection={enabled:true,markerHash:m.markerHash,reasonCode:m.reasonCode,mode,reason,continuationRequired:mode==='continuation_required',executionAuthority:false};if(lastMode===mode&&lastMarker?.markerHash===m.markerHash)return;
  const body={sequence:sequence+1,previous,bankHash:digest(host.bank),at:new Date(now()).toISOString(),markerHash:m.markerHash,markerAt:m.at,reasonCode:m.reasonCode,mode,reason,sourceSequence:stats.sourceSequence,sourceHash:stats.sourceHash},hash=payloadHash(body),fd=filesystem.openSync(journal,'a',0o600);try{filesystem.writeSync(fd,JSON.stringify({...body,hash})+'\n');filesystem.fsyncSync(fd);}finally{filesystem.closeSync(fd);}previous=hash;sequence++;lastMode=mode;lastMarker=m;};
 const consume=()=>{const m=marker();if(!m)return host.consume();const stats=host.status(),size=filesystem.statSync(source).size;
  if(!Number.isSafeInteger(stats.sourceBytes)||stats.sourceBytes<0||stats.sourceBytes>size)throw Error('FUNDAMENTAL_DRAIN_SOURCE');
  const raw=Buffer.alloc(size-stats.sourceBytes),fd=filesystem.openSync(source,'r');try{let n=0;while(n<raw.length){const read=filesystem.readSync(fd,raw,n,raw.length-n,stats.sourceBytes+n);if(!read)throw Error('FUNDAMENTAL_DRAIN_SOURCE');n+=read;}}finally{filesystem.closeSync(fd);}
  const tail=raw.subarray(0,raw.lastIndexOf(10)+1).toString(),batch=tail?tail.split('\n').slice(0,-1).map(JSON.parse):[];
  const admission=fundamentalDrainAdmission(stats,host.bank,batch);record(m,admission.allowed?'held_exit_only':'continuation_required',admission.reason,stats);
  return admission.allowed?host.consume():0;};
 const status=()=>({...host.status(),operatorEntryDrain:projection});
 return{...host,consume,status,publish(){host.publish();const m=marker();if(m){const stats=status();const file=path.join(host.dir,'operator-entry-drain-status.json'),fd=filesystem.openSync(file,'w',0o600);try{filesystem.writeSync(fd,JSON.stringify({bankHash:digest(host.bank),updatedAt:new Date(now()).toISOString(),...stats.operatorEntryDrain})+'\n');filesystem.fsyncSync(fd);}finally{filesystem.closeSync(fd);}}},entryDraining:()=>!!marker()};
}
