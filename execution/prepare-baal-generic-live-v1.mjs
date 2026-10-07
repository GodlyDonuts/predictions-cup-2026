// Explicit same-owner engineering migration; no API, key creation or restart.
import fs from 'node:fs';import path from 'node:path';import {fileURLToPath} from 'node:url';
import {readBaalCustodyFrontierV1} from './baal-custody-frontier-v1.mjs';
import {makeBaalGenericLiveRequestV1} from './baal-generic-live-host-v1.mjs';
import {validateUniverse} from '../live-paper/breadth.mjs';
import {windowsProcessSnapshot} from './prepare-baal-combined-custody-v6.mjs';
import {payloadHash} from './core.mjs';
const need=(v,c)=>{if(!v)throw Error('PREPARE_GENERIC_'+c);};
export function prepareBaalGenericLiveV1({runtimeDir,workspace,epoch,startedAt,processSnapshot,now=Date.now()}){
 need(path.isAbsolute(runtimeDir)&&path.isAbsolute(workspace)&&Number.isFinite(Date.parse(startedAt))&&Date.parse(startedAt)<=now&&now-Date.parse(startedAt)<=300000,'FRESH_ROOT_CLOCK');
 const stop=JSON.parse(fs.readFileSync(path.join(runtimeDir,'COLLECTOR_STOP')));
 need(stop.kind==='baal_root_controlled_engineering_stop_v1'&&stop.ownerId==='Codex'&&stop.humanStop===false&&stop.reason==='generic_v1_upgrade'&&Array.isArray(processSnapshot)&&!processSnapshot.some(p=>p.pid===stop.oldPid),'ROOT_STOP_AND_OLD_PID');
 need(!processSnapshot.some(p=>/live-paper[\\/]runner\.mjs/.test(p.commandLine)),'ANOTHER_COLLECTOR');
 need(!fs.existsSync(path.join(runtimeDir,'EXECUTION_STOP')),'HUMAN_OR_FINANCIAL_STOP');
 const frontier=readBaalCustodyFrontierV1({runtimeDir,workspace});
 for(const relative of ['baal-financial-owner/execution-owner.lock',`baal-combined-custody-runs/${frontier.currentState.scope.runId}/${frontier.migration.implementationEpoch}/coordinator/execution-owner.lock`,`baal-combined-custody-runs/${frontier.currentState.scope.runId}/${frontier.migration.implementationEpoch}/budget/execution-owner.lock`,'competition-runs/sig-pilot-median-20261006-human/execution-owner.lock'])need(!fs.existsSync(path.join(runtimeDir,relative)),'PRIOR_WRITER_LEASE');
 const universe=validateUniverse(JSON.parse(fs.readFileSync(path.join(workspace,'live-paper/price-universe.json'))),frontier.currentState.scope.tournamentId),request=makeBaalGenericLiveRequestV1({workspace,universe,frontier,epoch,startedAt,entryExpiresAt:new Date(Date.parse(startedAt)+3600000).toISOString()}),target=path.join(runtimeDir,'baal-generic-live-request-v1.json');need(!fs.existsSync(target),'NEW_REQUEST_ONLY');
 const fd=fs.openSync(target,'wx',0o600);try{fs.writeFileSync(fd,JSON.stringify(request,null,2)+'\n');fs.fsyncSync(fd);}finally{fs.closeSync(fd);}
 return{target,requestHash:payloadHash(request),epoch,sourceActorAnchor:frontier.currentActorAnchor,inheritedSummary:frontier.summary,entryExpiresAt:request.activation.entryExpiresAt,originalExitExpiresAt:request.activation.exitExpiresAt,marketScope:request.scope.exchanges.length,networkRequests:0};
}
if(process.argv[1]&&path.resolve(process.argv[1])===fileURLToPath(import.meta.url)){const args=new Map();for(let i=2;i<process.argv.length;i+=2)args.set(process.argv[i],process.argv[i+1]);console.log(JSON.stringify(prepareBaalGenericLiveV1({runtimeDir:path.join(process.env.LOCALAPPDATA,'SigTradingCup','paper-runtime'),workspace:process.cwd(),epoch:args.get('--epoch'),startedAt:args.get('--started-at'),processSnapshot:windowsProcessSnapshot()})));}
