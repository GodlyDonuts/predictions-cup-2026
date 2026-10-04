import {performance, monitorEventLoopDelay} from 'node:perf_hooks';

// Durations use a monotonic clock; persisted dates and rate reservations keep
// their existing wall-clock contract. Fake wall clocks remain deterministic.
export function createLatencyClock({now = Date.now, monotonicNow, monitor = now === Date.now} = {}) {
  const tick = monotonicNow ?? (now === Date.now ? () => performance.now() : now);
  if (typeof now !== 'function' || typeof tick !== 'function' || typeof monitor !== 'boolean') {
    throw Error('Invalid latency clock');
  }
  const histogram = monitor ? monitorEventLoopDelay({resolution: 20}) : null;
  histogram?.enable();
  let closed = false;
  return {
    now: tick,
    source: monotonicNow !== undefined || now !== Date.now ? 'injected' : 'monotonic',
    snapshot() {
      const count = closed ? 0 : histogram?.count ?? 0;
      const result = {
        eventLoopSamples: count,
        eventLoopP99Ms: count ? histogram.percentile(99) / 1e6 : null,
        eventLoopMaxMs: count ? histogram.max / 1e6 : null,
      };
      if (!closed) histogram?.reset();
      return result;
    },
    close() {
      if (!closed) histogram?.disable();
      closed = true;
    },
  };
}
