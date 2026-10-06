// Read-only source reference. It cannot authorize, fund or dispatch an order.
import fs from 'node:fs';
import readline from 'node:readline';
import {createHash} from 'node:crypto';
import {validatePriceObservation} from '../live-paper/market-price-publication.mjs';
import {money,price,instant,TICK} from '../paper/fixed.mjs';

export const MEDIAN_REFERENCE_POLICY=Object.freeze({historyMs:3600000,minimumSamples:20,minimumSpanMs:1800000,excludeMostRecentKnownBulkPoint:true,knownBeforeOriginalBookRequest:true,median:'upper',dipTicks:1});
const need=(ok,code)=>{if(!ok)throw Error('MEDIAN_REFERENCE_'+code);};
const hash=x=>createHash('sha256').update(JSON.stringify(x)).digest('hex');
const hex=x=>typeof x==='string'&&/^[a-f0-9]{64}$/.test(x);
// Match native bid_tick: exact binary representations of documented 0.005 ticks,
// including exact complement artifacts. No rounding or float money arithmetic.
const tickPrices=new Map();for(let i=1;i<200;i++){tickPrices.set(i/200,BigInt(i)*TICK);tickPrices.set(1-(200-i)/200,BigInt(i)*TICK);tickPrices.set(1-(1-i/200),BigInt(i)*TICK);}
function exactAsk(x){return typeof x==='string'?price(x):tickPrices.get(x);}
function validatePrefix(pin){need(pin&&Number.isSafeInteger(pin.bytes)&&pin.bytes>=0&&Number.isSafeInteger(pin.sequence)&&pin.sequence>=0&&hex(pin.sha256)&&(pin.sequence===0?pin.hash===''&&pin.bytes===0:hex(pin.hash)&&pin.bytes>0),'PREFIX_CONFIG');}

export function createMedianReferenceScreen({universe,sourcePrefix,cutoff}){
 validatePrefix(sourcePrefix);const cutoffMs=instant(cutoff),scope=structuredClone(universe),pin=structuredClone(sourcePrefix);
 need(scope&&typeof scope.hash==='string'&&typeof scope.tournamentId==='string'&&Array.isArray(scope.exchanges)&&scope.exchanges.length>0,'UNIVERSE');
 const members=new Map(scope.exchanges.map(x=>[x.marketId,x]));need(members.size===scope.exchanges.length&&new Set(scope.exchanges.map(x=>x.exchangeId)).size===scope.exchanges.length&&scope.exchanges.every(x=>x.option==='YES'),'UNIVERSE');
 let sequence=0,previous='',known=-Infinity,received=-Infinity,bytes=0,failed=false,sealed=pin.sequence===0;const history=new Map();
 function reject(code){failed=true;throw Error('MEDIAN_REFERENCE_'+code);}
 function accept(row,{endBytes}={}){try{
  need(!failed,'FAILED');need(row&&Object.keys(row).sort().join('|')==='at|event|hash|previous|sequence|state','ROW_SHAPE');const {hash:got,...body}=row;
  need(row.sequence===sequence+1&&row.previous===previous&&got===hash(body)&&row.state===null&&row.event?.type==='market_price_observation','SOURCE_CHAIN');
  const at=instant(row.at),snapshot=validatePriceObservation(row.event.snapshot,scope),receipt=instant(snapshot.receivedAt);need(at>=known&&receipt>=received&&receipt<=at,'SOURCE_CLOCK');
  need(Number.isSafeInteger(endBytes)&&endBytes>bytes,'SOURCE_BYTES');
  if(row.sequence<=pin.sequence)need(at<=cutoffMs&&endBytes<=pin.bytes,'PRIME_FUTURE');
  if(row.sequence===pin.sequence){need(endBytes===pin.bytes&&got===pin.hash,'SEALED_PREFIX');sealed=true;}
  if(row.sequence>pin.sequence)need(sealed,'UNSEALED_PREFIX');
  // Missing/null/crossed quotes do not fabricate a point or erase valid native
  // bulk history. Sequence/hash discontinuity is a hard failure, never a reset.
  for(const q of snapshot.quotes){if(q.option!=='YES'||q.crossed)continue;const bid=tickPrices.get(q.bestBid);if(bid===undefined)continue;const points=history.get(q.marketId)||[];while(points.length&&at-points[0].knownAt>MEDIAN_REFERENCE_POLICY.historyMs)points.shift();need(points.length<10000,'HISTORY_BOUND');points.push({knownAt:at,bid,sequence:row.sequence,hash:got,requestStartedAt:snapshot.requestStartedAt,receivedAt:snapshot.receivedAt});history.set(q.marketId,points);}
  sequence=row.sequence;previous=got;known=at;received=receipt;bytes=endBytes;return status();
 }catch(e){failed=true;throw e;}}
 function status(){return{sequence,hash:previous,bytes,knownAt:Number.isFinite(known)?new Date(known).toISOString():null,sealed,failed,universeHash:scope.hash,executionAuthority:false};}
 function reference({marketId,exchangeId,bookRequestStartedAt,availableAt,bestAsk}){need(!failed,'FAILED');need(sealed,'UNSEALED_PREFIX');const member=members.get(marketId);need(member&&member.exchangeId===exchangeId,'BOOK_SCOPE');const request=instant(bookRequestStartedAt),available=instant(availableAt);need(request<=available&&known<=available,'BOOK_CLOCK');const ask=exactAsk(bestAsk);need(typeof ask==='bigint','ASK_OFF_TICK');
  const prior=(history.get(marketId)||[]).filter(x=>available-x.knownAt<=MEDIAN_REFERENCE_POLICY.historyMs&&x.knownAt<request),dropped=prior.pop(),first=prior[0],last=prior.at(-1);
  const proof={sourcePrefix:structuredClone(pin),frontier:status(),marketId,exchangeId,bookRequestStartedAt,availableAt,eligibleSamplesBeforeDrop:prior.length+(dropped?1:0),retainedSamples:prior.length,firstKnownAt:first?new Date(first.knownAt).toISOString():null,lastKnownAt:last?new Date(last.knownAt).toISOString():null,excludedNewest:dropped?{sequence:dropped.sequence,hash:dropped.hash,knownAt:new Date(dropped.knownAt).toISOString()}:null,retainedPointsHash:hash(prior.map(x=>({sequence:x.sequence,hash:x.hash,knownAt:x.knownAt,bid:money(x.bid)}))),windowLowerBound:new Date(available-MEDIAN_REFERENCE_POLICY.historyMs).toISOString(),clockModel:'journal_known_before_original_book_request',depthAvailable:false,executable:false};
  const noSignal=reason=>({outcome:'outcomeNoSignal',outcomeNoSignal:true,reason,reference:null,sourceProof:proof,executionAuthority:false});
  if(prior.length<MEDIAN_REFERENCE_POLICY.minimumSamples)return noSignal('insufficient_lagged_samples');if(last.knownAt-first.knownAt<MEDIAN_REFERENCE_POLICY.minimumSpanMs)return noSignal('insufficient_lagged_span');
  const values=prior.map(x=>x.bid).sort((a,b)=>a<b?-1:a>b?1:0),median=values[Math.floor(values.length/2)],gap=median-ask;
  if(gap<TICK)return{...noSignal('insufficient_one_tick_gap'),reference:money(median),ask:money(ask),gap:money(gap)};
  return{outcome:'reference',outcomeNoSignal:false,reference:money(median),ask:money(ask),gap:money(gap),gapTicks:Number(gap/TICK),sourceProof:proof,executionAuthority:false};
 }
 return{accept,reference,status,assertPrefix(){need(!failed&&sealed&&sequence>=pin.sequence,'UNSEALED_PREFIX');return status();}};
}

