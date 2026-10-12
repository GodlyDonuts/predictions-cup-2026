import fs from 'node:fs';
import crypto from 'node:crypto';
import {payloadHash} from '../../execution/core.mjs';
import {createSharedReferenceWalletV6,advanceSharedReferenceWalletV6} from '../../paper/shared-reference-wallet-v6.mjs';
import {createSharedReferenceWalletV7,advanceSharedReferenceWalletV7} from '../../paper/shared-reference-wallet-v7.mjs';
import {sharedWalletEconomicsV6} from '../shared-wallet-economics-v6.mjs';
const cutoff='2026-10-12T08:00:00.000Z',root={id:'synthetic-rules'},rootHash=payloadHash(root);
const bankBody={exitMode:'net_harvest',cutoff,initialCash:'1000.000000',capitalLimit:'50.000000',fees:'0.000000',executionAuthority:false,latencyMs:3000,orderLifetimeMs:120000,maxBookAgeMs:20000,harvestMinimum:'0.005000',reentryGap:'0.005000',cooldownMs:60000,kellyFractionBps:1000,eventFractionBps:1000,instruments:[{marketId:'1',exchangeId:'1',eventId:'1',side:'YES',rootHash,sourceAt:cutoff,entryExpiresAt:'2026-10-12T09:00:00.000Z',lowerValue:'0.700000',minimumGap:'0.005000',maximumPrincipal:'50.000000'}]};
const b6={...bankBody,bankHash:payloadHash(bankBody)},risk={maxHoldingMs:30000,maximumAdverseTicks:4},b7Body={...bankBody,inventoryRisk:risk},b7={...b7Body,bankHash:payloadHash(b7Body)};
const sample=([second,bid,ask,depth=10000])=>{const at=new Date(Date.parse(cutoff)+second*1000).toISOString();return{book:{marketId:'1',exchangeId:'1',versionKey:'v'+second,requestStartedAt:at,receivedAt:at,bids:[{price:bid,quantity:depth}],asks:[{price:ask,quantity:depth}]},rules:{marketId:'1',root,rootHash,requestStartedAt:at,receivedAt:at}};};
const opening=[[1,'0.555000','0.560000'],[8,'0.545000','0.550000']];
// Predeclared adversarial and benign synthetic paths; not selected market data.
const paths={
 continuedDecline:[...opening,[12,'0.500000','0.505000'],[15,'0.500000','0.505000'],[18,'0.500000','0.505000'],[25,'0.400000','0.405000'],[28,'0.400000','0.405000'],[40,'0.400000','0.405000']],
 quickProfitableRebound:[...opening,[12,'0.580000','0.585000'],[15,'0.580000','0.585000'],[129,'0.580000','0.585000'],[132,'0.580000','0.585000']],
 stopThenRecovery:[...opening,[12,'0.500000','0.505000'],[15,'0.500000','0.505000'],[18,'0.500000','0.505000'],[129,'0.600000','0.605000'],[132,'0.600000','0.605000']],
 unchangedLosingLiquidation:[...opening,[39,'0.545000','0.550000'],[42,'0.545000','0.550000'],[45,'0.545000','0.550000']],
 liquidityDisappears:[...opening,[39,'0.545000','0.550000'],[42,'0.400000','0.405000',100],[45,'0.400000','0.405000',100]],
};
function run(bank,create,advance,path){const state=create(bank);const trace=path.map(row=>{const receipt=sample(row),decision=advance(state,bank,receipt);return{at:receipt.book.receivedAt,decision,positions:structuredClone(state.positions),pending:structuredClone(state.pending),realized:state.realized};});
 const last=path.at(-1)[0],markAt=new Date(Date.parse(cutoff)+(last+1)*1000).toISOString();return{bankHash:bank.bankHash,economics:sharedWalletEconomicsV6({state,bank,at:markAt}),riskStops:state.marketRiskStop??{},fills:state.fills,trace};}
const results=Object.entries(paths).map(([name,path])=>({name,path,original:run(b6,createSharedReferenceWalletV6,advanceSharedReferenceWalletV6,path),inventoryRisk:run(b7,createSharedReferenceWalletV7,advanceSharedReferenceWalletV7,path)}));
const codeIdentity=Object.fromEntries(['../../paper/shared-reference-wallet-v6.mjs','../../paper/shared-reference-wallet-v7.mjs','../shared-wallet-economics-v6.mjs','compare.mjs'].map(p=>[p,crypto.createHash('sha256').update(fs.readFileSync(new URL(p,import.meta.url))).digest('hex')]));
const output={schemaVersion:1,at:new Date().toISOString(),mode:'synthetic_causal_funded_inventory_policy_comparison',codeIdentity,policy:risk,results,networkRequests:0,actualOrders:0,marketProfitabilityEstablished:false,
 limitations:['Synthetic mechanism paths only; no strategy chosen by this score.','Risk exits may underperform recovery; losing inventory can remain if later exit depth disappears.','Declared horizon and adverse ticks are assumptions, not calibrated optimal values.','Later REST crossing supports hypothetical fills, not actual resting queue fills.','No source bank, collector, consumer or trading activation changed.']};
fs.writeFileSync(new URL('results.json',import.meta.url),JSON.stringify(output,null,2)+'\n');
console.log(JSON.stringify(results.map(r=>({name:r.name,original:r.original.economics,inventoryRisk:r.inventoryRisk.economics})),null,2));
