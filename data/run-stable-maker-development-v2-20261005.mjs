import fs from 'node:fs';import path from 'node:path';import {createHash} from 'node:crypto';
import {STABLE_MAKER_POLICY} from '../live-paper/stable-maker-paper-v2.mjs';import {runStableMakerReplay} from '../live-paper/stable-maker-replay-v2.mjs';
const sha=b=>createHash('sha256').update(b).digest('hex'),runtimeDir=path.join(process.env.LOCALAPPDATA,'SigTradingCup','paper-runtime');
const codeFiles=['live-paper/stable-maker-paper-v2.mjs','live-paper/stable-maker-replay-v2.mjs','live-paper/canonical-entry-exit-study.mjs','live-paper/probability-receipt.mjs','paper/fixed.mjs','execution/ledger.mjs','feed/contract.mjs'];
const codeIdentity=Object.fromEntries(codeFiles.map(f=>[f,sha(fs.readFileSync(f))]));const declaredAt=new Date().toISOString();
const legs=[{marketId:'363',exchangeId:'1052'},{marketId:'297',exchangeId:'986'},{marketId:'381',exchangeId:'1070'},{marketId:'382',exchangeId:'1071'}],tournamentId='bda92870-621e-47b0-bc3c-3602c5c26f55';
const declaration={schemaVersion:1,mode:'stable_two_side_trade_tape_paper_v2',name:'baal-stable-spread-20261005-v2',cutoff:declaredAt,entryExpiresAt:new Date(Date.parse(declaredAt)+21600000).toISOString(),tournamentId,legs,policy:STABLE_MAKER_POLICY,codeIdentity,
 dataUse:'Fixed new mechanism; earlier replay is development evidence. Only later post-declaration entries can supply prospective evidence.',executionAuthority:false,competitionOrders:0,fees:0};
fs.writeFileSync('data/stable-maker-declaration-v2-20261005.json',JSON.stringify(declaration,null,2)+'\n',{flag:'wx'});
const bank={...declaration,name:declaration.name+'-development',cutoff:'2026-10-04T16:59:49.117Z',entryExpiresAt:declaredAt};
const out='data/stable-maker-development-v2-20261005.json';const record=await runStableMakerReplay({runtimeDir,bank,executable:path.resolve('backend-rust/target/receipt-strategy-20261005/release/baal-backend.exe'),binaryHash:'cc775b6e91a9f27193a91adea6a5eda40b0a7735fd77b857ed52221835a81fd5',out});
console.log(JSON.stringify({out,declaration:declaredAt,processed:record.processed,timing:record.timing,counts:record.report.counts,wallets:record.report.wallets,profitabilityEstablished:false}));
