import fs from 'node:fs';
import path from 'node:path';

// This operator setting reallocates the existing queue; it is never a rate or
// execution permit. An expired setting restores normal collection immediately.
export function readProfitSprint(runtimeDir, now=Date.now(), filesystem=fs) {
 const file=path.join(runtimeDir,'COLLECTION_PROFIT_SPRINT');
 if(!filesystem.existsSync(file))return null;
 const v=JSON.parse(filesystem.readFileSync(file,'utf8'));
 if(!v||v.schemaVersion!==1||v.owner!=='Codex'||v.mode!=='source_bound_and_owned_risk'
  ||Object.keys(v).some(k=>!['schemaVersion','owner','mode','createdAt','expiresAt'].includes(k))
  ||!Number.isSafeInteger(now)||!Number.isFinite(Date.parse(v.createdAt))||!Number.isFinite(Date.parse(v.expiresAt))
  ||Date.parse(v.createdAt)>now||Date.parse(v.expiresAt)<=Date.parse(v.createdAt)
  ||Date.parse(v.expiresAt)-Date.parse(v.createdAt)>3600000)throw Error('INVALID_PROFIT_SPRINT_SETTING');
 return now<Date.parse(v.expiresAt)?v:null;
}

export function passiveHostsCompletelyFlat(statuses) {
 return Array.isArray(statuses)&&statuses.every(s=>s?.failed===false&&Array.isArray(s.riskMarketIds)&&!s.riskMarketIds.length
  &&s.wallets&&Object.keys(s.wallets).length>0&&Object.values(s.wallets).every(w=>Array.isArray(w.positions)&&!w.positions.length
   &&Array.isArray(w.unresolvedQuotes)&&!w.unresolvedQuotes.length&&Number(w.reserved)===0));
}

export function scopedRiskMarketIds(officialIds=[],paperFocus=null) {
 const paperIds=paperFocus?.ownedRiskReads===true?(paperFocus.markets||[]).filter(x=>['held','exit_pending','entry_pending','pending_sell','pending_buy'].includes(x.phase)).map(x=>x.marketId):[];
 const ids=[...new Set([...officialIds,...paperIds])];
 if(ids.some(id=>typeof id!=='string'||!/^([1-9]\d*)$/.test(id)||!Number.isSafeInteger(Number(id))))throw Error('INVALID_PROFIT_SPRINT_RISK');
 return ids;
}

export function focusProfitSprintReads(plan,{enabled=false,sourceMarketIds=[],riskMarketIds=[]}={}) {
 if(!enabled)return plan;
 const keep=new Set([...sourceMarketIds,...riskMarketIds,...(plan.managedMarketIds||[])]);
 if([...keep].some(id=>typeof id!=='string'||!/^([1-9]\d*)$/.test(id)))throw Error('INVALID_PROFIT_SPRINT_SCOPE');
 const reads=plan.reads.filter(r=>keep.has(r.marketId));
 // No source-bound slot can be manufactured; this only removes unrelated reads
 // from the already frozen four-book plan, preserving order/depth/metadata.
 return {...plan,reads,entryMarketIds:(plan.entryMarketIds||[]).filter(id=>keep.has(id)),
  profitSprint:{mode:'source_bound_and_owned_risk',removedBookMarketIds:plan.reads.filter(r=>!keep.has(r.marketId)).map(r=>r.marketId),executionAuthority:false}};
}
