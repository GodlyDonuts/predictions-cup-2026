import fs from 'node:fs';import vm from 'node:vm';import test from 'node:test';import assert from 'node:assert/strict';
import {frozenPaperStopView} from './paper-consumer-stop-gate.mjs';import {publicationFailure} from './publication-failure.mjs';
const source=fs.readFileSync(new URL('./runner.mjs',import.meta.url),'utf8'),start=source.indexOf('function publishBookEvent('),end=source.indexOf('async function collectHistory',start),fragment=source.slice(start,end);
test('actual runner STOP gate preserves held paper accounting and avoids a stopped-consumer exception',()=>{
 for(const stopped of [true,false]){
  let calls=0;const wallets={one:{positions:[{cost:'0.295000'}]}},runtime={settlementPaper:{wallets,bankHash:'f'.repeat(64)}};
  const context={runtime,runtimeDir:'/runtime',paperConsumerStopped:()=>stopped,frozenPaperStopView,publicationFailure,bookPublication:{publishBook:()=>true},settlementPaper:{consume(){calls++;throw Error('FUNDAMENTAL_REVISION_STOPPED');},status:()=>runtime.settlementPaper},dualNoPaper:null,dualNoPaperV2:null,noMaker:null,baalPair:null,stableHosts:()=>[]};
  vm.runInNewContext(fragment,context);
  if(stopped){assert.equal(context.publishBookEvent('publishBook',{}),true);assert.equal(calls,0);assert.equal(runtime.settlementPaper.status,'paused');assert.equal(runtime.settlementPaper.wallets,wallets);}
  else{assert.throws(()=>context.publishBookEvent('publishBook',{}),/Public book recording failed/);assert.equal(calls,1);assert.equal(runtime.settlementPaper.status,'failed');}
 }
});

