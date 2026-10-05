import fs from 'node:fs';
import path from 'node:path';
import {createHash} from 'node:crypto';
import {digest} from '../live-paper/model.mjs';

const runtime=path.join(process.env.LOCALAPPDATA,'SigTradingCup','paper-runtime');
const read=file=>JSON.parse(fs.readFileSync(file,'utf8'));
const s=read(path.join(runtime,'stats.json')),c=read(path.join(runtime,'competition-stats.json'));
const a=read(path.join(runtime,'competition-activation.json')),pids=read('data/current-runtime-pids.json');
const bank=read(path.join(runtime,'production-banks',a.strategyBankHash+'.json'));
const comparison=read('research/current-polls-v1/price-comparison-20261005-v1.json');
const assessments=read('research/current-polls-v1/assessments-20261005-v1.json');
const at=new Date().toISOString();
if(s.runtime.status!=='running'||Date.parse(at)-Date.parse(s.reportedAt)>60000||s.runtime.pid!==pids.collector||s.runtime.errors.length)throw Error('RECOVERED_COLLECTOR_NOT_CURRENT');
if(a.strategyBankHash!=='a948c395b14992a019e2180133604b8ee84dd78a9e94d08781efd18fbefc0128'||digest(bank)!==a.strategyBankHash||a.entryExpiresAt!=='2026-10-05T03:59:22.131Z')throw Error('ORIGINAL_ACTIVATION_CHANGED');
if(c.status!=='entry_window_closed'||c.entryWindow?.open!==false||c.ordersSubmitted!==0||c.unconfirmedIntents!==0||c.realizedPnl!=='0.000000'||!c.readiness?.complete)throw Error('CLOSED_PILOT_NOT_VERIFIED');
if(comparison.eligiblePriceReceipts!==0||comparison.rows.length!==3||comparison.rows.some(r=>Date.parse(r.bookRequestStartedAt)<Date.parse(r.assessmentAt)||r.executionAuthority!==false))throw Error('SUBSEQUENT_SOURCE_COMPARISON_NOT_VERIFIED');
const milestone={schemaVersion:1,id:'production-recovery-current-polls-20261005-v1',at,goalTurnClassification:'progress',requiredGoalWorkRemaining:true,
 authority:'existing_direct_human_assignment',runId:a.runId,bankHash:a.strategyBankHash,entryExpiresAt:a.entryExpiresAt,activationChanged:false,productionBankChanged:false,
 incident:{observedAt:'2026-10-05T11:19:00Z',observationPrecision:'minute',previousPids:{collector:4760,paper:17228,dashboard:22396},previousPidsAbsent:true,lastSavedCollectorReportAt:'2026-10-04T23:43:43.768Z',causeEstablished:false,humanStopRemoved:false},
 changes:['Derived production entry-window status preserves blocked, uncertain and owned-risk states.','Recovered the same three approved services; controlled collector reload deployed the status fix without changing activation.','Captured six hashed primary polling/ballot sources and retained original clocks and population distinctions.','Built separate Republican/Democratic source assessments with verified opposition, multinomial margin variance and frozen sensitivity assumptions.','Compared three actual subsequent official contract/book receipts using existing auxiliary slots and exact price policy.'],
 checks:{targetedPassed:23,failed:0,classes:['entry-state projection','settlement controller and expired-window integration','primary polling assessment']},
 runtime:{collectorPid:pids.collector,paperPid:pids.paper,dashboardPid:pids.dashboard,persistentServices:3,reportedAt:s.reportedAt,cycles:s.runtime.cycles,errors:s.runtime.errors.length,productionReportedAt:c.updatedAt,status:c.status,reason:c.reason,entryWindow:c.entryWindow,ordersSubmitted:c.ordersSubmitted,dispatchAttempts:c.dispatchAttempts,unconfirmedIntents:c.unconfirmedIntents,realizedPnl:c.realizedPnl,addedFees:c.addedFees,readinessComplete:c.readiness.complete},
 research:{recordId:assessments.id,assessedAt:assessments.assessments[0].assessment.assessedAt,assessments:assessments.assessments.length,comparisonId:comparison.id,subsequentPriceReceipts:comparison.rows.length,qualifiedReceipts:comparison.eligiblePriceReceipts,additionalAccountClients:0,installedInProduction:false},
 limitations:['No order or positive official net performance from this pilot. Earlier official net−0.380 remains separate.','Sensitivity endpoints are subjective valuations, not calibrated probability bounds or profit evidence.','Three subsequent price receipts establish causal comparisons only, not a strategy-performance sample.','The cause of the overnight service disappearance and reliable unattended recovery remain unresolved.','The expired original entry window was not extended.'],
 next:['Calibrate current party outcome forecasts and compare subsequent executable economics without tuning to force entries.','Resolve the observed service-lifetime failure using concrete process evidence.','Prepare a useful bounded continuation only after qualifying source/price evidence; preserve inventory and lifetime limits.'],profitabilityEstablished:false};
