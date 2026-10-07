import test from 'node:test';import assert from 'node:assert/strict';import {staleFundingFailureCanRetry as retry} from './funding-failure-classification-v1.mjs';
test('only an unmutated pre-fetch clock failure is retryable',()=>{
 const e=Error('REST_NO_RECEIPT_TIME'),s={journalUnchanged:true,alreadyFailed:false};assert.equal(retry(e,s),true);
 for(const change of[{durableRecordingFailed:true},{mayHaveExecuted:true},{code:'UNKNOWN_ACCEPTANCE'},{code:'EIO'}])assert.equal(retry(Object.assign(Error('REST_NO_RECEIPT_TIME'),change),s),false);
 assert.equal(retry(e,{...s,journalUnchanged:false}),false);assert.equal(retry(e,{...s,alreadyFailed:true}),false);
});
