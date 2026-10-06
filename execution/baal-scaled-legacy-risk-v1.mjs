import fs from 'node:fs';import path from 'node:path';import {createHash} from 'node:crypto';
import {Journal} from '../live-paper/transport-runtime.mjs';import {payloadHash,validateExecutionState} from './core.mjs';
import {verifyScopedRestNoSellV2} from './no-rest-sell-reconciliation-v2.mjs';import {buildPairedOwnedExitRetirementV2} from './paired-owned-exit-retirement-v2.mjs';
const need=(x,c)=>{if(!x)throw Error('BAAL_LEGACY_'+c);},sha=b=>createHash('sha256').update(b).digest('hex');
// Historical retirement is verified at its original evidence clock. Current
// account/exchange state must still be freshly checked by the new execution host.
export function readBaalScaledLegacyRisk({runtimeDir,getOtherState}){
 const file=path.join(runtimeDir,'baal-scaled-retired-sources.json');const d=JSON.parse(fs.readFileSync(file,'utf8'));
 need(d.schemaVersion===1&&d.ownerId==='Codex'&&d.kind==='verified_retired_baal_sources_v1','DESCRIPTOR');
 const rel=d.repairJournal;need(typeof rel==='string'&&!path.isAbsolute(rel)&&!rel.split(/[\\/]/).includes('..'),'PATH');
 const target=path.resolve(runtimeDir,rel),root=fs.realpathSync(runtimeDir);need(fs.realpathSync(target).startsWith(root+path.sep),'PATH_SCOPE');
 const bytes=fs.readFileSync(target);need(bytes.length===d.repairJournalBytes&&sha(bytes)===d.repairJournalSha256,'ORIGINAL_SOURCE_CHANGED');
 const j=new Journal(target),row=j.last;need(j.sequence===d.repairSequence&&j.previous===d.repairHash&&row.event.kind==='retired','RETIREMENT_FRONTIER');
 const proof=verifyScopedRestNoSellV2(row.event.sellReverification),overlay=buildPairedOwnedExitRetirementV2(row.event.evidence);
 need(payloadHash(proof)===payloadHash(row.event.evidence.sellProof)&&payloadHash(overlay)===payloadHash(row.state.retirement)&&overlay.overlayHash===d.overlayHash&&overlay.currentOwnedQuantity===0&&overlay.currentReservedCash==='0.000000','VERIFIED_RETIREMENT');
 const core=validateExecutionState(getOtherState());need(core.tournamentId===d.tournamentId&&Object.values(core.intents).every(x=>x.status==='closed')&&Object.values(core.owned).every(x=>x.quantity===0),'OTHER_CORE_NOT_FLAT');
 return{complete:true,sourceHash:payloadHash({descriptor:d,repairHash:j.previous,core}),currentOwnedQuantity:0,currentReservedCash:'0.000000',unresolvedIntentIds:[],historicalRetirementAt:overlay.recordedAt,retirementIsNewAuthority:false};
}
