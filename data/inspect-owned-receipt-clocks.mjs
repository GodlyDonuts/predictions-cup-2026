// Read-only local timing diagnostic; emits no account bodies or credentials.
import fs from 'node:fs';
import path from 'node:path';
import {inspectScopedRestNoAccount} from '../execution/no-rest-sell-reconciliation-v2.mjs';
const runtime=path.join(process.env.LOCALAPPDATA,'SigTradingCup','paper-runtime');
const file=path.join(runtime,'baal-scaled-live-runs','baal-scaled-no-20261006-human','controller','baal-scaled-no-journal.jsonl');
const text=fs.readFileSync(file,'utf8');
const row=JSON.parse(text.trimEnd().slice(text.trimEnd().lastIndexOf('\n')+1));
const results=Object.entries(row.state.inventory).filter(([,s])=>s.proof.remainingOwnedQuantity>0).map(([exchangeId,s])=>{
 const p=s.proof,a=s.after,at=Date.parse(p.readAt);
 let accountValidation='passed';
 try{inspectScopedRestNoAccount({account:a,binding:{...row.state.scope,exchangeId},readAt:p.readAt});}catch(e){accountValidation=e.message;}
 return {exchangeId,quantity:p.remainingOwnedQuantity,proofAt:p.readAt,accountValidation,receipts:[a.cash,a.positions,...a.openOrdersPages,...a.scopedOrdersPages].map(r=>({endpoint:r.endpoint,durationMs:Date.parse(r.receivedAt)-Date.parse(r.requestStartedAt),ageAtProofMs:at-Date.parse(r.receivedAt),startAgeAtProofMs:at-Date.parse(r.requestStartedAt)}))};
});
console.log(JSON.stringify({sourceSequence:row.sequence,results},null,2));
