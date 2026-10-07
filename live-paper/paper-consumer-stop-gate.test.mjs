import test from 'node:test';import assert from 'node:assert/strict';import path from 'node:path';
import {paperConsumerStopped,frozenPaperStopView} from './paper-consumer-stop-gate.mjs';
test('financial or collector stop pauses a paper consumer without erasing inventory',()=>{
 for(const name of ['EXECUTION_STOP','COLLECTOR_STOP','STOP'])assert.equal(paperConsumerStopped('/runtime',{existsSync:p=>p===path.join('/runtime',name)}),true);
 assert.equal(paperConsumerStopped('/runtime',{existsSync:()=>false}),false);
 const previous={bankHash:'x',wallets:{one:{positions:[{cost:'0.295000'}]}}},view=frozenPaperStopView(previous);
 assert.equal(view.wallets,previous.wallets);assert.equal(view.bankHash,'x');assert.equal(view.status,'paused');assert.equal(view.executionAuthority,false);
});

