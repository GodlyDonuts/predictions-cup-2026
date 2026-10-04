import fs from 'node:fs';
import {createHash} from 'node:crypto';
const sha=b=>createHash('sha256').update(b).digest('hex'),base='research/election-baseline-v2';
const pack='data/election-v2-publication-pack-20261004.json',i=process.argv.indexOf('--chunk');
if(i>=0){const n=Number(process.argv[i+1]);if(!Number.isSafeInteger(n)||n<0)throw Error('CHUNK');console.log(JSON.stringify(fs.readFileSync(pack,'utf8').slice(n*10000,(n+1)*10000)));}
else{
 const files=['AGENTS.md','README.md','coordination/execution_state.json','research/election-baseline-v1/README.md','research/election-baseline-v1/final-label-review.json',...['definition.json','model.mjs','model.test.mjs','final-stage-overrides.json','source-manifest.json','verify-fec-workbooks.py','download-sources.mjs','train.mjs','screen.mjs','README.md','model-summary.json','model-summary-affiliation-invariant.json','market-screen.json','market-screen-affiliation-invariant.json'].map(f=>base+'/'+f),'data/election-v2-milestone-20261004.json','data/prepare-election-v2-publication-20261004.mjs'];
 const milestone=JSON.parse(fs.readFileSync('data/election-v2-milestone-20261004.json'));
 milestone.codeHashes=Object.fromEntries(['model.mjs','train.mjs','screen.mjs','verify-fec-workbooks.py'].map(f=>[base+'/'+f,sha(fs.readFileSync(base+'/'+f))]));
 milestone.immutableReportFitReproduced={initial:true,affiliationInvariant:true};fs.writeFileSync('data/election-v2-milestone-20261004.json',JSON.stringify(milestone,null,2)+'\n');
 const elements=files.map(path=>{const content=fs.readFileSync(path,'utf8');if(/ace_[a-f0-9]{30,}|sk-[a-zA-Z0-9_-]{20,}|-----BEGIN [A-Z ]*PRIVATE KEY-----|https:\/\/[^\s"']+token=[A-Za-z0-9%]+/i.test(content))throw Error('SECRET_CHECK '+path);return{path,mode:'100644',type:'blob',content};});
 const bytes=JSON.stringify(elements);fs.writeFileSync(pack,bytes);console.log(JSON.stringify({files:files.length,characters:bytes.length,chunks:Math.ceil(bytes.length/10000),sha256:sha(bytes)}));
}
