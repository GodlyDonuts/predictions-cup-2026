const fs=require('node:fs'),crypto=require('node:crypto');
const read=f=>JSON.parse(fs.readFileSync(f));
const results=['c','d','e','f'].map(x=>read('data/baal-net-20261004-'+x+'-result.json'));
const milestone={schemaVersion:1,at:new Date().toISOString(),scopeClaims:[5977383916,5977484396,5977638451],rustScopeClaim:5977664356,
 source:{bytes:results[0].sourceBytes,records:results[0].sourceRecords,sha256:results[0].sourceHash,through:results[0].sourceThrough,configuredMarkets:237},
 runs:results.map(r=>({name:r.name,family:r.family,horizons:r.horizons||[60,180,300,600],workers:r.workers||1,
  acceptedSnapshots:r.snapshots,modelsTrained:r.trained,modelMarginPairs:r.modelMarginWallets,decision:r.decision,
  sourceHash:r.sourceHash,bankHash:r.bankHash,configHash:r.configHash,verifyMs:r.verifyMs,fitAndReplayMs:r.fitAndReplayMs,totalMs:r.totalMs,peakProcessRssMiB:r.peakRssMiB,
  windows:r.windows,holdout:r.holdout,validationLeaders:r.validationLeaders})),
 exactParallelParity:{modelWeights:true,walletMetrics:true,sourceBytes:true,exception:'The diagnostic count now reports unsupported instruments per wallet rather than across three margins.'},
 speedup:{serialMs:results[1].totalMs,parallelMs:results[2].totalMs,ratio:results[1].totalMs/results[2].totalMs,workers:4,scope:'Complete frozen original-prefix verification plus fitting and validation; non-isolated wall-clock sample, not10x.'},
 checks:{passed:63,failed:0,scope:'source, chronology, causal labels, sequential/parallel portfolio parity, worker failure cleanup, frozen horizons and current ML consumers; bounded run'},
 implementation:['Verified streamed original byte/hash prefixes without rewriting journals','Actual receipt ordering for new banks; missing-policy legacy order preserved','Indexed instrument label paths; same labels and outcomes','Parallel offline wallet replay with shared history-scope guard','Feature validation avoids unrelated portfolio simulation; same core version/clock checks','Explicit frozen holding horizons and entry-admission metrics'],
 findings:['Short-horizon comparison:384wallets,0intents/0fills;1642eligible observations each,415warmup,1227below margin,0edge passes.','Longer15min/30min/1h/4h comparison also chose no-trade; additional holding time alone has not produced a qualifying edge.','Longer experiment and repeats reuse the same historical source; these are development comparisons, not independent evidence.'],
 workerIncident:{detected:'Forced Windows Node/V8 worker termination during callback rejection produced a native crash in regression testing.',corrected:'Workers close their message ports and exit normally; failure paths wait for exit; the rejecting-callback regression now passes.'},
 competitionOrders:0,fees:0,additionalCollectors:0,persistentProcessesAdded:0,productionActivation:false,profitabilityEstablished:false,
 next:'Implement and benchmark a native Rust replay/compute backend on the complete multi-day capture; preserve live collector and frozen histories.'};
fs.writeFileSync('data/baal-fast-replay-milestone-20261004.json',JSON.stringify(milestone,null,2)+'\n');
const q=read('data/quant-development-state.json');q.updatedAt=milestone.at;q.baalReplayMilestone=milestone;
q.previousGoalTurnClassification={at:milestone.at,classification:'progress',reason:'Whole verified real capture trains and causally replays; confirmed chronology defect fixed,128short+128long models compared,63focused checks pass. Four-worker replay preserves results and measures2.02x. Rust migration requested and started. Profitability remains unproven.'};
q.nextConcreteMilestone=milestone.next;
q.baalBacklog=q.baalBacklog.map(x=>/compact training/.test(x.task)?{...x,status:'completed',evidence:'data/baal-fast-replay-milestone-20261004.json'}:x);
q.baalBacklog.push({status:'active',task:milestone.next});fs.writeFileSync('data/quant-development-state.json',JSON.stringify(q,null,2)+'\n');
const m=read('data/daenerys-monitor.json');m.last_checked_at_utc=milestone.at;m.github.comments=read('data/baal-replay-monitor-refs-20261004.json');m.baalReplayMilestone=milestone;
fs.writeFileSync('data/daenerys-monitor.json',JSON.stringify(m,null,2)+'\n');
const docs='docs/BAAL.md',append='\nOctober4 fast replay: the complete original4,255,286,972-byte capture is verified and replayed in receipt order without future observations or rewritten timestamps.128short-horizon models/384wallets and128long-horizon models/384wallets both selected no-trade. The short comparison saw1642eligible observations per wallet,415warmup and1227below its fixed edge margin. Four-worker replay measured279.65→138.53seconds including source verification, with exact model weights and wallet metrics. This is2.02x for this sample, not10x or profitability.63focused checks passed. Native Rust replay/compute migration is the current human-requested next implementation. See data/baal-fast-replay-milestone-20261004.json.\n';
if(!fs.readFileSync(docs,'utf8').includes('October4 fast replay:'))fs.appendFileSync(docs,append);
const files=['live-paper/training-source.mjs','live-paper/training-source.test.mjs','live-paper/snapshot-order.mjs','live-paper/snapshot-order.test.mjs','live-paper/label-paths.mjs','live-paper/label-paths.test.mjs','live-paper/portfolio-replay-pool.mjs','live-paper/portfolio-replay-pool.test.mjs','live-paper/portfolio-replay-worker.mjs','live-paper/training-horizons.mjs','live-paper/replay.mjs','live-paper/ml-bank.mjs','live-paper/ml-shadow.mjs','live-paper/ml-shadow-v2.mjs','live-paper/portfolio-bank.mjs','live-paper/portfolio-bank.test.mjs','live-paper/training-options.mjs','live-paper/execution-fix.test.mjs','data/train-baal-real-capture-20261004.mjs','data/baal-fast-replay-milestone-20261004.json','data/prepare-baal-replay-publication-20261004.cjs','docs/BAAL.md'];
const elements=files.map(path=>({path,mode:'100644',type:'blob',content:fs.readFileSync(path,'utf8')}));
for(const e of elements)if(/ace_[a-f0-9]{32,}|Bearer\s+[A-Za-z0-9_-]{20,}|BEGIN\s+PRIVATE\s+KEY/i.test(e.content))throw Error('Secret scan failed: '+e.path);
const pack={parent:'1cdefcbaa3ba054c3252c3c3a5ca73877c2e7760',baseTree:'95800c7a60230fa4992a9094e60ad1ec14f2a1f7',elements};
fs.writeFileSync('data/baal-replay-publication-pack-20261004.json',JSON.stringify(pack));console.log(JSON.stringify({files:files.length,characters:JSON.stringify(pack).length,secretScan:'passed'}));
