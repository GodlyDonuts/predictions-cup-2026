// These are the fixed margins used by the prospective ML paper runner. This is
// a descriptive development check, not another search over profitable cutoffs.
export const PAPER_EDGE_MARGINS = Object.freeze([0.005, 0.01, 0.015]);

export function netEdgeDiagnostics(rows, predictions, margins = PAPER_EDGE_MARGINS) {
  if (!Array.isArray(rows) || !Array.isArray(predictions) || rows.length !== predictions.length
      || predictions.some(x => !Number.isFinite(x)) || rows.some(r => !Number.isFinite(r?.y))
      || !Array.isArray(margins) || !margins.length || new Set(margins).size !== margins.length
      || margins.some(x => !Number.isFinite(x) || x <= 0)) throw Error('Invalid net-edge diagnostics');
  return {
    version: 'conditional-development-net-edge-v1',
    completedLabelCount: rows.length,
    margins: margins.map(margin => {
      const indices = predictions.flatMap((p, i) => p >= margin ? [i] : []);
      const count = indices.length;
      let observed = 0, predicted = 0, wins = 0, worst = Infinity;
      for (const i of indices) {
        observed += rows[i].y; predicted += predictions[i];
        if (rows[i].y > 0) wins++;
        worst = Math.min(worst, rows[i].y);
      }
      return {margin, decisions: count,
        meanPredictedNetPerShare: count ? predicted / count : null,
        meanObservedNetPerShare: count ? observed / count : null,
        positiveOutcomeFraction: count ? wins / count : null,
        worstNetPerShare: count ? worst : null};
    }),
    portfolioPnl: null,
    profitabilityEstablished: false,
    limitation: 'Overlapping flat-wallet probes conditional on completed labels; excludes censored/no-fill decisions. Development data are reused for model selection, not untouched forward validation. Fees are already included in the target and are not subtracted again.',
  };
}
