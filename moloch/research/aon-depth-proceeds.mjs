import {amount,price,money,SCALE,fee} from '../../paper/fixed.mjs';

// Diagnostic execution-price scenario. It does not mutate a wallet or assert
// that this later snapshot was available when an actual order would execute.
export function aonDepthProceeds(side,limit,quantity,book,policy,priorConsumed=0n){
 if(!['YES','NO'].includes(side)||!Number.isSafeInteger(quantity)||quantity<=0||typeof priorConsumed!=='bigint'||priorConsumed<0n)throw Error('AON_DEPTH_INPUT');
 const bound=price(limit),levels=(side==='YES'?book.bids:book.asks).map(l=>({price:side==='YES'?price(l.price):SCALE-price(l.price),quantity:l.quantity})).filter(l=>l.price>=bound);
 for(let i=0;i<levels.length;i++)if(!Number.isSafeInteger(levels[i].quantity)||levels[i].quantity<=0||(i&&levels[i].price>=levels[i-1].price))throw Error('AON_DEPTH_LEVELS');
 const capacity=levels.reduce((n,l)=>n+BigInt(l.quantity),0n),eligible=capacity>priorConsumed?capacity-priorConsumed:0n;
 if(eligible<BigInt(quantity))return{canFill:false,eligibleDepth:String(eligible),filledQuantity:0,gross:null,net:null,segments:[]};
 let skip=priorConsumed,left=BigInt(quantity),gross=0n,fees=0n;const segments=[];
 for(const l of levels){const available=BigInt(l.quantity),used=skip>available?available:skip;skip-=used;const remaining=available-used,n=left<remaining?left:remaining;if(!n)continue;const value=n*l.price,f=fee(value,Number(n),policy);gross+=value;fees+=f;left-=n;segments.push({quantity:Number(n),price:money(l.price),gross:money(value),fee:money(f)});if(!left)break;}
 if(left)throw Error('AON_DEPTH_ACCOUNTING');
 const lower=bound*BigInt(quantity),lowerFee=fee(lower,quantity,policy);
 return{canFill:true,eligibleDepth:String(eligible),filledQuantity:quantity,gross:money(gross),fee:money(fees),net:money(gross-fees),limitGross:money(lower),limitNet:money(lower-lowerFee),netImprovement:money(gross-fees-lower+lowerFee),segments};
}
