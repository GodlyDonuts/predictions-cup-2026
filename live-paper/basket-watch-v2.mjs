import {readVerifiedSource} from './journal-lines.mjs';
import fs from 'node:fs';
import path from 'node:path';
import {fileURLToPath} from 'node:url';
import {digest,normalizeBook} from './model.mjs';
import {verifySource} from './ml-shadow.mjs';
import {atomicJson,lock} from './runtime.mjs';
import {validateUniverse} from './breadth.mjs';
import {basketDefinitions} from './basket-inputs.mjs';
import {createBasketObserver,observeBasket,invalidateBasket,basketReport} from './basket-observer.mjs';

const directory=path.dirname(fileURLToPath(import.meta.url));
export function basketCodeIdentity(){return Object.fromEntries(['basket-watch.mjs','basket-observer.mjs','basket-inputs.mjs','model.mjs','../paper/fixed.mjs','../feed/contract.mjs'].map(p=>[p,digest(fs.readFileSync(path.resolve(directory,p),'utf8'))]));}
const canonical=x=>typeof x==='string'&&Number.isFinite(Date.parse(x))&&new Date(x).toISOString()===x;

export function createBasketCapture(records,definitions,cutoff,codeIdentity=basketCodeIdentity()){
 if(!records?.length||!records.prefixHashes?.length||!canonical(cutoff)||Date.parse(cutoff)<Date.parse(records.at(-1).event.clockAt||records.at(-1).at)||!Array.isArray(definitions)||!definitions.length||definitions.some(d=>typeof d.id!=='string'||!/^[a-f0-9]{64}$/.test(d.id))||new Set(definitions.map(d=>d.id)).size!==definitions.length)throw Error('Invalid prospective basket source');
 const sourceHash=records.prefixHashes.at(-1),pairs=definitions.map(d=>({id:d.id,...createBasketObserver({tournamentId:d.tournamentId,legs:d.legs,sourceHash,cutoff})}));
 const bank={schemaVersion:1,mode:'dual_no_basket_measurement',cutoff,sourceRecords:records.length,sourceHash,codeIdentity,definitions:structuredClone(definitions),pairs:pairs.map(p=>({id:p.id,bank:p.bank,initialState:p.state}))};
 return {bank,state:initialCapture(bank,records)};
}
function bound(bank,records){
 if(bank?.schemaVersion!==1||bank.mode!=='dual_no_basket_measurement'||!Number.isSafeInteger(bank.sourceRecords)||bank.sourceRecords<1||records?.prefixHashes?.[bank.sourceRecords-1]!==bank.sourceHash)throw Error('Basket source prefix changed');
}
function initialCapture(bank,records){bound(bank,records);return {bankHash:digest(bank),sourceSequence:bank.sourceRecords,lastRecordHash:records[bank.sourceRecords-1].hash,pairs:Object.fromEntries(bank.pairs.map(p=>[p.id,structuredClone(p.initialState)]))};}

