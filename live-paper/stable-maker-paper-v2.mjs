import {amount,money,SCALE,TICK,instant} from '../paper/fixed.mjs';
import {instantNs} from '../feed/contract.mjs';
import {payloadHash} from '../execution/ledger.mjs';
import {tapeTick} from './canonical-entry-exit-study.mjs';
import {probabilityAccountGate} from './probability-receipt.mjs';

const need=(ok,code)=>{if(!ok)throw Error('STABLE_MAKER_'+code);};
export const STABLE_MAKER_POLICY=Object.freeze({schemaVersion:2,initialCash:'100.000000',maximumReservedAndOwned:'5.000000',maximumOwnedMarkets:3,cumulativeBuyBudget:'25.000000',quantity:1,
 latencyMs:3000,lifetimeMs:120000,cooldownMs:60000,maximumHoldMs:7200000,stopLoss:'0.030000',profitTarget:'0.005000',exitAdverseTicks:1,depthFractionBps:100,feePerShare:'0.000000',
 historyMs:300000,minimumHistorySpanMs:240000,minimumDistinctMinutes:4,maximumHistoryGapMs:120000,maximumMidpointRange:'0.005000',minimumSpread:'0.005000',maximumSpread:'0.050000',improvementTicks:0,
 families:['YES','NO'],fillModel:'hypothetical_strict_trade_through_not_proven_execution'});
export function validateStableMakerBank(bank){
 need(bank?.schemaVersion===1&&bank.mode==='stable_two_side_trade_tape_paper_v2'&&payloadHash(bank.policy)===payloadHash(STABLE_MAKER_POLICY),'BANK');
 instant(bank.cutoff);instant(bank.entryExpiresAt);need(instant(bank.entryExpiresAt)>instant(bank.cutoff),'CLOCK');
 need(Array.isArray(bank.legs)&&bank.legs.length>0&&bank.legs.length<=16&&new Set(bank.legs.map(l=>l.exchangeId)).size===bank.legs.length,'SCOPE');
 for(const l of bank.legs)need(/^[1-9]\d*$/.test(l.marketId)&&/^[1-9]\d*$/.test(l.exchangeId),'IDENTITY');
 return bank;
}
export function createStableMakerState(bank){validateStableMakerBank(bank);return {schemaVersion:1,bankHash:payloadHash(bank),lastAt:bank.cutoff,nextId:1,seen:{},books:{},history:{},coverage:{},counts:{},
 wallets:Object.fromEntries(bank.policy.families.map(side=>[side,{cash:bank.policy.initialCash,realized:'0.000000',fees:'0.000000',cumulativeBuys:'0.000000',positions:{},quotes:{},fills:[],cooldowns:{},risk:{}}]))};}
const inc=(s,k)=>s.counts[k]=(s.counts[k]||0)+1;
const open=w=>Object.values(w.quotes).filter(q=>!q.resolvedAt);
const reserved=w=>open(w).filter(q=>q.action==='buy').reduce((n,q)=>n+amount(q.limit),0n);
const owned=w=>Object.values(w.positions).reduce((n,p)=>n+amount(p.cost),0n);
const openFor=(w,key)=>open(w).some(q=>q.exchangeId===key);
const prices=(b,side)=>side==='YES'?{bid:amount(b.bids[0].price),ask:amount(b.asks[0].price),buyDepth:b.bids[0].quantity,sellDepth:b.bids[0].quantity}:
 {bid:SCALE-amount(b.asks[0].price),ask:SCALE-amount(b.bids[0].price),buyDepth:b.asks[0].quantity,sellDepth:b.asks[0].quantity};
