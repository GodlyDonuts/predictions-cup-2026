import {PAPER_EDGE_MARGINS} from './execution-metrics.mjs';

export const PORTFOLIO_PLAN = 'sequential-portfolio-selection-v1';

// Old banks retain their original three independent paper margins. New plans
// freeze a single model/margin or explicitly select the no-trade baseline.
export function validateCandidatePlan(bank) {
  if (bank.candidatePlan === undefined) return;
  const p = bank.candidatePlan;
  if (!p || p.version !== PORTFOLIO_PLAN || !Array.isArray(p.candidates)
      || p.candidates.length > 1 || !Array.isArray(bank.selected)
      || bank.selected.length !== p.candidates.length
      || bank.targetPolicy !== 'delayed_fifo_roundtrip_net_v1'
      || bank.forecastLifecycle !== 'revoke-on-invalidation-v2') throw Error('Invalid frozen candidate plan');
  if (p.candidates.length) {
    const c = p.candidates[0];
    if (!c || typeof c.modelId !== 'string' || c.modelId !== bank.selected[0]?.id
        || !PAPER_EDGE_MARGINS.includes(c.margin)) throw Error('Invalid frozen candidate margin');
  }
}

export function modelMargins(bank, model) {
  return bank.candidatePlan === undefined ? PAPER_EDGE_MARGINS
    : bank.candidatePlan.candidates.filter(c => c.modelId === model.id).map(c => c.margin);
}
