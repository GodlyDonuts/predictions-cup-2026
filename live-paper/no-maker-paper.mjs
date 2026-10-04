import {amount,money,SCALE,TICK,instant} from '../paper/fixed.mjs';
import {instantNs} from '../feed/contract.mjs';
import {payloadHash} from '../execution/ledger.mjs';
import {tapeTick} from './canonical-entry-exit-study.mjs';
import {probabilityAccountGate} from './probability-receipt.mjs';
const need=(ok,c)=>{if(!ok)throw Error('NO_MAKER_'+c);};
export const NO_MAKER_POLICY=Object.freeze({schemaVersion:1,initialCash:'100.000000',maximumReservedAndOwned:'5.000000',
 maximumOwnedMarkets:2,quantity:1,latencyMs:3000,lifetimeMs:120000,cooldownMs:60000,maximumHoldMs:1800000,
 stopLoss:'0.030000',exitAdverseTicks:1,depthFractionBps:100,feePerShare:'0.000000',families:['at_quote','improve_one_tick'],
 fillModel:'hypothetical_strict_trade_through_not_proven_execution'});
export function createNoMakerState(bank){
 need(bank?.mode==='no_maker_trade_tape_paper'&&payloadHash(bank.policy)===payloadHash(NO_MAKER_POLICY),'BANK');instant(bank.cutoff);
 return {schemaVersion:1,bankHash:payloadHash(bank),lastAt:bank.cutoff,nextId:1,seen:{},books:{},counts:{},
  wallets:Object.fromEntries(bank.policy.families.map(f=>[f,{cash:bank.policy.initialCash,realized:'0.000000',fees:'0.000000',
   positions:{},quotes:{},fills:[],cooldowns:{},risk:{}}]))};
}
const inc=(s,k)=>s.counts[k]=(s.counts[k]||0)+1;
const reserved=w=>Object.values(w.quotes).filter(q=>!q.resolvedAt&&q.action==='buy').reduce((n,q)=>n+amount(q.limit)*BigInt(q.quantity),0n);
const owned=w=>Object.values(w.positions).reduce((n,p)=>n+amount(p.cost),0n);
export function assertNoMaker(state,bank){
 need(state.bankHash===payloadHash(bank),'BANK_CHANGED');
 for(const w of Object.values(state.wallets)){
  const buys=w.fills.filter(f=>f.action==='buy').reduce((n,f)=>n+amount(f.price)*BigInt(f.quantity),0n),sells=w.fills.filter(f=>f.action==='sell').reduce((n,f)=>n+amount(f.price)*BigInt(f.quantity),0n);
  need(amount(w.cash)===amount(bank.policy.initialCash)-buys+sells&&amount(w.cash)>=reserved(w),'CASH_CONSERVATION');
  need(owned(w)+reserved(w)<=amount(bank.policy.maximumReservedAndOwned)&&Object.values(w.positions).every(p=>p.quantity===1),'RISK');
  need(amount(w.realized)===sells-(buys-owned(w))&&w.fees==='0.000000','FIFO_CONSERVATION');
 }
}
function fill(s,w,q,at,source,bank){
 const cost=amount(q.limit)*BigInt(q.quantity),key=q.exchangeId;
 if(q.action==='buy'){need(!w.positions[key]&&amount(w.cash)>=cost,'BUY_RESERVATION');w.cash=money(amount(w.cash)-cost);w.positions[key]={marketId:q.marketId,exchangeId:key,quantity:q.quantity,cost:money(cost),enteredAt:at,sourceEntryAt:source.kind==='trade'?source.createdAt:at};}
 else{const p=w.positions[key];need(p?.quantity===q.quantity,'COVERED_EXIT');w.cash=money(amount(w.cash)+cost);w.realized=money(amount(w.realized)+cost-amount(p.cost));delete w.positions[key];delete w.risk[key];w.cooldowns[key]=instant(at)+bank.policy.cooldownMs;}
 q.resolvedAt=at;q.resolution='hypothetical_fill';w.fills.push({action:q.action,marketId:q.marketId,exchangeId:key,quantity:q.quantity,price:q.limit,at,quoteId:q.id,
  model:source.kind==='trade'?bank.policy.fillModel:'hypothetical_later_request_risk_taker',source:structuredClone(source)});inc(s,'hypothetical_'+q.action);
}
const openFor=(w,key)=>Object.values(w.quotes).some(q=>q.exchangeId===key&&!q.resolvedAt);
function quote(s,w,book,at,action,limit,bank){
 const px=amount(limit);need(px>=TICK&&px<=SCALE-TICK&&px%TICK===0n,'QUOTE_TICK');
 const activation=Math.max(instant(at)+bank.policy.latencyMs,Number(instantNs(book.sourceAt)/1000000n)+2000);
 const q={id:s.nextId++,action,marketId:book.marketId,exchangeId:book.exchangeId,quantity:1,limit,
  yesEquivalentLimit:money(SCALE-px),createdAt:at,activationAt:new Date(activation).toISOString(),expiresAt:new Date(instant(at)+bank.policy.lifetimeMs).toISOString(),
  bookVersion:book.versionKey,bookSequence:book.sequence,publicationHash:book.publicationHash,publicationSequence:book.publicationSequence,
  coverageBarrierSequence:null,resolvedAt:null,resolution:null};w.quotes[q.id]=q;inc(s,'quote_'+action);
}
export function advanceNoMaker(state,event,bank){
 need(state.bankHash===payloadHash(bank),'BANK_CHANGED');const s=structuredClone(state),p=bank.policy,at=event.availableAt;
 need(instant(at)>=instant(s.lastAt)&&instant(at)>=instant(bank.cutoff),'CONSUMER_CLOCK');s.lastAt=at;
 if(event.kind==='boundary'){
  for(const w of Object.values(s.wallets))for(const q of Object.values(w.quotes))if(!q.resolvedAt&&instant(at)<instant(q.expiresAt))q.expiresAt=at;
  s.books={};inc(s,'source_boundary');
 }else if(event.kind==='book'){
  const b=event.book,key=b.exchangeId;need(bank.legs.some(l=>l.marketId===b.marketId&&l.exchangeId===key)&&b.instrument===key+':YES','BOOK_SCOPE');
  need(instant(b.receivedAt)<=instant(at),'FUTURE_RECEIPT');s.books[key]=structuredClone(b);
  const fresh=instant(at)-instant(b.requestStartedAt)<=20000&&probabilityAccountGate(event.accountEligibility,b,at)===null;
  for(const [family,w]of Object.entries(s.wallets)){
   for(const q of Object.values(w.quotes))if(!q.resolvedAt&&q.exchangeId===key&&instant(b.requestStartedAt)>=instant(q.expiresAt)&&instantNs(b.sourceAt)>=BigInt(instant(q.expiresAt))*1000000n)q.coverageBarrierSequence=b.sequence;
   if(!fresh){inc(s,'book_not_fresh_or_excluded');continue;}
   const position=w.positions[key],bid=SCALE-amount(b.asks[0].price),ask=SCALE-amount(b.bids[0].price);
   if(position){const mark=bid-TICK,stop=amount(position.cost)-mark>=amount(p.stopLoss)||instant(at)-Date.parse(position.sourceEntryAt)>=p.maximumHoldMs;
    if(stop&&!w.risk[key]){w.risk[key]={createdAt:at,reason:instant(at)-Date.parse(position.sourceEntryAt)>=p.maximumHoldMs?'holding_limit':'loss_limit'};
     for(const q of Object.values(w.quotes))if(q.exchangeId===key&&!q.resolvedAt&&instant(at)<instant(q.expiresAt))q.expiresAt=at;}
    if(w.risk[key]){
     // Pending resting exits must be resolved by the reporting projection before
     // a second, potentially duplicate, hypothetical sell is admitted.
     if(!openFor(w,key)&&instant(b.requestStartedAt)>=instant(w.risk[key].createdAt)+p.latencyMs&&mark>0n&&Math.floor(b.asks[0].quantity*p.depthFractionBps/10000)>=1){
      const q={id:s.nextId++,action:'sell',marketId:b.marketId,exchangeId:key,quantity:1,limit:money(mark)};fill(s,w,q,at,{kind:'book',version:b.versionKey,publicationHash:b.publicationHash,publicationSequence:b.publicationSequence},bank);
     }continue;
    }
    if(!openFor(w,key)){const limit=ask-(family==='improve_one_tick'&&ask-bid>=3n*TICK?TICK:0n);if(limit>bid&&limit>amount(position.cost))quote(s,w,b,at,'sell',money(limit),bank);}continue;
   }
   if(!event.entryAllowed||instant(b.requestStartedAt)<=instant(bank.cutoff)||instant(at)>=instant(bank.reviewExpiresAt)||openFor(w,key)||(w.cooldowns[key]||0)>instant(at))continue;
   if(Object.keys(w.positions).length+new Set(Object.values(w.quotes).filter(q=>!q.resolvedAt&&q.action==='buy').map(q=>q.exchangeId)).size>=p.maximumOwnedMarkets)continue;
   const improve=family==='improve_one_tick'&&ask-bid>=3n*TICK,limit=bid+(improve?TICK:0n);
   if(ask-limit<2n*TICK||Math.floor(b.asks[0].quantity*p.depthFractionBps/10000)<1){inc(s,'insufficient_spread_or_depth');continue;}
   if(owned(w)+reserved(w)+limit>amount(p.maximumReservedAndOwned)||amount(w.cash)-reserved(w)<limit)continue;
   quote(s,w,b,at,'buy',money(limit),bank);
  }
 }else if(event.kind==='trades'){
  const page=event.page;need(page.tournamentId===bank.tournamentId&&bank.legs.some(l=>l.marketId===page.marketId&&l.exchangeId===page.exchangeId)
   &&instant(page.receivedAt)<=instant(at),'PAGE_SCOPE');
  const trades=page.trades.slice().sort((a,b)=>instantNs(a.createdAt)<instantNs(b.createdAt)?-1:instantNs(a.createdAt)>instantNs(b.createdAt)?1:a.key.localeCompare(b.key));
  for(const t of trades){need(instantNs(t.createdAt)<BigInt(instant(page.to))*1000000n&&instantNs(t.createdAt)>=BigInt(instant(page.from))*1000000n,'TRADE_WINDOW');if(s.seen[t.key])continue;s.seen[t.key]=true;
   const tick=tapeTick(t);if(!tick||tick.tick===0||tick.tick===200){inc(s,'unusable_trade_price');continue;}
   for(const w of Object.values(s.wallets))for(const q of Object.values(w.quotes)){
    if(q.resolvedAt||q.exchangeId!==page.exchangeId||instantNs(t.createdAt)<BigInt(instant(q.activationAt))*1000000n||instantNs(t.createdAt)>=BigInt(instant(q.expiresAt))*1000000n)continue;
    const yes=amount(tick.price),limit=amount(q.yesEquivalentLimit),through=q.action==='buy'?yes>=limit+TICK:yes<=limit-TICK;
    if(!through)continue;need(t.size>=1,'TRADE_QUANTITY');
    fill(s,w,q,at,{kind:'trade',key:t.key,id:t.id,createdAt:t.createdAt,firstAvailableAt:page.receivedAt,normalizedYesPrice:tick.price,rawYesPrice:t.yesPrice,
     priceRepresentation:tick.representation,aggressorSide:null,queuePositionProven:false,executionProven:false},bank);
   }
  }
  // Non-fill resolution needs both a complete page window and a reporting
  // checkpoint beyond an observed post-expiry book; otherwise capital stays reserved.
  for(const w of Object.values(s.wallets))for(const q of Object.values(w.quotes))if(!q.resolvedAt&&q.exchangeId===page.exchangeId&&q.coverageBarrierSequence!==null
   &&page.coverage.complete===true&&!page.pagination.hasMore&&page.coverage.projectedThroughSequence!==null
   &&BigInt(page.coverage.projectedThroughSequence)>=BigInt(q.coverageBarrierSequence)&&instant(page.to)>=instant(q.expiresAt)){
    q.resolvedAt=at;q.resolution='projection_observed_without_qualifying_trade';inc(s,'quote_unfilled_projection_resolved');
   }
 }else need(false,'EVENT_KIND');
 assertNoMaker(s,bank);return s;
}
export function noMakerReport(s,bank,at){
 assertNoMaker(s,bank);return {schemaVersion:1,mode:bank.mode,cohort:bank.name,bankHash:s.bankHash,updatedAt:at,cutoff:bank.cutoff,counts:s.counts,
  wallets:Object.fromEntries(Object.entries(s.wallets).map(([family,w])=>{let proceeds=0n,complete=true;const positions=Object.values(w.positions).map(pos=>{
   const b=s.books[pos.exchangeId],fresh=b&&instant(at)>=instant(b.receivedAt)&&instant(at)-instant(b.receivedAt)<=20000;
   const value=fresh&&Math.floor(b.asks[0].quantity/100)>=pos.quantity?SCALE-amount(b.asks[0].price)-TICK:null;if(value===null||value<=0n)complete=false;else proceeds+=value*BigInt(pos.quantity);
   return {...pos,estimatedLiquidationProceeds:value===null?null:money(value*BigInt(pos.quantity)),quoteAgeMs:b?instant(at)-instant(b.receivedAt):null};});
   return [family,{cash:w.cash,realizedPnl:w.realized,fees:w.fees,fills:w.fills.length,closedTrades:w.fills.filter(f=>f.action==='sell').length,
    positions,unresolvedQuotes:Object.values(w.quotes).filter(q=>!q.resolvedAt),reserved:money(reserved(w)),recentFills:w.fills.slice(-10),
    estimatedLiquidationPnl:complete?money(amount(w.cash)+proceeds-amount(bank.policy.initialCash)):null}];})),
  executionAuthority:false,competitionOrders:0,networkRequests:0,profitabilityEstablished:false,
  limitations:['All executions are local counterfactual models based on strict price-through or later-request risk depth.',
   'Anonymous trade space is not aggressor side; unseen market reaction and hidden liquidity can change actual fills.',
   'Two quote families share correlated opportunities; paper profit is not official-account profit.']};
}
