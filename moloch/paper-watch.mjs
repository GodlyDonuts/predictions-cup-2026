import fs from 'node:fs';import path from 'node:path';import {payloadHash} from '../execution/ledger.mjs';import {assertMoloch} from './engine.mjs';
export function watchedPaperOrders(directory,sourceFile,scopes){
 const file=path.join(directory,'state.json');let raw;try{raw=fs.readFileSync(file,'utf8');}catch(e){if(e.code==='ENOENT')return[];throw e;}
 const snapshot=JSON.parse(raw),{hash,...body}=snapshot,s=body.state;
 if(hash!==payloadHash(body)||s?.bot!=='Moloch'||payloadHash(s.policy)!==s.policyHash||!body.source?.path||path.resolve(body.source.path).toLowerCase()!==path.resolve(sourceFile).toLowerCase())throw Error('MOLOCH_PAPER_WATCH_PROOF');
 assertMoloch(s);const byId=new Map(scopes.map(p=>[p.exchangeId,p.marketId]));
 if(s.orders.some(q=>!q.retired&&byId.get(q.exchangeId)!==q.marketId)||Object.entries(s.positions).some(([id,p])=>[...p.YES,...p.NO].some(l=>l.quantity>0)&&byId.get(id)!==s.books[id]?.marketId))throw Error('MOLOCH_PAPER_WATCH_SCOPE');
 for(const q of s.orders.filter(q=>!q.retired))if(!Number.isSafeInteger(q.activationAt)||!Number.isSafeInteger(q.expiresAt)||q.cancelEffectiveAt!==undefined&&!Number.isSafeInteger(q.cancelEffectiveAt))throw Error('MOLOCH_PAPER_WATCH_CLOCK');
 return s.orders;
}
