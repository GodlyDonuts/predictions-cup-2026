import test from 'node:test';import assert from 'node:assert/strict';
import {verifyJournal,replayVariant,compareEpisode} from './compare.mjs';import {createMoloch} from './engine.mjs';import {payloadHash} from '../execution/ledger.mjs';
test('comparison rejects broken journal links, edited events and truncated tails',()=>{
  const body={sequence:1,previous:'',event:{type:'tick',receivedAt:'2026-10-13T00:00:00Z'},stateHash:'test'},row={...body,hash:payloadHash(body)},terminal={sequence:1,previous:row.hash};
  assert.equal(verifyJournal([row],terminal).length,1);
  assert.throws(()=>verifyJournal([{...row,event:{type:'gap'}}],terminal),/JOURNAL_CHAIN/);
  assert.throws(()=>verifyJournal([],terminal),/TERMINAL_JOURNAL/);
});
test('variant replays original clocks and carries cash and liabilities without mutating parent',()=>{
  const initial=createMoloch({initialCash:'123.456789'}),before=payloadHash(initial),events=[{type:'tick',receivedAt:'2026-10-13T00:00:00Z',processedAt:'2026-10-13T00:00:01Z'}];
  const r=replayVariant(initial,events,{renewBeforeMs:30000});assert.equal(r.report.cash,'123.456789');assert.equal(r.state.lastAt,events[0].processedAt);assert.equal(payloadHash(initial),before);assert.equal(r.newFills.length,0);
});
