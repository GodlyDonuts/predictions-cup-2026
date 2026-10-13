import {amount,money,price,SCALE,TICK,fee} from '../paper/fixed.mjs';
import {instantNs} from '../feed/contract.mjs';
import {payloadHash} from '../execution/ledger.mjs';

export const DEFAULT_POLICY=Object.freeze({initialCash:'1000.000000',maximumCommitted:'40.000000',
  maximumMarketCost:'10.000000',quantity:5,maximumMarkets:4,latencyMs:3000,lifetimeMs:120000,
  bookAgeMs:10000,minimumSpreadTicks:1,minimumEdge:'0.001000',participationBps:100,
  feeBps:0,feePerShare:'0.000000',minimumQuoteAgeMs:2500,writeGapMs:2500,writesPerMinute:24});
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
  for(const k of ['quantity','maximumMarkets','latencyMs','lifetimeMs','bookAgeMs','minimumSpreadTicks','participationBps','minimumQuoteAgeMs','writeGapMs','writesPerMinute'])check(Number.isSafeInteger(p[k])&&p[k]>0,'POLICY_'+k);
  check(p.quantity<=100&&p.participationBps<=10000&&p.writesPerMinute<=24&&p.writeGapMs>=2500&&p.lifetimeMs>p.latencyMs+p.minimumQuoteAgeMs,'POLICY_LIMITS');
  check(Number.isSafeInteger(p.feeBps)&&p.feeBps>=0&&amount(p.feePerShare)>=0n&&amount(p.minimumEdge)>0n&&amount(p.initialCash)>0n&&amount(p.maximumCommitted)>0n&&amount(p.maximumMarketCost)>0n,'POLICY_MONEY');
  return{schemaVersion:1,bot:'Moloch',policy:p,policyHash:payloadHash(p),cash:p.initialCash,realized:'0.000000',fees:'0.000000',
    positions:{},books:{},orders:[],fills:[],redemptions:[],seen:{},flow:{},writes:[],events:0,lastAt:null,nextOrderId:1,counts:{}};
}
const count=(s,k)=>s.counts[k]=(s.counts[k]??0)+1;
function fresh(s,b,at){return b&&!b.invalid&&at-ms(b.requestStartedAt)<=s.policy.bookAgeMs&&ms(b.receivedAt)<=at;}
function validateBook(b){
  check(typeof b.tournamentId==='string'&&b.tournamentId&&/^[1-9]\d*$/.test(b.marketId)&&/^[1-9]\d*$/.test(b.exchangeId),'BOOK_SCOPE');
  check(/^\d+$/.test(String(b.sequence)),'BOOK_SEQUENCE');
  check(ns(b.requestStartedAt)<=ns(b.receivedAt)&&ns(b.sourceAt)<=ns(b.receivedAt)+1000000000n,'BOOK_CLOCK');
  for(const [side,sign]of [['bids',-1],['asks',1]]){check(Array.isArray(b[side])&&b[side].length>0,'BOOK_DEPTH');let prev=null;for(const l of b[side]){const v=price(l.price);check(Number.isSafeInteger(l.quantity)&&l.quantity>0&&(prev===null||(sign===-1?v<prev:v>prev)),'BOOK_LEVEL');prev=v;}}
  check(price(b.bids[0].price)<price(b.asks[0].price),'BOOK_CROSSED');
}
function fill(s,q,n,at,sourceId){
  const value=amount(q.price)*BigInt(n),costFee=fee(value,n,s.policy),p=s.positions[q.exchangeId]??={YES:[],NO:[]};
  let realized=0n;
  if(q.action==='sell'){const removed=take(p[q.side],n);s.cash=money(amount(s.cash)+value-costFee);realized=value-removed-costFee;}
  else{check(amount(s.cash)>=value+costFee,'CASH');s.cash=money(amount(s.cash)-value-costFee);p[q.side].push({quantity:n,price:q.price});realized=-costFee;}
  q.filled+=n;if(!remaining(q))q.retired=true;
  s.fees=money(amount(s.fees)+costFee);s.realized=money(amount(s.realized)+realized);
  s.fills.push({orderId:q.id,marketId:q.marketId,exchangeId:q.exchangeId,side:q.side,action:q.action,quantity:n,price:q.price,fee:money(costFee),at,sourceId,counterfactual:true});
  const pairs=Math.min(qty(p.YES),qty(p.NO));
  if(pairs){const costs=take(p.YES,pairs)+take(p.NO,pairs),payout=SCALE*BigInt(pairs);s.cash=money(amount(s.cash)+payout);s.realized=money(amount(s.realized)+payout-costs);s.redemptions.push({exchangeId:q.exchangeId,quantity:pairs,payout:money(payout),cost:money(costs),at,conditionalPaperPayout:true});}
  prune(s);
}
function desired(s,at){
  const candidates=[];
  for(const b of Object.values(s.books)){
    if(!fresh(s,b,at))continue;
    const bid=price(b.bids[0].price),ask=price(b.asks[0].price),spread=ask-bid;
    const p=position(s,b.exchangeId),yes=qty(p.YES),no=qty(p.NO);
    if(yes||no){const side=yes?'YES':'NO',available=qty(p[side])-pledged(s,b.exchangeId,side)-oppositeBuys(s,b.exchangeId,side);
      // Retain current covered sale in desired, without spending its pledge twice.
      const existing=unresolved(s).filter(q=>q.exchangeId===b.exchangeId&&q.side===side&&q.action==='sell'&&end(q)>at).reduce((n,q)=>n+remaining(q),0);
      const quantity=Math.min(s.policy.quantity,available+existing);
      if(quantity>0)candidates.push({marketId:b.marketId,exchangeId:b.exchangeId,side,action:'sell',quantity,price:money(side==='YES'?ask:SCALE-bid),rank:1e12,source:b});
      continue;
    }
    if(spread<TICK*BigInt(s.policy.minimumSpreadTicks))continue;
    const micro=(ask*BigInt(b.bids[0].quantity)+bid*BigInt(b.asks[0].quantity))/BigInt(b.bids[0].quantity+b.asks[0].quantity);
    const recent=(s.flow[b.exchangeId]??[]).filter(t=>at-t.at<=900000);s.flow[b.exchangeId]=recent;
    const activity=recent.reduce((n,t)=>n+t.quantity,0);
    const rank=Number(spread)*Math.log1p(activity+1)/(1+Number(bid));
    for(const [side,limit,value]of [['YES',bid,micro],['NO',SCALE-ask,SCALE-micro]]){
      if(value-limit-fee(limit,1,s.policy)<amount(s.policy.minimumEdge))continue;
      candidates.push({marketId:b.marketId,exchangeId:b.exchangeId,side,action:'buy',quantity:s.policy.quantity,price:money(limit),rank,source:b});
    }
  }
  return candidates.sort((a,b)=>b.rank-a.rank||a.exchangeId.localeCompare(b.exchangeId)||a.side.localeCompare(b.side));
}
function reconcile(s,at){
  const targets=desired(s,at),key=q=>[q.exchangeId,q.side,q.action].join(':'),byKey=new Map(targets.map(q=>[key(q),q]));
  s.writes=s.writes.filter(t=>at-t<60000);
  if(s.writes.length>=s.policy.writesPerMinute||(s.writes.length&&at-s.writes.at(-1)<s.policy.writeGapMs)){count(s,'writeBudgetHolds');return;}
  const active=unresolved(s).filter(q=>end(q)>at);
  for(const q of active){const target=byKey.get(key(q));if(q.cancelEffectiveAt!==undefined)continue;
    if((!target||target.price!==q.price)&&at-q.createdAt>=s.policy.minimumQuoteAgeMs){q.cancelEffectiveAt=at+s.policy.latencyMs;s.writes.push(at);count(s,'cancels');return;}}
  const batch=[],markets=new Set([...Object.entries(s.positions).filter(([,p])=>qty(p.YES)+qty(p.NO)>0).map(([ex])=>ex),...unresolved(s).map(q=>q.exchangeId)]);
  let available=amount(s.cash)-reserve(s),headroom=amount(s.policy.maximumCommitted)-owned(s)-reserve(s);
  const marketCosts=new Map();for(const [ex,p]of Object.entries(s.positions))marketCosts.set(ex,basis(p.YES)+basis(p.NO));
  for(const q of unresolved(s).filter(q=>q.action==='buy'))marketCosts.set(q.exchangeId,(marketCosts.get(q.exchangeId)??0n)+buyCost(s,q.price,remaining(q)));
  for(const t of targets){
    if(active.some(q=>key(q)===key(t))||batch.length>=8)continue;
    if(!markets.has(t.exchangeId)&&markets.size>=s.policy.maximumMarkets)continue;
    const p=position(s,t.exchangeId);
    let n=t.quantity;
    if(t.action==='sell')n=Math.min(n,qty(p[t.side])-pledged(s,t.exchangeId,t.side)-oppositeBuys(s,t.exchangeId,t.side));
    else{
      let cost;while(n>0){cost=buyCost(s,t.price,n);if(cost<=available&&cost<=headroom&&cost+(marketCosts.get(t.exchangeId)??0n)<=amount(s.policy.maximumMarketCost))break;n--;}
      if(n>0){available-=cost;headroom-=cost;marketCosts.set(t.exchangeId,(marketCosts.get(t.exchangeId)??0n)+cost);}
    }
    if(n<1)continue;
    // Never place crossing YES/NO acquisitions against our own quotes.
    if(t.action==='buy'&&[...active,...batch].some(q=>q.exchangeId===t.exchangeId&&q.action==='buy'&&q.side!==t.side&&amount(q.price)+amount(t.price)>=SCALE))continue;
    markets.add(t.exchangeId);batch.push({...t,quantity:n});
  }
  if(batch.length){s.writes.push(at);for(const t of batch){s.orders.push({id:s.nextOrderId++,marketId:t.marketId,exchangeId:t.exchangeId,side:t.side,action:t.action,quantity:t.quantity,filled:0,price:t.price,createdAt:at,activationAt:at+s.policy.latencyMs,expiresAt:at+s.policy.lifetimeMs,sourceHash:payloadHash(t.source),sourceSequence:String(t.source.sequence),tournamentId:t.source.tournamentId,retired:false,participationCredit:0,barrier:null});}count(s,'submissionBatches');}
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
    if(s.seen[key]){check(s.seen[key]===hash,'TRADE_CONFLICT');duplicate=true;}else s.seen[key]=hash;
  }else if(event.type==='coverage'){
    check(event.complete===true&&ns(event.from)<ns(event.to)&&ns(event.to)<=atNs&&/^\d+$/.test(String(event.projectedThroughSequence)),'COVERAGE');
  }else check(event.type==='gap'||event.type==='tick','EVENT_TYPE');
  s.lastAt=event.processedAt??event.receivedAt;s.events++;
  if(event.type==='book'){
    const b=structuredClone(event.book);s.books[b.exchangeId]=b;
    for(const q of unresolved(s).filter(q=>q.exchangeId===b.exchangeId&&q.marketId===b.marketId&&q.tournamentId===b.tournamentId&&ms(b.requestStartedAt)>=end(q)))if(!q.barrier)q.barrier={sequence:String(b.sequence),endAt:end(q)};
  }else if(event.type==='gap'){
    check(typeof event.exchangeId==='string','GAP_SCOPE');if(s.books[event.exchangeId])s.books[event.exchangeId].invalid=true;count(s,'gaps');
  }else if(event.type==='trade'&&!duplicate){
    s.flow[event.exchangeId]??=[];s.flow[event.exchangeId].push({at:ms(event.executedAt),quantity:event.quantity});count(s,'uniqueTrades');
    let participation=Math.floor(event.quantity*s.policy.participationBps/10000);
    const executed=ns(event.executedAt),tradePrice=price(event.yesPrice);
    for(const q of unresolved(s).filter(q=>q.marketId===event.marketId&&q.exchangeId===event.exchangeId&&q.tournamentId===event.tournamentId).sort((a,b)=>a.createdAt-b.createdAt||a.id-b.id)){
      if(executed<BigInt(q.activationAt)*1000000n||executed>=BigInt(end(q))*1000000n)continue;
      const yesLimit=q.side==='YES'?amount(q.price):SCALE-amount(q.price),bid=(q.side==='YES')===(q.action==='buy');
      if(!(bid?tradePrice<=yesLimit-TICK:tradePrice>=yesLimit+TICK))continue;
      q.participationCredit+=event.quantity*s.policy.participationBps;
      // Fractional prints accumulate, but a print's total allocated shares is capped.
      const n=Math.min(event.quantity,remaining(q),Math.floor(q.participationCredit/10000),Math.max(participation,1));
      if(n>0){q.participationCredit-=n*10000;fill(s,q,n,s.lastAt,event.id);participation=Math.max(0,participation-n);break;}
    }
  }else if(event.type==='coverage'){
    // Complete original quote window plus a post-end engine barrier, not wall time alone.
    for(const q of unresolved(s).filter(q=>q.marketId===event.marketId&&q.exchangeId===event.exchangeId&&q.tournamentId===event.tournamentId))if(q.barrier&&q.barrier.endAt===end(q)&&ms(event.from)<=q.activationAt&&ms(event.to)>=end(q)&&BigInt(event.projectedThroughSequence)>=BigInt(q.barrier.sequence))q.retired=true;
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
    positions:s.positions,fills:s.fills.length,orders:s.orders.length,activeOrders:at===null?0:unresolved(s).filter(q=>end(q)>at).length,
    unresolvedOrders:unresolved(s).length,profitSignalEligible:unresolved(s).length===0&&known&&amount(s.realized)>0n&&amount(s.cash)+liquidation>amount(s.policy.initialCash),counts:s.counts,contestOrders:0,networkRequests:0,profitabilityEstablished:false,
    assumptions:['Independent simulated capital; no contest-account ownership.','Microprice ranks quotes; it is not calibrated alpha.','Strict one-tick trade-through and bounded participation are counterfactual, not verified queue fills.','Cancel acknowledgment does not release unresolved funding; complete window and post-end checkpoint are required.','YES/NO pair payout is a declared paper accounting assumption.']};
}
