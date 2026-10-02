import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import {atomicPaperJson,paperFault} from './paper-persistence.mjs';

function fixture(t){const dir=fs.mkdtempSync(path.join(os.tmpdir(),'paper-persistence-'));t.after(()=>fs.rmSync(dir,{recursive:true,force:true}));const file=path.join(dir,'dip-scale-state.json');fs.writeFileSync(file,'{"old":true}');return {dir,file};}
const failure=code=>Object.assign(Error('SECRET raw C:/private/path account payload'),{code,path:'SECRET',stack:'SECRET'});

test('Windows sharing failures reuse one fsynced temp and atomically replace only after success',async t=>{
 const {dir,file}=fixture(t),temps=[],waits=[],calls=[];let n=0,clock=0;
 const io={...fs,writeSync(...args){calls.push('write');return fs.writeSync(...args);},fsyncSync(fd){calls.push('fsync');fs.fsyncSync(fd);},renameSync(tmp,to){calls.push('rename');temps.push(tmp);assert.equal(fs.readFileSync(to,'utf8'),'{"old":true}');if(++n<3)throw failure(n===1?'EBUSY':'EPERM');fs.renameSync(tmp,to);},unlinkSync(){assert.fail('Must not delete any destination');}};
 const result=await atomicPaperJson(file,{next:true},{io,platform:'win32',clock:()=>clock,sleep:async ms=>{waits.push(ms);clock+=ms;}});
 assert.equal(result.attempts,3);assert.deepEqual(waits,[100,200]);assert.equal(new Set(temps).size,1);assert.ok(calls.indexOf('fsync')<calls.indexOf('rename'));assert.deepEqual(JSON.parse(fs.readFileSync(file)),{next:true});assert.deepEqual(fs.readdirSync(dir),['dip-scale-state.json']);
});

test('exhausted retry preserves both complete old destination and new evidence with redacted fault',async t=>{
 const {dir,file}=fixture(t);let tries=0,time=0;const waits=[];
 let error;try{await atomicPaperJson(file,{private:'SECRET'},{platform:'win32',io:{...fs,renameSync(){tries++;throw failure('EPERM');}},clock:()=>time,sleep:async ms=>{waits.push(ms);time+=ms;}});}catch(e){error=e;}
 assert.equal(tries,4);assert.deepEqual(waits,[100,200,400]);assert.equal(fs.readFileSync(file,'utf8'),'{"old":true}');
 const fault=paperFault(error,{cohort:'dip-scale',at:'2026-10-02T14:00:00.000Z',durableSourceSequence:1685,attemptedSourceSequence:1687});
 assert.equal(fault.phase,'rename');assert.equal(fault.osCode,'EPERM');assert.equal(fault.attempts,4);assert.deepEqual(JSON.parse(fs.readFileSync(path.join(dir,fault.temporary))),{private:'SECRET'});assert.equal(JSON.stringify(fault).includes('SECRET'),false);assert.equal(String(error).includes('SECRET'),false);
 // A later operation never overwrites earlier failed evidence.
 await atomicPaperJson(file,{later:true});assert.ok(fs.existsSync(path.join(dir,fault.temporary)));
});

test('permission, disk, path and non-Windows EPERM failures are never repeatedly retried',async t=>{
 for(const [code,platform]of [['EACCES','win32'],['ENOSPC','win32'],['ENOENT','win32'],['EXDEV','win32'],['EPERM','linux']]){
  const {file}=fixture(t);let calls=0;await assert.rejects(atomicPaperJson(file,{x:1},{platform,io:{...fs,renameSync(){calls++;throw failure(code);}},sleep:async()=>assert.fail('Unexpected retry')}));assert.equal(calls,1);assert.equal(fs.readFileSync(file,'utf8'),'{"old":true}');
 }
});

test('short writes complete, but failed write or fsync can never reach rename',async t=>{
 const {file}=fixture(t);await atomicPaperJson(file,{unicode:'é'.repeat(20)},{io:{...fs,writeSync(fd,b,o,n){return fs.writeSync(fd,b,o,Math.min(3,n));}}});assert.equal(JSON.parse(fs.readFileSync(file)).unicode,'é'.repeat(20));
 for(const phase of ['write','fsync']){
  const f=fixture(t);let renamed=false;const io={...fs,renameSync(){renamed=true;assert.fail('Unflushed candidate was renamed');}};
  io[phase==='write'?'writeSync':'fsyncSync']=()=>{throw failure('ENOSPC');};
  await assert.rejects(atomicPaperJson(f.file,{x:1},{io}),e=>e.details.phase===phase);assert.equal(renamed,false);assert.equal(fs.readFileSync(f.file,'utf8'),'{"old":true}');assert.equal(fs.readdirSync(f.dir).length,2);
 }
});

test('STOP/owner guard is repeated after waits and retry time is bounded',async t=>{
 for(const mode of ['stop','deadline']){
  const {file}=fixture(t);let allowed=true,calls=0,time=0;
  await assert.rejects(atomicPaperJson(file,{x:1},{platform:'win32',canCommit:()=>allowed,clock:()=>time,io:{...fs,renameSync(){calls++;throw failure('EBUSY');}},sleep:async()=>{if(mode==='stop')allowed=false;else time=2000;}}));
  assert.equal(calls,1);assert.equal(fs.readFileSync(file,'utf8'),'{"old":true}');
 }
});

test('arbitrary errors, codes and paths cannot escape fault projection',()=>{
 const record=paperFault(Object.assign(failure('SECRET'),{details:{phase:'SECRET',temporary:'SECRET',target:'SECRET'}}),{cohort:'SECRET',phase:'SECRET',at:'SECRET',bankHash:'SECRET',durableSourceSequence:'SECRET'});
 assert.equal(JSON.stringify(record).includes('SECRET'),false);assert.equal(record.phase,'host');assert.equal(record.osCode,'UNKNOWN');
});