export function assertStableMaker(s,bank){
 need(s.bankHash===payloadHash(bank),'BANK_CHANGED');
 for(const w of Object.values(s.wallets)){const buys=w.fills.filter(f=>f.action==='buy').reduce((n,f)=>n+amount(f.price),0n),sells=w.fills.filter(f=>f.action==='sell').reduce((n,f)=>n+amount(f.price),0n);
  need(amount(w.cash)===amount(bank.policy.initialCash)-buys+sells&&amount(w.cash)>=reserved(w),'CASH');
  need(amount(w.cumulativeBuys)===buys&&buys<=amount(bank.policy.cumulativeBuyBudget),'TURNOVER');
  need(amount(w.realized)===sells-(buys-owned(w))&&w.fees==='0.000000','FIFO');
  need(owned(w)+reserved(w)<=amount(bank.policy.maximumReservedAndOwned)&&Object.values(w.positions).every(p=>p.quantity===1),'RISK');
 }
}
function fill(s,w,q,at,source,bank){const key=q.exchangeId,cost=amount(q.limit);
 if(q.action==='buy'){need(!w.positions[key]&&amount(w.cash)>=cost,'BUY');w.cash=money(amount(w.cash)-cost);w.cumulativeBuys=money(amount(w.cumulativeBuys)+cost);
  w.positions[key]={marketId:q.marketId,exchangeId:key,side:q.side,quantity:1,cost:q.limit,enteredAt:at,sourceEntryAt:source.kind==='trade'?source.createdAt:at};}
 else{need(w.positions[key]?.quantity===1,'COVERED_EXIT');w.cash=money(amount(w.cash)+cost);w.realized=money(amount(w.realized)+cost-amount(w.positions[key].cost));delete w.positions[key];delete w.risk[key];w.cooldowns[key]=instant(at)+bank.policy.cooldownMs;}
 q.resolvedAt=at;q.resolution='hypothetical_fill';w.fills.push({action:q.action,marketId:q.marketId,exchangeId:key,side:q.side,quantity:1,price:q.limit,at,quoteId:q.id,source:structuredClone(source),model:source.kind==='trade'?bank.policy.fillModel:'hypothetical_later_request_taker'});inc(s,'hypothetical_'+q.action);
}
function quote(s,w,b,side,at,action,limit,bank){need(limit>=TICK&&limit<=SCALE-TICK&&limit%TICK===0n,'PRICE');
 const activation=Math.max(instant(at)+bank.policy.latencyMs,Number(instantNs(b.sourceAt)/1000000n)+2000),expiry=instant(at)+bank.policy.lifetimeMs;
 const q={id:s.nextId++,action,side,marketId:b.marketId,exchangeId:b.exchangeId,quantity:1,limit:money(limit),yesEquivalentLimit:money(side==='YES'?limit:SCALE-limit),createdAt:at,
  activationAt:new Date(activation).toISOString(),expiresAt:new Date(expiry).toISOString(),bookVersion:b.versionKey,bookSequence:b.sequence,publicationHash:b.publicationHash,publicationSequence:b.publicationSequence,
  coverageBarrierSequence:null,resolvedAt:null,resolution:null};w.quotes[q.id]=q;inc(s,'quote_'+action);
}
function observeHistory(s,b,at,p){const k=b.exchangeId,row={at:instant(at),request:instant(b.requestStartedAt),bid:b.bids[0].price,ask:b.asks[0].price,version:b.versionKey};let rows=s.history[k]||[];
 if(rows.length&&(row.at-rows.at(-1).at>p.maximumHistoryGapMs||row.request-rows.at(-1).request>p.maximumHistoryGapMs)){rows=[];inc(s,'history_gap');}
 // Repeated source versions can age a window, but cannot provide new observations.
 if(!rows.length||row.version!==rows.at(-1).version)rows.push(row);
 rows=rows.filter(x=>x.at>=row.at-p.historyMs);s.history[k]=rows;
 if(rows.length<4||row.at-rows[0].at<p.minimumHistorySpanMs||new Set(rows.map(x=>Math.floor(x.at/60000))).size<p.minimumDistinctMinutes)return false;
 const mid=rows.map(x=>amount(x.bid)+amount(x.ask));return mid.reduce((a,b)=>a>b?a:b)-mid.reduce((a,b)=>a<b?a:b)<=2n*amount(p.maximumMidpointRange);
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
export function advanceStableMaker(state,event,bank){need(state.bankHash===payloadHash(bank),'BANK_CHANGED');const s=structuredClone(state),p=bank.policy,at=event.availableAt,now=instant(at);need(now>=instant(s.lastAt),'CLOCK');s.lastAt=at;
 if(event.kind==='boundary'){s.books={};s.history={};inc(s,'source_boundary');}
 else if(event.kind==='book'){const b=event.book,key=b.exchangeId;need(bank.legs.some(l=>l.marketId===b.marketId&&l.exchangeId===key)&&b.instrument===key+':YES'&&b.tournamentId===bank.tournamentId,'BOOK_SCOPE');
  need(instant(b.receivedAt)<=now&&instant(b.requestStartedAt)<=instant(b.receivedAt),'RECEIPT');
  const fresh=now-instant(b.requestStartedAt)<=20000&&probabilityAccountGate(event.accountEligibility,b,at)===null;
  const stable=fresh&&observeHistory(s,b,at,p);if(fresh)s.books[key]=structuredClone(b);else{delete s.books[key];delete s.history[key];inc(s,'book_ineligible');}
  for(const [side,w]of Object.entries(s.wallets)){
   for(const q of open(w))if(q.exchangeId===key&&q.coverageBarrierSequence===null&&instant(b.requestStartedAt)>=instant(q.expiresAt)&&instantNs(b.sourceAt)>=BigInt(instant(q.expiresAt))*1000000n)q.coverageBarrierSequence=b.sequence;
   if(!fresh)continue;const px=prices(b,side),position=w.positions[key],mark=px.bid-TICK;
   if(position){const timedOut=now-Date.parse(position.sourceEntryAt)>=p.maximumHoldMs,loss=amount(position.cost)-mark>=amount(p.stopLoss);
    if((timedOut||loss)&&(!w.risk[key]||w.risk[key].reason==='profit_available'))w.risk[key]={createdAt:at,reason:timedOut?'holding_limit':'loss_limit'};
    // Existing resting orders retain their original server expiry, including on
    // a risk signal or source boundary. A later print must be resolved first.
    if(w.risk[key]){const allowed=w.risk[key].reason!=='profit_available'||mark>=amount(position.cost)+amount(p.profitTarget);if(allowed&&!openFor(w,key)&&instant(b.requestStartedAt)>=instant(w.risk[key].createdAt)+p.latencyMs&&mark>0n&&Math.floor(px.sellDepth*p.depthFractionBps/10000)>=1){const q={id:s.nextId++,action:'sell',side,marketId:b.marketId,exchangeId:key,limit:money(mark)};fill(s,w,q,at,{kind:'book',version:b.versionKey,publicationHash:b.publicationHash,publicationSequence:b.publicationSequence},bank);}continue;}
    if(!openFor(w,key)){const limit=amount(position.cost)+amount(p.profitTarget);if(limit>px.bid&&limit<=SCALE-TICK)quote(s,w,b,side,at,'sell',limit,bank);
     else if(limit<=mark&&!w.risk[key])w.risk[key]={createdAt:at,reason:'profit_available'};}
    continue;
   }
   if(instant(b.requestStartedAt)<=instant(bank.cutoff)||now>=instant(bank.entryExpiresAt)||openFor(w,key)||(w.cooldowns[key]||0)>now)continue;
   if(!stable){inc(s,'unstable_or_warmup');continue;}
   const spread=px.ask-px.bid,limit=px.bid+BigInt(p.improvementTicks)*TICK;if(spread<amount(p.minimumSpread)||spread>amount(p.maximumSpread)||limit>=px.ask||limit<SCALE/20n||limit>19n*SCALE/20n||Math.floor(px.buyDepth*p.depthFractionBps/10000)<1){inc(s,'spread_or_depth');continue;}
   const activeMarkets=new Set([...Object.keys(w.positions),...open(w).filter(q=>q.action==='buy').map(q=>q.exchangeId)]);
   if(activeMarkets.size>=p.maximumOwnedMarkets||owned(w)+reserved(w)+limit>amount(p.maximumReservedAndOwned)||amount(w.cumulativeBuys)+reserved(w)+limit>amount(p.cumulativeBuyBudget)||amount(w.cash)-reserved(w)<limit){inc(s,'capital');continue;}
   quote(s,w,b,side,at,'buy',limit,bank);
  }
 }else if(event.kind==='trades'){const page=event.page;need(page.tournamentId===bank.tournamentId&&bank.legs.some(l=>l.marketId===page.marketId&&l.exchangeId===page.exchangeId)&&instant(page.receivedAt)<=now,'PAGE_SCOPE');
  const trades=page.trades.slice().sort((a,b)=>instantNs(a.createdAt)<instantNs(b.createdAt)?-1:instantNs(a.createdAt)>instantNs(b.createdAt)?1:a.key.localeCompare(b.key));
  for(const t of trades){need(instantNs(t.createdAt)>=BigInt(instant(page.from))*1000000n&&instantNs(t.createdAt)<BigInt(instant(page.to))*1000000n,'TRADE_WINDOW');if(s.seen[t.key])continue;s.seen[t.key]=true;const tick=tapeTick(t);if(!tick||tick.tick===0||tick.tick===200){inc(s,'unusable_trade');continue;}
   for(const [side,w]of Object.entries(s.wallets))for(const q of open(w)){if(q.exchangeId!==page.exchangeId||instantNs(t.createdAt)<BigInt(instant(q.activationAt))*1000000n||instantNs(t.createdAt)>=BigInt(instant(q.expiresAt))*1000000n)continue;
    const yes=amount(tick.price),limit=amount(q.yesEquivalentLimit),through=(q.action==='buy')===(side==='YES')?yes<=limit-TICK:yes>=limit+TICK;if(!through)continue;
    need(t.size>=1,'TRADE_SIZE');fill(s,w,q,at,{kind:'trade',key:t.key,id:t.id,createdAt:t.createdAt,firstAvailableAt:page.receivedAt,normalizedYesPrice:tick.price,aggressorSide:null,queuePositionProven:false,executionProven:false},bank);
   }
  }
  recordCoverage(s,page);for(const w of Object.values(s.wallets))for(const q of open(w))if(q.exchangeId===page.exchangeId&&q.coverageBarrierSequence!==null&&covered(s.coverage[q.exchangeId],q)){q.resolvedAt=at;q.resolution='projection_observed_without_qualifying_trade';inc(s,'quote_unfilled_resolved');}
 }else need(false,'EVENT_KIND');assertStableMaker(s,bank);return s;
}
export function stableMakerReport(s,bank,at){assertStableMaker(s,bank);return{schemaVersion:1,mode:bank.mode,cohort:bank.name,bankHash:s.bankHash,updatedAt:at,cutoff:bank.cutoff,counts:s.counts,
 wallets:Object.fromEntries(Object.entries(s.wallets).map(([side,w])=>{let proceeds=0n,complete=true;const positions=Object.values(w.positions).map(pos=>{const b=s.books[pos.exchangeId],px=b&&prices(b,side),fresh=b&&instant(at)>=instant(b.receivedAt)&&instant(at)-instant(b.receivedAt)<=20000;const mark=fresh&&Math.floor(px.sellDepth*bank.policy.depthFractionBps/10000)>=1?px.bid-TICK:null;if(mark===null||mark<=0n)complete=false;else proceeds+=mark;return{...pos,estimatedLiquidationProceeds:mark===null||mark<=0n?null:money(mark)};});return[side,{cash:w.cash,realizedPnl:w.realized,fees:w.fees,cumulativeBuys:w.cumulativeBuys,fills:w.fills.length,closedTrades:w.fills.filter(f=>f.action==='sell').length,positions,unresolvedQuotes:open(w),reserved:money(reserved(w)),recentFills:w.fills.slice(-12),estimatedLiquidationPnl:complete?money(amount(w.cash)+proceeds-amount(bank.policy.initialCash)):null}];})),
 executionAuthority:false,competitionOrders:0,networkRequests:0,profitabilityEstablished:false,limitations:['Strict trade-through is a counterfactual fill assumption; anonymous trades do not prove our queue position or actual execution.','YES and NO wallets share opportunities, are correlated, and must not be added as independent returns.','Incomplete tape coverage retains cash and inventory reservations; source gaps do not create trades.']};}
