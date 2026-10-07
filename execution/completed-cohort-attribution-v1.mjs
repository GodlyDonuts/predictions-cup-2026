import {amount,money} from '../paper/fixed.mjs';
export function attributeCompletedCohortV1(orders){
 const groups=new Map(),ids=new Set();let invested=0n,proceeds=0n,realized=0n,ordersCount=0;
 for(const o of orders){if(!o.proof)continue;if(ids.has(o.intentId))throw Error('COHORT_ATTRIBUTION_DUPLICATE');ids.add(o.intentId);const p=o.proof;if(!p.scopedTradeVerified||p.fee!=='0.000000'||!['buy','sell'].includes(o.body.action))throw Error('COHORT_ATTRIBUTION_PROOF');
  const g=groups.get(o.body.exchangeId)??{exchangeId:o.body.exchangeId,marketId:o.marketId,bought:0,sold:0,cost:0n,proceeds:0n,realized:0n,firstBuy:null,lastSell:null,weightedHoldShareMs:0,lots:[],orders:0,dispatchFillLatenciesMs:[]};g.orders++;ordersCount++;
  const times=p.fills.map(f=>Date.parse(f.filledAt));if(times.some(t=>!Number.isFinite(t)))throw Error('COHORT_ATTRIBUTION_CLOCK');
  for(const [i,f]of p.fills.entries()){const quantity=Math.abs(f.quantity),t=times[i];if(!Number.isSafeInteger(quantity)||quantity<1)throw Error('COHORT_ATTRIBUTION_QUANTITY');
   if(o.body.action==='buy'){g.bought+=quantity;g.firstBuy=Math.min(g.firstBuy??t,t);g.lots.push({quantity,at:t});}
   else{g.sold+=quantity;g.lastSell=Math.max(g.lastSell??t,t);let left=quantity;for(const lot of g.lots){const take=Math.min(left,lot.quantity);if(t<lot.at)throw Error('COHORT_ATTRIBUTION_FIFO_CLOCK');g.weightedHoldShareMs+=take*(t-lot.at);lot.quantity-=take;left-=take;if(!left)break;}if(left)throw Error('COHORT_ATTRIBUTION_UNOWNED_SELL');}
   g.dispatchFillLatenciesMs.push(t-Date.parse(o.createdAt));
  }
  const cost=amount(p.scopedPrincipalCost),cash=amount(p.scopedProceeds),delta=amount(p.realizedPnl);g.cost+=cost;g.proceeds+=cash;g.realized+=delta;invested+=cost;proceeds+=cash;realized+=delta;groups.set(o.body.exchangeId,g);
 }
 const markets=[...groups.values()].map(g=>{if(g.bought!==g.sold||g.lots.some(l=>l.quantity))throw Error('COHORT_ATTRIBUTION_NOT_FLAT');if(g.proceeds-g.cost!==g.realized)throw Error('COHORT_ATTRIBUTION_ECONOMICS');return{exchangeId:g.exchangeId,marketId:g.marketId,bought:g.bought,sold:g.sold,buyCost:money(g.cost),sellProceeds:money(g.proceeds),realized:money(g.realized),costReturnPercent:Number(g.realized)*100/Number(g.cost),weightedHoldingHours:g.weightedHoldShareMs/g.sold/3600000,firstBuyAt:new Date(g.firstBuy).toISOString(),lastSellAt:new Date(g.lastSell).toISOString(),orders:g.orders,dispatchFillLatenciesMs:g.dispatchFillLatenciesMs};});
 return{schemaVersion:1,markets,verifiedOrders:ordersCount,invested:money(invested),proceeds:money(proceeds),realized:money(realized),returnOnDeployedPrincipalPercent:Number(realized)*100/Number(invested),fees:'0.000000',sampleMarkets:markets.length,scalableEdgeEstablished:false};
}

