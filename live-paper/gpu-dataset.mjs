import fs from 'node:fs';import {fileURLToPath} from 'node:url';
import {captureRows,trainingSupport} from './ml-bank.mjs';
import {roundTripLabel,ROUND_TRIP} from './roundtrip-target.mjs';
import {ISOLATED} from './history-policy.mjs';import {digest,validateConfig} from './model.mjs';
// Export labels, not journal/account payloads. This file stays in restricted local runtime.
export function gpuDataset(raw,config,{since=null}={}){
 const c=validateConfig(config);if(c.feeBps!==0)throw Error('GPU baseline requires zero additional fees');
 const source=captureRows(raw,c,{historyPolicy:ISOLATED,since}),rows=source.rows;if(rows.length<80)throw Error('Insufficient accepted snapshots');
 const split=rows[Math.floor(rows.length*.65)].at,sets=[];
 for(const horizon of [60,180,300,600]){
  const labels=[],rejected={};for(let i=0;i<rows.length;i++){const out=roundTripLabel(rows,i,c,horizon*1000);if(out.label)labels.push(out.label);else rejected[out.reason]=(rejected[out.reason]||0)+1;}
  const support=trainingSupport(labels.filter(r=>Date.parse(r.labelAt)<Date.parse(split)),labels.filter(r=>Date.parse(r.at)>=Date.parse(split)),c.marketIds,'supported-instruments-v1');
  const clean=r=>({x:r.x,y:r.y,persistence:r.persistence,at:r.at,labelAt:r.labelAt,marketId:r.marketId,instrument:r.instrument});
  sets.push({horizon,coverage:support.coverage,eligible:support.eligible,train:support.train.map(clean),test:support.test.map(clean),rejected,accepted:labels.length});
 }
 const data={schemaVersion:1,family:'gpu_mlp',historyPolicy:ISOLATED,targetPolicy:ROUND_TRIP,trainingScope:'supported-instruments-v1',requireInstrumentCoverage:true,breadthContext:false,config:c,configHash:digest(c),sourceHash:source.sourceHash,sourceRecords:source.sourceRecords,trainingCaptureThrough:source.cutoff,developmentSplitAt:split,trainingWindowStart:since,acceptedSnapshots:rows.length,sets};
 return {...data,datasetHash:digest(data)};
}
if(process.argv[1]&&fileURLToPath(import.meta.url)===process.argv[1]){
 const arg=n=>{const i=process.argv.indexOf(n);if(i<0)throw Error('Missing '+n);return process.argv[i+1];};
 const data=gpuDataset(fs.readFileSync(arg('--journal'),'utf8'),JSON.parse(fs.readFileSync(arg('--config'),'utf8')),{since:process.argv.includes('--since')?arg('--since'):null});
 fs.writeFileSync(arg('--out'),JSON.stringify(data),{flag:'wx',mode:0o600});console.log(JSON.stringify({snapshots:data.acceptedSnapshots,cutoff:data.trainingCaptureThrough,sets:data.sets.map(s=>({horizon:s.horizon,train:s.train.length,test:s.test.length,rejected:s.rejected}))}));
}
