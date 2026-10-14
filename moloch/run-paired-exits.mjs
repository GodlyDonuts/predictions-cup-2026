import fs from 'node:fs';import path from 'node:path';
import {run,completedTail,entryPolicyTransition,flatEntryTransition} from './run.mjs';
import {pairedClock} from './paired-clock.mjs';
import {payloadHash} from '../execution/ledger.mjs';

const [registrationFile,controlOutput,candidateOutput,exitPolicyFile,entryPolicyFile]=process.argv.slice(2);
if(!exitPolicyFile)throw Error('Usage: registration control-output candidate-output fixed-exit-policy');
const registration=JSON.parse(fs.readFileSync(registrationFile,'utf8'));
const exitPolicy=JSON.parse(fs.readFileSync(exitPolicyFile,'utf8'));
const entryPolicy=entryPolicyFile?JSON.parse(fs.readFileSync(entryPolicyFile,'utf8')):null;
if(entryPolicy&&registration.comparisonMode!=='entryInside')throw Error('PAIRED_REGISTERED_ENTRY_COMPARISON');
if(path.resolve(controlOutput)===path.resolve(candidateOutput))throw Error('PAIRED_DISTINCT_OUTPUTS');
const parent=completedTail(registration.parent);
if(parent.snapshot.hash!==registration.parentSnapshotHash)throw Error('PAIRED_PARENT_CHANGED');
const ids=registration.pairs.map(p=>p.exchangeId),states=[0,1].map(()=>structuredClone(parent.snapshot.state));
for(let i=0;i<2;i++){
 entryPolicyTransition(states[i],ids);flatEntryTransition(states[i],true,entryPolicy?exitPolicy:i?exitPolicy:null,i?entryPolicy:null);
 if(states[i].policyHash!==[registration.controlPolicyHash,registration.candidatePolicyHash][i])throw Error('PAIRED_REGISTERED_POLICY');
}
const strip=s=>{const p={...s.policy};delete p[entryPolicy?'insideTicks':'inventoryExitMode'];return p;};
if(payloadHash(strip(states[0]))!==payloadHash(strip(states[1])))throw Error('PAIRED_POLICY_CONFOUND');
const source=path.join(registration.source,'events.jsonl'),barrier=pairedClock();
const common=['--input',source,'--format','journal','--follow','--seconds','1800','--resume',registration.parent,'--entry-exchanges',ids.join(','),'--resume-entries','--execution-age'];
const jobs=[controlOutput,candidateOutput].map((out,i)=>run([...common,'--output',out,...(entryPolicy||i?['--exit-policy',exitPolicyFile]:[]),...(i&&entryPolicy?['--entry-policy',entryPolicyFile]:[])],{processingClock:barrier.clock(i)}).catch(error=>{barrier.abort(error);throw error;}));
await Promise.all(jobs);
