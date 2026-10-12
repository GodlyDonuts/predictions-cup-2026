import {amount,price,money,SCALE} from '../paper/fixed.mjs';

export function jointPairResearchChoicesV1(candidates){
 const groups=new Map();
 for(const c of candidates){
  if(![0,1].includes(c.firstLeg)||!Number.isSafeInteger(c.quantity)||c.quantity<1)throw Error('PAIR_JOINT_CANDIDATE');
  const first=price(c.firstLimit),peer=price(c.hedgeObservedLimit),exit=price(c.modeledUnwindLimit);
  const limits=(c.firstLeg===0?[first,peer]:[peer,first]).map(money),key=limits.join('|'),g=groups.get(key)??{limits,first:[null,null]},old=g.first[c.firstLeg];
  if(!old||c.quantity>old.quantity||c.quantity===old.quantity&&exit<price(old.modeledUnwindLimit))g.first[c.firstLeg]=c;
  groups.set(key,g);
 }
 return [...groups.values()].filter(g=>g.first.every(Boolean)).map(g=>{
  const quantity=Math.min(...g.first.map(c=>c.quantity)),principal=g.limits.reduce((n,p)=>n+price(p),0n)*BigInt(quantity),surplus=SCALE*BigInt(quantity)-principal;
  const loss=g.first.reduce((n,c)=>{const unit=price(c.firstLimit)-price(c.modeledUnwindLimit);return unit>n?unit:n;},0n)*BigInt(quantity);
  return {quantity,limits:g.limits,principal:money(principal),ordinarySurplus:money(surplus),worstUnmatchedUnwindLoss:money(loss)};
 }).filter(c=>amount(c.ordinarySurplus)>0n).sort((a,b)=>{
  for(const [field,descending]of [['ordinarySurplus',true],['principal',false],['worstUnmatchedUnwindLoss',false]]){const x=amount(a[field]),y=amount(b[field]);if(x!==y)return (x<y?-1:1)*(descending?-1:1);}
  return a.limits.join('|').localeCompare(b.limits.join('|'));
 });
}

// Scenario arithmetic, not an estimate of actual completion or exception odds.
export function pairCompletionEconomicsV1({ordinarySurplus,unmatchedUnwindLoss,zeroPayoutLoss,exceptionProbability='0.000000'}) {
 const gain=amount(ordinarySurplus),loss=amount(unmatchedUnwindLoss),capital=amount(zeroPayoutLoss),exception=amount(exceptionProbability);
 if(gain<=0n||loss<0n||capital<=0n||exception<0n||exception>SCALE)throw Error('PAIR_ECONOMICS_INPUT');
 const ordinary=SCALE-exception,numerator=ordinary*loss+exception*capital,denominator=ordinary*(gain+loss);
 const feasible=denominator>0n&&numerator<denominator;
 return {ordinarySurplus,unmatchedUnwindLoss,zeroPayoutLoss,exceptionProbability,positiveExpectedReturnPossible:feasible,completionProbabilityMustExceed:{numerator:numerator.toString(),denominator:denominator.toString(),roundedUpToSixDecimals:denominator?money((numerator*SCALE+denominator-1n)/denominator):null},exceptionProbabilitiesAreAssumptions:true,completionProbabilityEstimated:false,profitabilityEstablished:false,executionAuthority:false};
}
