import fs from 'node:fs';import path from 'node:path';import {createHash} from 'node:crypto';
import {payloadHash} from './core.mjs';
import {replayBaalGenericNoJournalV3,baalGenericNoControllerV3Identity} from './baal-generic-no-controller-v3.mjs';
const need=(v,c)=>{if(!v)throw Error('PRIOR_GENERIC_'+c);};
export function verifyBaalGenericPriorCustodyV1({runtimeDir,custodySeed}){
 const requestFile=path.join(runtimeDir,'baal-generic-live-request-v1.json');
 if(!fs.existsSync(requestFile))return null;
 const request=JSON.parse(fs.readFileSync(requestFile)),root=path.join(runtimeDir,'baal-generic-live-runs',request.runId,request.epoch);
 need(request.runId===custodySeed.runId&&request.ownerId===custodySeed.ownerId&&request.inheritanceHash===custodySeed.seedHash,'EXACT_INHERITANCE');
 const actor=path.join(root,'controller','baal-generic-no-v3-journal.jsonl'),budget=path.join(root,'budget','baal-generic-shared-budget-v3.jsonl');
 const replay=replayBaalGenericNoJournalV3({bodyText:fs.readFileSync(actor,'utf8'),expectedCodeIdentity:baalGenericNoControllerV3Identity(),custodySeed});
 need(replay.state.orders.length===0&&payloadHash(replay.state.inventory)===payloadHash(custodySeed.inventory),'NO_NEW_OR_PENDING_ORDERS');
 const seals=[requestFile,actor,budget].map(file=>{const bytes=fs.readFileSync(file);return{relative:path.relative(runtimeDir,file),bytes:bytes.length,sha256:createHash('sha256').update(bytes).digest('hex')};});
 return{epoch:request.epoch,anchor:replay.anchor,seals,newOrders:0,economicReplayPerformed:true};
}
