import {historicalReference} from './history.mjs';

const median = values => {
  const a = [...values].sort((x,y)=>x-y), n = a.length;
  return n ? (n%2 ? a[(n-1)/2] : (a[n/2-1]+a[n/2])/2) : null;
};

// Diagnostic only: preserves the admitted legacy reference and does not change
// frozen wallets, actual orders or classify a falling price as an opportunity.
export function historicalReferenceQuality(history, books, at, policy) {
  const reference = historicalReference(history, books, at, policy), now = Date.parse(at);
  if (!reference.ready) return {reference, quality: null};
  const candles = reference.source === 'closed_trade_candle_vwap_median';
  const points = (candles ? history.candles : books || []).filter(x => {
    const t = Date.parse(candles ? x.time : x.at);
    return candles ? t+60000 <= now && t >= now-policy.lookbackMs
      : t < now && Date.parse(x.receivedAt) <= now && t >= now-policy.lookbackMs;
  }).map(x=>({at:Date.parse(candles ? x.time : x.at)+(candles?60000:0), price:candles?x.vwap:x.mid}));
  if (points.some(x=>!Number.isFinite(x.price)||x.price<0||x.price>1||!Number.isFinite(x.at))) throw Error('INVALID_REFERENCE_POINTS');
  points.sort((a,b)=>a.at-b.at);
  const minutes = new Map();
  for (const p of points) { const minute = Math.floor(p.at/60000); if (!minutes.has(minute)) minutes.set(minute,[]); minutes.get(minute).push(p.price); }
  const balanced = [...minutes].sort((a,b)=>a[0]-b[0]).map(([minute,prices])=>({minute,price:median(prices),count:prices.length}));
  const edge = Math.max(1,Math.ceil(balanced.length/4)), older = median(balanced.slice(0,edge).map(x=>x.price)), recent = median(balanced.slice(-edge).map(x=>x.price));
  return {reference, quality: {rawSamples:points.length, distinctMinutes:balanced.length,
    largestMinuteWeight:Math.max(...balanced.map(x=>x.count))/points.length,
    minuteBalancedCenter:median(balanced.map(x=>x.price)), firstQuartileCenter:older,
    lastQuartileCenter:recent, recentMinusOlder:recent-older,
    referenceMinusRecent:reference.center-recent, lastObservationAgeMs:now-points.at(-1).at,
    spanMs:points.at(-1).at-points[0].at,
    interpretation:'Descriptive causal history only; repeated observations are not independent trials and a discount is not a probability of recovery.'}};
}
