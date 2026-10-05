import fs from 'node:fs';import path from 'node:path';
import {payloadHash} from '../execution/ledger.mjs';
import {frozenPaperModule} from './frozen-paper-runtime.mjs';
import * as currentHost from './dual-no-pair-host-v1.mjs';
export async function openCollectorDualNoPaper({runtimeDir,universe,canOperate=()=>true}){
 const file=path.join(runtimeDir,'dual-no-pair-active.json');if(!fs.existsSync(file))return null;
 const p=JSON.parse(fs.readFileSync(file));if(p.schemaVersion!==1||!/^[a-z][a-z0-9-]{1,63}$/.test(p.cohort)||!/^[a-f0-9]{64}$/.test(p.bankHash))throw Error('DUAL_NO_COLLECTOR_POINTER');
 const dir=path.join(runtimeDir,'dual-no-pair-cohorts',p.cohort),bank=JSON.parse(fs.readFileSync(path.join(dir,'bank.json')));
 if(payloadHash(bank)!==p.bankHash||bank.review.tournamentId!==universe.tournamentId||bank.review.legs.some(l=>!universe.exchanges.some(x=>x.marketId===l.marketId&&x.exchangeId===l.exchangeId&&x.option==='YES')))throw Error('DUAL_NO_COLLECTOR_SCOPE');
 if(fs.existsSync(path.join(dir,'STOP'))||fs.existsSync(path.join(dir,'OBSERVER_STOP')))throw Error('DUAL_NO_COLLECTOR_STOPPED_RECONCILE_INVENTORY');
 const module=await frozenPaperModule({runtimeDir,bank,modulePath:'live-paper/dual-no-pair-host-v1.mjs',fallback:currentHost});
 return module.openDualNoPaperHost({runtimeDir,cohort:p.cohort,canOperate});
}
