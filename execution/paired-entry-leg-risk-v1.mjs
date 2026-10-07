import {amount,money,price,SCALE,TICK} from '../paper/fixed.mjs';
// Compare both sequencing choices under full funding. This is a scenario
// calculation, not an estimated hedge probability or execution permission.
export function comparePairedEntryLegRiskV1({legs,quantity,maximumOrderCost='25.000000',exitStressTicks=1}){
 if(!Array.isArray(legs)||legs.length!==2||legs[0].exchangeId===legs[1].exchangeId||!Number.isSafeInteger(quantity)||quantity<1||quantity>100||!Number.isSafeInteger(exitStressTicks)||exitStressTicks<0||exitStressTicks>10)throw Error('PAIR_LEG_RISK_INPUT');
 const cap=amount(maximumOrderCost);if(cap<=0n||cap>25000000n)throw Error('PAIR_LEG_RISK_CAP');
 const costs=legs.map(l=>price(l.noBuyLimit)*BigInt(quantity));if(costs.some(c=>c>cap))throw Error('PAIR_LEG_RISK_NOT_FUNDED');const scenarios=legs.map((first,i)=>{
  const other=legs[1-i],exit=price(first.noSellLimit)-BigInt(exitStressTicks)*TICK;if(exit<TICK)throw Error('PAIR_LEG_RISK_EXIT_LIMIT');
  const loss=costs[i]-exit*BigInt(quantity),surplus=SCALE*BigInt(quantity)-costs[0]-costs[1];
  return{firstExchangeId:first.exchangeId,hedgeExchangeId:other.exchangeId,firstCost:money(costs[i]),hedgeCost:money(costs[1-i]),fullFunding:money(costs[0]+costs[1]),conditionalOrdinarySurplusIfBothFill:money(surplus),stressedFirstLegLiquidationPnlIfHedgeFails:money(-loss),minimumHedgeCompletionProbabilityForScenarioBreakEven:surplus>0n&&loss>0n?Number(loss)/Number(loss+surplus):surplus>0n?0:null,probabilityEstimated:false,unconditionalSettlementGuarantee:false};
 });
 return{schemaVersion:1,kind:'paired_entry_sequencing_scenario_v1',quantity,scenarios,preferLowerUnhedgedLossExchangeId:[...scenarios].sort((a,b)=>amount(a.stressedFirstLegLiquidationPnlIfHedgeFails)>amount(b.stressedFirstLegLiquidationPnlIfHedgeFails)?-1:1)[0].firstExchangeId,executionAuthority:false};
}

