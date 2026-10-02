import {decimalMicros,formatMicros} from './core.mjs';

// Numeric JSON fields can expose the platform's double complement artifact,
// e.g. 1 - (1 - .075) === .07499999999999996. Accept exactly the two known
// arithmetic representations of each official tick, not a tolerance or rounding.
const representations=new Map();
for(let tick=0;tick<=200;tick++){
  const micros=BigInt(tick)*5000n,price=formatMicros(micros),canonical=Number(price);
  const opposite=Number(formatMicros(1000000n-micros));
  for(const [value,kind] of [[canonical,'canonical_tick'],[1-opposite,'binary_tick_complement'],[1-(1-canonical),'binary_complement_roundtrip']]){
    const prior=representations.get(value);
    if(prior&&prior.price!==price)throw Error('API_TICK_REPRESENTATION_COLLISION');
    if(!prior)representations.set(value,Object.freeze({price,tick,representation:kind}));
  }
}
export function normalizeOfficialFillTick(value){
  if(typeof value==='number'){
    const found=Number.isFinite(value)&&!Object.is(value,-0)?representations.get(value):null;
    if(!found)throw Error('UNRECOGNIZED_API_TICK_REPRESENTATION');
    return found;
  }
  const micros=decimalMicros(value);
  if(micros>1000000n||micros%5000n!==0n)throw Error('OFFICIAL_FILL_PRICE_OFF_TICK');
  return Object.freeze({price:formatMicros(micros),tick:Number(micros/5000n),representation:'exact_decimal_tick'});
}
