import fs from 'node:fs';
import path from 'node:path';
import assert from 'node:assert/strict';
import {spawnSync} from 'node:child_process';
import {sha} from '../live-paper/native-dip-proof-v1.mjs';

assert(process.argv.slice(2).every(x=>x==='--future'));
const future=process.argv.includes('--future');
const runtime=path.join(process.env.LOCALAPPDATA,'SigTradingCup','paper-runtime');
const read=file=>JSON.parse(fs.readFileSync(file));
const protocolFile='data/native-mechanisms-protocol-20261004.json',protocol=read(protocolFile);
const dir=path.join(runtime,'research-cohorts',protocol.cohort),baseDir=path.join(runtime,'research-cohorts',protocol.baseCohort);
assert.equal(sha(fs.readFileSync(protocolFile)),sha(fs.readFileSync(path.join(dir,'protocol.json'))));
const executable=path.join(runtime,'native-builds',protocol.binaryHash,'baal-backend.exe');
assert.equal(sha(fs.readFileSync(executable)),protocol.binaryHash);
assert.equal(sha(fs.readFileSync(path.join(dir,'base-declaration.json'))),protocol.baseDeclarationHash);
const before=Object.fromEntries(['native-quote-active.json','native-dip-intent-active.json','coordinated-network-budget.json','competition-activation.json'].map(file=>[file,sha(fs.readFileSync(path.join(runtime,file)))]));
const launch=(command,sources,name,extra=[])=>{
 const output=path.join(dir,name);assert(!fs.existsSync(output));const start=performance.now();
 const child=spawnSync(executable,[command,'--declaration',path.join(dir,'base-declaration.json'),'--sources',sources,'--workers','8','--out',output,...extra],{encoding:'utf8',windowsHide:true,maxBuffer:65536});
 assert.equal(child.status,0,child.stderr);const receipt=JSON.parse(child.stdout),result=read(output);
 assert.equal(result.capture.engineHash,protocol.engineHash);
 return{result,receipt,wallMs:performance.now()-start,outputHash:sha(fs.readFileSync(output)),name};
};
let sources=path.join(dir,'development.sources.json');
const stamp=Date.now();let prefixProof=null;
if(future){
 // The existing archived host's immutable verified result supplies prefix hashes;
 // no separate network client or unbound hash of a moving file is introduced.
 const pointer=read(path.join(baseDir,'host-latest.json')),raw=fs.readFileSync(path.join(baseDir,pointer.output));
 assert.equal(sha(raw),pointer.outputHash);const prior=JSON.parse(raw),declaration=read(path.join(dir,'base-declaration.json'));
 const input=Object.fromEntries(Object.entries(prior.capture.sources).map(([key,value])=>[key,{
  path:path.join(runtime,{books:'public-books.jsonl',quotes:'market-prices.jsonl',eligibility:'account-eligibility.jsonl'}[key]),
  bytes:value.bytes,hash:value.hash,checkpoints:[declaration.development[key]]
 }]));
 assert(prior.capture.sources.quotes.through>protocol.signalAfter,'No post-cutoff quote prefix yet');
 sources=path.join(dir,'future-'+stamp+'.sources.json');fs.writeFileSync(sources,JSON.stringify(input),{flag:'wx'});
 prefixProof={hostOutputHash:pointer.outputHash,hostEngineHash:prior.capture.engineHash,inputHash:sha(fs.readFileSync(sources)),quotePrefix:prior.capture.sources.quotes};
}
const study=launch('quote-mechanisms',sources,future?'future-'+stamp+'.json':'development.json',future?['--signal-after',String(protocol.signalAfter)]:[]);
const m=study.result.markouts;assert.equal(m.candidates,36);assert.equal(m.fees,0);assert.equal(m.competitionOrders,0);
let preservation=null;
if(!future){
 const original=read(path.join(baseDir,'historical-side-endpoints-v1.json'));
 const side=launch('quote-side-endpoints',sources,'original-path-preservation.json');
 for(const label of ['yes','no']){
  assert.deepEqual(side.result.markouts[label].cards,original.markouts[label].cards);
  assert.deepEqual(side.result.markouts[label].bookBoundarySensitivityCards,original.markouts[label].bookBoundarySensitivityCards);
  assert.deepEqual(side.result.markouts[label+'EndpointSensitivity'].cards,original.markouts[label+'EndpointSensitivity'].cards);
 }
 const wallet=launch('quote-replay',path.join(baseDir,'sides-preservation.sources.json'),'original-wallet-preservation.json');
 const archived=read(path.join(baseDir,'sides-preservation-original.json'));
 assert.deepEqual(wallet.result.evaluation.scorecards,archived.evaluation.scorecards);
 preservation={strictBoundaryAndEndpointCardsExact:648,originalLiveWalletCardsExact:72,
  priorOutputHash:sha(fs.readFileSync(path.join(baseDir,'historical-side-endpoints-v1.json'))),
  newOutputHash:side.outputHash,originalWalletOutputHash:wallet.outputHash,
  originalLiveWalletThrough:wallet.result.evaluation.evaluationThrough};
}
for(const [file,hash] of Object.entries(before))assert.equal(sha(fs.readFileSync(path.join(runtime,file))),hash);
const summaries=Object.fromEntries(['yes','no'].map(side=>{
 const strict=m.sides[side].strict.cards;
 if(future)for(const card of strict)for(const episode of card.episodes){assert(episode.signal.publishedAt>protocol.signalAfter);assert(episode.signal.requestStartedAt>protocol.signalAfter);}
 return[side,{strictCards:strict.map(({strategy,parameters,scenario,featurePolicy,summary,signalDiagnostics})=>({strategy,parameters,scenario,featurePolicy,summary,signalDiagnostics})),
  endpointCards:m.sides[side].endpointSensitivity.cards.map(({strategy,parameters,scenario,featurePolicy,summary})=>({strategy,parameters,scenario,featurePolicy,summary})),
  uniqueSignalEvents:new Set(strict.flatMap(card=>card.episodes.map(e=>e.market+'/'+e.signal.sequence))).size,
  uniqueObservedIntervals:new Set(strict.flatMap(card=>card.episodes.filter(e=>e.outcome).map(e=>e.market+'/'+e.entry.sequence+'/'+e.outcome.sequence))).size}];
}));
const evidence={schemaVersion:1,at:new Date().toISOString(),claimComment:5983314037,
 status:future?'frozen_mechanism_post_cutoff_quote_probe':'fixed_mechanism_historical_comparison_completed',
 protocolHash:sha(fs.readFileSync(protocolFile)),protocol,mode:m.mode,signalAfter:m.signalAfter,
 sourceDays:study.receipt.sourceDays,sourceBytes:study.receipt.sourceBytes,
 quoteSpanDays:(study.result.capture.sources.quotes.through-study.result.capture.sources.quotes.firstAt)/86400000,
 quoteRows:m.quoteRows,markets:m.markets,cases:36,sources:study.result.capture.sources,
 timing:{wallMs:study.wallMs,sourceVerifyMs:study.receipt.captureMs,replayMs:study.receipt.replayMs,
  rereadOutputAndProcessMs:study.wallMs-study.receipt.captureMs-study.receipt.replayMs},
 fullOutputHash:study.outputHash,fullOutputName:study.name,prefixProof,preservation,
 interpretation:m.interpretation,sides:summaries,activeHashesUnchanged:before,
 activeCohortReplaced:false,originalWalletsReset:false,newLiveWallets:0,
 fees:0,networkRequests:0,competitionOrders:0,additionalPersistentProcesses:0,profitabilityEstablished:false};
const target=future?'data/native-mechanisms-future-'+stamp+'.json':'data/native-mechanisms-evidence-20261004.json';
fs.writeFileSync(target,JSON.stringify(evidence)+'\n',{flag:'wx'});
console.log(JSON.stringify({target,cases:36,timing:evidence.timing,preservation,
 sides:['yes','no'].map(side=>({side,uniqueSignals:summaries[side].uniqueSignalEvents,
  baseline:summaries[side].strictCards.filter(c=>c.scenario.id==='baseline_3s').map(c=>({strategy:c.strategy,episodes:c.summary.episodes,observed:c.summary.observed,net:c.summary.quotedNetMarkout.mean,status:c.summary.statusCounts})),
  endpoints:summaries[side].endpointCards.filter(c=>c.scenario.id==='baseline_3s').map(c=>({strategy:c.strategy,...c.summary}))})),profitabilityEstablished:false}));
