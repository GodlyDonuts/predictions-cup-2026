import fs from 'node:fs';import path from 'node:path';import {publishFlowRecoveryPriorityV1} from './flow-recovery-priority-v1.mjs';import {atomicJson} from '../execution/ledger.mjs';
export function publishFlowRecoveryPriorityV2(options){
 const report=publishFlowRecoveryPriorityV1(options),session=JSON.parse(fs.readFileSync(path.join(options.runtimeDir,'realtime-session.json')));
 const bound=report.source.file===session.file?{...report,tournamentId:session.scope.tournamentId}:{schemaVersion:1,at:report.at,marketIds:[],tournamentId:session.scope.tournamentId,source:{file:session.file},executionAuthority:false};atomicJson(path.join(options.runtimeDir,'flow-recovery-priority-v1.json'),bound);return bound;
}
export function readFlowRecoveryPriorityV2({runtimeDir,scope,now=Date.now}){
 try{const report=JSON.parse(fs.readFileSync(path.join(runtimeDir,'flow-recovery-priority-v1.json'))),session=JSON.parse(fs.readFileSync(path.join(runtimeDir,'realtime-session.json'))),age=now()-Date.parse(report.at),members=new Set(scope.markets.map(m=>m.marketId));if(report.schemaVersion!==1||report.executionAuthority!==false||report.source?.file!==session.file||report.tournamentId!==scope.tournamentId||session.scope?.tournamentId!==scope.tournamentId||!Number.isFinite(age)||age<0||age>120000||!Array.isArray(report.marketIds)||report.marketIds.length>4||new Set(report.marketIds).size!==report.marketIds.length||report.marketIds.some(id=>!members.has(id)))return null;return{at:report.at,marketIds:report.marketIds};}catch{return null;}
}
