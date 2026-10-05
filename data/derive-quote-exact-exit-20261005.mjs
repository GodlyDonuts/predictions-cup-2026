import fs from 'node:fs';
import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
const source='data/realtime-quote-dataset-20261005.json',out='data/realtime-quote-exact-exit-dataset-20261005.json';
const raw=fs.readFileSync(source),original=JSON.parse(raw),sha256=createHash('sha256').update(raw).digest('hex');
const expected=JSON.parse(fs.readFileSync('data/realtime-quote-dataset-summary-20261005.json')).sha256;
assert.equal(sha256,expected,'Original frozen dataset changed');
function normalized(data){return{features:data.features,sources:data.sources,policies:data.policies.map(p=>({...p,id:p.id.replace(/-exact-exit$/,''),exitSlippageTicks:1})),quotes:data.quotes.map(q=>({...q,policyId:q.policyId.replace(/-exact-exit$/,'')})),rows:data.rows.map(r=>({...r,policyId:r.policyId.replace(/-exact-exit$/,''),counterfactualNetTicks:r.kind==='marked'?r.counterfactualNetTicks-1:r.counterfactualNetTicks}))};}
if(fs.existsSync(out)){
 const derived=JSON.parse(fs.readFileSync(out));assert.deepEqual(normalized(derived),{features:original.features,sources:original.sources,policies:original.policies,quotes:original.quotes,rows:original.rows});
 console.log(JSON.stringify({mode:'verified_existing_exact_exit_derivation',sourceSha256:sha256,rows:original.rows.length,changedLabels:original.rows.filter(r=>r.kind==='marked').length}));
}else{
 const data=structuredClone(original);data.policies=data.policies.map(p=>({...p,id:p.id+'-exact-exit',exitSlippageTicks:0}));for(const q of data.quotes)q.policyId+='-exact-exit';for(const r of data.rows){r.policyId+='-exact-exit';if(r.kind==='marked')r.counterfactualNetTicks++;}
 data.derivation={source,sha256,operation:'Add one tick to supported one-sided markouts; paired, unfilled and censored outcomes unchanged.',assumption:'Exact subsequent executable bid for one share; strict-through maker fills and uncertain execution remain counterfactual.',stressModel:'data/realtime-quote-selectors-20261005.json'};
 fs.writeFileSync(out,JSON.stringify(data)+'\n',{flag:'wx'});console.log(JSON.stringify({mode:'created_exact_exit_sensitivity',sourceSha256:sha256,rows:data.rows.length}));
}
