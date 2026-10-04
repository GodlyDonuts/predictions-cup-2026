import fs from 'node:fs';
import path from 'node:path';
import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
const runtime=path.join(process.env.LOCALAPPDATA,'SigTradingCup','paper-runtime');
const root=path.join(runtime,'research-cohorts');
const laterDir=path.join(root,'baal-dip-subsequent-20261004-v1');
const costsDir=path.join(root,'baal-dip-development-costs-20261004-v1');
const read=p=>JSON.parse(fs.readFileSync(p,'utf8'));
const hash=bytes=>createHash('sha256').update(bytes).digest('hex');
const later=read(path.join(laterDir,'evaluation-v2-diagnostics.json'));
const earlierLater=read(path.join(laterDir,'evaluation-v1.json'));
const costs=read(path.join(costsDir,'evaluation-v1.json'));
const grid=read(path.join(root,'baal-net-20261004-e/native-dip-grid-v2.json'));
assert.deepEqual(later.evaluation.scorecards.map(({signalDiagnostics,...c})=>c),earlierLater.evaluation.scorecards);
const graph=['Cargo.toml','Cargo.lock','build.rs','.cargo/config.toml','src/main.rs','src/money.rs','src/types.rs','src/capture.rs','src/engine.rs','src/replay.rs','src/trace.rs','src/sweep.rs','src/evaluation.rs'];
const engine=createHash('sha256'),sourceHashes={};
for(const p of graph){const bytes=fs.readFileSync(path.join('backend-rust',p)),name=Buffer.from(p),size=Buffer.alloc(8),length=Buffer.alloc(8);size.writeBigUInt64LE(BigInt(name.length));length.writeBigUInt64LE(BigInt(bytes.length));engine.update(size).update(name).update(length).update(bytes);sourceHashes['backend-rust/'+p]=hash(bytes);}
const engineHash=engine.digest('hex');assert.equal(engineHash,later.capture.engine_hash);assert.equal(engineHash,costs.capture.engine_hash);
const binary=fs.readFileSync('backend-rust/target/release/baal-backend.exe'),binaryHash=hash(binary);
const archive=path.join(runtime,'native-builds',binaryHash);fs.mkdirSync(archive,{recursive:true});
for(const [name,bytes]of [['baal-backend.exe',binary],['libunwind.dll',fs.readFileSync('backend-rust/target/release/libunwind.dll')]]){const p=path.join(archive,name);if(fs.existsSync(p))assert.equal(hash(fs.readFileSync(p)),hash(bytes));else fs.writeFileSync(p,bytes,{flag:'wx',mode:0o600});}
const sourceArchive=path.join(runtime,'native-sources',engineHash);fs.mkdirSync(sourceArchive,{recursive:true});
for(const p of graph){const output=path.join(sourceArchive,p);fs.mkdirSync(path.dirname(output),{recursive:true});const bytes=fs.readFileSync(path.join('backend-rust',p));if(fs.existsSync(output))assert.equal(hash(fs.readFileSync(output)),hash(bytes));else fs.writeFileSync(output,bytes,{flag:'wx',mode:0o600});}
for(const [dir,name,result]of [[laterDir,'evaluation-v2-diagnostics.json',later],[costsDir,'evaluation-v1.json',costs]]){
 const receipt=read(path.join(dir,name+'.receipt.json'));
 assert.equal(receipt.binaryHash,binaryHash);assert.equal(receipt.engineHash,engineHash);assert.equal(receipt.outputHash,hash(fs.readFileSync(path.join(dir,name))));
 assert.equal(receipt.declarationFileHash,hash(fs.readFileSync(path.join(dir,'declaration.json'))));assert.equal(result.declarationFileHash,receipt.declarationFileHash);
}
let exactHistoricalBaselines=0;
for(const c of costs.evaluation.scorecards.filter(c=>c.scenario.id==='baseline'&&c.exitPolicy==='snapshot_limit_v1')){
 const old=grid.sweep.scorecards.find(g=>JSON.stringify(g.parameters)===JSON.stringify(c.parameters));assert(old);
 // The first record warms features only; these sources start with no trades.
 const fields=['cash','conservativePnl','realizedPnl','fees','turnover','completedTrades','fillExecutions','intents','expiredIntents','pendingIntents','openShares','unsoldShares'];
 for(const k of fields)assert.deepEqual(c.point[k],old.points.at(-1)[k],k+' '+c.strategy);exactHistoricalBaselines++;
}
const cards=later.evaluation.scorecards;assert.equal(cards.length,100);assert(cards.every(c=>c.fills.length===0&&c.point.intents===0&&c.point.conservativePnl==='0.000000'));
const baseline=costs.evaluation.scorecards.find(c=>c.strategy==='historical-positive-1'&&c.scenario.id==='baseline'&&c.exitPolicy==='snapshot_limit_v1');
const summarize=c=>({strategy:c.strategy,scenario:c.scenario,exitPolicy:c.exitPolicy,conservativePnl:c.point.conservativePnl,realizedPnl:c.point.realizedPnl,completedTrades:c.point.completedTrades,fillExecutions:c.point.fillExecutions,openShares:c.point.openShares,netPnl:c.economics.netPnl,midMove:c.economics.midMove,quotedSpreadAndLadderCost:c.economics.quotedSpreadAndLadderCost,slippageCost:c.economics.slippageCost,fees:c.economics.fees,byExitReason:c.economics.byExitReason});
const evidence={schemaVersion:1,at:new Date().toISOString(),claimComments:[5978288303,5978438004],status:'native_causal_evaluator_implemented_tested',engineHash,binaryHash,sourceHashes,
 implemented:['Original-byte prefix/clock/declaration binding','Fresh later wallets with prior-only feature warmup','Regular and experimental profit-floor/bounded-stop exits','Exact FIFO mid/spread/ladder/slippage/fee attribution','Separate flat-history/z/cost gate diagnostics','Eight-worker immutable cost/latency/depth comparisons','Explicit reused-development classification'],
 subsequent:{...(({scorecards,...meta})=>meta)(later.evaluation),source:later.capture,selection:read(path.join(laterDir,'source-binding.json')),feeBps:0,initialCash:'100.000000',allCardsZeroFills:true,allCardsZeroPnl:true,unchangedComparedWithEarlierResult:true,performanceInspectedBeforeFurtherChanges:true,signalRows:cards.filter(c=>c.scenario.id==='baseline'&&c.exitPolicy==='snapshot_limit_v1').map(c=>({strategy:c.strategy,parameters:c.parameters,observations:c.point.signalMetrics.observations,...c.signalDiagnostics})),receipt:read(path.join(laterDir,'evaluation-v2-diagnostics.json.receipt.json'))},
 developmentCosts:{classification:costs.evaluation.evaluationClassification,source:costs.capture,candidates:costs.evaluation.candidates,replayMs:costs.evaluation.replayMs,selection:read(path.join(costsDir,'source-binding.json')),exactHistoricalBaselines,baselineEconomics:summarize(baseline),sensitivity:costs.evaluation.scorecards.filter(c=>c.strategy==='historical-positive-1').map(summarize),comparison:costs.evaluation.scorecards.filter(c=>c.scenario.id==='baseline').map(summarize),receipt:read(path.join(costsDir,'evaluation-v1.json.receipt.json'))},
 checks:{nativeNodePassed:13,nativeNodeFailed:0,rustPassed:1,rustFailed:0,adjacentJsPreviouslyPassed:23,fullRepositorySuite:false},
 decisions:['No dip strategy or capital increase activated: tiny selected baseline gain is not robust to modeled cost','Experimental exit policy retained only for offline comparison: it did not improve measured development economics','Preserve original output/declaration/receipt and all live frozen wallets; diagnostics add new immutable outputs'],
 limitations:['Subsequent prices were collected before declaration; this is retrospective subsequent data, not live prospective trading','Historical grid selection and exit comparisons reuse development data; correlated cases are not independent samples','One tick after five closes is not established profitability','Snapshot fills omit exchange queue priority, impact and unobserved intrainterval movements','Unchanged latency outcomes on sparse snapshots cannot prove speed is unimportant in a dense live feed','Native compute/replay migrated; Node remains the live collector, dashboard and original fitting runtime'],
 networkRequests:0,competitionOrders:0,additionalPersistentProcesses:0,productionActivation:false,profitabilityEstablished:false};
