import fs from 'node:fs';
import path from 'node:path';
import {createHash,randomUUID} from 'node:crypto';
import {fileURLToPath} from 'node:url';
import {digest} from './model.mjs';
import {atomicJson} from '../execution/ledger.mjs';
import {validateUniverse} from './breadth.mjs';
import {validatePriceObservation} from './market-price-publication.mjs';
import {createBookPublicationState,applyBookPublication} from './book-publication.mjs';
import {BID_ANCHOR_POLICY} from './bid-anchor.mjs';
import {BULK_DIP_POLICY,createBulkDipState,advanceBulkPrices,advanceBulkBook,bulkDipReport} from './bulk-dip.mjs';

const need=(ok,why)=>{if(!ok)throw Error('BULK_HOST_V2_'+why);};
const sha=bytes=>createHash('sha256').update(bytes).digest('hex');
const dependencies=['./bulk-dip-host-v2.mjs','./bulk-dip.mjs','./bid-anchor.mjs','./bid-anchor-live.mjs',
 './market-price-publication.mjs','./breadth.mjs','./runtime.mjs','./edge-depth-audit.mjs','./model.mjs',
 './book-publication.mjs','../paper/fixed.mjs','../feed/contract.mjs','../execution/ledger.mjs'];
const identity=()=>Object.fromEntries(dependencies.map(p=>[p,sha(fs.readFileSync(new URL(p,import.meta.url)))]));
function write(file,bytes,flag){
 const fd=fs.openSync(file,flag,0o600);
 try{let offset=0;while(offset<bytes.length){const n=fs.writeSync(fd,bytes,offset,bytes.length-offset);need(n>0,'WRITE');offset+=n;}fs.fsyncSync(fd);}
 finally{fs.closeSync(fd);}
}
const complete=file=>{const b=fs.readFileSync(file);return b.subarray(0,b.lastIndexOf(10)+1);};
const rows=bytes=>bytes.length?bytes.toString().split('\n').slice(0,-1).map(JSON.parse):[];
const merge=(books,prices)=>[...books.map(row=>({kind:'book',row})),...prices.map(row=>({kind:'price',row}))]
 .sort((a,b)=>Date.parse(a.row.at)-Date.parse(b.row.at)||Number(a.kind==='book')-Number(b.kind==='book')||a.row.sequence-b.row.sequence);