async function prefixHash(file,bytes){const h=createHash('sha256');if(bytes){const stream=fs.createReadStream(file,{start:0,end:bytes-1});for await(const chunk of stream)h.update(chunk);}return h.digest('hex');}
// Bootstrap once; refresh explicitly once per collector batch, not per book.
// Rehash consumed bytes before refresh to fail closed on in-place source edits.
export async function openMedianReferenceReader({file,universe,sourcePrefix,cutoff}){
 const screen=createMedianReferenceScreen({universe,sourcePrefix,cutoff}),initial=fs.statSync(file);need(initial.size>=sourcePrefix.bytes,'PREFIX_TRUNCATED');let bytes=0,sha256=createHash('sha256').digest('hex'),failed=false;
 async function consume(end){if(end===bytes)return;const stream=fs.createReadStream(file,{start:bytes,end:end-1}),lines=readline.createInterface({input:stream,crlfDelay:Infinity});let pending=bytes;try{for await(const line of lines){need(Buffer.byteLength(line)<=2*1024*1024&&line.length>0,'ROW_BOUND');pending+=Buffer.byteLength(line)+1;need(pending<=end,'PARTIAL_RECORD');screen.accept(JSON.parse(line),{endBytes:pending});}need(pending===end,'PREFIX_BOUNDARY');bytes=pending;sha256=await prefixHash(file,bytes);}finally{lines.close();stream.destroy();}}
 need(await prefixHash(file,sourcePrefix.bytes)===sourcePrefix.sha256,'PREFIX_HASH');await consume(sourcePrefix.bytes);screen.assertPrefix();need(sha256===sourcePrefix.sha256,'PREFIX_CHANGED');
 return{reference:input=>{need(!failed,'FAILED');return screen.reference(input);},status:()=>({...screen.status(),sha256,fileIdentity:{dev:String(initial.dev),ino:String(initial.ino)}}),async refresh(){try{need(!failed,'FAILED');const current=fs.statSync(file);need(current.dev===initial.dev&&current.ino===initial.ino&&current.size>=bytes,'SOURCE_IDENTITY_OR_TRUNCATION');need(await prefixHash(file,bytes)===sha256,'CONSUMED_PREFIX_CHANGED');let end=current.size;if(end>bytes){const fd=fs.openSync(file,'r');try{const tailSize=Math.min(end-bytes,2*1024*1024+1),tail=Buffer.alloc(tailSize);need(fs.readSync(fd,tail,0,tailSize,end-tailSize)===tailSize,'SOURCE_READ');const last=tail.lastIndexOf(10);if(last<0){need(end-bytes<=2*1024*1024,'ROW_BOUND');end=bytes;}else end=end-tailSize+last+1;}finally{fs.closeSync(fd);}await consume(end);}return screen.status();}catch(e){failed=true;throw e;}}};
}