fs.writeFileSync('data/baal-native-evaluation-20261004.json',JSON.stringify(evidence,null,2)+'\n');
const q=read('data/quant-development-state.json');q.updatedAt=evidence.at;q.baalNativeEvaluationMilestone=evidence;
const duplicateTask='Use the native backend for subsequent untouched captures and execution-cost sensitivity; improve documented tournament realtime ingestion through the single collector to reduce sparse historical coverage.';
let replaced=false;q.baalBacklog=q.baalBacklog.filter(entry=>{if(entry.task!==duplicateTask)return true;if(replaced)return false;replaced=true;Object.assign(entry,{status:'completed',task:'Implement native fresh-wallet subsequent comparison and development-cost sensitivity; preserve original results',completedAt:evidence.at,evidence:'data/baal-native-evaluation-20261004.json'});return true;});
q.baalBacklog.push({status:'active',task:'Use measured spread/slippage and subsequent sparse coverage to compare a causally defined passive/source-backed edge; declare a new future cutoff before any new selection. Keep frozen wallets unchanged and no forced trades.'});
q.next='Native Rust replay and economic attribution are working. Next improve a causal passive/source-backed entry hypothesis using separate development data and a named future cutoff. Current dip tiny historical gains are not robust. Realtime auth coordination5978106312 remains pending; preserve the20-read allocation and disabled contest writes.';
fs.writeFileSync('data/quant-development-state.json',JSON.stringify(q,null,2)+'\n');
const m=read('data/daenerys-monitor.json');m.updatedAt=evidence.at;m.last_checked_at_utc=evidence.at;m.baalNativeEvaluationMilestone={evidence:'data/baal-native-evaluation-20261004.json',engineHash,claimComments:evidence.claimComments,subsequentNoEntries:true,developmentCostsDiagnosed:true,productionActivation:false};m.nextConcreteMilestone=q.next;fs.writeFileSync('data/daenerys-monitor.json',JSON.stringify(m,null,2)+'\n');
console.log(JSON.stringify({engineHash,binaryHash,cards:cards.length,developmentCards:costs.evaluation.candidates,exactHistoricalBaselines,archivedSourceFiles:graph.length,networkRequests:0,competitionOrders:0}));
