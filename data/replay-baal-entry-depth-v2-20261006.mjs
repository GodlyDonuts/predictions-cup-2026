// Causal proposal comparison on original captured packets, never hypothetical
// fills or renewed receipts. Raw source/account bodies remain in runtime.
import fs from 'node:fs';import path from 'node:path';import {createHash} from 'node:crypto';
import {replayBaalScaledControllerV1,baalScaledNoControllerIdentity} from '../execution/baal-scaled-controller-legacy-replay-v1.mjs';
import {planBaalScaledEntry} from '../execution/baal-scaled-entry-signal-v1.mjs';
import {planBaalScaledEntryV2} from '../execution/baal-scaled-entry-signal-v2.mjs';
const runtime=path.join(process.env.LOCALAPPDATA,'SigTradingCup','paper-runtime');
const file=path.join(runtime,'baal-scaled-live-runs','baal-scaled-no-20261006-human','controller','baal-scaled-no-journal.jsonl');
const bytes=fs.readFileSync(file),sha=b=>createHash('sha256').update(b).digest('hex');
const seed=replayBaalScaledControllerV1({bodyText:bytes.toString(),expectedCodeIdentity:baalScaledNoControllerIdentity()});
const packets=[];for(const line of bytes.toString().trimEnd().split('\n')){
 const row=JSON.parse(line),e=row.event.input;if(e.kind!=='prepared'||e.action!=='buy')continue;
 const input={...e.evidence,scope:row.state.scope,allocation:row.state.allocation,now:e.at};
 const old=planBaalScaledEntry(input),next=planBaalScaledEntryV2(input);
 packets.push({originalSequence:row.sequence,originalDecisionAt:e.at,originalPreparedQuantity:row.state.orders.at(-1).body.quantity,originalPreparedExchangeId:row.state.orders.at(-1).body.exchangeId,originalPolicy:{ready:old.proposalReady,reason:old.reason,quantity:old.chosenLeg?.quantity??0},depthPolicy:{ready:next.proposalReady,reason:next.reason,quantity:next.chosenLeg?.quantity??0,chosenExchangeId:next.chosenLeg?.exchangeId??null,unitMargin:next.conditionalOrdinaryGap??null,fullPackageReservation:next.fullPackageReservation??null}});
}
if(sha(fs.readFileSync(file))!==seed.anchor.sha256)throw Error('SOURCE_CHANGED');
const out={schemaVersion:1,observedAt:new Date().toISOString(),sourceAnchor:seed.anchor,packets,interpretation:'Original clocks and account/risk/rule/depth receipts. Proposal capacity only; no new fill, profitability or historical counterfactual P&L. Selected preparation packets include abandoned attempts and cannot establish broad strategy performance.',networkRequests:0,actualAdditionalOrders:0};
fs.writeFileSync('data/baal-entry-depth-v2-replay-20261006.json',JSON.stringify(out,null,2)+'\n');console.log(JSON.stringify(out));
