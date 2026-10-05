import fs from 'node:fs';import path from 'node:path';import {fileURLToPath} from 'node:url';
import {collectFrozenPaperFiles} from './frozen-paper-graph.mjs';
import {payloadHash} from '../execution/ledger.mjs';
import {nativeJournalBinding,journalEngineIdentity} from './native-journal-restore.mjs';
export const coupledRustCollectorCodeIdentity=()=>collectFrozenPaperFiles(path.dirname(path.dirname(fileURLToPath(import.meta.url))),['live-paper/coupled-rust-transport-config.mjs','live-paper/coupled-maker-host-rust.mjs']);
export function readCoupledRustTransport(runtimeDir,active){
 const file=path.join(runtimeDir,'coupled-bulk-rust-transport.json');if(!fs.existsSync(file))return null;
 const c=JSON.parse(fs.readFileSync(file));
 if(Object.keys(c).some(k=>!['schemaVersion','mode','cohort','bankHash','codeIdentity','sourceSnapshot','nativeBackend'].includes(k))||c.schemaVersion!==1||c.mode!=='original_coupled_wallet_rust_transport_v1'||c.cohort!==active.cohort||c.bankHash!==active.bankHash||!/^[a-f0-9]{64}$/.test(c.bankHash)||payloadHash(c.codeIdentity)!==payloadHash(coupledRustCollectorCodeIdentity())||payloadHash(c.codeIdentity)!==c.sourceSnapshot)throw Error('COUPLED_RUST_TRANSPORT_CONFIG');
 nativeJournalBinding(runtimeDir,c.nativeBackend);
 if(journalEngineIdentity(path.join(runtimeDir,'journal-native-sources',c.nativeBackend.engineHash))!==c.nativeBackend.engineHash)throw Error('COUPLED_RUST_TRANSPORT_NATIVE_SOURCE');
 return c;
}
export function coupledBulkStartupFailure(runtimeDir,filesystem=fs){
 let prior=null;try{const active=JSON.parse(filesystem.readFileSync(path.join(runtimeDir,'coupled-bulk-active.json')));if(/^[a-z][a-z0-9-]{1,70}$/.test(active.cohort))prior=JSON.parse(filesystem.readFileSync(path.join(runtimeDir,'coupled-bulk-cohorts',active.cohort,'stats.json')));}catch{}
 return{status:'failed',failed:true,executionAuthority:false,accountingPreserved:true,accountingVerified:false,liabilitiesUnverified:true,lastKnownAccounting:prior,knownRiskMarketIds:Array.isArray(prior?.riskMarketIds)?prior.riskMarketIds.filter(id=>typeof id==='string'&&/^[1-9]\d*$/.test(id)):[],reason:'COUPLED_STARTUP_FAILED_RECONCILIATION_REQUIRED'};
}
