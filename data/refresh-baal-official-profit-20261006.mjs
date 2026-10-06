import fs from 'node:fs';
import path from 'node:path';
import assert from 'node:assert/strict';
import {payloadHash} from '../execution/core.mjs';
import {amount,money} from '../paper/fixed.mjs';
import {replayBaalScaledControllerV1} from '../execution/baal-scaled-controller-legacy-replay-v1.mjs';

// Read existing local evidence only. Never query the account or publish raw receipts.
const runtime=path.join(process.env.LOCALAPPDATA,'SigTradingCup/paper-runtime');
const file=path.join(runtime,'baal-scaled-live-runs/baal-scaled-no-20261006-human/controller/baal-scaled-no-journal.jsonl');
const bodyText=fs.readFileSync(file,'utf8');
const first=JSON.parse(bodyText.split('\n',1)[0]);
const replay=replayBaalScaledControllerV1({bodyText,expectedCodeIdentity:first.event.codeIdentity});
const sells=replay.state.orders.filter(o=>o.body.action==='sell'&&o.proof).map(o=>{
 const p=o.proof,{verificationHash,...unsigned}=p;
 assert.equal(payloadHash(unsigned),verificationHash);
 assert.equal(p.fee,'0.000000');
 return {orderId:o.orderId,marketId:p.marketId,exchangeId:p.exchangeId,quantity:p.filledQuantity,
  removedPrincipal:p.removedPrincipalCost,proceeds:p.scopedProceeds,realizedPnl:p.realizedPnl,
  proofHash:verificationHash,readAt:p.readAt,
  fills:p.fills?.map(f=>({id:f.id??f.fillId,quantity:f.quantity,price:f.price,filledAt:f.filledAt}))??[]};
});
const inventory=Object.fromEntries(Object.entries(replay.state.inventory).map(([ex,v])=>[ex,{
 marketId:v.proof.marketId,quantity:v.proof.remainingOwnedQuantity,cost:v.proof.remainingOwnedCost,
 cumulativeRealizedPnl:v.proof.cumulativeRealizedPnl,proofHash:v.proof.verificationHash,eventId:v.package.eventId
}]));
const pnl=sells.reduce((sum,p)=>sum+amount(p.realizedPnl),0n);
const out={schemaVersion:1,runId:replay.state.scope.runId,observedAt:new Date().toISOString(),
 sourceThroughAt:JSON.parse(bodyText.trimEnd().split('\n').at(-1)).at,
 actualOrders:replay.state.orders.filter(o=>o.fetchStarts?.some(f=>f.method==='POST')).length,
 scaledRealizedPnl:money(pnl),oldFourClosedPilotNetRealized:'0.015000',fees:'0.000000',sells,inventory,
 remainingQuantity:Object.values(inventory).reduce((n,p)=>n+p.quantity,0),
 remainingCost:money(Object.values(inventory).reduce((n,p)=>n+amount(p.cost),0n)),
 sourceActorSequence:replay.anchor.sequence,sourceActorHash:replay.anchor.hash,
 sourceBytesHash:replay.anchor.sha256,proofsHashVerified:true,economicReplayPerformed:true,
 includesUnrealizedPnl:false,profitabilityAtScaleEstablished:false};
fs.writeFileSync('data/baal-scaled-official-profit-20261006.json',JSON.stringify(out,null,2)+'\n');
console.log(JSON.stringify({actualOrders:out.actualOrders,scaledRealizedPnl:out.scaledRealizedPnl,
 remainingQuantity:out.remainingQuantity,remainingCost:out.remainingCost,sourceActorSequence:out.sourceActorSequence}));
