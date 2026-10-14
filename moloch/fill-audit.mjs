import fs from 'node:fs';import path from 'node:path';import {pathToFileURL} from 'node:url';
import {payloadHash} from '../execution/ledger.mjs';import {createMoloch,advanceMoloch} from './engine.mjs';import {completedTail} from './run.mjs';import {amount,SCALE,TICK} from '../paper/fixed.mjs';import {instantNs} from '../feed/contract.mjs';
export function classifyQuotePrint(q,event){
 if(q.retired||q.executionStyle==='inventoryAON'||q.marketId!==event.marketId||q.exchangeId!==event.exchangeId||q.tournamentId!==event.tournamentId)return null;
 const executed=instantNs(event.executedAt),end=Math.min(q.expiresAt,q.cancelEffectiveAt??Infinity);
 if(executed<BigInt(q.activationAt)*1000000n)return 'beforeActivation';if(executed>=BigInt(end)*1000000n)return 'afterWindow';
 const limit=q.side==='YES'?amount(q.price):SCALE-amount(q.price),bid=(q.side==='YES')===(q.action==='buy'),p=amount(event.yesPrice);
 return (bid?p<=limit-TICK:p>=limit+TICK)?'through':p===limit?'touch':'away';
}
export function auditTail(directory){
 const dir=path.resolve(directory),snapshot=JSON.parse(fs.readFileSync(path.join(dir,'state.json'),'utf8')),{hash,...body}=snapshot;if(hash!==payloadHash(body))throw Error('FILL_AUDIT_SNAPSHOT');
 const state=snapshot.continuation?structuredClone(completedTail(snapshot.continuation.parentDirectory).snapshot.state):createMoloch(snapshot.state.policy);
 const rows=fs.existsSync(path.join(dir,'events.jsonl'))?fs.readFileSync(path.join(dir,'events.jsonl'),'utf8').split('\n').slice(0,snapshot.sequence).map(JSON.parse):[];
 let previous='',sequence=0,uniqueTrades=0,duplicateTrades=0;const counts={},byExchange={};const initialFills=state.fills.length;
 for(const row of rows){const{hash:h,...b}=row;if(h!==payloadHash(b)||b.sequence!==sequence+1||b.previous!==previous)throw Error('FILL_AUDIT_CHAIN');const e=b.event;
  if(e.type==='trade'){
   if(state.seen[[e.tournamentId,e.exchangeId,e.id].join(':')])duplicateTrades++;
   else{uniqueTrades++;const local=byExchange[e.exchangeId]??={uniqueTrades:0,shares:0,quotePrintPairs:{}};local.uniqueTrades++;local.shares+=e.quantity;
    for(const q of state.orders){const category=classifyQuotePrint(q,e);if(category===null)continue;counts[category]=(counts[category]??0)+1;const c=local.quotePrintPairs[category]??={pairs:0,shares:0};c.pairs++;c.shares+=e.quantity;}
   }
  }
  advanceMoloch(state,e);if(payloadHash(state)!==b.stateHash)throw Error('FILL_AUDIT_STATE');previous=h;sequence++;
 }
 if(sequence!==snapshot.sequence||previous!==snapshot.lastHash||payloadHash(state)!==payloadHash(snapshot.state))throw Error('FILL_AUDIT_TERMINAL');
 return{schemaVersion:1,snapshotHash:hash,at:state.lastAt,policy:state.policy,events:sequence,uniqueTrades,duplicateTrades,quotePrintPairs:counts,byExchange,newFills:state.fills.slice(initialFills),contestOrders:0,networkRequests:0,profitabilityEstablished:false,limitations:['Each count is an order/print pair; overlapping orders can count the same print more than once.','Touch volume does not prove aggressor direction, queue priority or executable fill.','Quotes are the actual simulated quotes known before each event, with original execution clocks and cancel deadlines.','A concurrent journal suffix is ignored beyond the pinned snapshot sequence; the checked prefix is reproduced exactly.']};
}
if(process.argv[1]&&import.meta.url===pathToFileURL(path.resolve(process.argv[1])).href){const r=auditTail(process.argv[2]);if(process.argv[3])fs.writeFileSync(process.argv[3],JSON.stringify(r,null,2)+'\n',{flag:'wx'});console.log(JSON.stringify({at:r.at,trades:r.uniqueTrades,pairs:r.quotePrintPairs,byExchange:r.byExchange,newFills:r.newFills.length},null,2));}
