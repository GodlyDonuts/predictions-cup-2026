import {payloadHash} from '../execution/ledger.mjs';
import {normalizeBook} from './model.mjs';
import {validateDualNoReview} from './dual-no-review-v1.mjs';
import {amount,money,SCALE,TICK,instant} from '../paper/fixed.mjs';

const need=(ok,code)=>{if(!ok)throw Error('PAIRED_CAPTURE_'+code);};
const safeId=s=>typeof s==='string'&&/^[1-9]\d*$/.test(s)&&Number.isSafeInteger(Number(s));
const POLICY=Object.freeze({rootMaximumAgeMs:300000,bookMaximumAgeMs:20000,maximumSkewMs:10000,accountMaximumAgeMs:20000,depthFractionBps:100,quantity:1,minimumMargin:'0.010000',maximumReservationAgeMs:10000});

// The caller supplies its EXISTING serialized client and enforced account-budget
// view. This helper owns no client, credentials, limiter, publisher, timer or
// execution authority. Roots are finished before a separately granted two-read
// book packet. Neither grant increases the ordinary four-book allocation.
export function createPairedCandidateCapture({universe,dispatch,sharedReadHeadroom,getAccountEligibility,canOperate=()=>true,signal,now=Date.now}){
 need(/^[a-f0-9]{8}(?:-[a-f0-9]{4}){3}-[a-f0-9]{12}$/.test(universe?.tournamentId||'')&&Array.isArray(universe.exchanges)&&typeof dispatch==='function'&&typeof sharedReadHeadroom==='function'&&typeof getAccountEligibility==='function','DEPENDENCIES');
 const permits=new WeakMap(),consumed=new Set();let candidate=null,busy=false;
 const operate=()=>need(canOperate()&&!signal?.aborted,'STOPPED');
 const budget=reads=>need(sharedReadHeadroom({reads,at:new Date(now()).toISOString()})===true,'SHARED_BUDGET_HEADROOM');
 const authorizes=spec=>canOperate()&&!signal?.aborted&&spec?.method==='GET'&&spec.body===undefined&&spec.bodyJson===undefined&&typeof spec.beforeDispatch==='function'&&typeof spec.beforeNetwork==='function'&&permits.get(spec.beforeDispatch)===spec.endpoint&&permits.get(spec.beforeNetwork)===spec.endpoint;
 function grant(g,reads){operate();need(g?.kind==='auxiliary_candidate_reads'&&g.sharedAccountBudget===true&&g.reservedReads===reads&&typeof g.reservationId==='string'&&/^[A-Za-z0-9_.:-]{1,120}$/.test(g.reservationId)&&!consumed.has(g.reservationId),'EXPLICIT_READ_RESERVATION');
  const issued=instant(g.issuedAt),expires=instant(g.expiresAt);need(issued<=now()&&expires>now()&&expires-issued<=POLICY.maximumReservationAgeMs,'RESERVATION_CLOCK');budget(reads);consumed.add(g.reservationId);return g;
 }
 const validGrant=g=>{operate();need(now()<instant(g.expiresAt),'RESERVATION_EXPIRED');};
 const accountView=()=>{const e=getAccountEligibility();need(e?.complete===true&&e.snapshot?.schemaVersion===1&&typeof e.availableAt==='string'&&Array.isArray(e.snapshot.excludedExchangeIds)&&e.snapshot.excludedExchangeIds.every(safeId),'ACCOUNT_COMPLETE');
  need(Object.keys(e.snapshot).sort().join('|')==='excludedExchangeIds|ordersAt|positionsAt|schemaVersion','ACCOUNT_SCHEMA');instant(e.availableAt);instant(e.snapshot.positionsAt);instant(e.snapshot.ordersAt);return structuredClone(e);
 };
 const accountGate=(e,at)=>{need(instant(e.availableAt)<=at&&[e.snapshot.positionsAt,e.snapshot.ordersAt].every(t=>instant(t)<=at&&at-instant(t)<=POLICY.accountMaximumAgeMs),'ACCOUNT_NOT_CAUSAL_OR_FRESH');need(!candidate.legs.some(l=>e.snapshot.excludedExchangeIds.includes(l.exchangeId)),'SCOPED_ACCOUNT_EXPOSURE');};
 async function read(endpoint,g,remaining,guard=()=>{}){
  const beforeDispatch=()=>{validGrant(g);budget(remaining);guard();},beforeNetwork=()=>{validGrant(g);budget(remaining-1);guard();};
  // The existing client durably admits this read BETWEEN these callbacks. Its
  // reservation must not be double-counted against the remaining shared quota.
  permits.set(beforeDispatch,endpoint);permits.set(beforeNetwork,endpoint);
  try{const r=await dispatch({method:'GET',endpoint,beforeDispatch,beforeNetwork});need(r?.status===200&&r.endpoint===endpoint&&instant(r.requestStartedAt)<=instant(r.receivedAt)&&instant(r.receivedAt)<=now()&&now()-instant(r.receivedAt)<=POLICY.bookMaximumAgeMs,'RECEIPT');need(r.data?.contexts?.length===1&&r.data.contexts[0].type==='tournament'&&r.data.contexts[0].tournament?.id===universe.tournamentId,'RESPONSE_SCOPE');return r;}finally{permits.delete(beforeDispatch);permits.delete(beforeNetwork);}
 }
 function reviewRoots(){const captured=candidate.roots;need(captured.length===2&&captured.every(c=>c.rootHash===payloadHash(c.root)&&now()-instant(c.receivedAt)<=POLICY.rootMaximumAgeMs),'ROOT_CACHE_HASH_OR_AGE');
  const knownAt=new Date(now()).toISOString(),review={schemaVersion:1,kind:'conditional_no_pair_review',id:candidate.name,tournamentId:universe.tournamentId,evidenceHash:payloadHash(captured),recordedAt:knownAt,expiresAt:new Date(Math.min(...captured.map(c=>instant(c.receivedAt)))+POLICY.rootMaximumAgeMs).toISOString(),normalResolutionOnly:true,administrativeRefundGapAcknowledged:true,
   legs:candidate.legs.map((l,i)=>({...l,captureMarketId:captured[i].marketId,outcome:'NO',rulesHash:captured[i].rootHash,root:captured[i].root})),
   assumptions:['Same General election race/stage/office/date and different party winners.','Normal mutual exclusion is conditional; voids, refunds, affiliation disputes or inconsistent administrative resolution can lose the whole cost.','Sequential observations and purchases are not atomic; a hedge may fail.'],scenarios:[{id:'democratic',category:'ordinary',payouts:[0,1]},{id:'republican',category:'ordinary',payouts:[1,0]},{id:'other',category:'ordinary',payouts:[1,1]},{id:'exception',category:'exception',payouts:[0,0]}]};validateDualNoReview(review);return review;
 }
 function screen(account){const books=candidate.books.map(c=>c.book),at=now(),starts=books.map(b=>instant(b.requestStartedAt)),receipts=books.map(b=>instant(b.receivedAt));accountGate(account,Math.min(...starts));need([account.snapshot.positionsAt,account.snapshot.ordersAt].every(t=>at-instant(t)<=POLICY.accountMaximumAgeMs),'ACCOUNT_STALE_AT_PACKET');need(starts.every(t=>t>=instant(candidate.review.recordedAt)),'RULES_NOT_KNOWN_AT_REQUEST');need(at-instant(candidate.review.recordedAt)<=POLICY.rootMaximumAgeMs,'ROOTS_EXPIRED');
  need(starts.every(t=>at-t<=POLICY.bookMaximumAgeMs)&&Math.max(...receipts)-Math.min(...starts)<=POLICY.maximumSkewMs,'BOOK_AGE_OR_SKEW');
  const prices=books.map(b=>{const level=b.bids.find(l=>Math.floor(l.quantity*POLICY.depthFractionBps/10000)>=1);return level?{price:money(SCALE-amount(level.price)+TICK),depth:Math.floor(level.quantity*POLICY.depthFractionBps/10000)}:null;});
  const cost=prices.every(Boolean)?prices.reduce((n,p)=>n+amount(p.price),0n):null,positive=cost!==null&&cost<=SCALE-amount(POLICY.minimumMargin)&&prices.every(p=>amount(p.price)>=TICK&&amount(p.price)<=SCALE-TICK);
  return{positiveConditionalScreen:positive,quantity:1,cushionedCost:cost===null?null:money(cost),conditionalOrdinarySurplus:cost===null?null:money(SCALE-cost),zeroPayoutExceptionLoss:cost===null?null:money(-cost),eligibleDepth:prices.map(p=>p?.depth??0),wholeWindowSkewMs:Math.max(...receipts)-Math.min(...starts),maximumBookAgeMs:at-Math.min(...starts),accountReceiptKnownBeforeBothRequests:true,accountSnapshotHash:payloadHash(account.snapshot),representation:'documented_same_exchange_yes_bid_complement',ordinaryResolutionOnly:true,publicationCreated:false,orderEligible:false,executionAuthority:false,fills:0,orders:0,profitabilityEstablished:false};
 }
 const status=()=>candidate?structuredClone({...candidate,policy:POLICY,executionAuthority:false,additionalAccountClients:0,fills:0,orders:0}):{phase:'idle',executionAuthority:false};
 return{authorizes,status,
  begin({name,marketIds}){operate();need(!candidate&&/^[a-z][a-z0-9-]{1,63}$/.test(name||'')&&Array.isArray(marketIds)&&marketIds.length===2&&new Set(marketIds).size===2,'SINGLE_PAIR_REQUEST');const legs=marketIds.map(marketId=>universe.exchanges.find(l=>l.marketId===marketId&&l.option==='YES'));need(legs.every(l=>l&&safeId(l.marketId)&&safeId(l.exchangeId)),'PAIR_SCOPE');candidate={name,legs:legs.map(({marketId,exchangeId})=>({marketId,exchangeId})),phase:'roots',roots:[],books:[],review:null,screen:null};return status();},
  async advance({reservation}){operate();need(candidate&&!busy&&!['complete','failed'].includes(candidate.phase),'SINGLE_PAIR_OPERATION');busy=true;try{
   if(candidate.phase==='roots'){const remaining=Math.min(2-candidate.roots.length,reservation?.reservedReads??0);need(remaining>0&&remaining<=2,'ROOT_READ_GRANT');const g=grant(reservation,remaining);for(let i=0;i<remaining;i++){const l=candidate.legs[candidate.roots.length],endpoint='/markets/'+l.marketId+'/nodes?tournamentId='+universe.tournamentId,r=await read(endpoint,g,remaining-i);need(r.data.market_id===l.marketId&&r.data.root?.node_type==='contract'&&r.data.root.contract_type==='Election Outcome','ROOT_SCOPE');candidate.roots.push({marketId:l.marketId,tournamentId:universe.tournamentId,requestStartedAt:r.requestStartedAt,receivedAt:r.receivedAt,root:structuredClone(r.data.root),rootHash:payloadHash(r.data.root),responseHash:payloadHash(r.data)});operate();}if(candidate.roots.length===2){candidate.review=reviewRoots();candidate.phase='books';}return status();}
   need(candidate.phase==='books','PHASE');const g=grant(reservation,2);candidate.review=reviewRoots();const account=accountView(),accountHash=payloadHash(account.snapshot);accountGate(account,now());
   for(let i=0;i<2;i++){const l=candidate.legs[i],endpoint='/markets/'+l.marketId+'/orderbook?tournamentId='+universe.tournamentId+'&depth=20';
    const r=await read(endpoint,g,2-i,()=>{accountGate(account,now());need(payloadHash(accountView().snapshot)===accountHash,'ACCOUNT_CHANGED_BETWEEN_LEGS');need(now()<instant(candidate.review.expiresAt),'ROOTS_EXPIRED');});
    accountGate(account,instant(r.requestStartedAt));const b=normalizeBook(r.data,l.marketId,universe.tournamentId,r.receivedAt,r.requestStartedAt,POLICY.bookMaximumAgeMs,new Set(),'authoritative_receipt_v1');need(b.exchangeId===l.exchangeId&&b.bids.length<=20&&b.asks.length<=20,'BOOK_EXCHANGE_OR_DEPTH');candidate.books.push({marketId:l.marketId,requestStartedAt:r.requestStartedAt,receivedAt:r.receivedAt,book:b,responseHash:payloadHash(r.data)});operate();
   }candidate.screen=screen(account);candidate.phase='complete';return status();
  }catch(e){candidate.phase='failed';candidate.failureCode=/^PAIRED_CAPTURE_[A-Z_]+$/.test(e.message)?e.message:e.status?'SHARED_CLIENT_HTTP_'+e.status:'SHARED_CLIENT_OR_VALIDATION_FAILURE';throw e;}finally{busy=false;}}
 };
}
