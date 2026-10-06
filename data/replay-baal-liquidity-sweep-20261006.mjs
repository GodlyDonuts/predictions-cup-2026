import fs from'node:fs';import path from'node:path';import{createHash}from'node:crypto';
import{payloadHash}from'../execution/core.mjs';import{amount,money}from'../paper/fixed.mjs';
import{planBaalPairedLiquiditySweep}from'../execution/baal-paired-liquidity-sweep-v1.mjs';
const runtime=path.join(process.env.LOCALAPPDATA,'SigTradingCup/paper-runtime');
const metadata=JSON.parse(fs.readFileSync(path.join(runtime,'baal-scaled-host-upgrade-v3-prefixes.json'))),anchor=metadata.anchors.controller;
const bytes=fs.readFileSync(path.join(runtime,'baal-scaled-live-runs',metadata.runId,'controller',anchor.name)).subarray(0,anchor.bytes);
const sha=b=>createHash('sha256').update(b).digest('hex');if(sha(bytes)!==anchor.sha256||bytes.at(-1)!==10)throw Error('SEALED_PREFIX_MISMATCH');
const rows=bytes.toString().trimEnd().split('\n').map(x=>JSON.parse(x)),samples=[];let prior='';
for(const[rowIndex,row]of rows.entries()){
 const{hash,...body}=row;if(row.sequence!==rowIndex+1||row.previous!==prior||sha(JSON.stringify(body))!==hash)throw Error('CHAIN');prior=hash;
 const e=row.event.input;if(e.kind!=='prepared'||e.action!=='buy')continue;
 const o=row.state.orders.at(-1),g=e.globalRisk,available=amount(row.state.allocation.capital)-amount(g.nominalExposure);
 const before=performance.now();
 const plan=planBaalPairedLiquiditySweep({binding:{ownerId:'Codex',runId:metadata.runId,tournamentId:row.state.scope.tournamentId,eventId:o.package.eventId,reviewHash:payloadHash(e.evidence.rootReceipts)},legs:e.evidence.bookReceipts,availableNominalCapital:money(available),now:e.at});
 samples.push({actorSequence:row.sequence,observedAt:e.at,marketIds:plan.marketIds,exchangeIds:plan.exchangeIds,originalPreparedQuantity:o.body.quantity,
   displayedBookSweep:plan.selected,onePercentBestRungControl:plan.onePercentBestRungControl,sourceHashes:plan.sourceHashes,computeMs:performance.now()-before,
   actualAcceptanceOrFillReplayed:false,conditionalMarginNotProfit:true});
}
const output={schemaVersion:1,evaluatedAt:new Date().toISOString(),mode:'sealed_actual_prepared_book_price_depth_comparison',runId:metadata.runId,
 actorPrefix:{bytes:anchor.bytes,sha256:anchor.sha256,sequence:anchor.sequence,hash:anchor.hash},samples,
 additionalDisplayedQuantityVersusPrepared:samples.reduce((n,s)=>n+Math.max(0,(s.displayedBookSweep?.quantity??0)-s.originalPreparedQuantity),0),
 samplePackets:samples.length,distinctEvents:new Set(samples.map(s=>s.marketIds.join(':'))).size,
 noNewDataOrApiTraffic:true,networkRequests:0,competitionOrders:0,simulatedFills:0,profitabilityEstablished:false,
 limitations:['Prepared packets include repeated abandoned intents; quantities and margins must not be summed as independent trades.','Only displayed depth is measured; latency, partial fills, queue priority and collateral effects are not replayed.','Removing the extra buy-price cushion is a separate execution-policy change from depth participation.','Ordinary settlement floors depend on the original mutually exclusive party-winner predicates; administrative outcomes are not guaranteed.']};
fs.writeFileSync('data/baal-liquidity-sweep-capture-comparison-20261006.json',JSON.stringify(output,null,2)+'\n');
console.log(JSON.stringify({samplePackets:output.samplePackets,distinctEvents:output.distinctEvents,additionalDisplayedQuantityVersusPrepared:output.additionalDisplayedQuantityVersusPrepared,
 samples:samples.map(s=>({markets:s.marketIds,originalQuantity:s.originalPreparedQuantity,sweepQuantity:s.displayedBookSweep?.quantity??0,controlQuantity:s.onePercentBestRungControl?.quantity??0,conditionalMargin:s.displayedBookSweep?.conditionalOrdinaryMarginLowerBound,computeMs:s.computeMs})),actualFills:0}));
