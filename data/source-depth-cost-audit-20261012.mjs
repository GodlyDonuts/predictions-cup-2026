import fs from 'node:fs';import path from 'node:path';import {createHash} from 'node:crypto';
import {collectFrozenPaperFiles} from '../live-paper/frozen-paper-graph.mjs';
import {atomicJson,payloadHash} from '../execution/ledger.mjs';import {amount,price,money,SCALE} from '../paper/fixed.mjs';
import {marketableAllocationCostV1} from '../research/marketable-allocation-cost-v1.mjs';
import {assertSharedReferenceWalletV4} from '../paper/shared-reference-wallet-v4.mjs';
import {sharedWalletEconomicsV4} from '../research/shared-wallet-economics-v4.mjs';
const rt=path.join(process.env.LOCALAPPDATA,'SigTradingCup','paper-runtime'),stateFile='data/depth-aware-source-value-paper-20261009-state.json',bytes=fs.readFileSync(stateFile),state=JSON.parse(bytes),bank=JSON.parse(fs.readFileSync('data/depth-aware-source-value-paper-20261009-bank.json'));
assertSharedReferenceWalletV4(state,bank);const fills=state.fills;if(fills.some(f=>f.action!=='buy'))throw Error('DEPTH_AUDIT_BUY_ONLY_SCOPE');
const wanted=new Set(fills.map(f=>f.sourceHash)),samples=new Map(),base=path.join(rt,'production-metadata');
for(const name of fs.readdirSync(base)){
 if(!name.startsWith('ma-structural-prospective-'))continue;
 for(const marketId of new Set(fills.map(f=>f.marketId))){const b=path.join(base,name,marketId+'.book.json'),r=path.join(base,name,marketId+'.json');if(!fs.existsSync(b)||!fs.existsSync(r))continue;const sample={book:JSON.parse(fs.readFileSync(b)).book,rules:JSON.parse(fs.readFileSync(r))},hash=payloadHash(sample);if(wanted.has(hash))samples.set(hash,sample);}
}
const usage={},rows=[];let original=0n,alternative=0n;
for(const f of fills){
 const s=samples.get(f.sourceHash),spec=bank.instruments.find(x=>x.marketId===f.marketId);
 if(!s||!spec||s.book.marketId!==f.marketId||s.book.exchangeId!==f.exchangeId||s.book.receivedAt!==f.at||s.rules.marketId!==f.marketId||payloadHash(s.rules.root)!==s.rules.rootHash||s.rules.rootHash!==spec.rootHash||Date.parse(s.rules.receivedAt)>Date.parse(s.book.requestStartedAt)||Date.parse(s.book.requestStartedAt)<Date.parse(bank.cutoff))throw Error('DEPTH_AUDIT_ORIGINAL_SOURCE');
 const levels=f.side==='YES'?s.book.asks:s.book.bids;
 for(const a of f.depthAllocations){const raw=price(a.observedRawPrice),level=levels.find(l=>(f.side==='YES'?price(l.price):SCALE-price(l.price))===raw),key=f.marketId+':'+f.side+':buy:'+s.book.versionKey+':'+a.observedRawPrice;if(!level||a.key!==key)throw Error('DEPTH_AUDIT_LEVEL_BINDING');usage[key]=(usage[key]??0)+a.quantity;if(usage[key]>Math.floor(level.quantity/100))throw Error('DEPTH_AUDIT_REUSED_LIQUIDITY');}
 const modeled=marketableAllocationCostV1({limit:f.price,quantity:f.quantity,allocations:f.depthAllocations,adverseTicks:1});if(modeled.worstLimitReservation!==f.value)throw Error('DEPTH_AUDIT_ORIGINAL_COST');original+=amount(f.value);alternative+=amount(modeled.modeledPrincipal);rows.push({marketId:f.marketId,at:f.at,sourceHash:f.sourceHash,quantity:f.quantity,...modeled});
}
const economics=sharedWalletEconomicsV4({state,bank,at:new Date().toISOString()});if(!economics.allPositionsMarked||economics.rows.some(r=>!r.complete))throw Error('DEPTH_AUDIT_INCOMPLETE_MARK');const liquidation=economics.rows.reduce((n,r)=>n+amount(r.liquidationValue??'0.000000'),0n);
const report={at:new Date().toISOString(),codeIdentity:collectFrozenPaperFiles(process.cwd(),['data/source-depth-cost-audit-20261012.mjs']),bankHash:bank.bankHash,stateHash:createHash('sha256').update(bytes).digest('hex'),fills:fills.length,originalSourcesMatched:samples.size,shares:fills.reduce((n,f)=>n+f.quantity,0),originalPrincipal:money(original),perRungStressedPrincipal:money(alternative),worstLimitPremium:money(original-alternative),datedLiquidation:money(liquidation),datedQuotePnlUnderOriginalCost:money(liquidation-original),datedQuotePnlUnderPerRungStress:money(liquidation-alternative),markClocks:economics.rows.map(r=>r.bookAt),allMarksFresh:economics.allMarksFresh,rows,originalWalletsModified:false,apiRequests:0,actualOrders:0,actualFillProven:false,originalIntentLatencyNotReproved:true,historicalCostDiagnosticNotNewPolicyProfit:true,profitabilityEstablished:false};atomicJson('data/source-depth-cost-audit-20261012.json',report);console.log(JSON.stringify({fills:report.fills,sources:report.originalSourcesMatched,shares:report.shares,premium:report.worstLimitPremium,originalPnl:report.datedQuotePnlUnderOriginalCost,alternativePnl:report.datedQuotePnlUnderPerRungStress}));
