// Leave room for one account-extra read, positions/orders, two paired reads,
// four protected ordinary books, and two spare admissions. The API queue still
// enforces its persistent rolling limit and Retry-After independently.
export function pairedReadCapacityDelayV1(timings, now) {
  if (!Array.isArray(timings) || !Number.isSafeInteger(now)) throw Error('PAIRED_CAPACITY_INPUT');
  const recent = timings.filter(x => x.admittedAt).map(x => Date.parse(x.admittedAt));
  if (recent.some(t => !Number.isSafeInteger(t) || t > now)) throw Error('PAIRED_CAPACITY_CLOCK');
  const active = recent.filter(t => now - t <= 60000).sort((a,b) => a-b);
  if (active.length + 11 <= 60) return 0;
  return Math.max(1, active[active.length - 49 - 1] + 60001 - now);
}