export function advanceBasketRecord(input,record,bank){
 if(input.bankHash!==digest(bank)||record.sequence!==input.sourceSequence+1||record.previous!==input.lastRecordHash)throw Error('Basket source continuity failure');
 const state=structuredClone(input),observations=[],e=record.event,at=e.clockAt||record.at;
 if(!canonical(at))throw Error('Invalid basket source clock');
 const invalidate=(marketId,clock)=>{for(const p of bank.pairs)if(!marketId||p.bank.legs.some(l=>l.marketId===marketId)){for(const l of p.bank.legs.filter(l=>!marketId||l.marketId===marketId))state.pairs[p.id]=invalidateBasket(state.pairs[p.id],l.marketId,clock,p.bank);}};
 if(Date.parse(at)>Date.parse(bank.cutoff)){
  if(e.type==='account_read_outage')invalidate(null,at);
  if(e.type==='cycle'){
   if(!Array.isArray(e.accountExclusions)||e.accountExclusions.some(id=>typeof id!=='string')||!canonical(e.exclusionObservedAt)||Date.parse(e.exclusionObservedAt)>Date.parse(at)||Object.values(state.pairs).some(s=>s.lastAt&&Date.parse(s.lastAt)>Date.parse(e.exclusionObservedAt)))invalidate(null,at);
   else{
    const deferredInvalidations=new Set(),excluded=new Set(e.accountExclusions),snapshots=[...(e.sourceSnapshots||[]),...(e.supplementalDepth||[])].map((r,index)=>({r,index,clock:r.receivedAt||r.at||at})).sort((a,b)=>(canonical(a.clock)?Date.parse(a.clock):Infinity)-(canonical(b.clock)?Date.parse(b.clock):Infinity)||a.index-b.index);
    for(const p of bank.pairs)for(const leg of p.bank.legs)if(excluded.has(leg.exchangeId))state.pairs[p.id]=invalidateBasket(state.pairs[p.id],leg.marketId,e.exclusionObservedAt,p.bank);
    for(const {r,clock} of snapshots){
     const marketId=r.marketId||/^\/markets\/([1-9]\d*)\/orderbook\?/.exec(r.endpoint||'')?.[1],pairs=bank.pairs.filter(p=>p.bank.legs.some(l=>l.marketId===marketId));
     if(!pairs.length)continue;
     if(!canonical(clock)||Date.parse(clock)>Date.parse(at)||Date.parse(clock)<Date.parse(e.exclusionObservedAt)){deferredInvalidations.add(marketId);continue;}
     if(r.skipped){invalidate(marketId,clock);continue;}
     if(Date.parse(clock)<=Date.parse(bank.cutoff))continue;
     for(const p of pairs){
      const leg=p.bank.legs.find(l=>l.marketId===marketId);
      if(excluded.has(leg.exchangeId)){state.pairs[p.id]=invalidateBasket(state.pairs[p.id],marketId,clock,p.bank);continue;}
      let book;
      try{
       const expected='/markets/'+marketId+'/orderbook?tournamentId='+p.bank.tournamentId+'&depth=20';
       if(r.endpoint!==expected)throw Error('Book endpoint mismatch');
       book={...normalizeBook(r.data,marketId,p.bank.tournamentId,r.receivedAt,r.requestStartedAt,20000,excluded,'authoritative_receipt_v1'),tournamentId:p.bank.tournamentId};
      }catch{state.pairs[p.id]=invalidateBasket(state.pairs[p.id],marketId,clock,p.bank);continue;}
      try{const result=observeBasket(state.pairs[p.id],book,p.bank);state.pairs[p.id]=result.state;if(result.observation)observations.push({pairId:p.id,...result.observation});}
      catch{state.pairs[p.id]=invalidateBasket(state.pairs[p.id],marketId,clock,p.bank);}
     }
    }
    for(const marketId of deferredInvalidations)invalidate(marketId,at);
    // Excluded books must not survive just because this cycle fetched neither leg.
    for(const p of bank.pairs)for(const leg of p.bank.legs)if(excluded.has(leg.exchangeId))state.pairs[p.id]=invalidateBasket(state.pairs[p.id],leg.marketId,at,p.bank);
   }
  }
 }
 state.sourceSequence=record.sequence;state.lastRecordHash=record.hash;return {state,observations};
}

