import test from 'node:test';import assert from 'node:assert/strict';import fs from 'node:fs';import os from 'node:os';import path from 'node:path';
import {admissionAt,assertDisabledBoundary,capture,validatePairs,loadCredential} from './capture.mjs';import {captureInvocation} from '../capture-invocation.mjs';
test('100/min rolling admissions are paced across multiple minutes without bursts',()=>{
 const budget={times:[],nextAt:0};let now=0;for(let i=0;i<300;i++){now=admissionAt(budget,now);budget.times.push(now);for(const t of budget.times)assert.ok(budget.times.filter(x=>x>t-60000&&x<=t).length<=100);}
 assert.equal(budget.times[99],59400);assert.equal(budget.times[100],60001);assert.ok(budget.times[299]>=179402);assert.ok(admissionAt({...budget,nextAt:now+60000},now)>=now+60000);
 assert.throws(()=>admissionAt(budget,now,101),/LIMIT/);assert.throws(()=>admissionAt(budget,0),/CLOCK/);
});
test('network stays off unless every independent control is deliberately enabled',async t=>{
 const control={networkEnabled:true,tradingEnabled:false,readsPerMinute:100};assert.doesNotThrow(()=>assertDisabledBoundary(control,{MOLOCH_NETWORK_ENABLED:'1'},false));
 for(const [c,e,m]of [[{...control,networkEnabled:false},{MOLOCH_NETWORK_ENABLED:'1'},false],[control,{},false],[control,{MOLOCH_NETWORK_ENABLED:'1'},true],[{...control,tradingEnabled:true},{MOLOCH_NETWORK_ENABLED:'1'},false]])assert.throws(()=>assertDisabledBoundary(c,e,m),/NETWORK_DISABLED/);
 const dir=fs.mkdtempSync(path.join(os.tmpdir(),'moloch-vps-test-'));t.after(()=>fs.rmSync(dir,{recursive:true,force:true}));fs.writeFileSync(path.join(dir,'deployment.json'),JSON.stringify({...control,networkEnabled:false}));
 await assert.rejects(capture({pairs:'missing',output:path.join(dir,'capture')},{MOLOCH_STATE_DIR:dir,MOLOCH_NETWORK_ENABLED:'0'}),/NETWORK_DISABLED/);assert.equal(fs.existsSync(path.join(dir,'capture.lock')),false);assert.equal(fs.existsSync(path.join(dir,'capture')),false);
});
test('Linux uses Node capture while Windows retains its existing adapter; scopes remain bounded',()=>{
 const options={scopedFile:'pairs.json',recoveryWindowsFile:'windows.json',capture:'capture',cycles:2,realtime:true};
 assert.equal(captureInvocation('/repo',options,'linux').command,process.execPath);assert.ok(captureInvocation('/repo',options,'linux').args.includes('--realtime'));assert.equal(captureInvocation('C:/repo',options,'win32').command,'pwsh');
 assert.throws(()=>validatePairs([{marketId:'1',exchangeId:'2'},{marketId:'1',exchangeId:'3'}]),/SCOPE/);assert.throws(()=>validatePairs([]),/SCOPE/);
});
test('Linux credential loading refuses readable-by-others files and symlinks',{skip:process.platform==='win32'},t=>{
 const dir=fs.mkdtempSync(path.join(os.tmpdir(),'moloch-key-test-'));t.after(()=>fs.rmSync(dir,{recursive:true,force:true}));const key=path.join(dir,'key');fs.writeFileSync(key,'DUMMY_CREDENTIAL_123456',{mode:0o600});assert.equal(loadCredential(key),'DUMMY_CREDENTIAL_123456');fs.chmodSync(key,0o644);assert.throws(()=>loadCredential(key),/PERMISSIONS/);fs.chmodSync(key,0o600);const link=path.join(dir,'link');fs.symlinkSync(key,link);assert.throws(()=>loadCredential(link),/PERMISSIONS/);
});
