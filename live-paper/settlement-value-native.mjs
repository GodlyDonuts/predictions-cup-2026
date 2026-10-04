import fs from 'node:fs';
import path from 'node:path';
import {spawnSync} from 'node:child_process';
import {digest} from './model.mjs';
import {amount,money} from '../paper/fixed.mjs';
import {sha} from './native-dip-proof-v1.mjs';
import {nativeExecutable} from './rust-backend.mjs';

// A finite proposal against a caller's verified public prefix. It never writes
// a wallet, submits an intent, or refreshes the assessment's original clock.
export function settlementInputFromProbability({bank,stats,publication,account,asOfMs}){
 if(stats.bankHash!==digest(bank)||publication.sequence!==stats.sourceSequence||publication.hash!==stats.sourceHash||!Number.isSafeInteger(asOfMs))throw Error('SETTLEMENT_ADAPTER_FRONTIER');
 const wallet=stats.wallets['3000'];
 if(!wallet||wallet.recentFills.length!==wallet.fills||wallet.pending.length)throw Error('SETTLEMENT_ADAPTER_COMPLETE_WALLET_REQUIRED');
 const groupFor=r=>{
  const evidence=bank.researchEvidence.find(x=>x.researchId===r.id);
  if(!evidence?.binding?.event)throw Error('SETTLEMENT_ADAPTER_EVENT_REQUIRED');
  return 'race_'+sha(evidence.binding.event).slice(0,24);
 };
 const positions=wallet.positions.map(p=>{
  const r=bank.research.find(x=>x.exchangeId+':'+x.side===p.instrument);
  if(!r)throw Error('SETTLEMENT_ADAPTER_POSITION_UNBOUND');
  return{candidateId:r.marketId+':'+r.side,quantity:p.quantity,cost:p.cost,enteredAtMs:Date.parse(p.enteredAt)};
 });
 const candidates=bank.research.map(r=>{
  const publicBook=publication.books[r.exchangeId+':'+r.side];
  const b=publicBook?.book||publicBook;
  // Publication V2 books carry their immutable row and publication clock.
  const availableAt=publicBook?.availableAt||publicBook?.publishedAt;
  const book=b?{requestStartedAtMs:Date.parse(b.requestStartedAt),receivedAtMs:Date.parse(b.receivedAt),availableAtMs:Date.parse(availableAt),sequence:Number(b.sequence),sourceHash:publicBook.publicationHash||publicBook.hash||publication.hash,representation:r.side==='YES'?'direct_yes':'canonical_yes_complement',bids:b.bids,asks:b.asks}:null;
  if(book&&Object.values(book).some(x=>typeof x==='number'&&!Number.isFinite(x)))throw Error('SETTLEMENT_ADAPTER_BOOK_CLOCK');
  const eligibility=account?{positionsAtMs:Date.parse(account.positionsAt),ordersAtMs:Date.parse(account.ordersAt),knownAtMs:Math.max(Date.parse(account.positionsAt),Date.parse(account.ordersAt)),complete:true,positionExcluded:account.positionExchangeIds.includes(r.exchangeId),openOrderExcluded:account.openOrderExchangeIds.includes(r.exchangeId)}:null;
  return{id:r.marketId+':'+r.side,marketId:r.marketId,exchangeId:r.exchangeId,side:r.side,predicateHash:r.predicateHash,riskGroups:['national_election',groupFor(r)],
   // Supplied tournament horizon, not a server resolution observation.
   settlesAtMs:Date.parse('2026-11-04T17:00:00.000Z'),research:{id:r.id,predicateHash:r.predicateHash,side:r.side,lower:r.lower,upper:r.upper,recordedAtMs:Date.parse(r.recordedAt),sourceAtMs:Date.parse(r.sourceAt),expiresAtMs:Date.parse(r.expiresAt),method:r.method,settlementReviewed:r.settlementReviewed,flags:r.flags,sources:r.sources},book,account:eligibility};
 });
 const cumulativeBuys=wallet.recentFills.filter(f=>f.action==='buy').reduce((n,f)=>n+amount(f.price)*BigInt(f.quantity),0n);
 return{schemaVersion:1,mode:'settlement_value_policy_v1',asOfMs,
  policy:{minimumEdge:'0.020000',maximumSpread:'0.050000',maximumBookAgeMs:20000,maximumResearchAgeMs:21600000,fractionalKellyBps:2500,slippageTicks:1,depthFractionBps:100,maxOrderCost:'1.000000',maxExposure:'10.000000',cashReserve:'90.000000',cumulativeBuyBudget:'100.000000',groupCaps:{national_election:'5.000000',...Object.fromEntries(candidates.map(c=>[c.riskGroups[1],'2.000000']))}},
  wallet:{capital:bank.config.initialCash,cash:wallet.cash,cumulativeBuys:money(cumulativeBuys),positions,reservations:[]},candidates};
}
export function runSettlementValue({input,dir,executable=nativeExecutable}){
 fs.mkdirSync(dir,{recursive:true,mode:0o700});const file=path.join(dir,'input.json'),out=path.join(dir,'output.json'),bytes=JSON.stringify(input)+'\n';
 const binaryHash=sha(fs.readFileSync(executable));fs.writeFileSync(file,bytes,{flag:'wx',mode:0o600});
 const start=performance.now(),child=spawnSync(executable,['settlement-policy','--input',file,'--expected-sha',sha(bytes),'--out',out],{encoding:'utf8',windowsHide:true,maxBuffer:4096});
 if(child.status!==0)throw Error('SETTLEMENT_ADAPTER_NATIVE_REJECTED');
 const result=JSON.parse(fs.readFileSync(out));
 if(result.inputHash!==sha(bytes)||sha(fs.readFileSync(executable))!==binaryHash||result.executionAuthority!==false||result.fills!==0||result.competitionOrders!==0)throw Error('SETTLEMENT_ADAPTER_OUTPUT');
 return{result,binaryHash,inputHash:sha(bytes),outputHash:sha(fs.readFileSync(out)),wallMs:performance.now()-start};
}
