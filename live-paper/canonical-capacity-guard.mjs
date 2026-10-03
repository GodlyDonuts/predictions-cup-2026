import fs from 'node:fs';import path from 'node:path';import {createHash} from 'node:crypto';
import {atomicJson,payloadHash} from '../execution/ledger.mjs';
const check=(ok,code)=>{if(!ok)throw Error('CAPACITY_GUARD_'+code);};
const sha=x=>createHash('sha256').update(x).digest('hex');
// External host protection. The frozen evaluator, bank and original journal stay intact.
export function guardCanonicalCapacity(monitor){
 if(!monitor)return null;
 const file=path.join(monitor.dir,'journal.jsonl'),anchorFile=path.join(monitor.dir,'collector-high-water.json'),statsFile=path.join(monitor.dir,'stats.json');
 let anchor=null;
 function verify(){
  const status=monitor.status(),bytes=fs.readFileSync(file);
  if(anchor){check(status.cohort===anchor.cohort&&status.bankHash===anchor.bankHash,'IDENTITY');
   check(bytes.length>=anchor.bytes&&sha(bytes.subarray(0,anchor.bytes))===anchor.prefixHash&&status.journalSequence>=anchor.sequence,'JOURNAL_ROLLBACK');
   if(status.journalSequence===anchor.sequence)check(status.journalHash===anchor.tailHash,'TAIL_CHANGED');
  }
  return {status,bytes};
 }
 function checkpoint(){const {status,bytes}=verify();const body={schemaVersion:1,cohort:status.cohort,bankHash:status.bankHash,bytes:bytes.length,prefixHash:sha(bytes),sequence:status.journalSequence,tailHash:status.journalHash};
  try{atomicJson(anchorFile,{...body,hash:payloadHash(body)});}catch{throw Error('CAPACITY_GUARD_DURABILITY');}anchor=body;
 }
 if(fs.existsSync(anchorFile)){const {hash,...body}=JSON.parse(fs.readFileSync(anchorFile));check(body.schemaVersion===1&&Number.isSafeInteger(body.bytes)&&body.bytes>=0&&Number.isSafeInteger(body.sequence)&&body.sequence>=0&&payloadHash(body)===hash,'ANCHOR_INVALID');anchor=body;}
 // Bootstrap from the previously published tail too: a surviving stats projection
 // can expose truncation that occurred before this external guard was introduced.
 if(fs.existsSync(statsFile)){const prior=JSON.parse(fs.readFileSync(statsFile)),current=monitor.status();check(prior.cohort===current.cohort&&prior.bankHash===current.bankHash&&Number.isSafeInteger(prior.journalSequence)&&prior.journalSequence<=current.journalSequence,'PUBLISHED_TAIL_ROLLBACK');if(prior.journalSequence===current.journalSequence)check(prior.journalHash===current.journalHash,'PUBLISHED_TAIL_CHANGED');}
 checkpoint();
 return {...monitor,
  status:()=>({...monitor.status(),externalHighWater:true}),
  inspect(publication){verify();const changed=monitor.inspect(publication);if(changed)checkpoint();return changed;},
  plan(base,context){verify();const next=monitor.plan(base,context),ids=next.canonicalCapacityFocus?.marketIds;
   if(ids?.length){const optional=new Set(['discovery','comparison','edge_observation']),protectedIds=new Set(base.reads.filter(x=>!optional.has(x.reason)).map(x=>x.marketId));
    const indexes=ids.map(id=>next.reads.findIndex(x=>x.marketId===id));
    // Never move a held-risk read to manufacture an adjacent research pair.
    if(ids.some(id=>protectedIds.has(id))||indexes.some(i=>i<0)||Math.abs(indexes[0]-indexes[1])!==1)return {...base,canonicalCapacityFocus:{marketIds:[],reason:'protected_leg_or_nonadjacent_pair',requestsAdded:0,executionAuthority:false}};
   }
   return next;
  }};
}
