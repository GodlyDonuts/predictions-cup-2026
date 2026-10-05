import {amount,money,SCALE,TICK,instant} from '../paper/fixed.mjs';
import {instantNs} from '../feed/contract.mjs';
import {payloadHash} from '../execution/ledger.mjs';
import {tapeTick} from './canonical-entry-exit-study.mjs';
import {validatePriceObservation} from './market-price-publication.mjs';
import {normalizeOfficialFillTick} from '../execution/api-tick-price.mjs';
import {validateUniverse} from './breadth.mjs';
import {probabilityAccountGate} from './probability-receipt.mjs';

const need=(ok,code)=>{if(!ok)throw Error('COUPLED_BULK_'+code);};
export const COUPLED_BULK_POLICY=Object.freeze({schemaVersion:2,initialCash:'100.000000',maximumReservedAndOwned:'5.000000',maximumOwnedMarkets:3,cumulativeBuyBudget:'25.000000',quantity:1,
 latencyMs:3000,lifetimeMs:120000,cooldownMs:60000,maximumHoldMs:7200000,stopLoss:'0.030000',profitTarget:'0.005000',exitAdverseTicks:1,depthFractionBps:100,feePerShare:'0.000000',
 historyMs:300000,minimumHistorySpanMs:240000,minimumDistinctMinutes:4,maximumHistoryGapMs:120000,maximumMidpointRange:'0.005000',minimumSpread:'0.005000',maximumSpread:'0.050000',improvementTicks:1,maximumHistoryReceiptAgeMs:90000,minimumPriceRequestSpacingMs:2000,
 families:['COUPLED'],fillModel:'hypothetical_strict_trade_through_not_proven_execution'});
export function validateCoupledBulkBank(bank){
 need(bank?.schemaVersion===1&&bank.mode==='coupled_bulk_history_trade_tape_paper_v2'&&payloadHash(bank.policy)===payloadHash(COUPLED_BULK_POLICY),'BANK');
 instant(bank.cutoff);instant(bank.entryExpiresAt);need(instant(bank.entryExpiresAt)>instant(bank.cutoff),'CLOCK');
 need(Array.isArray(bank.legs)&&bank.legs.length>0&&bank.legs.length<=300&&new Set(bank.legs.map(l=>l.exchangeId)).size===bank.legs.length,'SCOPE');
 need(bank.universe&&bank.universe.tournamentId===bank.tournamentId,'UNIVERSE');const {hash,...raw}=bank.universe,source=bank.universeSourceJson?JSON.parse(bank.universeSourceJson):raw,validated=validateUniverse(source,bank.tournamentId);need(validated.hash===hash&&payloadHash(validated)===payloadHash(bank.universe),'UNIVERSE_HASH');
 for(const l of bank.legs)need(/^[1-9]\d*$/.test(l.marketId)&&/^[1-9]\d*$/.test(l.exchangeId)&&bank.universe.exchanges.some(x=>x.exchangeId===l.exchangeId&&x.marketId===l.marketId&&x.option==='YES'),'IDENTITY');
 return bank;
}
export function createCoupledBulkState(bank){validateCoupledBulkBank(bank);return {schemaVersion:1,bankHash:payloadHash(bank),lastAt:bank.cutoff,nextId:1,seen:{},books:{},history:{},coverage:{},counts:{},
 wallets:Object.fromEntries(bank.policy.families.map(side=>[side,{cash:bank.policy.initialCash,realized:'0.000000',fees:'0.000000',cumulativeBuys:'0.000000',positions:{},quotes:{},fills:[],redemptions:[],cooldowns:{},risk:{}}]))};}
const inc=(s,k)=>s.counts[k]=(s.counts[k]||0)+1;
const open=w=>Object.values(w.quotes).filter(q=>!q.resolvedAt);
const reserved=w=>open(w).filter(q=>q.action==='buy').reduce((n,q)=>n+amount(q.limit),0n);
const owned=w=>Object.values(w.positions).reduce((n,p)=>n+amount(p.cost),0n);
const openFor=(w,key)=>open(w).some(q=>q.exchangeId===key);
const prices=(b,side)=>side==='YES'?{bid:amount(b.bids[0].price),ask:amount(b.asks[0].price),buyDepth:b.bids[0].quantity,sellDepth:b.bids[0].quantity}:
 {bid:SCALE-amount(b.asks[0].price),ask:SCALE-amount(b.bids[0].price),buyDepth:b.asks[0].quantity,sellDepth:b.asks[0].quantity};
