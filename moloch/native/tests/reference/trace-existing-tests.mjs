// Development-only extraction of successful paths from the original test suite.
// The deployed Rust runtime has no JavaScript dependency.
import fs from 'node:fs';import path from 'node:path';import {pathToFileURL,fileURLToPath} from 'node:url';
const root=path.resolve(fileURLToPath(new URL('../../../../',import.meta.url)));
const temp=path.join(root,'runtime/moloch/reference-traces');fs.mkdirSync(temp,{recursive:true});
const engineUrl=pathToFileURL(path.join(root,'moloch/engine.mjs')).href;
const wrapper=`import fs from 'node:fs';import * as engine from ${JSON.stringify(engineUrl)};
export const {reportMoloch,assertMoloch}=engine;
let testName='',index=0;const states=new Map(),traces=[];
export function test(name,fn){testName=name;fn();}
export function createMoloch(policy){const state=engine.createMoloch(policy);const trace={name:testName+' '+(++index),policy:structuredClone(state.policy),steps:[]};states.set(state,trace);traces.push(trace);return state;}
const fields=['policyHash','events','cash','realized','fees','ownedCost','reservedCash','netLiquidationPnl','positions','fills','orders','activeOrders','unresolvedOrders','fillModel','conditionalQueueFills','profitSignalEligible','counts'];
const orderFields=['id','marketId','exchangeId','side','action','quantity','filled','price','createdAt','activationAt','expiresAt','retired','cancelEffectiveAt','participationCredit','barrier','executionStyle','inventoryExitReason','participationBps','throughFillCapacity','fillModel','touchAttributionBps','queueAhead','queueSame','queueBetter','queueVisibleSame','queueVisibleBetter','touchRemainder'];
export function advanceMoloch(state,event){const policy=structuredClone(state.policy),input=structuredClone(event),trace=states.get(state);if(!trace)throw Error('UNTRACKED_REFERENCE_STATE');const seed=trace.steps.length?null:structuredClone(state);const result=engine.advanceMoloch(state,event);if(seed)trace.seed=seed;const report=engine.reportMoloch(state);trace.steps.push({event:input,policy,expected:structuredClone(Object.fromEntries(fields.map(k=>[k,report[k]]))),fillRecords:structuredClone(state.fills),orderRecords:structuredClone(state.orders.map(q=>Object.fromEntries(orderFields.filter(k=>q[k]!==undefined).map(k=>[k,q[k]]))))});return result;}
export function save(file){fs.writeFileSync(file,JSON.stringify(traces.filter(t=>t.steps.length))+'\\n');return traces.filter(t=>t.steps.length);}
`;
fs.writeFileSync(path.join(temp,'engine.mjs'),wrapper);
const files=['engine.test.mjs','inventory-minimum-gain.test.mjs','entry-backoff.test.mjs','inventory-exit-depth.test.mjs','repriced-hedge.test.mjs','fixed-gain-exit.test.mjs','entry-deadline.test.mjs','inventory-clock.test.mjs','entry-flow.test.mjs'];
for(const file of files){let original=fs.readFileSync(path.join(root,'moloch',file),'utf8').replace("import test from 'node:test'","import {test} from './engine.mjs'");original=original.replace(/from\s+(['"])(\.{1,2}\/[^'"]+)\1/g,(full,quote,specifier)=>specifier==='./engine.mjs'?full:`from ${JSON.stringify(pathToFileURL(path.resolve(root,'moloch',specifier)).href)}`);fs.writeFileSync(path.join(temp,file),original);await import(pathToFileURL(path.join(temp,file)).href);}
const {save}=await import(pathToFileURL(path.join(temp,'engine.mjs')).href);const traces=save(new URL('./inventory-golden.json',import.meta.url));console.log(`${traces.length} original engine paths, ${traces.reduce((n,t)=>n+t.steps.length,0)} financial/order checkpoints`);