function append(file,row){const fd=fs.openSync(file,'a',0o600),bytes=Buffer.from(JSON.stringify(row)+'\n');try{let n=0;while(n<bytes.length){const written=fs.writeSync(fd,bytes,n,bytes.length-n);if(written<=0)throw Error('Basket journal short write');n+=written;}fs.fsyncSync(fd);}finally{fs.closeSync(fd);}}
export function openBasketWatch(runtime,{initialize=false,cutoff=new Date().toISOString(),definitions,codeIdentity=basketCodeIdentity()}={}){
 const bankFile=path.join(runtime,'basket-bank.json'),journalFile=path.join(runtime,'basket-journal.jsonl');
 let records=readVerifiedSource(path.join(runtime,'journal.jsonl'));if(!records)throw Error('Basket source temporarily incomplete');
 let bank;
 if(initialize){
  if(fs.existsSync(bankFile)||fs.existsSync(journalFile))throw Error('Refusing to replace existing basket cohort');
  ({bank}=createBasketCapture(records,definitions,cutoff,codeIdentity));
  fs.writeFileSync(bankFile,JSON.stringify(bank,null,2),{flag:'wx',mode:0o600});fs.closeSync(fs.openSync(journalFile,'wx',0o600));
 }else bank=JSON.parse(fs.readFileSync(bankFile));
 if(digest(bank.codeIdentity)!==digest(codeIdentity))throw Error('Frozen basket code changed; new cohort required');
 let state=initialCapture(bank,records),sequence=0,previous=digest(bank);
 const raw=fs.readFileSync(journalFile,'utf8');if(raw&&!raw.endsWith('\n'))throw Error('Incomplete basket journal');
 for(const line of raw.split('\n').filter(Boolean)){
  const row=JSON.parse(line),{hash,...body}=row;
  if(hash!==digest(body)||body.sequence!==++sequence||body.previous!==previous||body.sourceSequence!==state.sourceSequence+1||records[body.sourceSequence-1]?.hash!==body.sourceHash)throw Error('Basket journal integrity failure');
  const output=advanceBasketRecord(state,records[body.sourceSequence-1],bank);
  if(digest(output.state)!==body.stateHash||digest(output.observations)!==body.observationsHash||digest(body.observations)!==body.observationsHash)throw Error('Basket replay mismatch');state=output.state;previous=hash;
 }
 const consume=()=>{
  records=readVerifiedSource(path.join(runtime,'journal.jsonl'));if(!records)return null;bound(bank,records);
  if(records.length<state.sourceSequence||records[state.sourceSequence-1]?.hash!==state.lastRecordHash)throw Error('Basket source changed');
  let consumed=0;
  for(const record of records.slice(state.sourceSequence)){
   const output=advanceBasketRecord(state,record,bank),body={sequence:sequence+1,previous,sourceSequence:record.sequence,sourceHash:record.hash,stateHash:digest(output.state),observationsHash:digest(output.observations),observations:output.observations},hash=digest(body);
   append(journalFile,{...body,hash});state=output.state;sequence++;previous=hash;consumed++;
  }
  atomicJson(path.join(runtime,'basket-state.json'),state);return consumed;
 };
 const status=()=>({schemaVersion:1,mode:'dual_no_basket_measurement',bankHash:digest(bank),cutoff:bank.cutoff,sourceSequence:state.sourceSequence,sourceHash:state.lastRecordHash,journalSequence:sequence,journalHash:previous,pairs:bank.pairs.map(p=>({pairId:p.id,...basketReport(state.pairs[p.id],p.bank)})),networkRequests:0,ordersSubmitted:0,simulatedFills:null,pnl:null,eligibleForExecution:false});
 return {consume,status,bank};
}

if(process.argv[1]&&path.resolve(process.argv[1])===fileURLToPath(import.meta.url)){
 const workspace=path.dirname(directory),runtime=process.env.SIG_PAPER_DIR||path.join(process.env.LOCALAPPDATA,'SigTradingCup','paper-runtime');
 const stops=[path.join(workspace,'STOP'),path.join(runtime,'STOP'),path.join(runtime,'BASKET_STOP')];let release,watch;
 const publish=status=>atomicJson(path.join(runtime,'basket-stats.json'),{...watch.status(),updatedAt:new Date().toISOString(),status,pid:process.pid});
 try{
  if(stops.some(f=>fs.existsSync(f)))throw Error('Basket STOP present');release=lock(path.join(runtime,'basket-watch.lock'));
  let definitions;if(process.argv.includes('--init')){const config=JSON.parse(fs.readFileSync(path.join(directory,'config.json'))),u=validateUniverse(JSON.parse(fs.readFileSync(path.join(directory,'price-universe.json'))),config.tournamentId);definitions=basketDefinitions(JSON.parse(fs.readFileSync(path.join(workspace,'data/markets.json'))),JSON.parse(fs.readFileSync(path.join(workspace,'data/nodes.json'))),u);}
  watch=openBasketWatch(runtime,{initialize:process.argv.includes('--init'),definitions});
  do{if(stops.some(f=>fs.existsSync(f)))break;const n=watch.consume();publish(n===null?'awaiting_capture':'running');if(process.argv.includes('--once'))break;for(let i=0;i<75&&!stops.some(f=>fs.existsSync(f));i++)await new Promise(r=>setTimeout(r,200));}while(!stops.some(f=>fs.existsSync(f)));
  publish(process.argv.includes('--once')?'initialized':'stopped');
 }catch{if(watch)publish('failed');console.error('Basket observer stopped; inspect local status and immutable journal');process.exitCode=1;}
 finally{release?.();}
}
