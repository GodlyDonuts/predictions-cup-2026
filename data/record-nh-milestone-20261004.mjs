import fs from 'node:fs';import path from 'node:path';import {atomicJson} from '../execution/ledger.mjs';
const r=path.join(process.env.LOCALAPPDATA,'SigTradingCup/paper-runtime'),read=f=>JSON.parse(fs.readFileSync(f));
const live=read('data/nh-paper-v5-live-20261004.json'),preserved=read('data/nh-paper-v5-preservation-20261004.json'),at=new Date().toISOString();
const milestone={at,status:'deployed_and_live_recovery_verified',claim:5979346397,finding:5979379893,research:5978588243,tests:42,
 evidence:['data/nh-paper-v5-live-20261004.json','data/nh-paper-v5-preservation-20261004.json','data/nh-conditional-refund-review-20261004.json','data/nh-source-capture-20261004.json'],
 cohorts:live.cohorts,bankHashes:{maker:preserved.maker.bankHash,pair:preserved.pair.bankHash},recoveryCompletion:live.recovery.completion,
 wallets:live.wallets,reviewExpiresAt:live.reviewExpiresAt,accountingReset:false,profitabilityEstablished:false,additionalReadSlots:0,additionalPersistentProcesses:0,competitionOrders:0,
 unresolved:['Independent review of checkpoint pinning, interleaved recovery pagination and preserved continuation frontiers','Fresh subsequent maker and paired economic measurements; no profit evidence from a reservation release','Optimize collector cold-start public source replay using existing pinned native validator']};
atomicJson('data/nh-paper-v5-milestone-20261004.json',milestone);
const q=read('data/quant-development-state.json');q.updatedAt=at;q.activeProcesses={collector:live.pids.collector,paper:live.pids.paper,dashboard:live.pids.dashboard,total:3};q.nhPaperV5Milestone=milestone;
for(const item of q.baalBacklog||[])if(item.evidence===5978588243){item.status='completed';item.completedAt=at;item.integrationEvidence=milestone.evidence[0];}
q.baalBacklog??=[];q.baalBacklog.push({status:'completed',task:'Repair moving post-expiry maker checkpoints and recover original quote coverage within existing serialized tape slots',completedAt:at,evidence:milestone.evidence[0]});
q.next='Optimize the remaining collector public-journal cold startup with pinned Rust validation and exact state/admission parity, preserving all frozen wallets. Measure fresh maker/pair fills and causal cost gates; resolved reservations are not profit. Existing read budget and Daenerys contest ownership remain unchanged. No extra clients, tokens or contest writes.';atomicJson('data/quant-development-state.json',q);
const m=read('data/daenerys-monitor.json');m.updatedAt=at;m.github.comments=read(path.join(r,'nh-issue-fingerprints.json'));m.nhPaperV5Milestone=milestone;atomicJson('data/daenerys-monitor.json',m);
console.log(JSON.stringify({at,status:milestone.status,cohorts:milestone.cohorts,fingerprints:m.github.comments.length,profitabilityEstablished:false}));
