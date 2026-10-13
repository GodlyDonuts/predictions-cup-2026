import {receiptEvents} from './sig-source.mjs';import {instantNs,compareVersion} from '../feed/contract.mjs';import {payloadHash} from '../execution/ledger.mjs';
import fs from 'node:fs';import path from 'node:path';
const check=(ok,code)=>{if(!ok)throw Error('STREAM_SOURCE_'+code);};
const version=b=>({sequence:Number(b.sequence),at:b.sourceAt});
const decimal=p=>{check(Number.isFinite(p)&&p>0&&p<1&&Math.abs(p*200-Math.round(p*200))<1e-7,'PRICE');return(Math.round(p*200)/200).toFixed(6);};
const tradeKey=t=>[t.tournamentId,t.exchangeId,t.id].join(':');
const tradeHash=t=>payloadHash({marketId:t.marketId,executedAt:t.executedAt,yesPrice:t.yesPrice,quantity:t.quantity});
export function predecessorTradeFacts(directory,state,runtimeRoot){
  const facts=new Map(),visited=new Set(),boundary=path.resolve(runtimeRoot)+path.sep;
  let current=directory?path.resolve(directory):null;
  while(current){check(current.startsWith(boundary)&&!visited.has(current)&&visited.size<100,'PARENT_SCOPE');visited.add(current);
    const manifest=JSON.parse(fs.readFileSync(path.join(current,'manifest.json'))),snapshot=JSON.parse(fs.readFileSync(path.join(current,'state.json'))),{hash,...body}=snapshot;check(hash===payloadHash(body),'PARENT_SNAPSHOT');
    let sequence=0,previous='';for(const line of fs.readFileSync(path.join(current,'events.jsonl'),'utf8').split(/\r?\n/).filter(Boolean)){const row=JSON.parse(line),{hash,...body}=row;check(hash===payloadHash(body)&&body.sequence===++sequence&&body.previous===previous,'PARENT_CHAIN');previous=hash;const t=body.event;if(t.type==='trade'&&state.seen[tradeKey(t)]===tradeHash(t))facts.set(tradeKey(t),t);}
    check(sequence===snapshot.sequence&&previous===snapshot.previous,'PARENT_TERMINAL');
    const next=manifest.continuation?.parentDirectory;if(next){check(path.resolve(next).startsWith(boundary),'PARENT_SCOPE');const parent=JSON.parse(fs.readFileSync(path.join(next,'state.json')));check(parent.hash===manifest.continuation.parentSnapshotHash,'PARENT_LINK');}current=next?path.resolve(next):null;
  }
  check(facts.size===Object.keys(state.seen).length,'MISSING_TRADE_PROVENANCE');return facts;
}
export class StreamSource {
  constructor(scopes,tournamentId,startedAt,facts=new Map()){this.scopes=new Map(scopes.map(p=>[p.marketId,p]));this.tournamentId=tournamentId;this.facts=new Map(facts);this.status=new Map(scopes.map(p=>[p.marketId,{invalidatedAt:startedAt,blocked:true,restBook:false,tape:false}]));this.books=new Map();this.revisions=new Map();this.sequence=0;this.previous='';this.metrics={rows:0,pushedBooks:0,olderBooks:0,initialResyncs:0,gaps:0,duplicates:0,unblocks:0,precisionReconciliations:0};}
  trade(t){const key=tradeKey(t),known=this.facts.get(key);if(!known){this.facts.set(key,t);return t;}
    check(known.marketId===t.marketId&&known.yesPrice===t.yesPrice&&known.quantity===t.quantity,'TRADE_FACT_CONFLICT');
    const a=instantNs(known.executedAt),b=instantNs(t.executedAt),official=new Set(['sig_engine_match','sig_reporting_tape_only']);
    const reportingNs=known.idNamespace==='sig_reporting_tape_only'?a:t.idNamespace==='sig_reporting_tape_only'?b:null;
    check(a===b||a/1000000n===b/1000000n&&reportingNs!==null&&reportingNs%1000000n===0n&&known.idNamespace!==t.idNamespace&&official.has(known.idNamespace)&&official.has(t.idNamespace),'TRADE_CLOCK_CONFLICT');
    if(known.executedAt!==t.executedAt){this.metrics.precisionReconciliations++;return{...t,executedAt:known.executedAt,originalExecutedAt:t.executedAt,executionClockUse:'first verified representation for duplicate only; original wire clock retained'};}return t;
  }
  invalidate(marketId,at){const s=this.status.get(marketId);check(s,'SCOPE');s.invalidatedAt=at;s.blocked=true;s.restBook=false;s.tape=false;return[{type:'gap',exchangeId:this.scopes.get(marketId).exchangeId,receivedAt:at}];}
  remember(book){const old=this.books.get(book.exchangeId);if(old){const order=compareVersion(version(book),version(old));if(order<0){this.metrics.olderBooks++;return false;}if(order===0)check(payloadHash([book.bids,book.asks])===payloadHash([old.bids,old.asks]),'VERSION_CONFLICT');}
    this.books.set(book.exchangeId,book);return true;}
  rest(r){
    const p=this.scopes.get(r.marketId);check(p&&p.exchangeId===r.exchangeId&&r.tournamentId===this.tournamentId,'REST_SCOPE');
    const events=receiptEvents(r),s=this.status.get(r.marketId),out=[];
    for(const e of events){
      if(e.type==='book'){const accepted=this.remember(e.book);if(s.blocked&&accepted)out.push({type:'book',receivedAt:e.receivedAt,book:{...e.book,invalid:true}});if(instantNs(r.requestStartedAt)>=instantNs(s.invalidatedAt))s.restBook=true;}
      else if(e.type==='coverage'){
        out.push(e);if(instantNs(e.from)<=instantNs(s.invalidatedAt)&&instantNs(e.to)>=instantNs(s.invalidatedAt))s.tape=true;
      }else if(e.type==='trade'){out.push(this.trade(e));}
      else if(e.type==='gap')out.push(...this.invalidate(r.marketId,r.receivedAt));
    }
    if(s.blocked&&s.restBook&&s.tape){s.blocked=false;this.metrics.unblocks++;}
    const held=this.books.get(p.exchangeId);
    if(!s.blocked&&held&&events.some(e=>e.type==='book'||e.type==='coverage')){const receivedAt=instantNs(held.receivedAt)>instantNs(r.receivedAt)?held.receivedAt:r.receivedAt;out.push({type:'book',receivedAt,book:{...held,receivedAt}});}
    return out;
  }
  row(row){
    const {hash,...body}=row;check(hash===payloadHash(body)&&body.sequence===this.sequence+1&&body.previous===this.previous,'CHAIN');this.sequence++;this.previous=hash;this.metrics.rows++;
    const e=body.event,p=this.scopes.get(e.marketId);check(p,'SCOPE');
    if(e.kind==='rest_reference')return[];
    if(e.kind==='subscribed'){this.metrics.initialResyncs++;return this.invalidate(e.marketId,e.receivedAt);}
    if(e.kind==='rejected')return this.invalidate(e.marketId,e.receivedAt);
    check(e.kind==='broadcast'&&e.source==='official_sig_realtime'&&e.tournamentId===this.tournamentId,'BROADCAST');
    if(e.event!=='market_batch')return this.invalidate(e.marketId,e.receivedAt);
    const batch=e.payload,d=batch.delivery,last=this.revisions.get(e.marketId);check(d?.model==='best-effort-authoritative-resync'&&Number.isSafeInteger(d.revision)&&d.revision>d.previousRevision,'REVISION');
    const duplicate=last!==undefined&&d.revision<=last,gap=last!==undefined&&d.previousRevision>last;if(!duplicate)this.revisions.set(e.marketId,d.revision);
    const bad=gap||batch.resyncRequired||batch.trades.some(t=>t.sequence===null)||batch.marketSettled.length;
    let out=[];if(bad){this.metrics.gaps++;out=this.invalidate(e.marketId,e.receivedAt);}
    const s=this.status.get(e.marketId);
    // Evaluate executions against quotes that existed before their newer book.
    if(!duplicate&&!bad&&!s.blocked)for(const t of batch.trades){check(t.marketId===p.marketId&&t.exchangeId===p.exchangeId&&t.tournamentId===this.tournamentId&&t.id!==null&&instantNs(t.executedAt)<=instantNs(e.receivedAt),'TRADE_SCOPE');out.push(this.trade({type:'trade',tournamentId:this.tournamentId,marketId:p.marketId,exchangeId:p.exchangeId,id:t.id,idNamespace:'sig_engine_match',executedAt:t.executedAt,receivedAt:e.receivedAt,yesPrice:decimal(t.price),quantity:t.quantity}));}
    if(duplicate)this.metrics.duplicates++;
    for(const b of batch.books){check(b.exchangeId===p.exchangeId&&Number.isSafeInteger(b.asOf?.sequence),'BOOK_SCOPE');const sourceAt=b.asOf.at;check(instantNs(sourceAt)<=instantNs(e.receivedAt),'BOOK_CLOCK');
      const book={tournamentId:this.tournamentId,marketId:p.marketId,exchangeId:p.exchangeId,sequence:String(b.asOf.sequence),sourceAt,receivedAt:e.receivedAt,requestStartedAt:e.receivedAt,transport:'official_sig_realtime',bids:b.bids.map(l=>({price:decimal(l.price),quantity:l.quantity})),asks:b.asks.map(l=>({price:decimal(l.price),quantity:l.quantity}))};
      if(!book.bids.length||!book.asks.length){out.push(...this.invalidate(e.marketId,e.receivedAt));continue;}
      if(this.remember(book)&&!s.blocked){out.push({type:'book',receivedAt:e.receivedAt,book});this.metrics.pushedBooks++;}
    }
    return out;
  }
}
