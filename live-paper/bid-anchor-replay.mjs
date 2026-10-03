import fs from 'node:fs';import path from 'node:path';import {createHash} from 'node:crypto';import {fileURLToPath} from 'node:url';
import {digest} from './model.mjs';import {atomicJson} from '../execution/ledger.mjs';
import {createBookPublicationState,applyBookPublication} from './book-publication.mjs';
import {BID_ANCHOR_POLICY,createBidAnchorState,advanceBidAnchor,bidAnchorReport} from './bid-anchor.mjs';
const sha=b=>createHash('sha256').update(b).digest('hex'),need=(ok,reason)=>{if(!ok)throw Error('BID_ANCHOR_REPLAY_'+reason);};
const identity=()=>Object.fromEntries(['./bid-anchor.mjs','./bid-anchor-replay.mjs','./edge-depth-audit.mjs','./model.mjs','./book-publication.mjs','../paper/fixed.mjs','../feed/contract.mjs','../execution/ledger.mjs'].map(p=>[p,sha(fs.readFileSync(new URL(p,import.meta.url)))]));
export function replayBidAnchor(bank,bytes){
 const complete=bytes.lastIndexOf(10)+1;need(complete>=bank.sourcePrefixBytes&&sha(bytes.subarray(0,bank.sourcePrefixBytes))===bank.sourcePrefixHash,'PREFIX');
 let publication=createBookPublicationState(bank.universe),state=createBidAnchorState(bank),offset=0,prefixChecked=false;
 for(const line of bytes.subarray(0,complete).toString().split('\n').slice(0,-1)){
  const row=JSON.parse(line);publication=applyBookPublication(publication,row);offset+=Buffer.byteLength(line)+1;
  if(offset===bank.sourcePrefixBytes){need(row.sequence===bank.sourcePrefixSequence&&row.hash===bank.sourcePrefixJournalHash,'PREFIX_CHAIN');prefixChecked=true;}
  advanceBidAnchor(state,row,bank);
 }
 need(prefixChecked&&publication.sequence>=bank.sourcePrefixSequence,'TRUNCATED');
 // Clock comes from the original last publication; wall time cannot relabel data.
 const last=JSON.parse(bytes.subarray(0,complete).toString().trimEnd().split('\n').at(-1));
 return {...bidAnchorReport(state,bank,last.at),stateHash:digest(state),sourceSequence:publication.sequence,sourceHash:publication.hash,sourceBytes:complete,sourceBytesHash:sha(bytes.subarray(0,complete)),sourceClosed:publication.closed,incompleteTailBytes:bytes.length-complete};
}
if(process.argv[1]&&fileURLToPath(import.meta.url)===path.resolve(process.argv[1])){
 const args=process.argv.slice(2),name=args[args.indexOf('--name')+1];need(args.includes('--name')&&/^[a-z][a-z0-9-]{1,63}$/.test(name),'NAME');
 const rt=path.join(process.env.LOCALAPPDATA,'SigTradingCup/paper-runtime'),dir=path.join(rt,'bid-anchor-cohorts',name),file=path.join(dir,'bank.json'),bytes=fs.readFileSync(path.join(rt,'public-books.jsonl'));let bank;
 if(args.includes('--initialize')){
  need(!fs.existsSync(file),'EXISTS');const end=bytes.lastIndexOf(10)+1,complete=bytes.subarray(0,end),last=JSON.parse(complete.toString().trimEnd().split('\n').at(-1)),config=JSON.parse(fs.readFileSync(new URL('./config.json',import.meta.url))),universe=JSON.parse(fs.readFileSync(new URL('./price-universe.json',import.meta.url))),cutoff=new Date().toISOString();
  need(Date.parse(last.at)<=Date.parse(cutoff),'FUTURE_SOURCE');
  bank={schemaVersion:1,name,cutoff,universe,policy:BID_ANCHOR_POLICY,config:{...config,strategyUniverse:'diversified-dip-v1',marketIds:universe.exchanges.map(x=>x.marketId),initialCash:'100.000000',cashReserve:'90.000000',maxExposure:'10.000000',feeBps:0,slippageTicks:1,depthFractionBps:100,orderShares:1,maxPositionShares:1,stopLoss:'0.020000',exitAfterMs:300000},sourcePrefixBytes:end,sourcePrefixHash:sha(complete),sourcePrefixSequence:last.sequence,sourcePrefixJournalHash:last.hash,codeIdentity:identity()};
  // Validate capture and replay before publishing an immutable definition.
  const result=replayBidAnchor(bank,bytes);fs.mkdirSync(dir,{recursive:true});fs.writeFileSync(file,JSON.stringify(bank,null,2)+'\n',{flag:'wx',mode:0o600});fs.writeFileSync(path.join(dir,'bank.sha256'),sha(fs.readFileSync(file))+'\n',{flag:'wx',mode:0o600});atomicJson(path.join(dir,'report.json'),result);console.log(JSON.stringify({cohort:name,bankHash:digest(bank),cutoff,report:result},null,2));
 }else{
  bank=JSON.parse(fs.readFileSync(file));need(sha(fs.readFileSync(file))===fs.readFileSync(path.join(dir,'bank.sha256'),'utf8').trim()&&digest(identity())===digest(bank.codeIdentity),'FROZEN_IDENTITY');const result=replayBidAnchor(bank,bytes);atomicJson(path.join(dir,'report.json'),result);console.log(JSON.stringify(result,null,2));
 }
}
