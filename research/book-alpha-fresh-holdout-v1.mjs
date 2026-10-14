import {BOOK_ALPHA_POLICY, causalBookLabels, fitBookAlpha, scoreBookAlpha} from './book-alpha-v1.mjs';

export const BOOK_ALPHA_FRESH_CUTOFF = '2026-10-09T00:00:00.000Z';
const need = (value, code) => { if (!value) throw Error(`BOOK_ALPHA_FRESH_${code}`); };

export function splitBookAlphaLabelsV1(labels, cutoff = BOOK_ALPHA_FRESH_CUTOFF) {
  const at = Date.parse(cutoff);
  need(Number.isFinite(at), 'CUTOFF');
  const training = [], purged = [], holdout = [];
  for (const row of labels) {
    if (row.at >= at) holdout.push(row);
    else if (row.labelAt <= at) training.push(row);
    else purged.push(row);
  }
  return {training, purged, holdout};
}

export function verifyPinnedBookPrefixV1({source, before, after, beforeHash, afterHash}) {
  need(source?.kind === 'sealed_public_books_prefix' && Number.isSafeInteger(source.bytes) && source.bytes > 0, 'SOURCE');
  need(/^[a-f0-9]{64}$/.test(source.sha256 ?? ''), 'SOURCE_HASH');
  need(before && after && Number.isSafeInteger(before.size) && Number.isSafeInteger(after.size), 'SOURCE_STAT');
  need(before.dev === after.dev && before.ino === after.ino, 'SOURCE_IDENTITY');
  need(before.size >= source.bytes && after.size >= source.bytes, 'SOURCE_PREFIX_SHORT');
  need(beforeHash === source.sha256 && afterHash === source.sha256, 'SOURCE_PREFIX_HASH');
  return {prefixBytes: source.bytes, prefixSha256: source.sha256, identityStable: true, prefixHashStable: true,
    bytesAtStart: before.size, bytesAtEnd: after.size};
}

function mean(values) { return values.length ? values.reduce((sum, value) => sum + value, 0) / values.length : null; }
function quantile(values, q) {
  if (!values.length) return null;
  const sorted = [...values].sort((a, b) => a - b), index = (sorted.length - 1) * q;
  const lower = Math.floor(index), fraction = index - lower;
  return sorted[lower] + (sorted[Math.min(lower + 1, sorted.length - 1)] - sorted[lower]) * fraction;
}

export function scoreBookAlphaFreshHoldoutV1(streams) {
  const decisions = streams.flat(), cutoff = Date.parse(BOOK_ALPHA_FRESH_CUTOFF), results = [];
  for (const horizonMs of BOOK_ALPHA_POLICY.horizonsMs) {
    const labels = causalBookLabels(streams, horizonMs, BOOK_ALPHA_POLICY);
    const split = splitBookAlphaLabelsV1(labels);
    try {
      const model = fitBookAlpha(labels, cutoff, BOOK_ALPHA_POLICY);
      need(model.trainLabels === split.training.length, 'TRAINING_BOUNDARY');
      const score = scoreBookAlpha(model, labels, decisions);
      const measured = score.selected.filter(row => row.oneTickPerSideMargin !== null);
      const rawMargins = measured.map(row => row.observedRawQuoteMargin);
      const stressedMargins = measured.map(row => row.oneTickPerSideMargin);
      const markets = new Set(measured.map(row => row.marketId));
      results.push({
        horizonMs, labels: labels.length, trainingLabels: split.training.length,
        purgedTrainingLabels: split.purged.length, heldOutLabels: split.holdout.length,
        holdoutMarkets: new Set(split.holdout.map(row => row.marketId)).size,
        laterDecisionObservations: score.laterDecisionObservations,
        modelRmse: score.rmse, unchangedBidRmse: score.unchangedBidRmse,
        rmseImprovement: score.unchangedBidRmse > 0 ? 1 - score.rmse / score.unchangedBidRmse : null,
        selectedQuoteSignals: score.selectedQuoteSignals,
        censoredSelectedSignals: score.censoredSelectedSignals,
        measuredSelectedSignals: measured.length, selectedSignalMarkets: markets.size,
        positiveRawMargins: rawMargins.filter(value => value > 0).length,
        positiveAfterOneTickPerSide: stressedMargins.filter(value => value > 0).length,
        meanRawMargin: mean(rawMargins), medianRawMargin: quantile(rawMargins, 0.5),
        meanOneTickPerSideMargin: mean(stressedMargins), medianOneTickPerSideMargin: quantile(stressedMargins, 0.5),
        model: {mean: model.mean, scale: model.scale, coefficients: model.coefficients},
      });
    } catch (error) {
      if (!String(error).includes('INSUFFICIENT_CHRONOLOGY')) throw error;
      results.push({horizonMs, labels: labels.length, trainingLabels: split.training.length,
        purgedTrainingLabels: split.purged.length, heldOutLabels: split.holdout.length,
        unavailable: 'insufficient_chronological_labels'});
    }
  }
  return {cutoff: BOOK_ALPHA_FRESH_CUTOFF, policy: BOOK_ALPHA_POLICY, results,
    actualFillsProven: false, realizedPnl: null, profitabilityEstablished: false};
}