fs.writeFileSync('data/production-recovery-20261005.json',JSON.stringify(milestone,null,2)+'\n');
for(const file of ['data/quant-development-state.json','data/daenerys-monitor.json']){
 const state=read(file);state.updatedAt=at;state.productionRecoveryCurrentPolls=milestone;state.nextConcreteMilestone=milestone.next;
 if(file.includes('quant-development')){state.activeProcesses={collector:pids.collector,dashboard:pids.dashboard,probabilityWorker:pids.paper,persistent:3,extra:0};state.currentRuntimePids=pids;state.productionNext=milestone.next;state.lastMilestone=milestone.id;}
 else{state.lastCheckAt=at;state.lastEngineeringMilestone=milestone.id;}
 fs.writeFileSync(file,JSON.stringify(state,null,2)+'\n');
}
const ownership=read('coordination/execution_state.json');
ownership.recorded_at_utc=at;ownership.activation_state='original_entry_window_closed';
ownership.automation_status_basis='Original human-assigned pilot preserved; entry expiry2026-10-05T03:59:22.131Z. Recovered collector reports entry_window_closed, zero orders and zero unconfirmed intents. This record is not a process mutex, renewed authority or profitability proof.';
ownership.production_pilot={...ownership.production_pilot,status:c.status,qualified_current_forecasts:c.eligibleResearchMarkets.length,orders_submitted:c.ordersSubmitted,entry_expires_at:a.entryExpiresAt,runtime_recovery_evidence:'data/production-recovery-20261005.json',latest_model_evidence:'research/current-polls-v1/assessments-20261005-v1.json',latest_model_installed:false};
fs.writeFileSync('coordination/execution_state.json',JSON.stringify(ownership,null,2)+'\n');
const base='research/current-polls-v1';
const files=['execution/production-entry-state.mjs','execution/production-entry-state.test.mjs','execution/settlement-controller.mjs','execution/settlement-controller.test.mjs',
 ...['capture.mjs','assessment.mjs','assessment.test.mjs','build.mjs','compare.mjs','README.md','sources.json','ballot-sources.json','assessments-20261005-v1.json','price-comparison-20261005-v1.json'].map(f=>base+'/'+f),
 'README.md','docs/CURRENT_STATE.md','coordination/execution_state.json','data/production-recovery-20261005.json','data/prepare-current-poll-publication-20261005.mjs'];
const sha=bytes=>createHash('sha256').update(bytes).digest('hex');
const elements=files.map(file=>{const content=fs.readFileSync(file,'utf8');if(/ace_[a-f0-9]{20,}|github_pat_[a-z0-9_]+|ghp_[a-z0-9]{20,}|Bearer\s+[a-z0-9._-]{20,}/i.test(content))throw Error('SECRET_SCAN_REJECTED');return{path:file,mode:'100644',type:'blob',content};});
const pack=JSON.stringify(elements),manifest=elements.map(e=>{const b=Buffer.from(e.content);return{path:e.path,bytes:b.length,sha256:sha(b),gitBlobSha1:createHash('sha1').update(Buffer.from('blob '+b.length+'\0')).update(b).digest('hex')};});
fs.writeFileSync('data/current-poll-publication-pack.json',pack);fs.writeFileSync('data/current-poll-publication-manifest.json',JSON.stringify(manifest,null,2)+'\n');
console.log(JSON.stringify({at,files:files.length,packChars:pack.length,runtime:milestone.runtime,research:milestone.research,profitabilityEstablished:false}));
