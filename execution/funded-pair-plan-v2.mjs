import {amount,money,price,instant,SCALE,TICK} from '../paper/fixed.mjs';import {payloadHash} from './core.mjs';
const min=(...v)=>v.reduce((a,b)=>a<b?a:b),need=(v,c)=>{if(!v)throw Error(c);};
// A two-market package has no atomic fill guarantee. Each first-leg failure
// scenario is budgeted independently of its conditional ordinary payout.
export function fundedPairPlanV2({pair,books,now,budgets,depthFractionBps=100,minimumPackageSurplus='0.005000',unwindAdverseTicks=1}){
 const base={depthParticipationBasis:'cumulative_whole_share_floor',executionAuthority:false,competitionOrders:0,profitabilityEstablished:false,proposalReady:false};
 try{
  const at=instant(now),edge=amount(minimumPackageSurplus);need(edge>=TICK&&edge<SCALE&&pair.ordinaryNoNoPayoutFloor==='1.000000'&&pair.legs.length===2&&pair.legs[0].marketId!==pair.legs[1].marketId,'PAIR_SCOPE');
  need(Number.isSafeInteger(depthFractionBps)&&depthFractionBps>0&&depthFractionBps<=10000&&Number.isSafeInteger(unwindAdverseTicks)&&unwindAdverseTicks>=0&&unwindAdverseTicks<=100,'RISK_PARAMETERS');
  const caps=['cashAvailable','eventRemaining','grossRemaining','maximumExceptionLoss'].map(k=>amount(budgets[k])),orphanCap=amount(budgets.maximumUnhedgedPrincipal),unwindCap=amount(budgets.maximumModeledUnwindLoss);need([...caps,orphanCap,unwindCap].every(v=>v>=0n),'NEGATIVE_BUDGET');
  const selected=pair.legs.map(l=>books.filter(b=>b.marketId===l.marketId&&b.exchangeId===l.exchangeId&&instant(b.receivedAt)<=at).sort((a,b)=>instant(b.receivedAt)-instant(a.receivedAt))[0]);need(selected.every(Boolean),'MISSING_DEPTH');
  const starts=selected.map(b=>instant(b.requestStartedAt)),ends=selected.map(b=>instant(b.receivedAt));need(starts.every((t,i)=>t<=ends[i]&&ends[i]<=at&&t>=instant(pair.legs[i].rootObservedAt)),'BOOK_CLOCK');
  const fresh=starts.every(t=>at-t<=20000)&&Math.max(...ends)-Math.min(...starts)<=10000;
  const curves=selected.map(b=>{
   function levels(rows,descending,complement){let prior=descending?SCALE:0n,total=0n,allocated=0n;return rows.map(l=>{const p=price(l.price);need((descending?p<prior:p>prior)&&Number.isSafeInteger(l.quantity)&&l.quantity>0,'DEPTH_CURVE');prior=p;total+=BigInt(l.quantity);const cumulative=total*BigInt(depthFractionBps)/10000n,q=cumulative-allocated;allocated=cumulative;return{p:complement?SCALE-p:p,q};}).filter(l=>l.q>0n);}
   return{buy:levels(b.bids,true,true),sell:levels(b.asks,false,true)};
  });
  const boundaries=new Set([0n]);for(const c of curves)for(const side of ['buy','sell']){let sum=0n;for(const l of c[side]){sum+=l.q;boundaries.add(sum);}}
  const depthAt=(curve,q)=>{let sum=0n;for(const l of curve){sum+=l.q;if(sum>=q)return l.p;}return null;};
  const endsOfRungs=[...boundaries].sort((a,b)=>a<b?-1:a>b?1:0),candidates=[];
  for(let i=1;i<endsOfRungs.length;i++){
   const lower=endsOfRungs[i-1]+1n,upper=min(endsOfRungs[i],2147483647n),raw=curves.map(c=>depthAt(c.buy,lower));if(raw.some(x=>x===null))continue;
   const limits=raw.map(v=>v+TICK),unit=limits[0]+limits[1];if(limits.some(v=>v>SCALE-TICK)||SCALE-unit<edge)continue;
   // Reserve the maximum allowed package price, not an observed VWAP.
   const reservationUnit=SCALE-edge;
   for(let first=0;first<2;first++){
    const unwind=depthAt(curves[first].sell,lower);if(unwind===null)continue;const exit=unwind-BigInt(unwindAdverseTicks)*TICK;if(exit<TICK)continue;
    const loss=limits[first]>exit?limits[first]-exit:0n;
    const q=min(upper,...caps.map(c=>c/reservationUnit),orphanCap/limits[first],loss>0n?unwindCap/loss:upper);if(q<lower)continue;
    const conditionalSurplus=q*(SCALE-unit);candidates.push({quantity:Number(q),firstLeg:first,firstLimit:money(limits[first]),hedgeObservedLimit:money(limits[1-first]),maximumHedgeLimitAfterFullFirstLimit:money(reservationUnit-limits[first]),maximumPackageReservation:money(q*reservationUnit),maximumObservedUniformLimitCost:money(q*unit),conditionalOrdinarySurplusAtUniformLimits:money(conditionalSurplus),maximumStrandedFirstPrincipal:money(q*limits[first]),modeledUnwindLimit:money(exit),modeledUnwindLoss:money(q*loss),zeroPayoutExceptionLoss:money(q*unit),surplus:conditionalSurplus,orphan:q*limits[first]});
   }
  }
  candidates.sort((a,b)=>a.surplus>b.surplus?-1:a.surplus<b.surplus?1:a.orphan<b.orphan?-1:a.orphan>b.orphan?1:a.firstLeg-b.firstLeg);
  const sanitize=({surplus,orphan,...v})=>v,chosen=candidates[0];return{...base,proposalReady:!!chosen,reason:chosen?'FUNDED_CONDITIONAL_PAIR_WITH_FAILURE_BUDGET':'NO_FUNDED_POSITIVE_PACKAGE_DEPTH',currentFresh:fresh,eligibleCurrentObservation:fresh&&!!chosen,chosen:chosen?sanitize(chosen):null,candidates:candidates.map(sanitize),administrativeRulesUnresolved:pair.administrativeOrRefundRulesUnresolved===true,sourceBooks:selected.map(b=>({marketId:b.marketId,exchangeId:b.exchangeId,hash:payloadHash(b),requestStartedAt:b.requestStartedAt,receivedAt:b.receivedAt})),atomicExecution:false,unwindFutureDepthGuaranteed:false,profitIsConditionalOnOrdinaryResolution:true};
 }catch(e){return{...base,reason:e.message};}
}
