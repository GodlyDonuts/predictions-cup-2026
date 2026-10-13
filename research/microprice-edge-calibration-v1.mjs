import {amount, SCALE, TICK} from '../paper/fixed.mjs';
import {instantNs} from '../feed/contract.mjs';

// Forward-mid calibration for a deliberately simple, frozen microstructure
// hypothesis. This does not model fills or claim executable returns.
export function createMicropriceCalibrationV1({horizonsMs=[1000,10000,60000,180000],purgeMs=200000,maximumLabelDelayMs=15000,splitAt=null}={}){
 if(!Array.isArray(horizonsMs)||!horizonsMs.length||horizonsMs.some(x=>!Number.isSafeInteger(x)||x<1000||x>3600000)||!Number.isSafeInteger(purgeMs)||purgeMs<0||!Number.isSafeInteger(maximumLabelDelayMs)||maximumLabelDelayMs<0)throw Error('MICROPRICE_OPTIONS');
 const split=splitAt===null?null:Date.parse(splitAt);if(splitAt!==null&&!Number.isFinite(split))throw Error('MICROPRICE_SPLIT');
 const pending=new Map(),versions=new Map(),groups=new Map(),diagnostics={points:0,duplicateOrRollback:0,censored:0,labels:0};let previousAt=-Infinity,firstAt=null;
 const spreadBucket=n=>n<=1?'1tick':n<=2?'2ticks':n<=4?'3to4ticks':'5plusTicks';
 const aggregate=(period,h,feature,market,edge,move)=>{const k=[period,h,feature].join('|');let a=groups.get(k);if(!a){a={samples:0,markets:new Set(),edgeSum:0,moveSum:0,wins:0,edgeWins:0};groups.set(k,a);}a.samples++;a.markets.add(market);a.edgeSum+=edge;a.moveSum+=move;if(move>0)a.wins++;if(edge>0)a.edgeWins++;};
 function observe(book,observedAt){
  const at=Date.parse(observedAt);if(!Number.isFinite(at)||at<previousAt)throw Error('MICROPRICE_CLOCK');previousAt=at;if(firstAt===null)firstAt=at;
  const ex=book.exchangeId,sourceNs=instantNs(book.sourceAt),seq=BigInt(book.sequence),old=versions.get(ex);if(old&&(seq<old.seq||seq===old.seq&&sourceNs<=old.sourceNs)){diagnostics.duplicateOrRollback++;return;}
  versions.set(ex,{seq,sourceNs});diagnostics.points++;
  const bid=amount(book.bids?.[0]?.price),ask=amount(book.asks?.[0]?.price),bq=book.bids?.[0]?.quantity,aq=book.asks?.[0]?.quantity;
  if(!(bid>0n&&ask>bid&&ask<SCALE&&Number.isSafeInteger(bq)&&bq>0&&Number.isSafeInteger(aq)&&aq>0))throw Error('MICROPRICE_BOOK');
  const mid=Number(bid+ask)/2,spread=Number(ask-bid)/Number(TICK),imb=(bq-aq)/(bq+aq),micro=(Number(ask)*bq+Number(bid)*aq)/(bq+aq),offset=(micro-mid)/Number(TICK),sign=x=>x>1e-9?'up':x< -1e-9?'down':'flat';
  const features={microSign:sign(offset),imbalanceSign:sign(imb),spread:spreadBucket(spread)};
  let lanes=pending.get(ex);if(!lanes){lanes=new Map(horizonsMs.map(h=>[h,[]]));pending.set(ex,lanes);}
  for(const h of horizonsMs){const lane=lanes.get(h);while(lane.length&&at>=lane[0].targetAt){const p=lane[0];if(at-p.targetAt>maximumLabelDelayMs){lane.shift();diagnostics.censored++;continue;}if(sourceNs<BigInt(p.targetAt)*1000000n)break;lane.shift();const future=Number(bid+ask)/2,move=(future-p.mid)/Number(TICK),edge=p.direction*move-p.spreadTicks,period=p.at<(split??firstAt)-purgeMs?'development':split!==null&&p.at>=split?'heldout':'purged';if(period==='purged')continue;aggregate(period,h,JSON.stringify(p.features),p.market,edge,p.direction*move);diagnostics.labels++;}}
  for(const h of horizonsMs){const q=lanes.get(h);q.push({at,targetAt:at+h,mid,direction:offset===0?0:Math.sign(offset),spreadTicks:spread,features,market:book.marketId});}
 }
 function report(){const rows=[];for(const [key,a]of groups){const [period,h,features]=key.split('|');rows.push({period,horizonMs:Number(h),features:JSON.parse(features),samples:a.samples,markets:a.markets.size,meanDirectionalMoveTicks:a.moveSum/a.samples,meanAfterOneSpreadTicks:a.edgeSum/a.samples,directionalWinRate:a.wins/a.samples,afterSpreadPositiveRate:a.edgeWins/a.samples,qualifiesForReview:a.samples>=100&&a.markets.size>=5});}rows.sort((a,b)=>a.horizonMs-b.horizonMs||a.period.localeCompare(b.period)||a.features.spread.localeCompare(b.features.spread)||a.features.microSign.localeCompare(b.features.microSign)||a.features.imbalanceSign.localeCompare(b.features.imbalanceSign));return{mode:'causal_microprice_forward_mid_calibration',splitAt:split===null&&firstAt===null?null:new Date(split??firstAt).toISOString(),purgeMs:purgeMs,diagnostics:{...diagnostics},groups:rows,fees:'0.000000',edgeProxy:'directional mid move less one full observed spread in ticks',fillsProven:false,profitabilityEstablished:false};}
 return{observe,report};
}
