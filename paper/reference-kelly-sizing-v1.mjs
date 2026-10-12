import {amount,price,money,SCALE} from './fixed.mjs';
const min=(...xs)=>xs.reduce((a,b)=>a<b?a:b);
export function referenceKellySizingV1({equityLower,cashAvailable,referenceValue,protectedPrice,currentQuantity,reservedBuyQuantity,depthQuantity,kellyFractionBps,eventFractionBps,eventPrincipal,grossRemaining}){
 const eq=amount(equityLower),cash=amount(cashAvailable),p=amount(referenceValue),unit=price(protectedPrice),event=amount(eventPrincipal),gross=amount(grossRemaining);
 if(eq<=0n||cash<0n||p<0n||p>SCALE||event<0n||![currentQuantity,reservedBuyQuantity,depthQuantity].every(q=>Number.isSafeInteger(q)&&q>=0)||![kellyFractionBps,eventFractionBps].every(x=>Number.isSafeInteger(x)&&x>0&&x<=10000))throw Error('REFERENCE_KELLY_INPUT');
 const base={quantity:0,maximumCost:'0.000000',executionAuthority:false,actualOrders:0,uncalibratedReferenceNotAConfidenceBound:true};
 if(p<=unit||gross<=0n)return{...base,reason:'NO_FUNDED_REFERENCE_EDGE'};
 const targetCapital=eq*(p-unit)*BigInt(kellyFractionBps)/((SCALE-unit)*10000n),committed=(BigInt(currentQuantity)+BigInt(reservedBuyQuantity))*unit,eventRemaining=eq*BigInt(eventFractionBps)/10000n-event,remainingTarget=targetCapital-committed,budget=min(cash,gross,eventRemaining,remainingTarget);
 if(budget<=0n)return{...base,reason:'REFERENCE_TARGET_ALREADY_FUNDED',targetCapital:money(targetCapital)};
 const quantity=min(budget/unit,BigInt(depthQuantity),2147483647n);
 return{...base,quantity:Number(quantity),maximumCost:money(quantity*unit),targetCapital:money(targetCapital),remainingTarget:money(remainingTarget),eventRemaining:money(eventRemaining),reason:quantity?'FUNDED_FRACTIONAL_KELLY_REFERENCE':'BELOW_ONE_SHARE_CAPACITY',historicalProfitabilityEstablished:false};
}
