import {amount,money,TICK,SCALE,instant} from '../paper/fixed.mjs';
import {payloadHash} from '../execution/ledger.mjs';
import {probabilityAccountGate} from './probability-receipt.mjs';

export const BAAL_PAIR_POLICY=Object.freeze({initialCash:'100.000000',quantity:1,latencyMs:3000,
 intentLifetimeMs:120000,maxAgeMs:20000,maxSkewMs:10000,depthFractionBps:100,adverseTicks:1,
 maxOwnedAndReserved:'5.000000',cumulativeBuyBudget:'25.000000',minimumPackageMargin:'0.010000',
 takeProfit:'0.010000',packageStopLoss:'0.050000',maximumPairedHoldMs:21600000,cooldownMs:60000,feePerShare:'0.000000'});
const need=(v,c)=>{if(!v)throw Error('BAAL_PAIR_'+c);},inc=(s,k)=>s.counts[k]=(s.counts[k]||0)+1;
const ids=b=>b.review.legs.map(l=>l.exchangeId);
const cost=s=>Object.values(s.positions).reduce((n,p)=>n+amount(p.cost),0n);
const reserve=s=>s.pending?.action==='buy'?amount(s.pending.limit):0n;
const currentReview=(b,at)=>instant(at)>=instant(b.review.recordedAt)&&instant(at)<instant(b.review.expiresAt);
function checkBank(b){need(b.mode==='baal_sequential_no_pair_paper'&&payloadHash(b.policy)===payloadHash(BAAL_PAIR_POLICY)&&b.review.legs.length===2,'BANK');instant(b.cutoff);}
export function createBaalPairState(bank){
 checkBank(bank);return{bankHash:payloadHash(bank),lastAt:bank.cutoff,cash:bank.policy.initialCash,realizedPnl:'0.000000',
 cumulativeBuys:'0.000000',positions:{},books:{},fills:[],counts:{},pending:null,package:null,anchor:null,nextIntentId:1,
 nextPackageId:1,cooldownUntil:0,roundtrips:[]};
}
export function assertBaalPair(s,b){
 need(s.bankHash===payloadHash(b),'BANK_CHANGED');
 let buys=0n,sells=0n;const quantities={};
 for(const f of s.fills){const v=amount(f.price)*BigInt(f.quantity);need(f.quantity===1&&ids(b).includes(f.exchangeId),'FILL_SCOPE');
  if(f.action==='buy'){buys+=v;quantities[f.exchangeId]=(quantities[f.exchangeId]||0)+1;}else{need(f.action==='sell','FILL_ACTION');sells+=v;quantities[f.exchangeId]=(quantities[f.exchangeId]||0)-1;need(quantities[f.exchangeId]>=0,'OVERSELL');}}
 need(amount(s.cash)===amount(b.policy.initialCash)-buys+sells&&amount(s.cash)>=reserve(s),'CASH_CONSERVATION');
 need(amount(s.realizedPnl)===sells-buys+cost(s)&&amount(s.cumulativeBuys)===buys,'FIFO_CONSERVATION');
 need(cost(s)+reserve(s)<=amount(b.policy.maxOwnedAndReserved)&&buys+reserve(s)<=amount(b.policy.cumulativeBuyBudget),'CAPITAL');
 for(const id of ids(b))need((s.positions[id]?.quantity||0)===(quantities[id]||0),'INVENTORY_CONSERVATION');
 need(Object.values(s.positions).every(p=>p.quantity===1)&&(!s.pending||s.pending.quantity===1),'QUANTITY');
 if(s.pending?.action==='sell')need(!!s.positions[s.pending.exchangeId],'COVERED_SELL');
 if(s.pending?.action==='buy')need(!s.positions[s.pending.exchangeId],'DUPLICATE_BUY');
 need(Boolean(s.package)===(cost(s)>0n||Boolean(s.pending)),'PACKAGE_OWNERSHIP');
}
function fresh(book,account,at,b){return book&&instant(book.receivedAt)<=instant(at)&&instant(at)-instant(book.requestStartedAt)<=b.policy.maxAgeMs&&probabilityAccountGate(account,book,at)===null;}
function pairBooks(s,account,at,b){const books=ids(b).map(id=>s.books[id]);if(!books.every(x=>fresh(x,account,at,b)))return null;
 return Math.max(...books.map(x=>instant(x.receivedAt)))-Math.min(...books.map(x=>instant(x.requestStartedAt)))<=b.policy.maxSkewMs?books:null;}
