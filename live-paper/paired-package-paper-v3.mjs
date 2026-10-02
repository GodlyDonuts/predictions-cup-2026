import {createHash} from 'node:crypto';
import {amount,money,price,SCALE,TICK,instant} from '../paper/fixed.mjs';
export const pairedPaperHash=x=>createHash('sha256').update(JSON.stringify(x)).digest('hex');
export const PAIRED_PAPER_POLICY=Object.freeze({latencyMs:2000,entryLifetimeMs:30000,maxQuoteAgeMs:20000,maxPairSkewMs:3000,maxConfirmationAgeMs:300000,depthFractionBps:100,maxShares:6,maxPackageCost:'6.000000',maxOrphanPrincipal:'5.000000',maxExposure:'25.000000',maxCumulativeBuys:'100.000000',minimumUnitMargin:'0.010000',cooldownMs:60000,pairedTakeProfitPerShare:'0.010000',feesBps:0});
export const PAIRED_PAPER_VARIANTS=Object.freeze(['sequential_dual_no']);
export const PAIRED_PAPER_CONFIRMATION=Object.freeze({independentPairedObservations:2});
const need=(x,m)=>{if(!x)throw Error(m);};
const legs=['381','382'];
function bankCheck(b){need(b&&b.markets.length===2&&legs.every(id=>b.markets.some(m=>m.marketId===id))&&pairedPaperHash(b.policy)===pairedPaperHash(PAIRED_PAPER_POLICY),'PAIRED_BANK_INVALID');instant(b.cutoff);}
export function createPairedPaper(bank){bankCheck(bank);return {bankHash:pairedPaperHash(bank),lastEventId:0,lastAvailableAt:null,books:{},anchor:null,lastEntryAt:null,nextIntentId:1,nextPackageId:1,pending:null,active:null,packages:[],roundTrips:[],positions:{},fills:[],outcomes:[],cash:'100000.000000',realizedPnl:'0.000000',cumulativeBuyCost:'0.000000',counts:{books:0,signals:0,firstLegUnfilled:0,secondLegUnfilled:0,orphanExits:0,completedPairs:0,closeSignals:0,completedRoundTrips:0,firstCloseUnfilled:0}};}
export function warmPairedPaper(s){return s;}
function intent(s,{marketId,quantity,limit,action,stage},at,b){const book=s.books[marketId];return {id:s.nextIntentId++,packageId:s.active.id,marketId,exchangeId:b.markets.find(m=>m.marketId===marketId).exchangeId,side:'no',action,stage,quantity,remaining:quantity,limit,createdAt:at,expiresAt:new Date(instant(at)+b.policy.entryLifetimeMs).toISOString(),decisionVersion:book?.versionKey,observedVersions:[],filledQuantity:0};}
function ownedCost(s){return Object.values(s.positions).reduce((n,p)=>n+amount(p.cost),0n);}
function completeFirst(s,at,b){const p=s.active,filled=p.fills[p.first];if(!filled){s.counts.firstLegUnfilled++;s.outcomes.push({packageId:p.id,status:'first_leg_unfilled',at});s.active=null;return;}s.pending=intent(s,{marketId:p.second,quantity:filled,limit:p.limits[p.second],action:'buy',stage:'second'},at,b);p.stage='second';}
function returnHolding(s,at){
 const p=s.active;need(p.fills[legs[0]]===p.fills[legs[1]],'UNMATCHED_HOLDING');p.pairedQuantity=p.fills[legs[0]];
 p.stage='holding';p.closeCycle=null;p.unresolvedOrphan=0;
 if(p.pairedQuantity)s.packages.push(p);
 else{need(legs.every(id=>amount(p.costs[id])===0n)&&amount(p.realizedPnl)===amount(p.saleProceeds)-amount(p.acquiredPrincipal),'PACKAGE_ROUNDTRIP_ACCOUNTING');
  s.roundTrips.push({packageId:p.id,openedAt:p.createdAt,closedAt:at,acquiredPrincipal:p.acquiredPrincipal,saleProceeds:p.saleProceeds,realizedPnl:p.realizedPnl});s.counts.completedRoundTrips++;}
 s.active=null;
}
function finishPackage(s,at,b,{entry=true}={}){
 const p=s.active,paired=Math.min(...legs.map(id=>p.fills[id])),orphans=legs.filter(id=>p.fills[id]>paired);
 p.pairedQuantity=paired;p.completedAt=at;if(entry&&paired)s.counts.completedPairs++;
 if(orphans.length){s.counts.secondLegUnfilled++;const id=orphans[0],bid=s.books[id]?.asks[0];p.stage='orphan_exit';
  s.pending=intent(s,{marketId:id,quantity:p.fills[id]-paired,limit:bid?money(SCALE-price(bid.price)-TICK):'0.005000',action:'sell',stage:'orphan_exit'},at,b);
  if(amount(s.pending.limit)<TICK)s.pending.limit='0.005000';
 }else returnHolding(s,at);
}
function terminal(s,at,b){
 const i=s.pending;s.pending=null;
 if(i.stage==='first')completeFirst(s,at,b);
 else if(i.stage==='second')finishPackage(s,at,b);
 else if(i.stage==='close_first'){
  if(!i.filledQuantity){s.counts.firstCloseUnfilled++;returnHolding(s,at);}
  else{const p=s.active;p.stage='close_second';s.pending=intent(s,{marketId:p.closeCycle.second,quantity:i.filledQuantity,limit:p.closeCycle.limits[p.closeCycle.second],action:'sell',stage:'close_second'},at,b);}
 }else if(i.stage==='close_second')finishPackage(s,at,b,{entry:false});
 else if(i.remaining){s.active.stage='orphan_exit_blocked';s.active.unresolvedOrphan=i.remaining;s.outcomes.push({packageId:s.active.id,status:'orphan_exit_censored',quantity:i.remaining,at});}
 else{s.counts.orphanExits++;returnHolding(s,at);}
}
function fill(s,event,at,b){const i=s.pending,book=event.book;if(!i||book.marketId!==i.marketId||book.versionKey===i.decisionVersion||instant(book.requestStartedAt)<instant(i.createdAt)+b.policy.latencyMs||instant(book.receivedAt)>instant(i.expiresAt))return;
 if(i.observedVersions.includes(book.versionKey))return;i.observedVersions.push(book.versionKey);
 const side=i.action==='buy'?book.bids:book.asks;let remaining=i.remaining,cost=0n,quantity=0;
 for(const level of side){const effective=i.action==='buy'?SCALE-price(level.price)+TICK:SCALE-price(level.price)-TICK;if(effective<TICK||effective>SCALE-TICK)continue;if(i.action==='buy'?effective>price(i.limit):effective<price(i.limit))break;const cap=Number(BigInt(level.quantity)*BigInt(b.policy.depthFractionBps)/10000n),n=Math.min(remaining,cap);if(n){cost+=BigInt(n)*effective;quantity+=n;remaining-=n;}if(!remaining)break;}
 if(!quantity)return;
 const p=s.positions[i.marketId]||{marketId:i.marketId,quantity:0,cost:'0.000000'};
 if(i.action==='buy'){p.quantity+=quantity;p.cost=money(amount(p.cost)+cost);s.cash=money(amount(s.cash)-cost);s.cumulativeBuyCost=money(amount(s.cumulativeBuyCost)+cost);s.active.fills[i.marketId]+=quantity;s.active.costs[i.marketId]=money(amount(s.active.costs[i.marketId])+cost);s.active.acquiredPrincipal=money(amount(s.active.acquiredPrincipal)+cost);}else{need(p.quantity>=quantity,'PAIRED_OVERSELL');need(s.active.fills[i.marketId]>=quantity,'PACKAGE_LOT_OVERSELL');const basis=amount(s.active.costs[i.marketId])*BigInt(quantity)/BigInt(s.active.fills[i.marketId]);p.quantity-=quantity;p.cost=money(amount(p.cost)-basis);s.cash=money(amount(s.cash)+cost);s.realizedPnl=money(amount(s.realizedPnl)+cost-basis);s.active.fills[i.marketId]-=quantity;s.active.costs[i.marketId]=money(amount(s.active.costs[i.marketId])-basis);s.active.realizedPnl=money(amount(s.active.realizedPnl)+cost-basis);s.active.saleProceeds=money(amount(s.active.saleProceeds)+cost);}
 s.positions[i.marketId]=p;i.remaining=remaining;i.filledQuantity+=quantity;s.fills.push({intentId:i.id,packageId:i.packageId,marketId:i.marketId,action:i.action,side:'no',quantity,principal:money(cost),sourceVersion:book.versionKey,receivedAt:book.receivedAt,availableAt:at,model:'hypothetical_next_independent_book'});
 if(!remaining)terminal(s,at,b);
}
function eligibleBooks(s,at,b){const now=instant(at),books=legs.map(id=>s.books[id]);return books.every(x=>x&&x.bids.length&&x.asks.length&&instant(x.receivedAt)<=now&&now-instant(x.receivedAt)<=b.policy.maxQuoteAgeMs)&&Math.abs(instant(books[0].receivedAt)-instant(books[1].receivedAt))<=b.policy.maxPairSkewMs?books:null;}
function preparePairedClose(s,books,at,b){
 if(!books||s.pending||s.active)return;
 for(let index=0;index<s.packages.length;index++){
  const p=s.packages[index],q=p.pairedQuantity;need(q>0&&legs.every(id=>p.fills[id]===q),'HOLDING_PACKAGE_QUANTITY');
  const n=Math.min(q,...books.map(x=>Number(BigInt(x.asks[0].quantity)*BigInt(b.policy.depthFractionBps)/10000n)));
  if(!n)continue;const units=books.map(x=>SCALE-price(x.asks[0].price)-TICK);
  if(units.some(x=>x<TICK||x>SCALE-TICK))continue;
  const lotCost=legs.reduce((sum,id)=>sum+amount(p.costs[id]),0n),basisCeiling=(lotCost*BigInt(n)+BigInt(q)-1n)/BigInt(q),proceeds=BigInt(n)*(units[0]+units[1]);
  if(proceeds<basisCeiling+BigInt(n)*amount(b.policy.pairedTakeProfitPerShare))continue;
  const first=amount(p.costs[legs[0]])>=amount(p.costs[legs[1]])?legs[0]:legs[1],second=legs.find(id=>id!==first);
  s.packages.splice(index,1);s.active=p;p.stage='close_first';
  p.closeCycle={quantity:n,first,second,limits:Object.fromEntries(legs.map((id,i)=>[id,money(units[i])])),preparedAt:at,quotedProceeds:money(proceeds),basisCeiling:money(basisCeiling),minimumQuotedGain:money(proceeds-basisCeiling)};
  s.pending=intent(s,{marketId:first,quantity:n,limit:p.closeCycle.limits[first],action:'sell',stage:'close_first'},at,b);s.counts.closeSignals++;return;
 }
}
export function advancePairedPaper(input,events,bank,{availableAt,allowDecisions=true}={}){
 bankCheck(bank);need(input.bankHash===pairedPaperHash(bank),'PAIRED_BANK_CHANGED');const at=instant(availableAt);need(!input.lastAvailableAt||at>=instant(input.lastAvailableAt),'PAIRED_CLOCK_ROLLBACK');const s=structuredClone(input);
 for(const e of events){need(e.eventId===s.lastEventId+1&&e.tournamentId===bank.tournamentId&&instant(e.confirmedAt)<=at,'PAIRED_EVENT_ORDER');s.lastEventId=e.eventId;if(e.kind==='invalidate'){delete s.books[e.marketId];s.anchor=null;continue;}if(!legs.includes(e.marketId))continue;need(e.book.exchangeId===bank.markets.find(m=>m.marketId===e.marketId).exchangeId,'PAIRED_EXCHANGE_MISMATCH');s.books[e.marketId]=e.book;s.counts.books++;fill(s,e,availableAt,bank);}
 if(s.pending&&at>instant(s.pending.expiresAt)){s.outcomes.push({intentId:s.pending.id,packageId:s.pending.packageId,status:'expired',remaining:s.pending.remaining,at:availableAt});terminal(s,availableAt,bank);}
 const books=eligibleBooks(s,availableAt,bank);
 if(allowDecisions&&!s.pending&&s.active?.stage==='orphan_exit_blocked'){
  const p=s.active,id=legs.find(id=>p.fills[id]>p.pairedQuantity),book=s.books[id];
  if(book&&events.some(e=>e.kind==='book'&&e.marketId===id)&&book.asks.length&&at-instant(book.receivedAt)<=bank.policy.maxQuoteAgeMs){const unit=SCALE-price(book.asks[0].price)-TICK;if(unit>=TICK){p.stage='orphan_exit';s.pending=intent(s,{marketId:id,quantity:p.fills[id]-p.pairedQuantity,limit:money(unit),action:'sell',stage:'orphan_exit'},availableAt,bank);}}
 }
 if(s.anchor&&at-instant(s.anchor.at)>bank.policy.maxConfirmationAgeMs)s.anchor=null;
 if(allowDecisions)preparePairedClose(s,books,availableAt,bank);
 if(allowDecisions&&!s.pending&&!s.active&&at>instant(bank.cutoff)&&books&&(!s.lastEntryAt||at-instant(s.lastEntryAt)>=bank.policy.cooldownMs)){
  const limits=books.map(x=>SCALE-price(x.bids[0].price)+TICK),sum=limits[0]+limits[1],versions=books.map(x=>x.versionKey);
  if(SCALE-sum>=amount(bank.policy.minimumUnitMargin)&&limits.every(x=>x>=TICK&&x<=SCALE-TICK)){
   if(s.anchor&&versions.every((v,i)=>v!==s.anchor.versions[i])){
    let q=Math.min(bank.policy.maxShares,...books.map(x=>Number(BigInt(x.bids[0].quantity)*BigInt(bank.policy.depthFractionBps)/10000n)));const exposure=ownedCost(s),used=amount(s.cumulativeBuyCost);
    while(q>0&&(BigInt(q)*sum>amount(bank.policy.maxPackageCost)||exposure+BigInt(q)*sum>amount(bank.policy.maxExposure)||used+BigInt(q)*sum>amount(bank.policy.maxCumulativeBuys)||limits.some(x=>BigInt(q)*x>amount(bank.policy.maxOrphanPrincipal))))q--;
    if(q){const first=limits[0]>=limits[1]?legs[0]:legs[1],second=legs.find(x=>x!==first);s.active={id:s.nextPackageId++,first,second,stage:'first',quantity:q,limits:Object.fromEntries(legs.map((id,i)=>[id,money(limits[i])])),fills:Object.fromEntries(legs.map(id=>[id,0])),costs:Object.fromEntries(legs.map(id=>[id,'0.000000'])),createdAt:availableAt,versions,realizedPnl:'0.000000',saleProceeds:'0.000000',acquiredPrincipal:'0.000000',conditionalNormalSettlementOnly:true};s.pending=intent(s,{marketId:first,quantity:q,limit:s.active.limits[first],action:'buy',stage:'first'},availableAt,bank);s.counts.signals++;s.lastEntryAt=availableAt;s.anchor=null;}
   }else if(!s.anchor)s.anchor={versions,at:availableAt};
  }else s.anchor=null;
 }
 s.lastAvailableAt=availableAt;need(amount(s.cash)+ownedCost(s)===100000000000n+amount(s.realizedPnl),'PAIRED_CASH_BASIS_IDENTITY');
 const lots=[...s.packages,...(s.active?[s.active]:[])];for(const id of legs){need(lots.reduce((q,p)=>q+p.fills[id],0)===(s.positions[id]?.quantity||0)&&lots.reduce((cost,p)=>cost+amount(p.costs[id]),0n)===amount(s.positions[id]?.cost||'0.000000'),'PACKAGE_LOTS_POSITION_IDENTITY');}
 return s;
}
export function pairedPaperReport(s,b,at){const fresh=eligibleBooks(s,at,b);let liquidation=0n,complete=true;const positions=Object.values(s.positions).filter(p=>p.quantity>0).map(p=>{const book=s.books[p.marketId];let remain=p.quantity,value=0n;if(fresh)for(const l of book.asks){const n=Math.min(remain,Number(BigInt(l.quantity)*BigInt(b.policy.depthFractionBps)/10000n)),unit=SCALE-price(l.price)-TICK;if(unit>=TICK){value+=BigInt(n)*unit;remain-=n;}if(!remain)break;}const ready=!remain;complete&&=ready;liquidation+=value;return {...p,markReady:ready,liquidationValue:ready?money(value):null,uncoveredQuantity:remain};});const q=Math.min(...legs.map(id=>s.positions[id]?.quantity||0)),conditional=BigInt(q)*SCALE-ownedCost(s);return {schemaVersion:1,mode:'prospective_sequential_dual_no_paper',cutoff:b.cutoff,bankHash:s.bankHash,reportedAt:at,lastEventId:s.lastEventId,lastAvailableAt:s.lastAvailableAt,wallets:{sequential_dual_no:{cash:s.cash,realizedPnl:s.realizedPnl,totalPnl:complete?money(amount(s.cash)+liquidation-100000000000n):null,liquidationPnl:complete?money(amount(s.cash)+liquidation-100000000000n):null,marksComplete:complete,positions,conditionalNormalSettlementPnl:money(conditional),conditionalNormalPairedShares:q,pending:s.pending,activePackage:s.active,completedRoundTrips:s.roundTrips.length,recentRoundTrips:s.roundTrips.slice(-20),counts:s.counts,cumulativeBuyCost:s.cumulativeBuyCost,fees:'0.000000',fills:s.fills.length,recentFills:s.fills.slice(-20),recentOutcomes:s.outcomes.slice(-20)}},profitabilityEstablished:false,competitionOrders:0,executionAuthority:false,assumptions:['All fills are local counterfactuals with independent subsequent request,2s latency,1% depth and one adverse tick per executed side.','Expensive leg is filled first; second leg is sized to actual first-leg fills. A missing second leg creates bounded orphan exposure.','Matched shares may close sequentially when fresh padded sale depth covers their own acquired basis plus the fixed target; second-leg failure can lose money. Conditional settlement value is not booked profit or refund/override protection.','No inference of resting queue fills, official NO REST price convention or continuous price paths.']};}
