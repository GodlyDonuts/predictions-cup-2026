// Compare the decimal representations of prices, policy and estimates without
// rounding the reference to exchange ticks or adding a threshold tolerance.
function decimal(value) {
 if(typeof value!=='string'&&typeof value!=='number'||typeof value==='number'&&!Number.isFinite(value))throw Error('INVALID_GATE_DECIMAL');
 const text=String(value),m=/^(-?)(\d+)(?:\.(\d+))?(?:e([+-]?\d+))?$/i.exec(text);
 if(!m||text.length>128)throw Error('INVALID_GATE_DECIMAL');
 const exponent=Number(m[4]||0)-(m[3]?.length||0);
 if(!Number.isSafeInteger(exponent)||Math.abs(exponent)>400)throw Error('INVALID_GATE_DECIMAL');
 const coefficient=BigInt(m[2]+(m[3]||''))*(m[1]? -1n:1n);
 return exponent>=0?{n:coefficient*10n**BigInt(exponent),d:1n}:{n:coefficient,d:10n**BigInt(-exponent)};
}
const sub=(a,b)=>({n:a.n*b.d-b.n*a.d,d:a.d*b.d});
const mul=(a,b)=>({n:a.n*b.n,d:a.d*b.d});
const compare=(a,b)=>a.n*b.d<b.n*a.d?-1:a.n*b.d>b.n*a.d?1:0;
export function evaluateEntryGates(reference,ask,bid,policy) {
 if(!reference.ready)return {admitted:false,tests:null};
 const a=decimal(ask),b=decimal(bid),center=decimal(reference.center),deviation=decimal(reference.robustDeviation),discount=sub(center,a),floor=decimal(.001),zero=decimal(0);
 const denominator=compare(center,floor)>=0?center:floor;
 const tests={
  price:compare(a,decimal(policy.minimumPrice))>=0&&compare(a,decimal(policy.maximumPrice))<=0,
  spread:compare(a,b)>0&&compare(sub(a,b),decimal(policy.maximumSpread))<=0,
  minimumDiscount:compare(discount,decimal(policy.minimumDip))>=0,
  relativeDiscount:compare(discount,mul(denominator,decimal(policy.minimumRelativeDip)))>=0,
  maximumDiscount:compare(discount,decimal(policy.maximumDip))<=0,
  robustZ:compare(deviation,zero)>0&&compare(discount,mul(deviation,decimal(policy.minimumZ)))>=0
 };
 return {admitted:Object.values(tests).every(Boolean),tests};
}
