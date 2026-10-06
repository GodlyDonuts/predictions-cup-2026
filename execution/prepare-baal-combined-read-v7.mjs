// Root-controlled engineering upgrade. Reuses every original economic identity
// and deadline; this tool has no client and does not create an activation.
import fs from 'node:fs';
import path from 'node:path';
import {fileURLToPath} from 'node:url';
import {payloadHash} from './core.mjs';
import {baalCombinedCustodyV6Identity} from './baal-combined-custody-host-v6.mjs';
import {baalCombinedCustodyV7Identity} from './baal-combined-custody-host-v7.mjs';
const need=(v,c)=>{if(!v)throw Error('READ_UPGRADE_V7_'+c);};
export function prepareBaalCombinedReadV7({runtimeDir,workspace,recordedAt=new Date().toISOString()}){
 need(path.isAbsolute(runtimeDir)&&path.isAbsolute(workspace),'ABSOLUTE_PATHS');
 const d=JSON.parse(fs.readFileSync(path.join(runtimeDir,'baal-combined-custody-request-v6.json')));
 need(d.kind==='baal_combined_custody_host_v6'&&d.ownerId==='Codex'&&payloadHash(d.codeIdentity)===payloadHash(baalCombinedCustodyV6Identity(workspace)),'ORIGINAL_DESCRIPTOR_GRAPH');
 need(Number.isFinite(Date.parse(recordedAt))&&Date.parse(recordedAt)>=Date.parse(d.recordedAt),'CLOCK');
 const epoch=path.join(runtimeDir,'baal-combined-custody-runs',d.runId,d.migration.implementationEpoch);
 for(const dir of[path.join(runtimeDir,'baal-financial-owner'),path.join(epoch,'coordinator'),path.join(epoch,'budget')])need(!fs.existsSync(path.join(dir,'execution-owner.lock')),'ACTIVE_WRITER');
 need(!fs.existsSync(path.join(runtimeDir,'EXECUTION_STOP')),'EXECUTION_STOP');
 need(fs.existsSync(path.join(runtimeDir,'COLLECTOR_STOP')),'ENGINEERING_STOP_REQUIRED');
 const stop=JSON.parse(fs.readFileSync(path.join(runtimeDir,'COLLECTOR_STOP')));
 need(stop.kind==='baal_root_controlled_engineering_stop_v1'&&stop.ownerId==='Codex'&&stop.humanStop===false&&stop.reason==='combined_read_v7_upgrade'&&stop.runId===d.runId,'ENGINEERING_STOP_PROVENANCE');
 const target=path.join(runtimeDir,'baal-combined-read-upgrade-v7.json');need(!fs.existsSync(target),'NEW_UPGRADE_ONLY');
 const upgrade={schemaVersion:1,kind:'baal_combined_read_upgrade_v7',ownerId:d.ownerId,runId:d.runId,tournamentId:d.tournamentId,originalDescriptorHash:payloadHash(d),codeIdentity:baalCombinedCustodyV7Identity(workspace),recordedAt};
 const fd=fs.openSync(target,'wx',0o600);try{fs.writeFileSync(fd,JSON.stringify(upgrade,null,2)+'\n');fs.fsyncSync(fd);}finally{fs.closeSync(fd);}
 return{target,upgradeHash:payloadHash(upgrade),originalDescriptorHash:upgrade.originalDescriptorHash,originalHedgeExpiresAt:d.activation.hedgeExpiresAt,originalExitExpiresAt:d.activation.exitExpiresAt,networkRequests:0};
}
if(process.argv[1]&&path.resolve(process.argv[1])===fileURLToPath(import.meta.url))console.log(JSON.stringify(prepareBaalCombinedReadV7({runtimeDir:path.join(process.env.LOCALAPPDATA,'SigTradingCup','paper-runtime'),workspace:process.cwd()})));
