import test from 'node:test';import assert from 'node:assert/strict';import fs from 'node:fs';import os from 'node:os';import path from 'node:path';import {execFileSync} from 'node:child_process';import {fileURLToPath} from 'node:url';
import {bookExportEvents,replayEvents} from './run.mjs';
const root=fileURLToPath(new URL('../',import.meta.url)),binary=path.join(root,'moloch/native/target/release/moloch-replay'+(process.platform==='win32'?'.exe':''));
test('Rust virtual-clock event output preserves Moloch financial decisions on the actual bounded SIG book archive',{skip:!fs.existsSync(binary)},t=>{
  const dir=fs.mkdtempSync(path.join(os.tmpdir(),'moloch-native-parity-'));t.after(()=>fs.rmSync(dir,{recursive:true,force:true}));
  const input=path.join(root,'data/maker-public-book-export-20261012.json'),output=path.join(dir,'events.ndjson'),stats=path.join(dir,'stats.json');
  execFileSync(binary,['--input',input,'--format','book-export','--output',output,'--stats',stats],{stdio:'pipe'});
  const expected=replayEvents(bookExportEvents(JSON.parse(fs.readFileSync(input))));
  const actual=replayEvents(fs.readFileSync(output,'utf8').trim().split('\n').map(JSON.parse));
  assert.deepEqual(actual.state,expected.state);assert.deepEqual(actual.report,expected.report);
  const perf=JSON.parse(fs.readFileSync(stats));assert.equal(perf.sleeps,0);assert.equal(perf.events,20);assert.equal(perf.profitabilityEstablished,false);
});
