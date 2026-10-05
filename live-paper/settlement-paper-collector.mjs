import fs from 'node:fs';
import path from 'node:path';
import {digest} from './model.mjs';
import {payloadHash} from '../execution/ledger.mjs';
import {frozenPaperModule} from './frozen-paper-runtime.mjs';
import * as currentHost from './settlement-paper-host.mjs';
import {validateUniverse} from './breadth.mjs';
import {wrapFundamentalEntryDrain} from './fundamental-entry-drain.mjs';
import {wrapFundamentalValueRevision} from './fundamental-value-revision-v1.mjs';

export async function openCollectorSettlementPaper({runtimeDir,universe,now=Date.now,workspaceDir=process.cwd()}){
 const file=path.join(runtimeDir,'settlement-paper-active.json');if(!fs.existsSync(file))return null;
 const pointer=JSON.parse(fs.readFileSync(file));if(!/^[a-z][a-z0-9-]{1,63}$/.test(pointer.cohort))throw Error('SETTLEMENT_COLLECTOR_SCOPE');
 const dir=path.join(runtimeDir,'settlement-paper-cohorts',pointer.cohort),bank=JSON.parse(fs.readFileSync(path.join(dir,'bank.json')));
 if(fs.existsSync(path.join(dir,'OBSERVER_STOP')))return null;
 const {hash:bankDeclaredHash,...bankRaw}=bank.universe,{hash:liveDeclaredHash,...liveRaw}=universe;
 const checked=validateUniverse(bankRaw,universe.tournamentId),liveUniverse=validateUniverse(liveRaw,universe.tournamentId);
 if(bankDeclaredHash&&bankDeclaredHash!==checked.hash||liveDeclaredHash&&liveDeclaredHash!==liveUniverse.hash)throw Error('SETTLEMENT_COLLECTOR_UNIVERSE_HASH');
 if(digest(bank)!==pointer.bankHash||checked.hash!==liveUniverse.hash||payloadHash(checked.exchanges)!==payloadHash(liveUniverse.exchanges))throw Error('SETTLEMENT_COLLECTOR_BINDING');
 const module=await frozenPaperModule({runtimeDir,bank,modulePath:'live-paper/settlement-paper-host.mjs',fallback:currentHost}),original=wrapFundamentalEntryDrain(await module.openSettlementPaper({runtimeDir,cohort:pointer.cohort,now,workspaceDir}),{runtimeDir,now});
 const originalLock=path.join(original.dir,'worker.lock'),originalToken=fs.readFileSync(originalLock,'utf8');
 let host;try{host=wrapFundamentalValueRevision(original,{runtimeDir,now,canOperate:()=>![path.join(workspaceDir,'STOP'),path.join(runtimeDir,'STOP'),path.join(runtimeDir,'EXECUTION_STOP'),path.join(runtimeDir,'COLLECTOR_STOP'),path.join(original.dir,'STOP'),path.join(original.dir,'OBSERVER_STOP')].some(f=>fs.existsSync(f))&&fs.existsSync(originalLock)&&fs.readFileSync(originalLock,'utf8')===originalToken});}catch(error){original.close();throw error;}
 const focus=()=>{const stats=host.status(),markets=new Map();for(const w of Object.values(stats.wallets)){for(const p of w.positions)markets.set(p.marketId,{marketId:p.marketId,phase:'held'});for(const p of w.pending)if(!markets.has(p.marketId))markets.set(p.marketId,{marketId:p.marketId,phase:p.action==='sell'?'pending_sell':'pending_buy'});}
  for(const r of bank.research)if(!host.entryDraining()&&r.settlementReviewed&&!r.flags.length&&now()<Date.parse(r.expiresAt)&&now()-Date.parse(r.sourceAt)<=bank.policy.maximumResearchAgeMs&&!markets.has(r.marketId))markets.set(r.marketId,{marketId:r.marketId,phase:'depth_required'});
  return{bankHash:pointer.bankHash,markets:[...markets.values()],ownedRiskReads:true,executionAuthority:false};};
 return{...host,focus:host.operatorRevisionActive?host.focus:focus};
}
