import {amount, money, price, SCALE, uint, instant} from '../../paper/fixed.mjs';
import {referenceKellySizingV1} from '../../paper/reference-kelly-sizing-v1.mjs';

// Shadow settlement-risk screen. Scenario values are assumptions, never
// calibrated confidence bounds, and are not forecasts of a harvest exit price.
export function stressCapitalScreen({sizing, scenarios, auditQuantity, evidence}) {
  uint(auditQuantity, 'auditQuantity');
  if (!Array.isArray(scenarios) || scenarios.length === 0) throw Error('SCENARIOS_REQUIRED');
  const names = new Set(), unit = price(sizing.protectedPrice);
  const rows = scenarios.map(scenario => {
    if (typeof scenario.id !== 'string' || !scenario.id || names.has(scenario.id)) throw Error('SCENARIO_ID');
    names.add(scenario.id);
    const value = amount(scenario.value);
    if (value < 0n || value > SCALE) throw Error('SCENARIO_VALUE');
    const result = referenceKellySizingV1({...sizing, referenceValue:scenario.value});
    return {id:scenario.id, assumedValue:money(value), edgePerShare:money(value-unit),
      conditionalExpectedSettlementSurplus:money(BigInt(auditQuantity)*(value-unit)),
      fundedQuantity:result.quantity, maximumCost:result.maximumCost, sizingReason:result.reason};
  });
  const worst = rows.reduce((a,b) => amount(a.assumedValue) <= amount(b.assumedValue) ? a : b);
  const blockers = [];
  const auditAt = instant(evidence.auditAt), bookAt = instant(evidence.bookAt);
  uint(evidence.maxBookAgeMs, 'maxBookAgeMs');
  const age = auditAt-bookAt;
  if (age < 0 || age > evidence.maxBookAgeMs) blockers.push('NO_FRESH_EXECUTABLE_BOOK');
  if (evidence.sourceCensusComplete !== true) blockers.push('INCOMPLETE_SOURCE_CENSUS');
  if (evidence.populationTransportValidated !== true) blockers.push('UNVALIDATED_POPULATION_TRANSPORT');
  if (evidence.feeScheduleVerified !== true) blockers.push('UNVERIFIED_FEES');
  if (evidence.independentPerformanceValidated !== true) blockers.push('NO_INDEPENDENT_EXECUTION_PERFORMANCE');
  return {schemaVersion:1, mode:'shadow_settlement_capital_stress', executionAuthority:false, actualOrders:0,
    auditQuantity, auditedPrincipal:money(BigInt(auditQuantity)*unit), maximumSettlementLoss:money(BigInt(auditQuantity)*unit),
    allWinSettlementSurplus:money(BigInt(auditQuantity)*(SCALE-unit)), scenarios:rows,
    worstScenarioId:worst.id, stressFundedQuantity:Math.min(...rows.map(r=>r.fundedQuantity)),
    evidenceQualifiedShadowQuantity:blockers.length ? 0 : Math.min(...rows.map(r=>r.fundedQuantity)),
    evidenceBlockers:blockers, bookAgeMs:age, scenariosAreNotConfidenceBounds:true,
    harvestExitProfitInferred:false, historicalProfitabilityEstablished:false};
}
