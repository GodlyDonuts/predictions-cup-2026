import fs from 'node:fs';import crypto from 'node:crypto';
import {amount,money,price,SCALE} from '../../paper/fixed.mjs';
import {pairCompletionEconomicsV1} from '../pair-completion-economics-v1.mjs';
import {pairScenarioReturn,pairScenarioPriceCap} from './price-cap.mjs';
const root=new URL('../../',import.meta.url),file='data/paired-capture-economics-20261012.json',sourceBytes=fs.readFileSync(new URL(file,root)),source=JSON.parse(sourceBytes);
const plannerPath='execution/funded-pair-plan-v2.mjs',planner=fs.readFileSync(new URL(plannerPath,root));
if(!planner.toString().includes("minimumPackageSurplus='0.005000'"))throw Error('PLANNER_DEFAULT_REQUIRES_REVIEW');
const minimumPackageSurplus='0.005000',reservationUnit=SCALE-amount(minimumPackageSurplus);
const exceptions=['0.000000','0.005000','0.010000','0.020000'],completions=['0.500000','0.750000','0.900000','1.000000'];
const rows=source.cases.filter(c=>c.jointBothFirstLegScenariosFunded).map(c=>{
 const q=BigInt(c.quantity),observedCost=c.limits.reduce((n,p)=>n+price(p),0n)*q;
 if(observedCost!==amount(c.principal)||q*SCALE-observedCost!==amount(c.ordinarySurplus))throw Error('CAPTURE_ECONOMICS_ARITHMETIC');
 const worstCost=reservationUnit*q,worstGain=q*SCALE-worstCost;
 return{illustrativeCashBudget:c.illustrativeCashBudget,quantity:c.quantity,observedUniformLimits:c.limits,observedPrincipal:money(observedCost),observedOrdinarySurplus:c.ordinarySurplus,
  maximumPermittedPackagePrincipal:money(worstCost),minimumPermittedOrdinarySurplus:money(worstGain),worstUnmatchedUnwindLoss:c.worstUnmatchedUnwindLoss,
  thresholdSensitivity:exceptions.map(exceptionProbability=>({exceptionProbability,observedUniformLimits:pairCompletionEconomicsV1({ordinarySurplus:c.ordinarySurplus,unmatchedUnwindLoss:c.worstUnmatchedUnwindLoss,zeroPayoutLoss:c.principal,exceptionProbability}),maximumPermittedLimits:pairCompletionEconomicsV1({ordinarySurplus:money(worstGain),unmatchedUnwindLoss:c.worstUnmatchedUnwindLoss,zeroPayoutLoss:money(worstCost),exceptionProbability})})),
  scenarioGrid:exceptions.flatMap(exceptionProbability=>completions.map(completionProbability=>({exceptionProbability,completionProbability,
   observedReturn:pairScenarioReturn({quantity:c.quantity,principal:c.principal,unmatchedUnwindLoss:c.worstUnmatchedUnwindLoss,completionProbability,exceptionProbability}),
   maximumPermittedReturn:pairScenarioReturn({quantity:c.quantity,principal:money(worstCost),unmatchedUnwindLoss:c.worstUnmatchedUnwindLoss,completionProbability,exceptionProbability}),
   firstLegCaps:c.limits.map(firstLegLimit=>pairScenarioPriceCap({quantity:c.quantity,firstLegLimit,unmatchedUnwindLoss:c.worstUnmatchedUnwindLoss,completionProbability,exceptionProbability,maximumPackageReservation:money(worstCost),minimumPackageSurplus}))}))) };
});
const sha=bytes=>crypto.createHash('sha256').update(bytes).digest('hex');
const result={schemaVersion:1,at:new Date().toISOString(),mode:'historical_observed_vs_permitted_pair_price_scenario_audit',captureAt:source.captureAt,
 sources:[{file,sha256:sha(sourceBytes)},{file:plannerPath,sha256:sha(planner)}],defaultMinimumPackageSurplus:minimumPackageSurplus,maximumPermittedPackageUnit:money(reservationUnit),rows,
 actualOrders:0,accountRequests:0,currentExecutableDepthVerified:false,completionProbabilityEstimated:false,exceptionProbabilityEstimated:false,modelParametersSelectedByScore:false,profitabilityEstablished:false,
 assumptions:['Maximum permitted reservation derives from the referenced V2 planner default; no live order or activation verified.','The cited ordinary payout floor requires independent contract/affiliation verification.','Completion and exception probabilities are scenario inputs, not estimated or validated bounds.','Unmatched unwind loss/depth is historical modeled evidence, not guaranteed future liquidity.','Zero-fee scenario only; actual fees and administrative settlement scenarios require separate verification.']};
fs.writeFileSync(new URL('results.json',import.meta.url),JSON.stringify(result,null,2)+'\n');
console.log(JSON.stringify(rows.map(r=>({cash:r.illustrativeCashBudget,quantity:r.quantity,observedSurplus:r.observedOrdinarySurplus,worstPermittedSurplus:r.minimumPermittedOrdinarySurplus,zeroExceptionObservedThreshold:r.thresholdSensitivity[0].observedUniformLimits.completionProbabilityMustExceed.roundedUpToSixDecimals,zeroExceptionWorstThreshold:r.thresholdSensitivity[0].maximumPermittedLimits.completionProbabilityMustExceed.roundedUpToSixDecimals,onePercentExceptionWorstFeasible:r.thresholdSensitivity[2].maximumPermittedLimits.positiveExpectedReturnPossible})),null,2));
