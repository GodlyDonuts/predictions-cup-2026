import test from 'node:test';import assert from 'node:assert/strict';import fs from 'node:fs';import os from 'node:os';import path from 'node:path';
import {paperExperimentRetired} from './experiment-retirement.mjs';
test('explicit retirement is separate from ordinary STOP, missing or malformed evidence',t=>{
 const dir=fs.mkdtempSync(path.join(os.tmpdir(),'sig-retirement-'));t.after(()=>{assert.equal(path.dirname(path.resolve(dir)),path.resolve(os.tmpdir()));assert.ok(path.basename(dir).startsWith('sig-retirement-'));fs.rmSync(dir,{recursive:true,force:true});});
 const record={owner:'Codex',at:'2026-10-03T00:52:00.000Z',reason:'Retired obsolete experiment at human request',restartRequires:'Explicitly selected future experiment'};
 for(const marker of ['DIP_STOP','SHOCK_STOP','ENTRY_COMPARISON_STOP','NATIVE_PRICE_PAPER_STOP']){
  const file=path.join(dir,marker);assert.equal(paperExperimentRetired(dir,marker),false);
  for(const content of ['human stop','{}','{',JSON.stringify({...record,owner:'other'}),JSON.stringify({...record,reason:'temporary maintenance'}),JSON.stringify({...record,at:null}),' '.repeat(4097)]){fs.writeFileSync(file,content);assert.equal(paperExperimentRetired(dir,marker),false);}
  fs.writeFileSync(file,JSON.stringify(record));assert.equal(paperExperimentRetired(dir,marker),true);
 }
 assert.throws(()=>paperExperimentRetired(dir,'STOP'),/INVALID_RETIREMENT_SCOPE/);
});
