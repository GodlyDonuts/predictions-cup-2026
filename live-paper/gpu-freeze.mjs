import fs from 'node:fs';import path from 'node:path';import {fileURLToPath} from 'node:url';
import {digest} from './model.mjs';import {predict} from './ml-bank.mjs';import {verifySource,bindTrainingSource} from './ml-shadow.mjs';
export function validateGPU(data,bank,raw){
 const {datasetHash,...body}=data;if(datasetHash!==digest(body)||bank.datasetHash!==datasetHash||bank.configHash!==digest(data.config)||digest(bank.config)!==digest(data.config))throw Error('GPU dataset/config integrity failed');
 bindTrainingSource(verifySource(raw),bank);if(bank.family!=='gpu_mlp'||bank.config.feeBps!==0||bank.modelsTrained!==bank.models.length||bank.models.length!==bank.inferenceParity.length||!bank.models.length)throw Error('Invalid GPU bank');
 const ids=new Set(),sets=new Map(data.sets.map(s=>[s.horizon,s]));let maxError=0;
 for(const m of bank.models){
  if(ids.has(m.id)||m.family!=='gpu_mlp')throw Error('Duplicate/invalid model');ids.add(m.id);
  const s=sets.get(m.horizon);if(!s?.eligible||s.train.length<20||s.test.length<10||m.trainRows!==s.train.length||m.testRows!==s.test.length||digest(m.coverage)!==digest(s.coverage))throw Error('Coverage mismatch');
  if(s.train.some(r=>Date.parse(r.labelAt)>=Date.parse(bank.developmentSplitAt))||s.test.some(r=>Date.parse(r.at)<Date.parse(bank.developmentSplitAt)))throw Error('Unpurged chronology');
  const maxLabel=new Date(Math.max(...s.train.map(r=>Date.parse(r.labelAt)))).toISOString();if(Date.parse(m.maxTrainingLabelAt)!==Date.parse(maxLabel))throw Error('Training maturity mismatch');
  for(let j=0;j<m.width;j++){const mean=s.train.reduce((n,r)=>n+r.x[j],0)/s.train.length,scale=Math.max(1e-6,Math.sqrt(s.train.reduce((n,r)=>n+(r.x[j]-mean)**2,0)/s.train.length));if(Math.abs(m.means[j]-mean)>1e-8||Math.abs(m.scales[j]-scale)>1e-8)throw Error('Training normalization mismatch');}
  const targetMean=s.train.reduce((n,r)=>n+r.y,0)/s.train.length,targetScale=Math.max(1e-6,Math.sqrt(s.train.reduce((n,r)=>n+(r.y-targetMean)**2,0)/s.train.length));if(Math.abs(m.targetMean-targetMean)>1e-10||Math.abs(m.targetScale-targetScale)>1e-10)throw Error('Training target scaling mismatch');
  const rmse=Math.sqrt(s.test.reduce((n,r)=>n+(predict(m,r.x)-r.y)**2,0)/s.test.length);if(Math.abs(rmse-m.rmse)>1e-10)throw Error('Development score mismatch');
  const p=bank.inferenceParity.find(p=>p.id===m.id),err=Math.abs(predict(m,p.x)-p.expected);if(err>1e-10)throw Error('GPU inference parity failed');maxError=Math.max(maxError,err);
 }
 for(const m of bank.selected)if(digest(m)!==digest(bank.models.find(x=>x.id===m.id)))throw Error('Selected model changed');
 return {models:bank.models.length,selected:bank.selected.length,parityChecks:bank.inferenceParity.length,maxAbsoluteInferenceError:maxError,sourceVerified:true,purgedChronologyVerified:true,trainingOnlyNormalizationVerified:true,developmentScoresReproduced:true};
}
if(process.argv[1]&&fileURLToPath(import.meta.url)===process.argv[1]){
 const arg=n=>{const i=process.argv.indexOf(n);if(i<0)throw Error('Missing '+n);return process.argv[i+1];},root=arg('--cohort');
 const data=JSON.parse(fs.readFileSync(path.join(root,'training-dataset.json'))),bank=JSON.parse(fs.readFileSync(path.join(root,'candidate-bank.json')));
 const evidence=validateGPU(data,bank,fs.readFileSync(arg('--journal'),'utf8'));
 bank.forecastLifecycle='revoke-on-invalidation-v2';bank.validation=evidence;fs.writeFileSync(path.join(root,'ml-bank.json'),JSON.stringify(bank,null,2),{flag:'wx',mode:0o600});console.log(JSON.stringify({...evidence,cutoff:bank.trainingCaptureThrough,configuredWallets:bank.selected.length*3,feeBps:0}));
}
