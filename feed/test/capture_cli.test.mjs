import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import {execFileSync,spawn} from 'node:child_process';
import {fileURLToPath} from 'node:url';
import {Capture,sanitize} from '../capture.mjs';
import {replay} from '../cli.mjs';
import {at,scope} from './helpers.mjs';
const root=fileURLToPath(new URL('..',import.meta.url));
const fixture=fs.readFileSync(path.join(root,'fixtures/scenario.jsonl'),'utf8');
const temp=()=>fs.mkdtempSync(path.join(os.tmpdir(),'feed-test-'));
test('deterministic fixture replay matches golden and has no complete-history claim',async()=>{
 const expected=JSON.parse(fs.readFileSync(path.join(root,'fixtures/expected-status.json'),'utf8'));
 assert.deepEqual(await replay(fixture),expected);assert.deepEqual(await replay(fixture),await replay(fixture));assert.equal(expected.coverage.tradeTapeComplete,false);assert.equal(expected.coverage.universeComplete,false);
});
test('capture is append-only exclusive, private, sanitized, indexed and timestamped',async()=>{
 const dir=temp();try{const file=path.join(dir,'capture.jsonl');const c=new Capture(file);await replay(fixture,{capture:c});c.close();const rows=fs.readFileSync(file,'utf8').trim().split('\n').map(JSON.parse);
 assert.equal(rows[0].kind,'config');assert.equal(rows[0].format,'sig-feed-capture');assert.equal(rows.at(-1).kind,'summary');assert.ok(rows.every((r,i)=>r.captureIndex===i&&r.schemaVersion===1&&r.observedAt));assert.ok(rows.some(r=>r.kind==='snapshot'));assert.ok(rows.some(r=>r.kind==='invalidation'&&r.reason==='GAP'));
 assert.equal(fs.statSync(file).mode&0o777,0o600);assert.throws(()=>new Capture(file));assert.throws(()=>c.append({}),/CLOSED/);c.close();
 }finally{fs.rmSync(dir,{recursive:true,force:true});}
});
test('token and nested-header redaction with raw malformed payloads absent',()=>{
 const clean=sanitize({Authorization:'Bearer EXAMPLE',nested:{'X-API-Key':'SECRET',cookie:'COOKIE',safe:'prefix Bearer SHOULD_NOT_PERSIST'},text:'eyJhbGciOiJIUzI1NiJ9.eyJmb28iOiJiYXIifQ.c2lnbmF0dXJl'});
 const s=JSON.stringify(clean);for(const secret of ['EXAMPLE','SECRET','COOKIE','SHOULD_NOT_PERSIST','eyJ'])assert.equal(s.includes(secret),false);assert.ok(s.includes('[REDACTED]'));
});
test('capture schema is distinct, never silently interpreted as a fixture',async()=>{await assert.rejects(replay(JSON.stringify({format:'sig-feed-capture',schemaVersion:1,kind:'config',scope,observedAt:at(0)})),/UNKNOWN_REPLAY_SCHEMA/);});
test('CLI no arguments is off, live flags fail before credential/network use',()=>{
 const cli=path.join(root,'cli.mjs');const text=execFileSync(process.execPath,[cli],{encoding:'utf8',env:{...process.env,SIG_API_KEY:'TEST_MUST_NOT_READ'}});assert.match(text,/Collection is disabled/);
 for(const args of [['--live'],['--collect'],['--replay','missing','--live']])assert.throws(()=>execFileSync(process.execPath,[cli,...args],{stdio:'pipe'}));
});
test('CLI STOP startup does not even read missing input or create output',()=>{
 const dir=temp();try{fs.writeFileSync(path.join(dir,'STOP'),'stop');const output=execFileSync(process.execPath,[path.join(root,'cli.mjs'),'--replay','missing','--capture','out.jsonl'],{cwd:dir,encoding:'utf8'});assert.equal(JSON.parse(output).state,'stopped');assert.equal(fs.existsSync(path.join(dir,'out.jsonl')),false);}finally{fs.rmSync(dir,{recursive:true,force:true});}
});
test('malformed input error cannot leak raw secrets',()=>{
 const dir=temp();try{const f=path.join(dir,'bad.jsonl');fs.writeFileSync(f,'Bearer SECRET_DO_NOT_PRINT');let error;try{execFileSync(process.execPath,[path.join(root,'cli.mjs'),'--replay',f],{encoding:'utf8',stdio:'pipe'});}catch(e){error=e;}assert.ok(error);assert.equal(String(error.stderr).includes('SECRET_DO_NOT_PRINT'),false);}finally{fs.rmSync(dir,{recursive:true,force:true});}
});
test('replay stop callback yields cooperatively and returns stopped',async()=>{
 let calls=0;const result=await replay(fixture,{stop:()=>++calls>5});assert.equal(result.state,'stopped');
});
test('replay rejects missing config, future schema and nonmonotonic arrival',async()=>{
 await assert.rejects(replay(''));await assert.rejects(replay(JSON.stringify({format:'sig-feed-replay',schemaVersion:99,observedAt:at(0)})));
 const rows=fixture.trim().split('\n');const row=JSON.parse(rows[2]);row.observedAt=at(-1);rows[2]=JSON.stringify(row);await assert.rejects(replay(rows.join('\n')),/NONMONOTONIC/);
});
test('SIGTERM interrupts long offline replay and cleanly closes capture',async()=>{
 const dir=temp();try{
 const rows=fixture.trim().split('\n');const prefix=rows.slice(0,3);const tick=JSON.stringify({format:'sig-feed-replay',schemaVersion:1,kind:'tick',observedAt:at(1)});const f=path.join(dir,'long.jsonl');fs.writeFileSync(f,[...prefix,...Array(100000).fill(tick)].join('\n'));
 const out=path.join(dir,'capture.jsonl');const proc=spawn(process.execPath,[path.join(root,'cli.mjs'),'--replay',f,'--capture',out],{stdio:['ignore','pipe','pipe']});let stdout='';proc.stdout.on('data',x=>stdout+=x);let stderr='';proc.stderr.on('data',x=>stderr+=x);
 const monitor=setInterval(()=>{if(fs.existsSync(out)&&fs.statSync(out).size>0){clearInterval(monitor);proc.kill('SIGTERM');}},2);
 const deadline=setTimeout(()=>proc.kill('SIGKILL'),5000);const code=await new Promise(r=>proc.on('exit',r));clearTimeout(deadline);clearInterval(monitor);assert.equal(code,0,stderr);assert.equal(JSON.parse(stdout).state,'stopped');
 }finally{fs.rmSync(dir,{recursive:true,force:true});}
});
