import {amount,money,price,SCALE,TICK,uint} from '../../paper/fixed.mjs';
const min=(...v)=>v.reduce((a,b)=>a<b?a:b);
const floor=(a,b)=>a>=0n?a/b:-((-a+b-1n)/b);
function probability(p){const n=amount(p);if(n<0n||n>SCALE)throw Error('SCENARIO_PROBABILITY');return n;}
const flags={executionAuthority:false,actualOrders:0,probabilitiesAreScenarioAssumptions:true,completionProbabilityEstimated:false,exceptionProbabilityEstimated:false,profitabilityEstablished:false,zeroFeeScenarioOnly:true};

// Completion probability is conditional on ordinary resolution. The same
// event's zero-payout exception loses the full allowed package principal.
export function pairScenarioReturn({quantity,principal,unmatchedUnwindLoss,completionProbability,exceptionProbability}){
 uint(quantity,'quantity',1);const payout=BigInt(quantity)*SCALE,cost=amount(principal),loss=amount(unmatchedUnwindLoss),c=probability(completionProbability),e=probability(exceptionProbability);
 if(cost<=0n||loss<0n)throw Error('SCENARIO_COST');
 const numerator=(SCALE-e)*(c*(payout-cost)-(SCALE-c)*loss)-e*SCALE*cost;
 return{...flags,expectedReturnFloor:money(floor(numerator,SCALE*SCALE)),expectedReturnNumerator:numerator.toString(),expectedReturnDenominator:(SCALE*SCALE).toString(),strictlyPositive:numerator>0n};
}

// Solve the scenario inequality exactly, then floor to the exchange's tick.
// This price guard does not make the assumed probabilities empirical bounds.
export function pairScenarioPriceCap({quantity,firstLegLimit,unmatchedUnwindLoss,completionProbability,exceptionProbability,maximumPackageReservation,minimumExpectedReturn='0.000001',minimumPackageSurplus='0.005000'}){
 uint(quantity,'quantity',1);const q=BigInt(quantity),first=price(firstLegLimit),loss=amount(unmatchedUnwindLoss),c=probability(completionProbability),e=probability(exceptionProbability),reserve=amount(maximumPackageReservation),target=amount(minimumExpectedReturn),edge=amount(minimumPackageSurplus);
 if(loss<0n||reserve<0n||target<=0n||edge<TICK||edge>=SCALE)throw Error('SCENARIO_RISK_LIMIT');
 const base={...flags,feasible:false,quantity,firstLegLimit,minimumExpectedReturn,minimumPackageSurplus};
 const ordinary=SCALE-e,denominator=ordinary*c+e*SCALE;
 const numerator=ordinary*c*q*SCALE-ordinary*(SCALE-c)*loss-target*SCALE*SCALE;
 if(denominator<=0n||numerator<=0n)return{...base,reason:'NO_POSITIVE_SCENARIO_PACKAGE_COST'};
 const bound=numerator/(denominator*q),cap=min(bound,reserve/q,SCALE-edge);
 const packageUnit=cap/TICK*TICK,hedge=packageUnit-first;
 if(hedge<TICK||hedge>SCALE-TICK)return{...base,reason:'NO_POSITIVE_PROTECTED_HEDGE_LIMIT',scenarioPackageUnitBound:money(bound)};
 const principal=q*packageUnit,returnAtCap=pairScenarioReturn({quantity,principal:money(principal),unmatchedUnwindLoss,completionProbability,exceptionProbability});
 if(BigInt(returnAtCap.expectedReturnNumerator)<target*SCALE*SCALE)throw Error('PRICE_CAP_INVARIANT');
 return{...base,feasible:true,reason:'SCENARIO_EXPECTATION_PRICE_CAP',scenarioPackageUnitBound:money(bound),maximumPackageUnitLimit:money(packageUnit),maximumHedgeLimit:money(hedge),maximumPackagePrincipal:money(principal),ordinarySurplusAtCap:money(q*SCALE-principal),returnAtCap,unwindFutureDepthGuaranteed:false,atomicExecution:false};
}
