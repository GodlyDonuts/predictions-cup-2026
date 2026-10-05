import {amount, money, TICK, instant} from '../paper/fixed.mjs';
import {digest} from './model.mjs';
import {validateResearch} from './probability-paper.mjs';
import {probabilityAccountGate} from './probability-receipt.mjs';

const need=(ok,code)=>{if(!ok)throw Error('SETTLEMENT_PAPER_'+code);};
export const SETTLEMENT_PAPER_POLICY=Object.freeze({minimumEdge:'0.020000',maximumSpread:'0.020000',maximumBookAgeMs:20000,maximumResearchAgeMs:21600000,fractionalKellyBps:2500,slippageTicks:1,depthFractionBps:100,maxOrderCost:'1.000000',maxExposure:'3.000000',cashReserve:'90.000000',cumulativeBuyBudget:'10.000000',groupCaps:{national_election:'3.000000',texas_governor:'1.000000'}});
export const SETTLEMENT_PAPER_FILLS=Object.freeze({initialCash:'100.000000',fees:0,sharesPerOrder:1,maxPositionShares:1,latenciesMs:[1000,3000],intentLifetimeMs:180000,cooldownMs:900000});
export function validateSettlementPaperBank(bank){
 need(bank.schemaVersion===1&&bank.mode==='funded_settlement_value_paper_v1'&&digest(bank.policy)===digest(SETTLEMENT_PAPER_POLICY)&&digest(bank.fillPolicy)===digest(SETTLEMENT_PAPER_FILLS),'POLICY');
 need(bank.research.length===1&&Object.keys(bank.identities).length===1,'SCOPE');
 const r=bank.research[0],identity=bank.identities[r.marketId];validateResearch(r);
 need(r.side==='YES'&&instant(r.recordedAt)<=instant(bank.cutoff)&&identity?.exchangeId===r.exchangeId&&identity.predicateHash===r.predicateHash&&bank.universe.exchanges.some(x=>x.marketId===r.marketId&&x.exchangeId===r.exchangeId&&x.option==='YES'),'RESEARCH_BINDING');
 need(bank.riskGroups?.[r.marketId]?.join('|')==='national_election|texas_governor'&&Number.isSafeInteger(bank.valuationDeadlineMs)&&bank.valuationDeadlineMs>instant(bank.cutoff),'HORIZON');
 return bank;
}
export function createSettlementPaperState(bank){
 validateSettlementPaperBank(bank);
 return{bankHash:digest(bank),lastAvailableAt:bank.cutoff,barriers:{global:bank.cutoff,markets:{}},counts:{},decisions:[],books:{},wallets:Object.fromEntries(bank.fillPolicy.latenciesMs.map(ms=>[ms,{cash:bank.fillPolicy.initialCash,realizedPnl:'0.000000',cumulativeBuys:'0.000000',positions:{},pending:{},fills:[],consumedVersions:{},lastExitedAtMs:{},intents:0}]))};
}
export function assertSettlementPaperState(state,bank){
 need(state.bankHash===digest(bank),'BANK_CHANGED');
 for(const w of Object.values(state.wallets)){
  let cash=amount(bank.fillPolicy.initialCash),realized=0n,buys=0n;const positions={};
  for(const f of w.fills){need(f.quantity===1&&f.fee==='0.000000'&&['buy','sell'].includes(f.action),'FILL');const value=amount(f.price);
   if(f.action==='buy'){need(!positions[f.marketId],'DUPLICATE_HOLDING');positions[f.marketId]={quantity:1,cost:f.price,enteredAt:f.at};cash-=value;buys+=value;}
   else{need(positions[f.marketId],'OVERSELL');realized+=value-amount(positions[f.marketId].cost);cash+=value;delete positions[f.marketId];}
  }
  need(cash===amount(w.cash)&&realized===amount(w.realizedPnl)&&buys===amount(w.cumulativeBuys)&&digest(positions)===digest(w.positions),'CONSERVATION');
  const reserved=Object.values(w.pending).filter(p=>p.action==='buy').reduce((sum,p)=>sum+amount(p.limit),0n),held=Object.values(w.positions).reduce((sum,p)=>sum+amount(p.cost),0n);
  need(cash-reserved>=amount(bank.policy.cashReserve)&&held+reserved<=amount(bank.policy.maxExposure)&&buys+reserved<=amount(bank.policy.cumulativeBuyBudget),'RESERVATIONS');
  need(Object.entries(w.pending).every(([id,p])=>p.quantity===1&&p.marketId===id&&!!w.positions[id]===(p.action==='sell')),'PENDING_BINDING');
 }
}
function boundary(s,id,at,reason){
 if(id===null){s.barriers.global=at;s.books={};}else{s.barriers.markets[id]=at;delete s.books[id];}
 for(const w of Object.values(s.wallets))for(const market of Object.keys(w.pending))if(id===null||id===market)delete w.pending[market];
 s.counts[reason]=(s.counts[reason]||0)+1;
}
export function settlementPaperInput(s,bank,latency,row,availableAt,account,{ignorePending=false}={}){
 const b=row.payload.book,w=s.wallets[latency],knownAtMs=Math.max(instant(account.positionsAt),instant(account.ordersAt));
 return{schemaVersion:1,mode:'settlement_value_policy_v1',asOfMs:instant(availableAt),policy:bank.policy,
  wallet:{capital:bank.fillPolicy.initialCash,cash:w.cash,cumulativeBuys:w.cumulativeBuys,positions:Object.entries(w.positions).map(([id,p])=>({candidateId:id+':YES',quantity:p.quantity,cost:p.cost,enteredAtMs:instant(p.enteredAt)})),reservations:ignorePending?[]:Object.entries(w.pending).filter(([,p])=>p.action==='buy').map(([id,p])=>({candidateId:id+':YES',maximumCost:p.limit}))},
  candidates:bank.research.map(r=>({id:r.marketId+':YES',marketId:r.marketId,exchangeId:r.exchangeId,side:r.side,predicateHash:r.predicateHash,riskGroups:bank.riskGroups[r.marketId],settlesAtMs:bank.valuationDeadlineMs,
   research:{id:r.id,predicateHash:r.predicateHash,side:r.side,lower:r.lower,upper:r.upper,recordedAtMs:instant(r.recordedAt),sourceAtMs:instant(r.sourceAt),expiresAtMs:instant(r.expiresAt),method:r.method,settlementReviewed:r.settlementReviewed,flags:r.flags,sources:r.sources},
   book:r.marketId===b.marketId?{requestStartedAtMs:instant(b.requestStartedAt),receivedAtMs:instant(b.receivedAt),availableAtMs:instant(availableAt),sequence:row.sequence,sourceHash:row.hash,representation:'direct_yes',bids:b.bids,asks:b.asks}:null,
   account:{positionsAtMs:instant(account.positionsAt),ordersAtMs:instant(account.ordersAt),knownAtMs,complete:true,positionExcluded:account.excludedExchangeIds.includes(r.exchangeId),openOrderExcluded:account.excludedExchangeIds.includes(r.exchangeId)}}))};
}
// The native value policy chooses an action. Only a later, fresh REST request
// can simulate its one-share execution; the initial quote never makes a fill.
export function advanceSettlementPaper(s,row,bank,availableAt,account,evaluate){
 need(s.bankHash===digest(bank)&&instant(row.at)<=instant(availableAt)&&instant(availableAt)>=instant(s.lastAvailableAt),'CLOCK');s.lastAvailableAt=availableAt;
 if(row.kind!=='book'){need(['session_start','session_stop','invalidation'].includes(row.kind),'ROW_KIND');boundary(s,row.kind==='invalidation'?row.payload.marketId:null,availableAt,'source_boundary');return s;}
 const b=row.payload.book;if(!bank.identities[b.marketId])return s;
 const now=instant(availableAt),request=instant(b.requestStartedAt),id=b.marketId;
 if(now-instant(b.receivedAt)>bank.policy.maximumBookAgeMs||[s.barriers.global,s.barriers.markets[id],bank.cutoff].some(at=>at&&request<=instant(at))){boundary(s,id,availableAt,'stale_or_pre_boundary');return s;}
 const gate=probabilityAccountGate(account,b,availableAt);if(gate){boundary(s,id,availableAt,gate);return s;}
 s.books[id]={...b,availableAt,sourceHash:row.hash,publicationSequence:row.sequence};
 for(const [latency,w] of Object.entries(s.wallets)){
  const input=settlementPaperInput(s,bank,latency,row,availableAt,account,{ignorePending:true}),output=evaluate(input),d=output.decisions.find(c=>c.marketId===id);
  need(d&&output.executionAuthority===false&&output.competitionOrders===0&&output.fills===0,'NATIVE_SCOPE');
  const expectedAction=d.action==='candidate_buy'?'buy':d.action==='reduce'?'sell':null;let pending=w.pending[id],filled=false;
  if(pending){
   if(expectedAction!==pending.action||now-instant(pending.createdAt)>bank.fillPolicy.intentLifetimeMs){delete w.pending[id];pending=null;}
   else if(request>=instant(pending.createdAt)+Number(latency)&&request>instant(pending.createdAt)&&row.sequence>pending.sourceSequence&&w.consumedVersions[id]!==b.versionKey){
    const levels=pending.action==='buy'?b.asks:b.bids;
    for(const level of levels){const price=amount(level.price)+(pending.action==='buy'?TICK:-TICK);
     if(price<TICK||price>1000000n-TICK||pending.action==='buy'&&price>amount(pending.limit)||pending.action==='sell'&&price<amount(pending.limit))break;
     if(Math.floor(level.quantity*bank.policy.depthFractionBps/10000)<1)continue;
     if(pending.action==='buy'&&(w.positions[id]||amount(w.cash)-price<amount(bank.policy.cashReserve)||amount(w.cumulativeBuys)+price>amount(bank.policy.cumulativeBuyBudget)))break;
     const fill={intentId:pending.id,marketId:id,exchangeId:b.exchangeId,instrument:b.instrument,action:pending.action,quantity:1,price:money(price),fee:'0.000000',at:availableAt,requestStartedAt:b.requestStartedAt,receivedAt:b.receivedAt,sourceVersion:b.versionKey,sourceSequence:row.sequence,sourceHash:row.hash,intentSourceHash:pending.sourceHash,researchHash:pending.researchHash,latencyMs:Number(latency),model:'later_rest_request_counterfactual',reason:pending.reason};
     if(fill.action==='buy'){w.cash=money(amount(w.cash)-price);w.cumulativeBuys=money(amount(w.cumulativeBuys)+price);w.positions[id]={quantity:1,cost:fill.price,enteredAt:availableAt};}
     else{need(w.positions[id],'SELL_WITHOUT_POSITION');w.cash=money(amount(w.cash)+price);w.realizedPnl=money(amount(w.realizedPnl)+price-amount(w.positions[id].cost));delete w.positions[id];w.lastExitedAtMs[id]=now;}
     w.fills.push(fill);w.consumedVersions[id]=b.versionKey;delete w.pending[id];filled=true;break;
    }
   }
  }
  const reason=filled?'later_receipt_fill':d.reason;s.counts[reason]=(s.counts[reason]||0)+1;
  s.decisions.push({at:availableAt,latencyMs:Number(latency),marketId:id,sourceSequence:row.sequence,sourceHash:row.hash,action:d.action,reason,scenarioEdge:d.topScenarioEdge??null,liquidation:d.liquidation??null});if(s.decisions.length>200)s.decisions.shift();
  // A filled sell cannot immediately re-enter from the same sampled liquidity.
  if(!filled&&!w.pending[id]&&expectedAction&&d.quantity>=1&&(expectedAction!=='buy'||now-(w.lastExitedAtMs[id]||0)>=bank.fillPolicy.cooldownMs)){
   const limit=expectedAction==='buy'?d.hardLimitWithCushion:money(amount(b.bids[0].price)>TICK?amount(b.bids[0].price)-TICK:TICK);
   w.pending[id]={id:++w.intents,marketId:id,action:expectedAction,quantity:1,limit,createdAt:availableAt,sourceSequence:row.sequence,sourceHash:row.hash,researchHash:digest(bank.research.find(r=>r.marketId===id)),reason:d.reason};
  }
 }
 assertSettlementPaperState(s,bank);return s;
}
export function settlementPaperReport(s,bank,at){
 assertSettlementPaperState(s,bank);const now=instant(at);
 const wallets=Object.fromEntries(Object.entries(s.wallets).map(([ms,w])=>{
  let proceeds=0n,complete=true;const positions=Object.entries(w.positions).map(([id,p])=>{
   const b=s.books[id],fresh=b&&now>=instant(b.availableAt)&&now-instant(b.receivedAt)<=bank.policy.maximumBookAgeMs,level=fresh?b.bids.find(l=>Math.floor(l.quantity*bank.policy.depthFractionBps/10000)>=p.quantity):null,liquidation=level?money(amount(level.price)-TICK):null;
   if(liquidation===null)complete=false;else proceeds+=amount(liquidation);
   return{marketId:id,instrument:bank.identities[id].exchangeId+':YES',...p,liquidation,liquidationPnl:liquidation===null?null:money(amount(liquidation)-amount(p.cost)),researchExpired:now>=instant(bank.research.find(r=>r.marketId===id).expiresAt)};
  });
  return[ms,{cash:w.cash,realizedPnl:w.realizedPnl,cumulativeBuys:w.cumulativeBuys,reservedCost:money(Object.values(w.pending).filter(p=>p.action==='buy').reduce((n,p)=>n+amount(p.limit),0n)),fills:w.fills.length,closedTrades:w.fills.filter(f=>f.action==='sell').length,positions,pending:Object.values(w.pending),recentFills:w.fills.slice(-25),liquidationPnl:complete?money(amount(w.cash)+proceeds-amount(bank.fillPolicy.initialCash)):null}];
 }));
 return{mode:bank.mode,cohort:bank.name,updatedAt:at,bankHash:s.bankHash,cutoff:bank.cutoff,counts:s.counts,decisions:s.decisions,wallets,executionAuthority:false,networkRequests:0,competitionOrders:0,profitabilityEstablished:false,fees:0,holdingPolicy:'current_fundamental_value_without_fixed_timeout_or_price_only_stop',limitations:['Settlement probabilities describe scenarios, not a near-term repricing forecast or a guaranteed payout.','Expired research stops new entries and requires review; existing held risk remains attributed.','One-share counterfactual fills require a later REST request, causal account exclusions, one adverse tick and one-percent depth.','Latency wallets share the same opportunities; do not add their P&L together.','A deadline is not a settlement event. This engine never manufactures a payout.']};
}
