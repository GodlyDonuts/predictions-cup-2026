import test from 'node:test';import assert from 'node:assert/strict';import fs from 'node:fs';import path from 'node:path';import os from 'node:os';import {createHash} from 'node:crypto';
import {collectFrozenPaperFiles} from './frozen-paper-graph.mjs';
test('snapshot closure includes transitive imports, dynamic literals and worker URLs without looping over cycles',t=>{
 const root=fs.mkdtempSync(path.join(os.tmpdir(),'sig-source-graph-'));t.after(()=>{assert.equal(path.dirname(root),os.tmpdir());assert.ok(path.basename(root).startsWith('sig-source-graph-'));fs.rmSync(root,{recursive:true,force:true});});
 fs.mkdirSync(path.join(root,'nested'));const sources={'entry.mjs':"import {x} from './nested/a.mjs'; import('./lazy.mjs'); new URL('./worker.mjs', import.meta.url); import fs from 'node:fs';",'nested/a.mjs':"import '../entry.mjs'; export const x=1;",'lazy.mjs':'export const y=2;','worker.mjs':'export const z=3;'};
 for(const[file,text]of Object.entries(sources))fs.writeFileSync(path.join(root,file),text);const graph=collectFrozenPaperFiles(root,['entry.mjs']);assert.deepEqual(Object.keys(graph).sort(),Object.keys(sources).sort());for(const[file,text]of Object.entries(sources))assert.equal(graph[file],createHash('sha256').update(text).digest('hex'));
 fs.writeFileSync(path.join(root,'lazy.mjs'),"import '../outside.mjs';");assert.throws(()=>collectFrozenPaperFiles(root,['entry.mjs']),/FROZEN_GRAPH_SCOPE/);fs.writeFileSync(path.join(root,'lazy.mjs'),"import './missing.mjs';");assert.throws(()=>collectFrozenPaperFiles(root,['entry.mjs']),/ENOENT/);
});
