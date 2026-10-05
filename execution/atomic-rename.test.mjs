import test from 'node:test';import assert from 'node:assert/strict';import fs from 'node:fs';import os from 'node:os';import path from 'node:path';
import {replaceFileSync} from './atomic-rename.mjs';
test('temporary sharing denial retries identical synced bytes without removing the last valid view',t=>{
 const dir=fs.mkdtempSync(path.join(os.tmpdir(),'baal-rename-'));t.after(()=>{assert.equal(path.dirname(path.resolve(dir)),path.resolve(os.tmpdir()));assert(path.basename(dir).startsWith('baal-rename-'));fs.rmSync(dir,{recursive:true,force:true});});
 const source=path.join(dir,'next.tmp'),target=path.join(dir,'stats.json');fs.writeFileSync(source,'next');fs.writeFileSync(target,'prior');let calls=0;const delays=[];
 const result=replaceFileSync(source,target,{platform:'win32',sleep:n=>delays.push(n),renameSync:(a,b)=>{assert.equal(a,source);assert.equal(b,target);assert.equal(fs.readFileSync(source,'utf8'),'next');assert.equal(fs.readFileSync(target,'utf8'),'prior');if(++calls<3)throw Object.assign(Error('temporary sharing denial'),{code:'EPERM'});fs.renameSync(a,b);}});
 assert.deepEqual(delays,[10,20]);assert.equal(result.attempts,3);assert.equal(fs.readFileSync(target,'utf8'),'next');assert(!fs.existsSync(source));
});
test('persistent replacement failures remain bounded and retain the original error',()=>{
 let calls=0;const delays=[],error=Object.assign(Error('persistent permission failure'),{code:'EACCES'});
 assert.throws(()=>replaceFileSync('source','target',{platform:'win32',sleep:n=>delays.push(n),renameSync:()=>{calls++;throw error}}),e=>e===error);assert.equal(calls,6);assert.equal(delays.reduce((a,b)=>a+b,0),310);
});
test('other operating systems and storage errors are never hidden by sharing retries',()=>{
 for(const [platform,code]of[['linux','EPERM'],['win32','ENOSPC'],['win32','EIO']]){let calls=0;assert.throws(()=>replaceFileSync('source','target',{platform,sleep:()=>{throw Error('unexpected wait')},renameSync:()=>{calls++;throw Object.assign(Error('storage error'),{code})}}),{code});assert.equal(calls,1);}
});
