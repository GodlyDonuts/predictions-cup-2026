import fs from 'node:fs';import path from 'node:path';
import {atomicJson,payloadHash} from '../execution/ledger.mjs';
import {selectAuxiliaryReads} from './trade-tape.mjs';
const need=ok=>{if(!ok)throw Error('BAAL_AUXILIARY_STATE_INVALID');};
export function createBaalReadAttention({runtimeDir,tournamentId,canOperate=()=>true}){
 const file=path.join(runtimeDir,'baal-auxiliary-reservations.json'),scopeHash=payloadHash({tournamentId,policy:'maker_tape_three_cycles_history_one_v1'});
 let state=fs.existsSync(file)?JSON.parse(fs.readFileSync(file)):{schemaVersion:1,scopeHash,cyclesReserved:0};
 need(state.schemaVersion===1&&state.scopeHash===scopeHash&&Number.isSafeInteger(state.cyclesReserved)&&state.cyclesReserved>=0);
 return {reserve(budget,{makerActive=false}={}){
  need(Number.isSafeInteger(budget)&&budget>=0&&budget<=2&&typeof makerActive==='boolean');
  if(!budget||!canOperate())return{historyReads:0,tapeReads:0,totalReads:0,reservedOrdinal:null};
  const ordinal=state.cyclesReserved,tapeReads=makerActive?(ordinal%4===3?0:1):selectAuxiliaryReads(budget,ordinal).tapeReads;
  const next={...state,cyclesReserved:ordinal+1};need(Number.isSafeInteger(next.cyclesReserved));atomicJson(file,next);state=next;
  return{historyReads:budget-tapeReads,tapeReads,totalReads:budget,reservedOrdinal:ordinal,policy:makerActive?'maker_tape_three_cycles_history_one_v1':'alternating_history_tape_v1',executionAuthority:false};
 }};
}
