import fs from 'node:fs';import path from 'node:path';import {createHash} from 'node:crypto';
import {atomicJson,payloadHash} from '../execution/ledger.mjs';
import {openDualNoPaperHostV2} from '../live-paper/dual-no-pair-host-v2.mjs';
import {collectFrozenPaperFiles} from '../live-paper/frozen-paper-graph.mjs';
import {frozenPaperModule} from '../live-paper/frozen-paper-runtime.mjs';
const rt=path.join(process.env.LOCALAPPDATA,'SigTradingCup/paper-runtime'),cohort='michigan-governor-dual-no-20261005-v2',dir=path.join(rt,'production-metadata/michigan-dual-no-profit-sprint-20261005');
const sha=b=>createHash('sha256').update(b).digest('hex'),universe=JSON.parse(fs.readFileSync('live-paper/price-universe.json'));
const expected=[{marketId:'305',exchangeId:'994'},{marketId:'306',exchangeId:'995'}],captures=expected.map(l=>JSON.parse(fs.readFileSync(path.join(dir,l.marketId+'.json'))));
for(const [i,c]of captures.entries())if(c.marketId!==expected[i].marketId||c.rootHash!==payloadHash(c.root)||!universe.exchanges.some(x=>x.marketId===expected[i].marketId&&x.exchangeId===expected[i].exchangeId&&x.option==='YES')||Date.now()-Date.parse(c.receivedAt)>3600000)throw Error('Scoped metadata missing, changed or stale');
for(const f of ['STOP',path.join(rt,'STOP'),path.join(rt,'dual-no-pair-v2-active.json')])if(fs.existsSync(f))throw Error('Existing stop or pointer requires inspection');
const review={schemaVersion:1,kind:'conditional_no_pair_review',id:cohort,tournamentId:universe.tournamentId,evidenceHash:payloadHash(captures),recordedAt:new Date().toISOString(),expiresAt:'2026-10-05T22:00:00.000Z',normalResolutionOnly:true,administrativeRefundGapAcknowledged:true,
 legs:expected.map((l,i)=>({...l,captureMarketId:captures[i].marketId,outcome:'NO',rulesHash:captures[i].rootHash,root:captures[i].root})),
 assumptions:['Both predicates identify the same General election race/stage/office/date and different party winners.','Normal resolution cannot award both parties a win; a third-party win pays both NO legs.','Administrative overrides, voids, refunds and inconsistent resolution are outside the normal payout floor.','Sequential fills can leave an unhedged orphan; account exclusions and later executable depth remain required.'],
 scenarios:[{id:'democratic_winner',category:'ordinary',payouts:[0,1]},{id:'republican_winner',category:'ordinary',payouts:[1,0]},{id:'other_winner',category:'ordinary',payouts:[1,1]},{id:'zero_payout_exception_stress',category:'exception',payouts:[0,0]}]};
const files=collectFrozenPaperFiles(process.cwd(),['live-paper/dual-no-pair-host-v2.mjs']),snapshot=payloadHash(files),archive=path.join(rt,'frozen-code',snapshot);fs.mkdirSync(archive,{recursive:true,mode:0o700});
for(const name of Object.keys(files)){const target=path.join(archive,name);fs.mkdirSync(path.dirname(target),{recursive:true,mode:0o700});const bytes=fs.readFileSync(name);if(fs.existsSync(target)){if(sha(fs.readFileSync(target))!==sha(bytes))throw Error('Archive collision');}else fs.writeFileSync(target,bytes,{flag:'wx',mode:0o600});}
atomicJson(path.join(archive,'manifest.json'),{schemaVersion:1,files});
const host=openDualNoPaperHostV2({runtimeDir:rt,cohort,initialize:true,bundle:{review}});let bank,stats;try{bank=host.bank;stats=host.status();host.publish();}finally{host.close();}
if(payloadHash(collectFrozenPaperFiles(process.cwd(),['live-paper/dual-no-pair-host-v2.mjs']))!==snapshot)throw Error('Code changed during preparation');
const indexFile=path.join(rt,'frozen-code/index.json'),index=JSON.parse(fs.readFileSync(indexFile));index.banks[payloadHash(bank)]={snapshot};atomicJson(indexFile,index);
await frozenPaperModule({runtimeDir:rt,bank,modulePath:'live-paper/dual-no-pair-host-v2.mjs'});
atomicJson(path.join(rt,'dual-no-pair-v2-active.json'),{schemaVersion:1,cohort,bankHash:payloadHash(bank)});
const evidence={at:new Date().toISOString(),cohort,bankHash:payloadHash(bank),cutoff:bank.cutoff,sourcePrefixBytes:bank.sourcePrefixBytes,sourcePrefixHash:bank.sourcePrefixHash,sourcePrefixSequence:bank.sourcePrefixSequence,sourcePrefixTailHash:bank.sourcePrefixTailHash,snapshot,frozenFiles:Object.keys(files).length,review,initialRealizedPnl:stats.realizedPnl,initialFills:stats.fills,initialInventory:stats.positions.length,competitionOrders:0,additionalAccountClients:0,additionalPersistentServices:0,profitabilityEstablished:false};
atomicJson('data/dual-no-sprint-v2-initialization-20261005.json',evidence);console.log(JSON.stringify({cohort,bankHash:evidence.bankHash,cutoff:evidence.cutoff,initialFills:stats.fills,orders:0}));
