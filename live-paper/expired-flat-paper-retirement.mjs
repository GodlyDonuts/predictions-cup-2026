import fs from 'node:fs';import path from 'node:path';
import {payloadHash} from '../execution/ledger.mjs';
export function flatExpiredPaper(status,expiresAt,now=Date.now()){
 if(status?.failed!==false||!Array.isArray(status.riskMarketIds)||status.riskMarketIds.length||!Number.isFinite(Date.parse(expiresAt))||Date.parse(expiresAt)>now)return false;
 if(status.wallets)return Object.keys(status.wallets).length>0&&Object.values(status.wallets).every(w=>Array.isArray(w.positions)&&w.positions.length===0&&Array.isArray(w.unresolvedQuotes)&&w.unresolvedQuotes.length===0&&w.reserved==='0.000000');
 return Array.isArray(status.positions)&&status.positions.length===0&&status.pending===null&&status.package===null&&status.reserved==='0.000000';
}
export function readExpiredFlatPaperRetirement(runtimeDir,pointerFile,folder){
 const file=path.join(runtimeDir,pointerFile);if(!fs.existsSync(file))return null;
 const active=JSON.parse(fs.readFileSync(file));if(!/^[a-z][a-z0-9-]{1,80}$/.test(active.cohort))throw Error('RETIREMENT_SCOPE');
 const dir=path.join(runtimeDir,folder,active.cohort),marker=path.join(dir,'STOP');if(!fs.existsSync(marker))return null;
 const m=JSON.parse(fs.readFileSync(marker));if(m.source!=='expired_flat_paper_retirement_20261005')return null;
 const bank=JSON.parse(fs.readFileSync(path.join(dir,'bank.json')));if(m.owner!=='Codex'||m.bankHash!==active.bankHash||payloadHash(bank)!==m.bankHash||!flatExpiredPaper(m.verifiedStatus,m.reviewExpiresAt)||payloadHash(m.verifiedStatus)!==m.statusHash)throw Error('RETIREMENT_PROOF');
 return{...m.verifiedStatus,status:'retired',accountingPreserved:true,retiredAt:m.at,retirementReason:'expired_and_verified_flat',executionAuthority:false};
}
