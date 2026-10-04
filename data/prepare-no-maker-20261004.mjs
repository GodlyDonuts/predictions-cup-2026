import fs from 'node:fs';import path from 'node:path';import {createHash} from 'node:crypto';import {payloadHash,atomicJson} from '../execution/ledger.mjs';
import {NO_MAKER_POLICY} from '../live-paper/no-maker-paper.mjs';import {noMakerCodeIdentity} from '../live-paper/no-maker-host.mjs';
const r=path.join(process.env.LOCALAPPDATA,'SigTradingCup','paper-runtime'),sha=b=>createHash('sha256').update(b).digest('hex');
const a=JSON.parse(fs.readFileSync(path.join(r,'canonical-capacity-active.json'))),base=JSON.parse(fs.readFileSync(path.join(r,'canonical-capacity-cohorts',a.cohort,'bank.json')));
const source=fs.readFileSync(path.join(r,'anonymous-trades.jsonl'));if(source.length&&source.at(-1)!==10)throw Error('Partial tape source; prepare again after complete append');
const rows=source.length?source.toString().trimEnd().split('\n').map(JSON.parse):[];let sequence=0,previous='';for(const row of rows){const{hash,...body}=row;if(row.sequence!==++sequence||row.previous!==previous||sha(JSON.stringify(body))!==hash)throw Error('Tape integrity');previous=hash;}
const name='no-maker-tape-20261004-v1',dir=path.join(r,'no-maker-cohorts',name),cutoff=new Date().toISOString();if(fs.existsSync(dir))throw Error('Frozen cohort already exists; no overwrite');
const bank={schemaVersion:1,mode:'no_maker_trade_tape_paper',name,cutoff,policy:NO_MAKER_POLICY,tournamentId:base.tournamentId,reviewExpiresAt:base.review.expiresAt,
 legs:base.review.legs,review:base.review,capacityPolicy:base.policy,baseCapacityBankHash:a.bankHash,codeIdentity:noMakerCodeIdentity(),
 tapePrefixBytes:source.length,tapePrefixHash:sha(source),tapePrefixSequence:sequence,tapePrefixTailHash:previous,
 representation:'documented_same_exchange_yes_book_complement',nativeNoObserved:false,executionAuthority:false};
fs.mkdirSync(dir,{recursive:true,mode:0o700});atomicJson(path.join(dir,'bank.json'),bank);atomicJson(path.join(r,'no-maker-active.json'),{cohort:name,bankHash:payloadHash(bank)});
fs.writeFileSync('data/no-maker-bank-20261004.json',JSON.stringify(bank,null,2));console.log(JSON.stringify({cohort:name,cutoff,bankHash:payloadHash(bank),priorTapePagesSkipped:sequence,priorTapeBytes:source.length,ordersSubmitted:0,newProcesses:0}));
