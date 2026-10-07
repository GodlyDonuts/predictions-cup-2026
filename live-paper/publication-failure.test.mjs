import test from 'node:test';import assert from 'node:assert/strict';
import {publicationFailure} from './publication-failure.mjs';
test('publication failures retain bounded codes without disclosing arbitrary messages',()=>{
 const e=publicationFailure(Error('PUBLIC_BOOK_APPEND_CONFLICT'),'publishBook');
 assert.equal(e.code,'PUBLIC_BOOK_APPEND_CONFLICT');assert.equal(e.publicationOperation,'publishBook');assert.equal(e.fatal,true);assert.equal(e.durableRecordingFailed,true);
 for(const message of['private response body','C:\\private\\account.json','token=abc','A'.repeat(200)]){
  const f=publicationFailure(Error(message),'arbitrary');assert.equal(f.code,'UNCLASSIFIED_PUBLICATION_FAILURE');assert.equal(f.publicationOperation,'unknown');assert.ok(!f.message.includes(message));
 }
});