const buyUnit=book=>SCALE-amount(book.bids[0].price)+TICK;
const sellUnit=book=>SCALE-amount(book.asks[0].price)-TICK;
const depth=(book,action,b)=>Math.floor(book[action==='buy'?'bids':'asks'][0].quantity*b.policy.depthFractionBps/10000);
function pending(s,b,id,action,stage,limit,at){
 const book=s.books[id],leg=b.review.legs.find(l=>l.exchangeId===id);
 need(limit>=TICK&&limit<=SCALE-TICK&&limit%TICK===0n,'LIMIT');
 s.pending={id:s.nextIntentId++,packageId:s.package.id,marketId:leg.marketId,exchangeId:id,action,stage,quantity:1,limit:money(limit),
  createdAt:at,expiresAt:new Date(instant(at)+b.policy.intentLifetimeMs).toISOString(),decisionVersion:book?.versionKey??null};
 inc(s,'intent_'+stage);
}
function finish(s,b,at,reason){
 need(Object.keys(s.positions).length===0,'FINISH_OWNED');const p=s.package;
 s.roundtrips.push({id:p.id,openedAt:p.createdAt,closedAt:at,reason,buyCost:p.buyCost,saleProceeds:p.saleProceeds,
  realizedPnl:money(amount(p.saleProceeds)-amount(p.buyCost)),completedPair:p.completedPair});
 s.package=null;s.pending=null;s.anchor=null;s.cooldownUntil=instant(at)+b.policy.cooldownMs;inc(s,'closed_packages');
}
function orphan(s,b,at,reason){
 s.pending=null;s.anchor=null;
 if(!Object.keys(s.positions).length){finish(s,b,at,reason);return;}
 s.package.stage='orphan';s.package.orphanReason=reason;inc(s,'orphan_obligation');
}
function expired(s,b,at,reason){
 const i=s.pending;need(i,'EXPIRY');s.pending=null;inc(s,'intent_expired');
 if(i.stage==='entry_first')finish(s,b,at,reason);
 else if(i.stage==='entry_second'||i.stage==='exit_second')orphan(s,b,at,reason);
 else if(i.stage==='exit_first'){s.package.stage='holding';s.package.exitLimits=null;}
 else need(i.stage==='orphan','EXPIRY_STAGE');
}
function fill(s,book,at,b){
 const i=s.pending;if(!i||i.exchangeId!==book.exchangeId||book.versionKey===i.decisionVersion
  ||instant(book.requestStartedAt)<instant(i.createdAt)+b.policy.latencyMs||instant(book.receivedAt)>instant(i.expiresAt)
  ||instant(at)>instant(i.expiresAt)||depth(book,i.action,b)<1)return;
 const unit=i.action==='buy'?buyUnit(book):sellUnit(book);
 if(unit<TICK||unit>SCALE-TICK||(i.action==='buy'?unit>amount(i.limit):unit<amount(i.limit)))return;
 const p=s.package;
 if(i.action==='buy'){
  need(!s.positions[i.exchangeId],'DUPLICATE_POSITION');s.positions[i.exchangeId]={marketId:book.marketId,exchangeId:book.exchangeId,quantity:1,cost:money(unit),enteredAt:at};
  s.cash=money(amount(s.cash)-unit);s.cumulativeBuys=money(amount(s.cumulativeBuys)+unit);p.buyCost=money(amount(p.buyCost)+unit);
 }else{
  const position=s.positions[i.exchangeId];need(position,'COVERED_EXIT');s.cash=money(amount(s.cash)+unit);
  s.realizedPnl=money(amount(s.realizedPnl)+unit-amount(position.cost));p.saleProceeds=money(amount(p.saleProceeds)+unit);delete s.positions[i.exchangeId];
 }
 s.fills.push({...i,price:money(unit),at,fees:'0.000000',model:'counterfactual_later_independent_book_not_proven_execution',
  source:{requestStartedAt:book.requestStartedAt,receivedAt:book.receivedAt,version:book.versionKey,publicationSequence:book.publicationSequence,publicationHash:book.publicationHash,eligibleDepth:depth(book,i.action,b)}});
 inc(s,'hypothetical_'+i.action);s.pending=null;s.anchor=null;
 if(i.stage==='entry_first'){
  p.stage='entry_second';pending(s,b,p.second,'buy','entry_second',amount(p.entryLimits[p.second]),at);
 }else if(i.stage==='entry_second'){
  need(cost(s)<=SCALE-amount(b.policy.minimumPackageMargin),'PACKAGE_MARGIN');p.stage='holding';p.completedPair=true;p.completedAt=at;inc(s,'completed_pairs');
 }else if(i.stage==='exit_first'){
  p.stage='exit_second';pending(s,b,p.secondExit,'sell','exit_second',amount(p.exitLimits[p.secondExit]),at);
 }else if(i.stage==='exit_second'||i.stage==='orphan')finish(s,b,at,i.stage==='orphan'?p.orphanReason:p.exitReason);
 else need(false,'FILL_STAGE');
}
export function advanceBaalPair(input,event,bank){
 checkBank(bank);assertBaalPair(input,bank);const s=structuredClone(input),b=bank,at=event.availableAt;
 need(instant(at)>=instant(s.lastAt)&&instant(at)>=instant(b.cutoff),'CLOCK');s.lastAt=at;
 if(event.kind==='boundary'){
  s.books={};s.anchor=null;if(s.pending)expired(s,b,at,'source_boundary');inc(s,'source_boundary');assertBaalPair(s,b);return s;
 }
 need(event.kind==='book'&&b.review.legs.some(l=>l.exchangeId===event.book.exchangeId&&l.marketId===event.book.marketId),'BOOK_SCOPE');
 const book=event.book,account=event.accountEligibility;
 for(const [id,mark]of Object.entries(s.books))if(!fresh(mark,account,at,b))delete s.books[id];
 for(const mark of event.pairBooks||[])if(b.review.legs.some(l=>l.exchangeId===mark.exchangeId&&l.marketId===mark.marketId)&&fresh(mark,account,at,b))s.books[mark.exchangeId]=structuredClone(mark);
 if(fresh(book,account,at,b))s.books[book.exchangeId]=structuredClone(book);else delete s.books[book.exchangeId];
 const excludedLeg=Array.isArray(account?.excludedExchangeIds)&&ids(b).some(id=>account.excludedExchangeIds.includes(id));
 if(s.pending&&(instant(at)>instant(s.pending.expiresAt)||(s.pending.action==='buy'&&(!currentReview(b,at)||excludedLeg))))expired(s,b,at,excludedLeg?'account_exposure':'intent_or_review_expiry');
 if(fresh(book,account,at,b))fill(s,book,at,b);else inc(s,'unavailable_or_excluded_book');
 const books=pairBooks(s,account,at,b),p=s.package;
 if(p?.stage==='orphan'&&!s.pending){
  const id=Object.keys(s.positions)[0],mark=s.books[id];if(book.exchangeId===id&&fresh(mark,account,at,b)&&sellUnit(mark)>=TICK)pending(s,b,id,'sell','orphan',sellUnit(mark),at);
 }else if(p?.stage==='holding'&&!s.pending&&books){
  const units=books.map(sellUnit),proceeds=units.reduce((a,v)=>a+v,0n),basis=cost(s),stop=instant(at)-instant(p.completedAt)>=b.policy.maximumPairedHoldMs||basis-proceeds>=amount(b.policy.packageStopLoss);
  if(units.every(x=>x>=TICK)&&books.every(x=>depth(x,'sell',b)>=1)&&(proceeds>=basis+amount(b.policy.takeProfit)||stop)){
   p.exitReason=proceeds>=basis+amount(b.policy.takeProfit)?'paired_profit_target':instant(at)-instant(p.completedAt)>=b.policy.maximumPairedHoldMs?'paired_holding_limit':'paired_loss_limit';
   p.exitLimits=Object.fromEntries(books.map((x,i)=>[x.exchangeId,money(units[i])]));
   const first=Object.values(s.positions).sort((a,z)=>amount(a.cost)>amount(z.cost)?-1:1)[0].exchangeId;p.secondExit=ids(b).find(id=>id!==first);
   p.stage='exit_first';pending(s,b,first,'sell','exit_first',amount(p.exitLimits[first]),at);inc(s,'paired_exit_obligation');
  }
 }else if(!p&&!s.pending&&instant(at)>=s.cooldownUntil&&books&&event.admission?.positiveConditionalCapacity&&currentReview(b,at)
  &&books.every(x=>instant(x.requestStartedAt)>instant(b.cutoff))){
  const units=books.map(buyUnit),sum=units.reduce((a,v)=>a+v,0n),versions=books.map(x=>x.versionKey);
  if(units.every(v=>v>=TICK&&v<=SCALE-TICK)&&books.every(x=>depth(x,'buy',b)>=1)&&sum<=SCALE-amount(b.policy.minimumPackageMargin)
   &&sum<=amount(b.policy.maxOwnedAndReserved)&&amount(s.cumulativeBuys)+sum<=amount(b.policy.cumulativeBuyBudget)&&amount(s.cash)>=sum){
   if(s.anchor&&versions.every((v,i)=>v!==s.anchor.versions[i])){
    // Cheaper first bounds the principal lost if the price-capped hedge fails.
    const first=books[units[0]<=units[1]?0:1].exchangeId,second=ids(b).find(id=>id!==first);
    s.package={id:s.nextPackageId++,first,second,stage:'entry_first',createdAt:at,completedPair:false,buyCost:'0.000000',saleProceeds:'0.000000',
     entryLimits:Object.fromEntries(books.map((x,i)=>[x.exchangeId,money(units[i])])),entryVersions:versions};
    pending(s,b,first,'buy','entry_first',amount(s.package.entryLimits[first]),at);s.anchor=null;inc(s,'paired_entry_obligation');
   }else if(!s.anchor)s.anchor={versions,at};
  }else{s.anchor=null;inc(s,'no_package_capacity');}
 }else if(!p)s.anchor=null;
 assertBaalPair(s,b);return s;
}
export function baalPairReport(s,b,at){
 assertBaalPair(s,b);let liquidation=0n,complete=true;
 const positions=Object.values(s.positions).map(p=>{const book=s.books[p.exchangeId],freshMark=book&&instant(at)>=instant(book.receivedAt)&&instant(at)-instant(book.requestStartedAt)<=b.policy.maxAgeMs&&depth(book,'sell',b)>=1&&sellUnit(book)>=TICK;
  const value=freshMark?money(sellUnit(book)):null;if(value===null)complete=false;else liquidation+=amount(value);return{...p,estimatedLiquidationProceeds:value,unsoldShares:value===null?1:0,
   quoteReceivedAt:book?.receivedAt??null,quoteRequestStartedAt:book?.requestStartedAt??null,quoteAgeMs:book?instant(at)-instant(book.requestStartedAt):null};});
 const paired=positions.length===2&&s.package?.completedPair,reviewCurrent=currentReview(b,at);
 return{schemaVersion:1,mode:b.mode,cohort:b.name,bankHash:s.bankHash,cutoff:b.cutoff,updatedAt:at,reviewExpiresAt:b.review.expiresAt,
  cash:s.cash,realizedPnl:s.realizedPnl,estimatedLiquidationPnl:complete?money(amount(s.cash)+liquidation-amount(b.policy.initialCash)):null,
  positions,pending:s.pending,package:s.package,reserved:money(reserve(s)),cumulativeBuys:s.cumulativeBuys,counts:s.counts,
  fills:s.fills.length,recentFills:s.fills.slice(-10),closedPackages:s.roundtrips.filter(x=>amount(x.buyCost)>0n).length,recentRoundtrips:s.roundtrips.slice(-10),fees:'0.000000',
  ordinaryConditionalSettlementSurplus:paired&&reviewCurrent?money(SCALE-cost(s)):null,zeroPayoutExceptionLoss:money(-cost(s)),reviewCurrent,
  executionAuthority:false,competitionOrders:0,networkRequests:0,profitabilityEstablished:false,
  limitations:['Two sequential hypothetical fills are not atomic execution or evidence of queue priority.','Ordinary settlement surplus is conditional on reviewed rules and is never booked as realized or liquidation P&L.','A failed hedge or second exit creates an orphan obligation that may realize a loss.','Matched inventory remains bounded by the loss and six-hour holding limits; all costs and losses stay in the original journal.']};
}
