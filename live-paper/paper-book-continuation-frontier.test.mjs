import test from 'node:test';import assert from 'node:assert/strict';import fs from 'node:fs';import os from 'node:os';import path from 'node:path';
import {payloadHash} from '../execution/ledger.mjs';import {paperBookContinuationFrontier} from './paper-book-continuation-frontier.mjs';
test('pair continuation permits unrelated and boundary records but rejects an unconsumed relevant book',t=>{
 const dir=fs.mkdtempSync(path.join(os.tmpdir(),'pair-frontier-'));t.after(()=>fs.rmSync(dir,{recursive:true,force:true}));const file=path.join(dir,'source');
 const rows=[];let previous='';const add=(kind,marketId)=>{const body={sequence:rows.length+1,previousHash:previous,kind,payload:marketId?{book:{marketId}}:{}};const row={...body,hash:payloadHash(body)};rows.push(row);previous=row.hash;fs.writeFileSync(file,rows.map(x=>JSON.stringify(x)+'\n').join(''));};
 add('book','381');const priorState={sourceSequence:1,sourceHash:previous},legs=[{marketId:'381'},{marketId:'382'}];add('book','500');add('session_stop');
 const check=()=>paperBookContinuationFrontier({file,endByte:fs.statSync(file).size,priorState,legs});assert.equal(check().unrelatedOrBoundarySuffix,2);
 add('book','382');assert.throws(check,/UNCONSUMED_RELEVANT_BOOK/);rows.pop();fs.writeFileSync(file,rows.map(x=>JSON.stringify(x)+'\n').join(''));
 priorState.sourceHash='a'.repeat(64);assert.throws(check,/FRONTIER_CHANGED/);priorState.sourceSequence=10;assert.throws(check,/FRONTIER_TRUNCATED/);
});
