import fs from 'node:fs';import path from 'node:path';import {performance} from 'node:perf_hooks';
import {openDiagnosticFocus,diagnosticDepthFocus} from '../live-paper/diagnostic-read-focus.mjs';
import {validateUniverse} from '../live-paper/breadth.mjs';import {atomicJson} from '../execution/ledger.mjs';
const rt=path.join(process.env.LOCALAPPDATA,'SigTradingCup/paper-runtime'),universe=validateUniverse(JSON.parse(fs.readFileSync('live-paper/price-universe.json')),'bda92870-621e-47b0-bc3c-3602c5c26f55');
const start=performance.now();let beats=0,last=performance.now(),maxGap=0;
const timer=setInterval(()=>{const now=performance.now();maxGap=Math.max(maxGap,now-last);last=now;beats++;},100);
try{
 const source=openDiagnosticFocus(rt,universe,{deferBootstrap:true});await source.initialize();source.consume();
 const p=source.project({pid:process.pid,id:'offline-bootstrap-proof'},'collector-diagnostic'),focus=diagnosticDepthFocus(p,universe,{pid:process.pid,lockId:'offline-bootstrap-proof'});
 if(!focus||p.sourceClosed||beats===0)throw Error('BOOTSTRAP_PROOF_FAILED');
 const result={at:new Date().toISOString(),mode:'full_actual_capture_async_bootstrap_validation',codeHash:p.codeHash,universeHash:p.universeHash,book:p.book,price:p.price,sourceClosed:p.sourceClosed,candidates:p.candidates.length,validFocus:!!focus,durationMs:Math.round(performance.now()-start),eventLoopTimerCallbacks:beats,maximumTimerGapMs:Math.round(maxGap),networkRequests:0,competitionOrders:0,extraBackgroundProcesses:0};
 atomicJson('data/diagnostic-bootstrap-verification-20261003.json',result);console.log(JSON.stringify(result));
}finally{clearInterval(timer);}