export function openBulkDip({runtimeDir,cohort,initialize=false,now=Date.now,
 workspaceDir=path.dirname(path.dirname(fileURLToPath(import.meta.url)))}={}){
 need(path.isAbsolute(runtimeDir)&&/^[a-z][a-z0-9-]{1,63}$/.test(cohort),'DIRECTORY');
 const dir=path.join(runtimeDir,'bulk-dip-cohorts',cohort),bankFile=path.join(dir,'bank.json'),
  journal=path.join(dir,'journal.jsonl'),lockFile=path.join(dir,'worker.lock'),
  sources={book:path.join(runtimeDir,'public-books.jsonl'),price:path.join(runtimeDir,'market-prices.jsonl')};
 const stopped=()=>[path.join(workspaceDir,'STOP'),path.join(runtimeDir,'STOP'),path.join(runtimeDir,'BULK_DIP_STOP'),path.join(dir,'STOP')].some(p=>fs.existsSync(p));
 need(!stopped(),'STOPPED');fs.mkdirSync(dir,{recursive:true});
 const token=JSON.stringify({pid:process.pid,id:randomUUID()});write(lockFile,Buffer.from(token),'wx');
 let closed=false,failed=false;
 const live=()=>need(!closed&&!failed&&!stopped()&&fs.readFileSync(lockFile,'utf8')===token,'OWNERSHIP');
 try{
  const codeIdentity=identity(),sourceStats=Object.fromEntries(Object.entries(sources).map(([k,f])=>[k,fs.statSync(f)])),
   raw=Object.fromEntries(Object.entries(sources).map(([k,f])=>[k,complete(f)])),
   all=Object.fromEntries(Object.entries(raw).map(([k,b])=>[k,rows(b)]));
  need(all.book.length&&all.price.length,'EMPTY');let bank;
  if(initialize){
   need(!fs.existsSync(bankFile)&&!fs.existsSync(journal),'EXISTS');
   const cutoff=new Date(now()).toISOString(),template=JSON.parse(fs.readFileSync(new URL('./config.json',import.meta.url))),
    universe=JSON.parse(fs.readFileSync(new URL('./price-universe.json',import.meta.url)));
   need(Object.values(all).every(a=>Date.parse(a.at(-1).at)<=Date.parse(cutoff)),'FUTURE');
   bank={schemaVersion:2,hostVersion:2,mode:'market_wide_bulk_reference_paper',name:cohort,cutoff,universe,
    policy:BID_ANCHOR_POLICY,bulkPolicy:BULK_DIP_POLICY,
    config:{...template,strategyUniverse:'diversified-dip-v1',marketIds:universe.exchanges.map(x=>x.marketId),
     initialCash:'100.000000',cashReserve:'90.000000',maxExposure:'10.000000',feeBps:0,slippageTicks:1,
     depthFractionBps:100,orderShares:1,maxPositionShares:1,stopLoss:'0.020000',exitAfterMs:300000},
    prefix:Object.fromEntries(Object.entries(raw).map(([k,b])=>[k,{bytes:b.length,hash:sha(b),sequence:all[k].at(-1).sequence,journalHash:all[k].at(-1).hash}])),codeIdentity};
  }else{
   bank=JSON.parse(fs.readFileSync(bankFile));
   need(bank.schemaVersion===2&&bank.hostVersion===2&&bank.name===cohort,'BANK_VERSION');
   need(digest(bank.codeIdentity)===digest(codeIdentity),'FROZEN_CODE');
  }
  const universe=validateUniverse(bank.universe,bank.config.tournamentId),marketByExchange=new Map(universe.exchanges.map(x=>[x.exchangeId,x.marketId]));
  let bookState=createBookPublicationState(bank.universe),priceState={sequence:0,hash:'',at:null},state=createBulkDipState(bank);
  const offset={book:0,price:0},index={book:0,price:0};
  const validate=(kind,row)=>{
   if(kind==='book'){bookState=applyBookPublication(bookState,row);return;}
   const {hash,...body}=row;
   need(body.sequence===priceState.sequence+1&&body.previous===priceState.hash&&digest(body)===hash
    &&body.state===null&&body.event?.type==='market_price_observation'&&Number.isFinite(Date.parse(body.at))
    &&Date.parse(body.at)>=Date.parse(body.event.snapshot.receivedAt)&&(!priceState.at||Date.parse(body.at)>=Date.parse(priceState.at)),'PRICE_CHAIN');
   validatePriceObservation(body.event.snapshot,universe);priceState={sequence:body.sequence,hash,at:body.at};
  };
  for(const kind of ['book','price']){
   while((kind==='book'?bookState.sequence:priceState.sequence)<bank.prefix[kind].sequence){
    live();const row=all[kind][index[kind]++];need(row,'PREFIX_TRUNCATED');validate(kind,row);offset[kind]=raw[kind].indexOf(10,offset[kind])+1;
   }
   need(offset[kind]===bank.prefix[kind].bytes&&sha(raw[kind].subarray(0,offset[kind]))===bank.prefix[kind].hash
    &&(kind==='book'?bookState.hash:priceState.hash)===bank.prefix[kind].journalHash,'PREFIX');
  }
  // Include both source and consumer boundaries. Bulk requests that straddle a
  // boundary cannot resurrect an invalidated reference after their response.
  state.referenceBarriers={global:bookState.invalidations.global?.at||null,
   markets:Object.fromEntries(Object.entries(bookState.invalidations.markets).map(([id,b])=>[id,b.at]))};
  const filteredPrices=snapshot=>{
   const permitted=id=>[state.referenceBarriers.global,state.referenceBarriers.markets[id]].every(at=>!at||Date.parse(snapshot.requestStartedAt)>Date.parse(at));
   const quotes=snapshot.quotes.filter(q=>permitted(q.marketId)),missingIds=snapshot.missingIds.filter(id=>permitted(marketByExchange.get(id)));
   const excluded=snapshot.quotes.length+snapshot.missingIds.length-quotes.length-missingIds.length;
   if(excluded)state.bulk.counts.price_request_before_source_barrier=(state.bulk.counts.price_request_before_source_barrier||0)+excluded;
   return quotes.length+missingIds.length?{...snapshot,quotes,missingIds}:null;
  };
  const start=Date.parse(bank.cutoff)-bank.policy.historyMs;
  if(!bookState.closed)for(const row of all.price.slice(0,index.price)){
   const snapshot=row.event.snapshot;
   if(Date.parse(snapshot.receivedAt)>=start&&Date.parse(snapshot.receivedAt)<=Date.parse(bank.cutoff)){
    const filtered=filteredPrices(snapshot);if(filtered)advanceBulkPrices(state,filtered,bank,bank.cutoff,{warmup:true});
   }
  }
  state.live={lastAvailableAt:bank.cutoff,barriers:Object.fromEntries(Object.keys(bookState.invalidations.markets).map(id=>[id,bank.cutoff])),counts:{}};
  let sequence=0,previous=digest(bank),journalBytes=0,journalBytesHash=null;
  const expected=()=>({bookBytes:offset.book,bookSequence:bookState.sequence,bookHash:bookState.hash,
   priceBytes:offset.price,priceSequence:priceState.sequence,priceHash:priceState.hash,stateHash:digest(state)});
  const reduce=(batch,at)=>{
   for(const {kind,row}of batch){
    need(Date.parse(row.at)<=Date.parse(at),'FUTURE_ROW');validate(kind,row);
    if(kind==='price'){const filtered=filteredPrices(row.event.snapshot);if(filtered)advanceBulkPrices(state,filtered,bank,at);continue;}
    if(['session_start','session_stop'].includes(row.kind)||row.kind==='invalidation'&&row.payload.marketId===null)state.referenceBarriers.global=at;
    else if(row.kind==='invalidation')state.referenceBarriers.markets[row.payload.marketId]=at;
    else if(row.kind==='book'&&Date.parse(at)-Date.parse(row.payload.book.receivedAt)>bank.config.maxDataAgeMs){
     const id=row.payload.book.marketId;state.referenceBarriers.markets[id]=at;delete state.bulk.history[id];delete state.bulk.candidates[id];
    }
    advanceBulkBook(state,row,bank,at);
   }
  };
  if(!initialize){
   const bytes=fs.readFileSync(journal);need(bytes.length&&bytes.at(-1)===10,'PARTIAL_JOURNAL');
   for(const record of rows(bytes)){
    const {hash,...body}=record;need(body.sequence===++sequence&&body.previous===previous&&body.bankHash===digest(bank)&&digest(body)===hash,'JOURNAL');
    if(body.kind==='genesis')need(sequence===1&&body.stateHash===digest(state),'GENESIS');
    else{
     need(body.kind==='batch','CHECKPOINT');const batch={book:[],price:[]};
     for(const kind of ['book','price']){
      const target=body[kind+'Sequence'],current=kind==='book'?bookState.sequence:priceState.sequence;
      need(Number.isSafeInteger(target)&&target>=current,'SEQUENCE');
      while(current+batch[kind].length<target){need(all[kind][index[kind]],'SOURCE_TRUNCATED');batch[kind].push(all[kind][index[kind]++]);offset[kind]=raw[kind].indexOf(10,offset[kind])+1;}
     }
     need(batch.book.length+batch.price.length>0,'EMPTY_BATCH');reduce(merge(batch.book,batch.price),body.availableAt);
     need(Object.entries(expected()).every(([k,v])=>body[k]===v),'REPLAY');
    }
    previous=hash;
   }
   need(sequence>0,'GENESIS_MISSING');journalBytes=bytes.length;journalBytesHash=sha(bytes);
  }
  const verifyJournal=()=>{const bytes=fs.readFileSync(journal);need(bytes.length===journalBytes&&sha(bytes)===journalBytesHash,'JOURNAL_CHANGED');};
  const append=body=>{
   live();verifyJournal();const item={sequence:sequence+1,previous,bankHash:digest(bank),...body},hash=digest(item),bytes=Buffer.from(JSON.stringify({...item,hash})+'\n');
   write(journal,bytes,'a');sequence++;previous=hash;journalBytes+=bytes.length;journalBytesHash=sha(fs.readFileSync(journal));
  };
  if(initialize){write(bankFile,Buffer.from(JSON.stringify(bank,null,2)+'\n'),'wx');write(journal,Buffer.alloc(0),'wx');journalBytesHash=sha(Buffer.alloc(0));append({kind:'genesis',stateHash:digest(state)});}
  const bankBytesHash=sha(fs.readFileSync(bankFile));
  let prefixHashes=Object.fromEntries(Object.entries(raw).map(([k,b])=>[k,sha(b.subarray(0,offset[k]))]));
  const consume=()=>{
   live();try{
    need(sha(fs.readFileSync(bankFile))===bankBytesHash&&digest(identity())===digest(codeIdentity),'IDENTITY_CHANGED');verifyJournal();
    const batch={book:[],price:[]},nextOffset={...offset},nextHashes={...prefixHashes};
    // Verify both original consumed prefixes before parsing or applying either
    // source. Equal size is not evidence that consumed bytes are unchanged.
    for(const [kind,file]of Object.entries(sources)){
     const stat=fs.statSync(file);need(stat.ino===sourceStats[kind].ino&&stat.birthtimeMs===sourceStats[kind].birthtimeMs&&stat.size>=offset[kind],'SOURCE_REPLACED');
     const bytes=complete(file);need(bytes.length>=offset[kind]&&sha(bytes.subarray(0,offset[kind]))===prefixHashes[kind],'SOURCE_CHANGED');
     batch[kind]=rows(bytes.subarray(offset[kind]));nextOffset[kind]=bytes.length;nextHashes[kind]=sha(bytes);
    }
    const count=batch.book.length+batch.price.length;if(!count)return 0;
    const at=new Date(now()).toISOString();reduce(merge(batch.book,batch.price),at);Object.assign(offset,nextOffset);prefixHashes=nextHashes;
    append({kind:'batch',availableAt:at,...expected()});return count;
   }catch(error){failed=true;throw error;}
  };
  const status=()=>{
   const report=bulkDipReport(state,bank,new Date(now()).toISOString());
   // Bind every original pending-map key directly to its scoped instrument.
   // An expired intent can reprepare without a fresh admitted signal.
   report.wallets=Object.fromEntries(Object.entries(report.wallets).map(([latency,wallet])=>[latency,{...wallet,
    pending:Object.entries(state.wallets[latency].state.strategies.mean_reversion.pending).map(([instrument,intent])=>{
     const spec=universe.exchanges.find(x=>x.exchangeId+':YES'===instrument);need(spec,'PENDING_IDENTITY');
     return {...intent,instrument,marketId:spec.marketId};
    })}]));
   return {...report,hostVersion:2,referenceBarriers:state.referenceBarriers,cohort,...expected(),
    journalSequence:sequence,journalHash:previous,sourceClosed:bookState.closed,failed};
  };
  const close=()=>{if(closed)return;closed=true;need(fs.readFileSync(lockFile,'utf8')===token,'LOCK_CHANGED');fs.unlinkSync(lockFile);};
  return {dir,bank,consume,status,close};
 }catch(error){if(fs.existsSync(lockFile)&&fs.readFileSync(lockFile,'utf8')===token)fs.unlinkSync(lockFile);throw error;}
}
if(process.argv[1]&&fileURLToPath(import.meta.url)===path.resolve(process.argv[1])){
 const args=process.argv.slice(2),worker=openBulkDip({runtimeDir:path.join(process.env.LOCALAPPDATA,'SigTradingCup/paper-runtime'),cohort:args[args.indexOf('--name')+1],initialize:args.includes('--initialize')});
 try{worker.consume();atomicJson(path.join(worker.dir,'stats.json'),worker.status());console.log(JSON.stringify(worker.status(),null,2));}finally{worker.close();}
}
