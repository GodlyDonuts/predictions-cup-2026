import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import {execFileSync} from 'node:child_process';
test('the unchanged 12-case publication contract also passes against the faster implementation',()=>{
 const dir=fs.mkdtempSync(path.join(os.tmpdir(),'sig-publisher-contract-'));
 try{
  const source=fs.readFileSync(new URL('./book-publication.test.mjs',import.meta.url),'utf8').replace("'./book-publication.mjs'",JSON.stringify(new URL('./book-publication-v2.mjs',import.meta.url).href));
  const file=path.join(dir,'contract.test.mjs');fs.writeFileSync(file,source);
  const env={...process.env};delete env.NODE_TEST_CONTEXT;
  const output=execFileSync(process.execPath,['--test','--test-reporter=tap',file],{env,encoding:'utf8',timeout:30000});assert.match(output,/# fail 0/);assert.match(output,/# tests 12/);
 }finally{fs.rmSync(dir,{recursive:true,force:true});}
});
