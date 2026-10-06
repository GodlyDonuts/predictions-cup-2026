import {amount,money,price,TICK,instant} from '../paper/fixed.mjs';
const need=(v,c)=>{if(!v)throw Error('RECEIPT_FILL_'+c);};
// Counterfactual execution pricing; independent of order-limit construction.
// No wallet mutation or contest dispatch. A later receipt is not proof that
// an actual order obtained the displayed liquidity or queue priority.
export function receiptFillPrice({intent,book,sourceSequence,sourceHash,availableAt,latencyMs,depthFractionBps=100,residualAdverseTicks=0,maximumBookAgeMs=20000}){
 need(['buy','sell'].includes(intent?.action)&&Number.isSafeInteger(intent.quantity)&&intent.quantity>0&&Number.isSafeInteger(intent.sourceSequence)&&intent.sourceSequence>0&&/^[a-f0-9]{64}$/.test(intent.sourceHash)&&typeof intent.decisionVersion==='string'&&Number.isSafeInteger(latencyMs)&&latencyMs>=0,'INTENT');
 need(Number.isSafeInteger(depthFractionBps)&&depthFractionBps>0&&depthFractionBps<=10000&&[0,1,2].includes(residualAdverseTicks),'MODEL');
 const created=instant(intent.createdAt),requested=instant(book.requestStartedAt),received=instant(book.receivedAt),available=instant(availableAt);
 need(book.marketId===intent.marketId&&book.exchangeId===intent.exchangeId&&book.instrument===intent.exchangeId+':YES','SCOPE');
 need(Number.isSafeInteger(sourceSequence)&&/^[a-f0-9]{64}$/.test(sourceHash),'SOURCE');
 const reject=reason=>({eligible:false,reason,executionAuthority:false,competitionOrders:0,walletChanged:false});
 if(sourceSequence<=intent.sourceSequence||sourceHash===intent.sourceHash||book.versionKey===intent.decisionVersion)return reject('same_source');
 if(requested<=created||requested<created+latencyMs)return reject('not_independently_later');
 if(available>instant(intent.expiresAt))return reject('intent_expired');
 if(received<requested||received>available||available-requested>maximumBookAgeMs)return reject('stale_or_future_receipt');
 const limit=price(intent.limit),levels=book[intent.action==='buy'?'asks':'bids'];need(Array.isArray(levels),'DEPTH');let remaining=intent.quantity,total=0n,rawTotal=0n;const fills=[];
 for(const [index,level]of levels.entries()){need(Number.isSafeInteger(level.quantity)&&level.quantity>=0,'DEPTH_QUANTITY');const raw=price(level.price),adjustment=BigInt(residualAdverseTicks)*TICK,unit=raw+(intent.action==='buy'?adjustment:-adjustment);
  if(unit<TICK||unit>1000000n-TICK||intent.action==='buy'&&unit>limit||intent.action==='sell'&&unit<limit)break;
  const eligible=Number(BigInt(level.quantity)*BigInt(depthFractionBps)/10000n),take=Math.min(remaining,eligible);if(!take)continue;rawTotal+=raw*BigInt(take);total+=unit*BigInt(take);remaining-=take;fills.push({levelIndex:index,quantity:take,rawPrice:money(raw),modeledPrice:money(unit)});if(!remaining)break;
 }
 if(remaining)return reject('insufficient_limit_compliant_depth');
 return{eligible:true,quantity:intent.quantity,total:money(total),rawTotal:money(rawTotal),residualStressAmount:money(total-rawTotal),protectiveLimit:money(limit),residualAdverseTicks,fees:'0.000000',fills,source:{sequence:sourceSequence,hash:sourceHash,version:book.versionKey,requestStartedAt:book.requestStartedAt,receivedAt:book.receivedAt},model:residualAdverseTicks===0?'later_receipt_raw_baseline':'later_receipt_plus_declared_residual_stress',counterfactualOnly:true,executionAuthority:false,competitionOrders:0,walletChanged:false};
}
