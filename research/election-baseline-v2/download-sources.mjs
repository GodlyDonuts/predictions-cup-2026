import fs from 'node:fs';
import assert from 'node:assert/strict';
import { hash } from './model.mjs';
const base='research/election-baseline-v2', manifest=JSON.parse(fs.readFileSync(`${base}/source-manifest.json`));
fs.mkdirSync(`${base}/sources`,{recursive:true});
for(const s of manifest.sources){
  const target=`${base}/sources/${s.filename}`;
  if(fs.existsSync(target)){assert.equal(hash(fs.readFileSync(target)),s.sha256);continue;}
  const response=await fetch(s.url);assert(response.ok,`Public FEC download ${response.status}`);
  const bytes=Buffer.from(await response.arrayBuffer());assert.equal(bytes.length,s.bytes);assert.equal(hash(bytes),s.sha256);
  fs.writeFileSync(target,bytes,{flag:'wx'});
}
console.log(JSON.stringify({sourceIntegrityVerified:true,files:manifest.sources.length}));