export function assertCoupledBulk(s,bank){
 need(s.bankHash===payloadHash(bank)&&Object.keys(s.wallets).join('|')==='COUPLED','BANK_CHANGED');const w=s.wallets.COUPLED;
 const buys=w.fills.filter(f=>f.action==='buy').reduce((n,f)=>n+amount(f.price),0n),sells=w.fills.filter(f=>f.action==='sell').reduce((n,f)=>n+amount(f.price),0n),payouts=BigInt(w.redemptions.length)*SCALE;
 need(amount(w.cash)===amount(bank.policy.initialCash)-buys+sells+payouts&&amount(w.cash)>=reserved(w),'CASH');
 need(amount(w.cumulativeBuys)===buys&&buys+reserved(w)<=amount(bank.policy.cumulativeBuyBudget),'TURNOVER');
 need(amount(w.realized)===sells+payouts-(buys-owned(w))&&w.fees==='0.000000','FIFO');
 need(owned(w)+reserved(w)<=amount(bank.policy.maximumReservedAndOwned)&&Object.values(w.positions).every(p=>p.quantity===1&&['YES','NO'].includes(p.side)),'RISK');
 need(new Set([...Object.keys(w.positions),...open(w).map(q=>q.exchangeId)]).size<=bank.policy.maximumOwnedMarkets,'MARKET_LIMIT');
 for(const key of new Set(open(w).map(q=>q.exchangeId)))for(const side of ['YES','NO'])need(open(w).filter(q=>q.exchangeId===key&&q.side===side).length<=1,'DUPLICATE_QUOTE');
}
function fill(s,w,q,at,source,bank){const key=q.exchangeId,cost=amount(q.limit),position=w.positions[key];
 if(q.action==='buy'){need(amount(w.cash)>=cost&&(!position||position.side!==q.side),'BUY');w.cash=money(amount(w.cash)-cost);w.cumulativeBuys=money(amount(w.cumulativeBuys)+cost);
  if(position){const gain=SCALE-amount(position.cost)-cost;w.cash=money(amount(w.cash)+SCALE);w.realized=money(amount(w.realized)+gain);
   w.redemptions.push({marketId:q.marketId,exchangeId:key,quantity:1,payout:'1.000000',acquisitionCosts:[position.cost,q.limit],realizedPnl:money(gain),at,firstLegSide:position.side,firstLegSourceAt:position.sourceEntryAt,secondLegSourceAt:source.kind==='trade'?source.createdAt:at,quoteId:q.id});
   delete w.positions[key];delete w.risk[key];w.cooldowns[key]=instant(at)+bank.policy.cooldownMs;inc(s,'hypothetical_pair_redemption');
  }else w.positions[key]={marketId:q.marketId,exchangeId:key,side:q.side,quantity:1,cost:q.limit,enteredAt:at,sourceEntryAt:source.kind==='trade'?source.createdAt:at};
 }else{need(position?.quantity===1&&position.side===q.side,'COVERED_EXIT');w.cash=money(amount(w.cash)+cost);w.realized=money(amount(w.realized)+cost-amount(position.cost));delete w.positions[key];delete w.risk[key];w.cooldowns[key]=instant(at)+bank.policy.cooldownMs;}
 q.resolvedAt=at;q.resolution='hypothetical_fill';w.fills.push({action:q.action,marketId:q.marketId,exchangeId:key,side:q.side,quantity:1,price:q.limit,at,quoteId:q.id,source:structuredClone(source),model:source.kind==='trade'?bank.policy.fillModel:'hypothetical_later_request_taker'});inc(s,'hypothetical_'+q.action);
}
function quote(s,w,b,side,at,action,limit,bank){need(limit>=TICK&&limit<=SCALE-TICK&&limit%TICK===0n,'PRICE');
 const activation=Math.max(instant(at)+bank.policy.latencyMs,Number(instantNs(b.sourceAt)/1000000n)+2000),expiry=instant(at)+bank.policy.lifetimeMs;
 const q={id:s.nextId++,action,side,marketId:b.marketId,exchangeId:b.exchangeId,quantity:1,limit:money(limit),yesEquivalentLimit:money(side==='YES'?limit:SCALE-limit),createdAt:at,
  activationAt:new Date(activation).toISOString(),expiresAt:new Date(expiry).toISOString(),bookVersion:b.versionKey,bookSequence:b.sequence,publicationHash:b.publicationHash,publicationSequence:b.publicationSequence,
  coverageBarrierSequence:null,resolvedAt:null,resolution:null};w.quotes[q.id]=q;inc(s,'quote_'+action);
}
function observeBulkHistory(s,snapshot,at,bank){validatePriceObservation(snapshot,bank.universe);const p=bank.policy,received=instant(snapshot.receivedAt),request=instant(snapshot.requestStartedAt),known=instant(at);
 need(snapshot.tournamentId===bank.tournamentId&&received<=known,'PRICE_CLOCK');const members=new Set(bank.legs.map(x=>x.exchangeId));
 for(const id of snapshot.missingIds)if(members.has(id))delete s.history[id];
 for(const q of snapshot.quotes){if(!members.has(q.exchangeId))continue;let bid,ask;try{bid=normalizeOfficialFillTick(q.bestBid).price;ask=normalizeOfficialFillTick(q.bestAsk).price;}catch{delete s.history[q.exchangeId];inc(s,'unusable_bulk_quote');continue;}
  if(amount(bid)>=amount(ask)||request<=instant(bank.cutoff)||known-received>p.maximumHistoryReceiptAgeMs){delete s.history[q.exchangeId];continue;}
  let rows=s.history[q.exchangeId]||[];if(rows.length&&received-rows.at(-1).received>p.maximumHistoryGapMs){rows=[];inc(s,'bulk_history_gap');}
  if(!rows.length||request>rows.at(-1).request+p.minimumPriceRequestSpacingMs)rows=rows.concat({received,request,known,bid,ask,marketId:q.marketId});
  s.history[q.exchangeId]=rows.filter(r=>received-r.received<=p.historyMs);inc(s,'bulk_feature_observation');
 }
}
function stableBulkWindow(s,key,now,p,book=null){const request=book?instant(book.requestStartedAt):now,rows=(s.history[key]||[]).filter(r=>r.received>=request-p.historyMs&&r.received<=request&&r.known<=request),last=rows.at(-1);
 if(!last||now-last.received>p.maximumHistoryReceiptAgeMs||rows.length<4||last.received-rows[0].received<p.minimumHistorySpanMs||new Set(rows.map(r=>Math.floor(r.request/60000))).size<p.minimumDistinctMinutes)return false;
 const mids=rows.map(r=>amount(r.bid)+amount(r.ask));if(book)mids.push(amount(book.bids[0].price)+amount(book.asks[0].price));
 return mids.reduce((a,b)=>a>b?a:b)-mids.reduce((a,b)=>a<b?a:b)<=2n*amount(p.maximumMidpointRange);
}
export function coupledBulkCandidates(s,bank,at){const now=instant(at),p=bank.policy,out=[];
 for(const leg of bank.legs){if(!stableBulkWindow(s,leg.exchangeId,now,p))continue;const last=s.history[leg.exchangeId].at(-1),bid=amount(last.bid),ask=amount(last.ask),spread=ask-bid,total=SCALE-spread+2n*BigInt(p.improvementTicks)*TICK;
  if(spread<amount(p.minimumSpread)||spread>amount(p.maximumSpread)||total>SCALE-amount(p.profitTarget)||bid<SCALE/20n||ask>19n*SCALE/20n)continue;
  out.push({...leg,spreadTicks:Number(spread/TICK),indicativeOnly:true,depthAvailable:false,executionEligible:false});
 }return out.sort((a,b)=>b.spreadTicks-a.spreadTicks||Number(a.marketId)-Number(b.marketId));
}
function recordCoverage(s,page){const c=s.coverage[page.exchangeId]??={pending:{},windows:[]},request=page.requestCursor??null,key=payloadHash([page.from,page.to]);
 let chain=request===null?{from:page.from,to:page.to,expected:null,complete:true,checkpoint:null}:c.pending[key];
 if(!chain||chain.expected!==request||chain.from!==page.from||chain.to!==page.to){delete c.pending[key];inc(s,'coverage_cursor_gap');return;}
 const cp=page.coverage.projectedThroughSequence;chain.complete=chain.complete&&page.coverage.complete===true&&cp!==null;
 if(cp!==null)chain.checkpoint=chain.checkpoint===null?String(cp):String(BigInt(chain.checkpoint)<BigInt(cp)?chain.checkpoint:cp);
 if(page.pagination.hasMore){need(typeof page.pagination.nextCursor==='string'&&page.pagination.nextCursor&&page.pagination.nextCursor!==request,'CURSOR');chain.expected=page.pagination.nextCursor;c.pending[key]=chain;need(Object.keys(c.pending).length<=16,'WINDOW_LIMIT');}
 else{if(chain.complete)c.windows.push({from:chain.from,to:chain.to,checkpoint:chain.checkpoint});delete c.pending[key];}
 const active=Object.values(s.wallets).flatMap(w=>open(w).filter(q=>q.exchangeId===page.exchangeId).map(q=>instant(q.activationAt))),oldest=active.length?Math.min(...active):instant(s.lastAt);
 c.windows=c.windows.filter(x=>instant(x.to)>oldest);
}
function covered(c,q){let through=instant(q.activationAt);for(const x of(c?.windows||[]).filter(x=>BigInt(x.checkpoint)>=BigInt(q.coverageBarrierSequence)).sort((a,b)=>instant(a.from)-instant(b.from))){if(instant(x.from)>through)break;through=Math.max(through,instant(x.to));if(through>=instant(q.expiresAt))return true;}return false;}
export function advanceCoupledBulk(state,event,bank){need(state.bankHash===payloadHash(bank),'BANK_CHANGED');const s={...state,counts:{...state.counts},books:{...state.books},history:event.kind==='prices'?{...state.history}:state.history,wallets:event.kind==='prices'?state.wallets:structuredClone(state.wallets),coverage:event.kind==='trades'?structuredClone(state.coverage):state.coverage,seen:event.kind==='trades'?{...state.seen}:state.seen},p=bank.policy,at=event.availableAt,now=instant(at);need(now>=instant(s.lastAt),'CLOCK');s.lastAt=at;
 if(event.kind==='boundary'){s.books={};s.history={};inc(s,'source_boundary');}
 else if(event.kind==='prices'){observeBulkHistory(s,event.snapshot,at,bank);}
 else if(event.kind==='book'){const b=event.book,key=b.exchangeId;need(bank.legs.some(l=>l.marketId===b.marketId&&l.exchangeId===key)&&b.instrument===key+':YES'&&b.tournamentId===bank.tournamentId,'BOOK_SCOPE');
  need(instant(b.receivedAt)<=now&&instant(b.requestStartedAt)<=instant(b.receivedAt),'RECEIPT');
  const fresh=now-instant(b.requestStartedAt)<=20000&&probabilityAccountGate(event.accountEligibility,b,at)===null;
  const stable=fresh&&stableBulkWindow(s,key,now,p,b);if(fresh)s.books[key]=structuredClone(b);else{delete s.books[key];inc(s,'book_ineligible');}
  const w=s.wallets.COUPLED;
  for(const q of open(w))if(q.exchangeId===key&&q.coverageBarrierSequence===null&&instant(b.requestStartedAt)>=instant(q.expiresAt)&&instantNs(b.sourceAt)>=BigInt(instant(q.expiresAt))*1000000n)q.coverageBarrierSequence=b.sequence;
  if(fresh){const position=w.positions[key];
   if(position){const px=prices(b,position.side),mark=px.bid-TICK,timedOut=now-Date.parse(position.sourceEntryAt)>=p.maximumHoldMs,loss=amount(position.cost)-mark>=amount(p.stopLoss);
    if((timedOut||loss)&&(!w.risk[key]||w.risk[key].reason==='profit_available'))w.risk[key]={createdAt:at,reason:timedOut?'holding_limit':'loss_limit'};
    if(!w.risk[key]&&mark>=amount(position.cost)+amount(p.profitTarget))w.risk[key]={createdAt:at,reason:'profit_available'};
    if(w.risk[key]){const allowed=w.risk[key].reason!=='profit_available'||mark>=amount(position.cost)+amount(p.profitTarget);
     if(allowed&&!openFor(w,key)&&instant(b.requestStartedAt)>=instant(w.risk[key].createdAt)+p.latencyMs&&mark>0n&&Math.floor(px.sellDepth*p.depthFractionBps/10000)>=1){const q={id:s.nextId++,action:'sell',side:position.side,marketId:b.marketId,exchangeId:key,limit:money(mark)};fill(s,w,q,at,{kind:'book',version:b.versionKey,publicationHash:b.publicationHash,publicationSequence:b.publicationSequence},bank);}
    }else if(!openFor(w,key)){const side=position.side==='YES'?'NO':'YES',opposite=prices(b,side),target=SCALE-amount(position.cost)-amount(p.profitTarget),improved=opposite.bid+BigInt(p.improvementTicks)*TICK,passive=improved<opposite.ask?improved:opposite.ask-TICK,limit=passive<target?passive:target;
     if(limit>=TICK&&limit<=SCALE-TICK&&Math.floor(opposite.buyDepth*p.depthFractionBps/10000)>=1&&amount(w.cash)>=limit&&owned(w)+limit<=amount(p.maximumReservedAndOwned)&&amount(w.cumulativeBuys)+limit<=amount(p.cumulativeBuyBudget))quote(s,w,b,side,at,'buy',limit,bank);
    }
   }else if(instant(b.requestStartedAt)>instant(bank.cutoff)&&now<instant(bank.entryExpiresAt)&&!openFor(w,key)&&(w.cooldowns[key]||0)<=now){
    if(!stable)inc(s,'unstable_or_warmup');else{const yes=prices(b,'YES'),no=prices(b,'NO'),spread=yes.ask-yes.bid,limits=[yes.bid+BigInt(p.improvementTicks)*TICK,no.bid+BigInt(p.improvementTicks)*TICK],total=limits[0]+limits[1];
     if(spread<amount(p.minimumSpread)||spread>amount(p.maximumSpread)||limits[0]>=yes.ask||limits[1]>=no.ask||total>SCALE-amount(p.profitTarget)||limits.some(x=>x<SCALE/20n||x>19n*SCALE/20n)||Math.floor(yes.buyDepth*p.depthFractionBps/10000)<1||Math.floor(no.buyDepth*p.depthFractionBps/10000)<1)inc(s,'spread_or_depth');
     else{const activeMarkets=new Set([...Object.keys(w.positions),...open(w).map(q=>q.exchangeId)]);
      if(activeMarkets.size>=p.maximumOwnedMarkets||owned(w)+reserved(w)+total>amount(p.maximumReservedAndOwned)||amount(w.cumulativeBuys)+reserved(w)+total>amount(p.cumulativeBuyBudget)||amount(w.cash)-reserved(w)<total)inc(s,'capital');
      else{quote(s,w,b,'YES',at,'buy',limits[0],bank);quote(s,w,b,'NO',at,'buy',limits[1],bank);}
     }
    }
   }
  }
 }else if(event.kind==='trades'){const page=event.page;need(page.tournamentId===bank.tournamentId&&bank.legs.some(l=>l.marketId===page.marketId&&l.exchangeId===page.exchangeId)&&instant(page.receivedAt)<=now,'PAGE_SCOPE');
  const trades=page.trades.slice().sort((a,b)=>instantNs(a.createdAt)<instantNs(b.createdAt)?-1:instantNs(a.createdAt)>instantNs(b.createdAt)?1:a.key.localeCompare(b.key));
  for(const t of trades){need(instantNs(t.createdAt)>=BigInt(instant(page.from))*1000000n&&instantNs(t.createdAt)<BigInt(instant(page.to))*1000000n,'TRADE_WINDOW');if(s.seen[t.key])continue;s.seen[t.key]=true;const tick=tapeTick(t);if(!tick||tick.tick===0||tick.tick===200){inc(s,'unusable_trade');continue;}
   const w=s.wallets.COUPLED;let remaining=t.size;need(Number.isSafeInteger(remaining)&&remaining>0,'TRADE_SIZE');
   for(const q of open(w)){if(remaining===0||q.exchangeId!==page.exchangeId||instantNs(t.createdAt)<BigInt(instant(q.activationAt))*1000000n||instantNs(t.createdAt)>=BigInt(instant(q.expiresAt))*1000000n)continue;
    const yes=amount(tick.price),limit=amount(q.yesEquivalentLimit),through=(q.action==='buy')===(q.side==='YES')?yes<=limit-TICK:yes>=limit+TICK;if(!through)continue;
    fill(s,w,q,at,{kind:'trade',key:t.key,id:t.id,createdAt:t.createdAt,firstAvailableAt:page.receivedAt,normalizedYesPrice:tick.price,aggressorSide:null,queuePositionProven:false,executionProven:false},bank);remaining--;
   }
  }
  recordCoverage(s,page);for(const w of Object.values(s.wallets))for(const q of open(w))if(q.exchangeId===page.exchangeId&&q.coverageBarrierSequence!==null&&covered(s.coverage[q.exchangeId],q)){q.resolvedAt=at;q.resolution='projection_observed_without_qualifying_trade';inc(s,'quote_unfilled_resolved');}
 }else need(false,'EVENT_KIND');assertCoupledBulk(s,bank);return s;
}
export function coupledBulkReport(s,bank,at){assertCoupledBulk(s,bank);const w=s.wallets.COUPLED;let proceeds=0n,complete=true;
 const positions=Object.values(w.positions).map(pos=>{const b=s.books[pos.exchangeId],px=b&&prices(b,pos.side),fresh=b&&instant(at)>=instant(b.receivedAt)&&instant(at)-instant(b.receivedAt)<=20000;const mark=fresh&&Math.floor(px.sellDepth*bank.policy.depthFractionBps/10000)>=1?px.bid-TICK:null;if(mark===null||mark<=0n)complete=false;else proceeds+=mark;return{...pos,estimatedLiquidationProceeds:mark===null||mark<=0n?null:money(mark)};});
 return{schemaVersion:1,mode:bank.mode,cohort:bank.name,bankHash:s.bankHash,updatedAt:at,cutoff:bank.cutoff,counts:s.counts,bulkStableCandidates:coupledBulkCandidates(s,bank,at).length,featureScope:bank.legs.length,wallets:{COUPLED:{cash:w.cash,realizedPnl:w.realized,fees:w.fees,cumulativeBuys:w.cumulativeBuys,fills:w.fills.length,closedTrades:w.fills.filter(f=>f.action==='sell').length+w.redemptions.length,pairedRedemptions:w.redemptions.length,positions,unresolvedQuotes:open(w),reserved:money(reserved(w)),recentFills:w.fills.slice(-12),recentRedemptions:w.redemptions.slice(-12),estimatedLiquidationPnl:complete?money(amount(w.cash)+proceeds-amount(bank.policy.initialCash)):null}},executionAuthority:false,competitionOrders:0,networkRequests:0,profitabilityEstablished:false,
 limitations:['Bulk receipts provide indicative stability only; fresh full-book depth and account eligibility are still mandatory.','One shared cash wallet nets opposite one-share fills at the documented binary payout; unmatched inventory still bears loss.','Strict trade-through is a counterfactual fill assumption; anonymous trades do not prove queue position or actual execution.','Expired unresolved quotes keep reservations until original-window projection is complete; learned fill times never enable retrospective exits.']};
}
