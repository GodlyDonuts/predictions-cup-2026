import {amount,money,price,SCALE,TICK,fee} from '../paper/fixed.mjs';
import {instantNs,timestamp} from '../feed/contract.mjs';
import {payloadHash} from '../execution/ledger.mjs';

export const DEFAULT_POLICY=Object.freeze({initialCash:'1000.000000',maximumCommitted:'40.000000',
  maximumMarketCost:'10.000000',quantity:5,maximumMarkets:4,latencyMs:3000,lifetimeMs:120000,
  bookAgeMs:10000,maximumSourceSilenceMs:60000,minimumSpreadTicks:1,minimumEdge:'0.001000',participationBps:100,
  feeBps:0,feePerShare:'0.000000',quoteMode:'pairedSpread',entriesEnabled:true,fillModel:'strictThrough',touchAttributionBps:10000,insideTicks:0,renewBeforeMs:0,minimumQuoteAgeMs:2500,writeGapMs:2500,writesPerMinute:24});
const ns=x=>instantNs(x), ms=x=>Number(ns(x)/1000000n), iso=x=>new Date(x).toISOString();
const check=(ok,code)=>{if(!ok)throw Error('MOLOCH_'+code);};
const position=(s,ex)=>s.positions[ex]??{YES:[],NO:[]};
const qty=lots=>lots.reduce((n,l)=>n+l.quantity,0);
const basis=lots=>lots.reduce((n,l)=>n+amount(l.price)*BigInt(l.quantity),0n);
const remaining=q=>q.quantity-q.filled;
const end=q=>Math.min(q.expiresAt,q.cancelEffectiveAt??Infinity);
const unresolved=s=>s.orders.filter(q=>!q.retired);
// Reserve rounding for one-share partial fills, not an optimistic single fill.
const buyCost=(s,limit,n)=>(amount(limit)+fee(amount(limit),1,s.policy))*BigInt(n);
const reserve=s=>unresolved(s).filter(q=>q.action==='buy').reduce((n,q)=>n+buyCost(s,q.price,remaining(q)),0n);
const owned=s=>Object.values(s.positions).reduce((n,p)=>n+basis(p.YES)+basis(p.NO),0n);
const pledged=(s,ex,side)=>unresolved(s).filter(q=>q.exchangeId===ex&&q.side===side&&q.action==='sell').reduce((n,q)=>n+remaining(q),0);
const oppositeBuys=(s,ex,side)=>unresolved(s).filter(q=>q.exchangeId===ex&&q.side!==side&&q.action==='buy').reduce((n,q)=>n+remaining(q),0);
function take(lots,n){let cost=0n;for(const lot of lots){const k=Math.min(n,lot.quantity);cost+=BigInt(k)*amount(lot.price);lot.quantity-=k;n-=k;if(!n)break;}check(n===0,'UNCOVERED_SELL');return cost;}
function prune(s){for(const p of Object.values(s.positions))for(const side of ['YES','NO'])p[side]=p[side].filter(l=>l.quantity);}

export function createMoloch(policy={}){
  const p={...DEFAULT_POLICY,...policy};
  check(['pairedSpread','microprice','cheapSpread'].includes(p.quoteMode),'QUOTE_MODE');
  check(p.minimumDepthImbalanceBps===undefined||p.quoteMode==='microprice'&&Number.isSafeInteger(p.minimumDepthImbalanceBps)&&p.minimumDepthImbalanceBps>0&&p.minimumDepthImbalanceBps<=10000,'DEPTH_IMBALANCE_POLICY');
  check(typeof p.entriesEnabled==='boolean','ENTRIES_ENABLED');
  check(amount(p.maximumEntryPrice??'0.100000')>0n&&amount(p.maximumEntryPrice??'0.100000')<=SCALE/2n,'CHEAP_PRICE_CAP');
  check(typeof(p.throughFillCapacity??false)==='boolean'&&(!p.throughFillCapacity||p.fillModel==='strictThrough'),'THROUGH_CAPACITY');
  check(p.inventoryTakerAfterMs===undefined||Number.isSafeInteger(p.inventoryTakerAfterMs)&&p.inventoryTakerAfterMs>=0&&p.inventoryTakerAfterMs<=86400000,'INVENTORY_TAKER_AGE');
  check(p.maximumInventoryAgeMs===undefined||Number.isSafeInteger(p.maximumInventoryAgeMs)&&p.maximumInventoryAgeMs>=0&&p.maximumInventoryAgeMs<=86400000,'INVENTORY_MAX_AGE');
  check(p.inventoryAgeClock===undefined||['receipt','execution'].includes(p.inventoryAgeClock),'INVENTORY_AGE_CLOCK');
  check(typeof(p.retainProfitableHedge??false)==='boolean','RETAIN_HEDGE');
  check(typeof(p.repriceProfitableHedge??false)==='boolean'&&(!p.repriceProfitableHedge||p.retainProfitableHedge===true),'REPRICE_HEDGE');
  check(p.entryExchangeIds==null||Array.isArray(p.entryExchangeIds)&&p.entryExchangeIds.length<=p.maximumMarkets&&new Set(p.entryExchangeIds).size===p.entryExchangeIds.length&&p.entryExchangeIds.every(id=>typeof id==='string'&&/^[1-9]\d*$/.test(id)),'ENTRY_EXCHANGES');
  check(Number.isSafeInteger(p.maximumHedgeOvershoot??0)&&(p.maximumHedgeOvershoot??0)>=0&&(p.maximumHedgeOvershoot??0)<=p.quantity,'HEDGE_OVERSHOOT');
  check(['strictThrough','conditionalFifo'].includes(p.fillModel)&&Number.isSafeInteger(p.touchAttributionBps)&&p.touchAttributionBps>=0&&p.touchAttributionBps<=10000,'FILL_MODEL');
  check(Number.isSafeInteger(p.insideTicks)&&p.insideTicks>=0&&p.insideTicks<=3,'INSIDE_TICKS');
  check(Number.isSafeInteger(p.renewBeforeMs)&&p.renewBeforeMs>=0&&p.renewBeforeMs<p.lifetimeMs-p.latencyMs-p.minimumQuoteAgeMs,'RENEWAL_WINDOW');
  for(const k of ['quantity','maximumMarkets','latencyMs','lifetimeMs','bookAgeMs','maximumSourceSilenceMs','minimumSpreadTicks','participationBps','minimumQuoteAgeMs','writeGapMs','writesPerMinute'])check(Number.isSafeInteger(p[k])&&p[k]>0,'POLICY_'+k);
  check(p.quantity<=2147483647&&p.participationBps<=10000&&p.writesPerMinute<=24&&p.writeGapMs>=2500&&p.lifetimeMs>p.latencyMs+p.minimumQuoteAgeMs,'POLICY_LIMITS');
  check(p.entryDepthBps===undefined||Number.isSafeInteger(p.entryDepthBps)&&p.entryDepthBps>0&&p.entryDepthBps<=10000,'ENTRY_DEPTH_LIMIT');
  check(Number.isSafeInteger(p.feeBps)&&p.feeBps>=0&&amount(p.feePerShare)>=0n&&amount(p.minimumEdge)>0n&&amount(p.initialCash)>0n&&amount(p.maximumCommitted)>0n&&amount(p.maximumMarketCost)>0n,'POLICY_MONEY');
  return{schemaVersion:1,bot:'Moloch',policy:p,policyHash:payloadHash(p),cash:p.initialCash,realized:'0.000000',fees:'0.000000',
    positions:{},books:{},priceHistory:{},orders:[],fills:[],redemptions:[],seen:{},flow:{},writes:[],events:0,lastAt:null,nextOrderId:1,counts:{}};
}
const count=(s,k)=>s.counts[k]=(s.counts[k]??0)+1;
function visibleQueue(q,b){const limit=q.side==='YES'?amount(q.price):SCALE-amount(q.price),bid=(q.side==='YES')===(q.action==='buy');let same=0n,better=0n;for(const l of bid?b.bids:b.asks){const p=price(l.price);if(p===limit)same+=BigInt(l.quantity);else if(bid?p>limit:p<limit)better+=BigInt(l.quantity);}return{same,better};}
function fresh(s,b,at){return b&&!b.invalid&&at-ms(b.transport==='official_sig_realtime'?b.sourceAt:b.requestStartedAt)<=s.policy.bookAgeMs&&ms(b.receivedAt)<=at;}
function validateBook(b){
  check(typeof b.tournamentId==='string'&&b.tournamentId&&/^[1-9]\d*$/.test(b.marketId)&&/^[1-9]\d*$/.test(b.exchangeId),'BOOK_SCOPE');
  check(/^\d+$/.test(String(b.sequence)),'BOOK_SEQUENCE');
  check(ns(b.requestStartedAt)<=ns(b.receivedAt)&&ns(b.sourceAt)<=ns(b.receivedAt)+1000000000n,'BOOK_CLOCK');
  for(const [side,sign]of [['bids',-1],['asks',1]]){check(Array.isArray(b[side])&&b[side].length>0,'BOOK_DEPTH');let prev=null;for(const l of b[side]){const v=price(l.price);check(Number.isSafeInteger(l.quantity)&&l.quantity>0&&(prev===null||(sign===-1?v<prev:v>prev)),'BOOK_LEVEL');prev=v;}}
  check(price(b.bids[0].price)<price(b.asks[0].price),'BOOK_CROSSED');
}
function fill(s,q,n,at,sourceId,executedAt=at){
  const value=amount(q.price)*BigInt(n),costFee=fee(value,n,s.policy),p=s.positions[q.exchangeId]??={YES:[],NO:[]};
  let realized=0n;
  if(q.action==='sell'){const removed=take(p[q.side],n);s.cash=money(amount(s.cash)+value-costFee);realized=value-removed-costFee;}
  else{check(amount(s.cash)>=value+costFee,'CASH');s.cash=money(amount(s.cash)-value-costFee);p[q.side].push({quantity:n,price:q.price,...(s.policy.inventoryAgeClock==='execution'?{executedAt}:{})});realized=-costFee;}
  q.filled+=n;if(!remaining(q))q.retired=true;
  s.fees=money(amount(s.fees)+costFee);s.realized=money(amount(s.realized)+realized);
  s.fills.push({orderId:q.id,marketId:q.marketId,exchangeId:q.exchangeId,side:q.side,action:q.action,quantity:n,price:q.price,fee:money(costFee),at,sourceId,counterfactual:true,...(s.policy.inventoryAgeClock==='execution'?{executedAt}:{})});
  const pairs=Math.min(qty(p.YES),qty(p.NO));
  if(pairs){const costs=take(p.YES,pairs)+take(p.NO,pairs),payout=SCALE*BigInt(pairs);s.cash=money(amount(s.cash)+payout);s.realized=money(amount(s.realized)+payout-costs);s.redemptions.push({exchangeId:q.exchangeId,quantity:pairs,payout:money(payout),cost:money(costs),at,counterfactual:true,mechanism:'engine_opposite_buy_fifo_netting'});}
  prune(s);
}
const entryAllowed=(s,exchangeId)=>s.policy.entriesEnabled!==false&&(s.policy.entryExchangeIds==null||s.policy.entryExchangeIds.includes(exchangeId));
function retainableHedge(s,q){
  if(s.policy.retainProfitableHedge!==true||!entryAllowed(s,q.exchangeId)||q.action!=='buy'||remaining(q)<1||q.cancelEffectiveAt!==undefined)return false;
  const side=q.side==='YES'?'NO':'YES',lots=position(s,q.exchangeId)[side],held=qty(lots);
  if(!held||oppositeBuys(s,q.exchangeId,side)+pledged(s,q.exchangeId,side)>held+(s.policy.maximumHedgeOvershoot??0))return false;
  const worst=lots.reduce((highest,l)=>amount(l.price)>highest?amount(l.price):highest,0n),limit=amount(q.price);
  return SCALE-worst-limit-fee(worst,1,s.policy)-fee(limit,1,s.policy)>=amount(s.policy.minimumEdge);
}
function desired(s,at){
  const candidates=[];
  for(const b of Object.values(s.books)){
    if(!fresh(s,b,at))continue;
    const bestBid=price(b.bids[0].price),bestAsk=price(b.asks[0].price),spread=bestAsk-bestBid;
    let bid=bestBid,ask=bestAsk;
    const micro=(bestAsk*BigInt(b.bids[0].quantity)+bestBid*BigInt(b.asks[0].quantity))/BigInt(b.bids[0].quantity+b.asks[0].quantity);
    const p=position(s,b.exchangeId),yes=qty(p.YES),no=qty(p.NO);
    if(yes||no){if(!s.policy.repriceProfitableHedge)for(const q of unresolved(s).filter(q=>q.exchangeId===b.exchangeId&&end(q)>at&&retainableHedge(s,q)))candidates.push({marketId:b.marketId,exchangeId:b.exchangeId,side:q.side,action:'buy',quantity:remaining(q),price:q.price,rank:1e12,source:b});
      const side=yes?'YES':'NO',available=qty(p[side])-pledged(s,b.exchangeId,side)-oppositeBuys(s,b.exchangeId,side);
      // Retain current covered sale in desired, without spending its pledge twice.
      const existing=unresolved(s).filter(q=>q.exchangeId===b.exchangeId&&q.side===side&&q.action==='sell'&&end(q)>at).reduce((n,q)=>n+remaining(q),0);
      const quantity=Math.min(s.policy.quantity,available+existing);
      let exitImprove=TICK*BigInt(Math.min(s.policy.insideTicks??0,Math.max(0,Number(spread/TICK-1n))));
      const baseExit=side==='YES'?bestAsk:SCALE-bestBid,worstBasis=p[side].reduce((highest,lot)=>amount(lot.price)>highest?amount(lot.price):highest,0n);
      const lastBuy=s.policy.inventoryTakerAfterMs===undefined&&s.policy.maximumInventoryAgeMs===undefined?null:s.fills.filter(f=>f.exchangeId===b.exchangeId&&f.side===side&&f.action==='buy').at(-1),executable=side==='YES'?bestBid:SCALE-bestAsk;
      if(lastBuy&&s.policy.inventoryAgeClock==='execution')check(p[side].every(l=>typeof l.executedAt==='string'),'INVENTORY_EXECUTION_CLOCK_MISSING');
      const birth=!lastBuy?null:s.policy.inventoryAgeClock==='execution'?p[side].reduce((oldest,l)=>Math.min(oldest,ms(l.executedAt)),Infinity):ms(lastBuy.at);
      const maximumAge=birth!==null&&s.policy.maximumInventoryAgeMs!==undefined&&at-birth>=s.policy.maximumInventoryAgeMs;
      const profitableAge=birth!==null&&s.policy.inventoryTakerAfterMs!==undefined&&at-birth>=s.policy.inventoryTakerAfterMs&&executable-worstBasis-fee(worstBasis,1,s.policy)-fee(executable,1,s.policy)>=0n;
      if(quantity>0&&(maximumAge||profitableAge)){candidates.push({marketId:b.marketId,exchangeId:b.exchangeId,side,action:'sell',quantity,price:money(executable),executionStyle:'inventoryAON',...(maximumAge?{inventoryExitReason:'maximumAge'}:{}),rank:1e12,source:b});continue;}
      if(s.policy.repriceProfitableHedge&&!maximumAge&&!profitableAge&&entryAllowed(s,b.exchangeId)){
        const hedgeSide=side==='YES'?'NO':'YES',limit=side==='YES'?SCALE-bestAsk:bestBid,worst=p[side].reduce((n,l)=>amount(l.price)>n?amount(l.price):n,0n),existingHedge=unresolved(s).filter(q=>q.exchangeId===b.exchangeId&&q.side===hedgeSide&&q.action==='buy'&&end(q)>at).reduce((n,q)=>n+remaining(q),0),quantity=Math.min(s.policy.quantity,available+existingHedge);
        if(quantity>0&&SCALE-worst-limit-fee(worst,1,s.policy)-fee(limit,1,s.policy)>=amount(s.policy.minimumEdge)){candidates.push({marketId:b.marketId,exchangeId:b.exchangeId,side:hedgeSide,action:'buy',quantity,price:money(limit),hedgeForSide:side,rank:1e12,source:b});continue;}
      }
      while(exitImprove>0n&&baseExit-exitImprove-worstBasis-fee(worstBasis,1,s.policy)-fee(baseExit-exitImprove,1,s.policy)<amount(s.policy.minimumEdge))exitImprove-=TICK;
      if(quantity>0)candidates.push({marketId:b.marketId,exchangeId:b.exchangeId,side,action:'sell',quantity,price:money(side==='YES'?bestAsk-exitImprove:SCALE-bestBid-exitImprove),rank:1e12,source:b});
      continue;
    }
    if(!entryAllowed(s,b.exchangeId)||spread<TICK*BigInt(s.policy.minimumSpreadTicks))continue;
    for(let step=0;step<(s.policy.insideTicks??0);step++){
      const sides=micro-bid>=ask-micro?['YES','NO']:['NO','YES'];
      for(const side of sides){const nextBid=bid+(side==='YES'?TICK:0n),nextAsk=ask-(side==='NO'?TICK:0n),limit=side==='YES'?nextBid:SCALE-nextAsk,value=side==='YES'?micro:SCALE-micro;
        if(nextAsk-nextBid<TICK||nextAsk-nextBid-fee(nextBid,1,s.policy)-fee(SCALE-nextAsk,1,s.policy)<2n*amount(s.policy.minimumEdge)||value-limit-fee(limit,1,s.policy)<amount(s.policy.minimumEdge))continue;bid=nextBid;ask=nextAsk;
      }
    }
    const quotedSpread=ask-bid;
    if(s.policy.quoteMode==='pairedSpread'&&quotedSpread-fee(bid,1,s.policy)-fee(SCALE-ask,1,s.policy)<2n*amount(s.policy.minimumEdge))continue;
    const recent=(s.flow[b.exchangeId]??[]).filter(t=>at-t.at<=900000);s.flow[b.exchangeId]=recent;
    const activity=recent.reduce((n,t)=>n+t.quantity,0);
    const rank=Number(spread)*Math.log1p(activity+1)/(1+Number(bid));
    for(const [side,limit,value]of [['YES',bid,micro],['NO',SCALE-ask,SCALE-micro]]){
      if(s.policy.minimumDepthImbalanceBps!==undefined){const bids=b.bids.reduce((n,l)=>n+BigInt(l.quantity),0n),asks=b.asks.reduce((n,l)=>n+BigInt(l.quantity),0n),signed=(bids-asks)*(side==='YES'?1n:-1n);if(signed*10000n<BigInt(s.policy.minimumDepthImbalanceBps)*(bids+asks))continue;}
      if(s.policy.quoteMode==='cheapSpread'&&(side!==(bid<=SCALE-ask?'YES':'NO')||limit>amount(s.policy.maximumEntryPrice??'0.100000')||quotedSpread-fee(limit,1,s.policy)-fee(limit+quotedSpread,1,s.policy)<amount(s.policy.minimumEdge)))continue;
      if(s.policy.quoteMode==='microprice'&&value-limit-fee(limit,1,s.policy)<amount(s.policy.minimumEdge))continue;
      candidates.push({marketId:b.marketId,exchangeId:b.exchangeId,side,action:'buy',quantity:s.policy.quantity,price:money(limit),rank:s.policy.quoteMode==='cheapSpread'?Number(quotedSpread-fee(limit,1,s.policy)-fee(limit+quotedSpread,1,s.policy))/Number(2n*limit+quotedSpread)*Math.log1p(activity+1):rank,source:b});
    }
  }
  return candidates.sort((a,b)=>b.rank-a.rank||a.exchangeId.localeCompare(b.exchangeId)||a.side.localeCompare(b.side));
}
function reconcile(s,at){
  const targets=desired(s,at),key=q=>[q.exchangeId,q.side,q.action].join(':'),byKey=new Map(targets.map(q=>[key(q),q]));
  s.writes=s.writes.filter(t=>at-t<60000);
  if(s.writes.length>=s.policy.writesPerMinute||(s.writes.length&&at-s.writes.at(-1)<s.policy.writeGapMs)){count(s,'writeBudgetHolds');return;}
  const active=unresolved(s).filter(q=>end(q)>at&&(s.policy.retainProfitableHedge!==true||remaining(q)>0));
  for(const q of active){const target=byKey.get(key(q));if(q.cancelEffectiveAt!==undefined)continue;
    const b=s.books[q.exchangeId],p=position(s,q.exchangeId);
    // An entry deadline does not shorten an already-funded resting order's life.
    // Do not place replacements from stale data; retain only still-covered old
    // quotes until a changed/gapped source or the explicit silence timeout.
    const stillCovered=q.action==='sell'?qty(p[q.side])>=remaining(q):qty(p.YES)+qty(p.NO)===0||retainableHedge(s,q);
    if(!target&&b&&!b.invalid&&!fresh(s,b,at)&&at-ms(b.receivedAt)<s.policy.maximumSourceSilenceMs&&stillCovered&&(entryAllowed(s,q.exchangeId)||q.action==='sell'))continue;
    const renew=target&&target.price===q.price&&fresh(s,b,at)&&s.policy.renewBeforeMs>0&&q.expiresAt-at<=s.policy.renewBeforeMs;
    if((!target||target.price!==q.price||target.executionStyle!==q.executionStyle||renew)&&at-q.createdAt>=s.policy.minimumQuoteAgeMs){q.cancelEffectiveAt=at+s.policy.latencyMs;s.writes.push(at);count(s,'cancels');if(renew)count(s,'earlyRenewals');return;}}
  const batch=[],markets=new Set([...Object.entries(s.positions).filter(([,p])=>qty(p.YES)+qty(p.NO)>0).map(([ex])=>ex),...unresolved(s).map(q=>q.exchangeId)]);
  let available=amount(s.cash)-reserve(s),headroom=amount(s.policy.maximumCommitted)-owned(s)-reserve(s);
  const marketCosts=new Map();for(const [ex,p]of Object.entries(s.positions))marketCosts.set(ex,basis(p.YES)+basis(p.NO));
  for(const q of unresolved(s).filter(q=>q.action==='buy'))marketCosts.set(q.exchangeId,(marketCosts.get(q.exchangeId)??0n)+buyCost(s,q.price,remaining(q)));
  for(const t of targets){
    if(active.some(q=>key(q)===key(t))||batch.length>=8||t.executionStyle==='inventoryAON'&&batch.length)continue;
    if(!markets.has(t.exchangeId)&&markets.size>=s.policy.maximumMarkets)continue;
    const p=position(s,t.exchangeId);
    let n=t.quantity;
    if(t.action==='sell')n=Math.min(n,qty(p[t.side])-pledged(s,t.exchangeId,t.side)-oppositeBuys(s,t.exchangeId,t.side));
    else{
      if(t.hedgeForSide)n=Math.min(n,qty(p[t.hedgeForSide])-pledged(s,t.exchangeId,t.hedgeForSide)-oppositeBuys(s,t.exchangeId,t.hedgeForSide));
      const mate=batch.find(q=>q.exchangeId===t.exchangeId&&q.action==='buy'&&q.side!==t.side);
      if(mate)n=Math.min(n,mate.quantity);
      const opposite=s.policy.quoteMode==='pairedSpread'&&!mate&&!active.some(q=>q.exchangeId===t.exchangeId&&q.action==='buy'&&q.side!==t.side)?targets.find(q=>q.exchangeId===t.exchangeId&&q.action==='buy'&&q.side!==t.side):null;
      if(s.policy.entryDepthBps!==undefined){const depth=q=>BigInt(q.side==='YES'?q.source.bids[0].quantity:q.source.asks[0].quantity)*BigInt(s.policy.entryDepthBps)/10000n;n=Math.min(n,Number(depth(t)),...(opposite?[Number(depth(opposite))]:[]));}
      const unit=buyCost(s,t.price,1)+(opposite?buyCost(s,opposite.price,1):0n),marketRoom=amount(s.policy.maximumMarketCost)-(marketCosts.get(t.exchangeId)??0n);
      const room=[available,headroom,marketRoom].reduce((a,b)=>a<b?a:b);n=Math.min(n,room>0n?Number(room/unit):0);const cost=buyCost(s,t.price,n);
      if(n>0){available-=cost;headroom-=cost;marketCosts.set(t.exchangeId,(marketCosts.get(t.exchangeId)??0n)+cost);}
    }
    if(n<1)continue;
    // Never place crossing YES/NO acquisitions against our own quotes.
    if(t.action==='buy'&&[...active,...batch].some(q=>q.exchangeId===t.exchangeId&&q.action==='buy'&&q.side!==t.side&&amount(q.price)+amount(t.price)>=SCALE))continue;
    markets.add(t.exchangeId);batch.push({...t,quantity:n});if(t.executionStyle==='inventoryAON')break;
  }
  if(batch.length){s.writes.push(at);for(const t of batch){const queue=visibleQueue(t,t.source),fifo=s.policy.fillModel==='conditionalFifo'?{fillModel:'conditionalFifo',touchAttributionBps:s.policy.touchAttributionBps,queueAhead:String(queue.same+queue.better),queueSame:String(queue.same),queueBetter:String(queue.better),queueVisibleSame:String(queue.same),queueVisibleBetter:String(queue.better),touchRemainder:0}:{};s.orders.push({id:s.nextOrderId++,marketId:t.marketId,exchangeId:t.exchangeId,side:t.side,action:t.action,quantity:t.quantity,filled:0,price:t.price,createdAt:at,activationAt:at+s.policy.latencyMs,expiresAt:t.executionStyle==='inventoryAON'?at+s.policy.latencyMs+s.policy.bookAgeMs:at+s.policy.lifetimeMs,sourceHash:payloadHash(t.source),sourceSequence:String(t.source.sequence),tournamentId:t.source.tournamentId,retired:false,participationCredit:0,barrier:null,...(s.policy.throughFillCapacity?{throughFillCapacity:true}:{}),...(t.executionStyle?{executionStyle:t.executionStyle}:{}),...(t.inventoryExitReason?{inventoryExitReason:t.inventoryExitReason}:{}),...(s.policy.participationBps===100?{}:{participationBps:s.policy.participationBps}),...fifo});}count(s,'submissionBatches');}
}
export function advanceMoloch(s,event){
  check(payloadHash(s.policy)===s.policyHash,'POLICY_CHANGED');
  const atNs=ns(event.processedAt??event.receivedAt),at=Number(atNs/1000000n);check(atNs>=ns(event.receivedAt),'PROCESSING_PRECEDES_RECEIPT');check(s.lastAt===null||atNs>=ns(s.lastAt),'RECEIPT_ORDER');let duplicate=false;
  // Event validation is completed before mutation; the CLI journals only success.
  if(event.type==='book'){
    validateBook(event.book);check(event.book.receivedAt===event.receivedAt,'BOOK_RECEIPT');
    const old=s.books[event.book.exchangeId];if(old){check(old.marketId===event.book.marketId&&old.tournamentId===event.book.tournamentId,'SCOPE_CHANGED');check(BigInt(event.book.sequence)>BigInt(old.sequence)||(BigInt(event.book.sequence)===BigInt(old.sequence)&&ns(event.book.sourceAt)>=ns(old.sourceAt)),'BOOK_ROLLBACK');if(event.book.sequence===old.sequence&&event.book.sourceAt===old.sourceAt)check(payloadHash({bids:event.book.bids,asks:event.book.asks})===payloadHash({bids:old.bids,asks:old.asks}),'BOOK_CONFLICT');}
  }else if(event.type==='trade'){
    check(/^[1-9]\d*$/.test(event.marketId)&&/^[1-9]\d*$/.test(event.exchangeId)&&typeof event.tournamentId==='string'&&event.tournamentId&&typeof event.id==='string'&&event.id,'TRADE_SCOPE');
    check(Number.isSafeInteger(event.quantity)&&event.quantity>0&&event.quantity<=2147483647&&ns(event.executedAt)<=ns(event.receivedAt),'TRADE_CLOCK_OR_SIZE');price(event.yesPrice);
    const b=s.books[event.exchangeId];if(b)check(b.marketId===event.marketId&&b.tournamentId===event.tournamentId,'TRADE_SCOPE_CHANGED');
    const key=[event.tournamentId,event.exchangeId,event.id].join(':'),hash=payloadHash({marketId:event.marketId,executedAt:event.executedAt,yesPrice:event.yesPrice,quantity:event.quantity});
    if(s.seen[key]){
      // REST and realtime serialize the same execution instant with different
      // fractional precision. Accept only hashes for mathematically identical
      // instants and unchanged price/quantity, preserving old ledger hashes.
      const canonical=timestamp(event.executedAt),prefix=canonical.slice(0,19),fraction=canonical.slice(20,29);
      const equivalents=[hash];for(let digits=0;digits<=9;digits++){const time=prefix+(digits?'.'+fraction.slice(0,digits):'')+'Z';if(ns(time)===ns(event.executedAt))equivalents.push(payloadHash({marketId:event.marketId,executedAt:time,yesPrice:event.yesPrice,quantity:event.quantity}));}
      check(equivalents.includes(s.seen[key]),'TRADE_CONFLICT');duplicate=true;
    }else s.seen[key]=hash;
  }else if(event.type==='candle'){
    check(event.historicalReleaseClockAssumed===true&&ns(event.availableAt)===ns(event.receivedAt)&&ns(event.bucketStartedAt)<ns(event.availableAt)&&Number.isFinite(event.close)&&event.close>=0&&event.close<=1&&typeof event.exchangeId==='string','CANDLE');
  }else if(event.type==='coverage'){
    check(event.complete===true&&ns(event.from)<ns(event.to)&&ns(event.to)<=atNs&&/^\d+$/.test(String(event.projectedThroughSequence)),'COVERAGE');
  }else{check(event.type==='gap'||event.type==='tick','EVENT_TYPE');if(event.type==='gap'){const ids=event.exchangeIds??[event.exchangeId];check(Array.isArray(ids)&&ids.length>0&&ids.length<=1000&&new Set(ids).size===ids.length&&ids.every(id=>typeof id==='string'&&/^[1-9]\d*$/.test(id)),'GAP_SCOPE');}}
  s.lastAt=event.processedAt??event.receivedAt;s.events++;
  if(event.type==='book'){
    const b=structuredClone(event.book);s.books[b.exchangeId]=b;
    for(const q of unresolved(s).filter(q=>q.exchangeId===b.exchangeId&&q.executionStyle==='inventoryAON'&&fresh(s,b,at)&&ms(b.transport==='official_sig_realtime'?b.sourceAt:b.requestStartedAt)>=q.activationAt&&ms(b.transport==='official_sig_realtime'?b.sourceAt:b.requestStartedAt)<end(q))){
      const capacity=(q.side==='YES'?b.bids:b.asks).filter(l=>q.side==='YES'?amount(l.price)>=amount(q.price):SCALE-amount(l.price)>=amount(q.price)).reduce((n,l)=>n+BigInt(l.quantity),0n),key=[q.tournamentId,q.exchangeId,q.side,b.sequence].join(':'),consumed=BigInt(s.aonConsumed?.[key]??0),eligible=capacity>consumed?capacity-consumed:0n,n=eligible>=BigInt(remaining(q))?remaining(q):0;
      q.aonEvidence={bookHash:payloadHash(b),sourceAt:b.sourceAt,requestStartedAt:b.requestStartedAt,sequence:String(b.sequence),eligibleDepth:String(eligible),filledQuantity:n,priceLowerBound:q.price,priorConsumedDepth:String(consumed)};if(n){s.aonConsumed??={};s.aonConsumed[key]=String(consumed+BigInt(n));fill(s,q,n,s.lastAt,'book:'+b.exchangeId+':'+b.sequence+':'+q.id);s.fills.at(-1).executionStyle='inventoryAON';if(q.inventoryExitReason)s.fills.at(-1).inventoryExitReason=q.inventoryExitReason;}q.retired=true;count(s,'inventoryAonEvaluations');
    }
    for(const q of unresolved(s).filter(q=>q.exchangeId===b.exchangeId&&q.fillModel==='conditionalFifo')){const visible=visibleQueue(q,b),oldSame=BigInt(q.queueVisibleSame),oldBetter=BigInt(q.queueVisibleBetter);if(visible.same>oldSame&&ns(b.sourceAt)<=BigInt(q.activationAt)*1000000n)q.queueSame=String(BigInt(q.queueSame)+visible.same-oldSame);if(visible.better>oldBetter)q.queueBetter=String(BigInt(q.queueBetter)+visible.better-oldBetter);q.queueVisibleSame=String(visible.same);q.queueVisibleBetter=String(visible.better);q.queueAhead=String(BigInt(q.queueSame)+BigInt(q.queueBetter));}
    for(const q of unresolved(s).filter(q=>q.exchangeId===b.exchangeId&&q.marketId===b.marketId&&q.tournamentId===b.tournamentId&&ms(b.transport==='official_sig_realtime'?b.sourceAt:b.requestStartedAt)>=end(q)))if(!q.barrier)q.barrier={sequence:String(b.sequence),endAt:end(q)};
  }else if(event.type==='gap'){
    for(const id of event.exchangeIds??[event.exchangeId]){if(s.books[id])s.books[id].invalid=true;count(s,'gaps');}
  }else if(event.type==='trade'&&!duplicate){
    s.flow[event.exchangeId]??=[];s.flow[event.exchangeId].push({at:ms(event.executedAt),quantity:event.quantity});count(s,'uniqueTrades');
    const executed=ns(event.executedAt),tradePrice=price(event.yesPrice);
    for(const q of unresolved(s).filter(q=>q.marketId===event.marketId&&q.exchangeId===event.exchangeId&&q.tournamentId===event.tournamentId).sort((a,b)=>a.createdAt-b.createdAt||a.id-b.id)){
      if(q.executionStyle==='inventoryAON')continue;
      if(executed<BigInt(q.activationAt)*1000000n||executed>=BigInt(end(q))*1000000n)continue;
      const yesLimit=q.side==='YES'?amount(q.price):SCALE-amount(q.price),bid=(q.side==='YES')===(q.action==='buy');
      const through=bid?tradePrice<=yesLimit-TICK:tradePrice>=yesLimit+TICK,touch=q.fillModel==='conditionalFifo'&&tradePrice===yesLimit;
      if(!through&&!touch)continue;
      let eligibleVolume=event.quantity;
      if(through&&q.fillModel==='conditionalFifo'){q.queueAhead='0';q.queueSame='0';q.queueBetter='0';}
      if(touch){const numerator=event.quantity*q.touchAttributionBps+q.touchRemainder,attributed=Math.floor(numerator/10000);q.touchRemainder=numerator%10000;if(attributed)q.queueBetter='0';const ahead=BigInt(q.queueSame),consumed=BigInt(attributed)<ahead?BigInt(attributed):ahead;q.queueSame=String(ahead-consumed);q.queueAhead=String(BigInt(q.queueSame)+BigInt(q.queueBetter));eligibleVolume=attributed-Number(consumed);count(s,'conditionalTouches');if(!eligibleVolume)break;}
      if(through&&q.throughFillCapacity===true){const n=Math.min(event.quantity,remaining(q));if(n){fill(s,q,n,s.lastAt,event.id,event.executedAt);s.fills.at(-1).throughCapacityFill=true;count(s,'throughCapacityFills');break;}}
      const participationBps=q.participationBps??100,participation=Math.floor(event.quantity*participationBps/10000);q.participationCredit+=eligibleVolume*participationBps;
      // Fractional prints accumulate, but a print's total allocated shares is capped.
      const n=Math.min(event.quantity,remaining(q),Math.floor(q.participationCredit/10000),Math.max(participation,1));
      if(n>0){q.participationCredit-=n*10000;fill(s,q,n,s.lastAt,event.id,event.executedAt);if(touch){s.fills.at(-1).conditionalQueueFill=true;s.fills.at(-1).touchAttributionBps=q.touchAttributionBps;}break;}if(touch)break;
    }
  }else if(event.type==='candle'){
    s.priceHistory??={};const prior=s.priceHistory[event.exchangeId];s.priceHistory[event.exchangeId]={close:event.close,at:event.availableAt,observations:(prior?.observations??0)+1,historicalReleaseClockAssumed:true};count(s,'priceOnlyObservations');
  }else if(event.type==='coverage'){
    // Complete original quote window plus a post-end engine barrier, not wall time alone.
    for(const q of unresolved(s).filter(q=>q.marketId===event.marketId&&q.exchangeId===event.exchangeId&&q.tournamentId===event.tournamentId))if(q.barrier&&q.barrier.endAt===end(q)&&ns(event.from)<=BigInt(q.activationAt)*1000000n&&ns(event.to)>=BigInt(end(q))*1000000n&&BigInt(event.projectedThroughSequence)>=BigInt(q.barrier.sequence))q.retired=true;
  }
  if(duplicate)count(s,'duplicateTrades');reconcile(s,at);assertMoloch(s);return s;
}
export function assertMoloch(s){
  let cash=amount(s.policy.initialCash),realized=0n;
  for(const f of s.fills){const value=amount(f.price)*BigInt(f.quantity);cash+=(f.action==='buy'?-value:value)-amount(f.fee);}
  for(const r of s.redemptions)cash+=amount(r.payout);
  // Equity identity verifies exact cash/FIFO accounting after every event.
  realized=cash+owned(s)-amount(s.policy.initialCash);
  check(cash===amount(s.cash)&&realized===amount(s.realized),'LEDGER');
  check(cash>=reserve(s)&&owned(s)+reserve(s)<=amount(s.policy.maximumCommitted),'FUNDING');
  for(const [ex,p]of Object.entries(s.positions))for(const side of ['YES','NO'])check(qty(p[side])>=pledged(s,ex,side)+Math.min(qty(p[side]),oppositeBuys(s,ex,side)),'SHARE_PLEDGE');
  return true;
}
export function reportMoloch(s){
  const at=s.lastAt===null?null:ms(s.lastAt);let liquidation=0n,known=true;
  for(const [ex,p]of Object.entries(s.positions))for(const side of ['YES','NO']){
    let n=qty(p[side]);if(!n)continue;const b=s.books[ex];if(at===null||!fresh(s,b,at)){known=false;continue;}
    const levels=side==='YES'?b.bids:b.asks;for(const l of levels){const k=Math.min(n,l.quantity),v=(side==='YES'?price(l.price):SCALE-price(l.price))*BigInt(k);liquidation+=v-fee(v,k,s.policy);n-=k;if(!n)break;}if(n)known=false;
  }
  return{bot:'Moloch',at:s.lastAt,policyHash:s.policyHash,events:s.events,cash:s.cash,realized:s.realized,fees:s.fees,
    ownedCost:money(owned(s)),reservedCash:money(reserve(s)),netLiquidationPnl:known?money(amount(s.cash)+liquidation-amount(s.policy.initialCash)):null,
    positions:s.positions,priceHistory:s.priceHistory??{},fills:s.fills.length,orders:s.orders.length,activeOrders:at===null?0:unresolved(s).filter(q=>end(q)>at).length,
    unresolvedOrders:unresolved(s).length,fillModel:s.policy.fillModel??'strictThrough',conditionalQueueFills:s.fills.filter(f=>f.conditionalQueueFill).length,profitSignalEligible:s.policy.fillModel!=='conditionalFifo'&&!s.fills.some(f=>f.conditionalQueueFill||f.throughCapacityFill||f.executionStyle==='inventoryAON')&&unresolved(s).length===0&&known&&amount(s.realized)>0n&&amount(s.cash)+liquidation>amount(s.policy.initialCash),counts:s.counts,contestOrders:0,networkRequests:0,profitabilityEstablished:false,
    assumptions:['Independent simulated capital; no contest-account ownership.',s.policy.quoteMode==='pairedSpread'?'Flat entries quote both sides only when combined spread covers modeled fees and required edge; partial fills remain inventory risk.':'Microprice gates quotes; it is not calibrated alpha.',s.policy.fillModel==='conditionalFifo'?'Touch fills conditionally attribute observed volume to our side after visible queue exhaustion; aggressor direction and true FIFO priority are unverified.':'Strict one-tick trade-through and bounded participation are counterfactual, not verified queue fills.','Cancel acknowledgment does not release unresolved funding; complete window and post-end checkpoint are required.','Opposite BUY FIFO netting models the documented engine credit of one per netted share; all modeled fills remain counterfactual.',...(s.policy.inventoryTakerAfterMs===undefined?[]:['Aged inventory all-or-none simulations require later fresh depth after modeled latency; payout uses the limit-price lower bound and no capacity is reused at the same side/engine sequence.'])]};
}
