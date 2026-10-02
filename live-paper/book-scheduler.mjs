import {createHash} from 'node:crypto';

// Scheduling authorizes reads only. The caller must still apply reconciliation,
// freshness, depth, signal and risk checks, including the entry capacity below.
const id = value => typeof value === 'string' && /^[1-9]\d*$/.test(value) && Number.isSafeInteger(Number(value));
const unique = values => [...new Set(values)];
const canonicalClock = value => typeof value === 'string' && Number.isFinite(Date.parse(value)) && new Date(value).toISOString() === value;

function scopeOf(scope) {
  const universe = scope?.universe;
  if (typeof universe?.tournamentId !== 'string' || !universe.tournamentId || !Array.isArray(universe.exchanges) || !universe.exchanges.length) throw Error('Invalid scheduler universe');
  const exchanges = new Set(), markets = new Map();
  for (const spec of universe.exchanges) {
    if (!id(spec?.marketId) || !id(spec.exchangeId) || spec.option !== 'YES' || exchanges.has(spec.exchangeId) || markets.has(spec.marketId)) throw Error('Ambiguous scheduler market scope');
    exchanges.add(spec.exchangeId);
    markets.set(spec.marketId, {marketId: spec.marketId, exchangeId: spec.exchangeId, option: spec.option});
  }
  if (!Array.isArray(scope.coreMarketIds) || new Set(scope.coreMarketIds).size !== scope.coreMarketIds.length || scope.coreMarketIds.some(value => !id(value) || !markets.has(value))) throw Error('Invalid scheduler core scope');
  const marketIds = [...markets.keys()], core = new Set(scope.coreMarketIds);
  const scopeHash = createHash('sha256').update(JSON.stringify({tournamentId: universe.tournamentId, exchanges: [...markets.values()], coreMarketIds: [...core].sort()})).digest('hex');
  return {marketIds, markets, core, scopeHash};
}

function proposals(values, scope, label) {
  if (values === undefined) return [];
  if (!Array.isArray(values) || values.some(value => !id(value) || !scope.markets.has(value))) throw Error('Invalid or foreign scheduler ' + label);
  return unique(values);
}

function checkedState(state, scope) {
  if (state?.schemaVersion !== 1 || state.scopeHash !== scope.scopeHash) throw Error('Scheduler state scope mismatch');
  for (const kind of ['book', 'history']) {
    if (!Number.isSafeInteger(state[kind + 'Attempts']) || state[kind + 'Attempts'] < 0 || !Number.isSafeInteger(state[kind + 'Cursor']) || state[kind + 'Cursor'] < 0 || state[kind + 'Cursor'] >= scope.marketIds.length) throw Error('Invalid scheduler cursor');
    const last = state[kind + 'LastAttempt'];
    if (!last || typeof last !== 'object' || Array.isArray(last)) throw Error('Invalid scheduler attempt ledger');
    const ordinals = new Set();
    for (const [marketId, sequence] of Object.entries(last)) {
      if (!scope.markets.has(marketId) || !Number.isSafeInteger(sequence) || sequence < 1 || sequence > state[kind + 'Attempts'] || ordinals.has(sequence)) throw Error('Invalid scheduler attempt ledger');
      ordinals.add(sequence);
    }
  }
  if (state.historyLastAttemptAt !== undefined) {
    const clocks = state.historyLastAttemptAt;
    if (!clocks || typeof clocks !== 'object' || Array.isArray(clocks) || Object.entries(clocks).some(([marketId, at]) => !scope.markets.has(marketId) || state.historyLastAttempt[marketId] === undefined || !canonicalClock(at))) throw Error('Invalid scheduler attempt clock ledger');
  }
  return structuredClone(state);
}

function fairOrder(ids, state, scope, kind) {
  const cursor = state[kind + 'Cursor'], last = state[kind + 'LastAttempt'];
  const rank = new Map(scope.marketIds.map((marketId, index) => [marketId, (index - cursor + scope.marketIds.length) % scope.marketIds.length]));
  return unique(ids).sort((a, b) => (last[a] || 0) - (last[b] || 0) || rank.get(a) - rank.get(b));
}

function inputsOf(inputs, scope) {
  return Object.fromEntries(['priorityManagedMarketIds', 'heldMarketIds', 'pendingMarketIds', 'candidateMarketIds', 'excludedMarketIds', 'usableMarketIds'].map(key => [key, proposals(inputs[key], scope, key)]));
}

export function createSchedulerState(scope) {
  const {scopeHash} = scopeOf(scope);
  return {schemaVersion: 1, scopeHash, bookAttempts: 0, bookCursor: 0, bookLastAttempt: {}, historyAttempts: 0, historyCursor: 0, historyLastAttempt: {}};
}

export function validateSchedulerState(state, scope) {
  return checkedState(state, scopeOf(scope));
}

// Four full books per cycle is a hard upper bound. Three managed markets leaves
// one slot for comparison/discovery. With measured cycles near two minutes this
// is a capacity limit, not a promise that any stale-data deadline will be met.
export function planBookReads(state, rawScope, inputs = {}, {budget = 4, managedCapacity = 3, comparisonSlots = 2} = {}) {
  const scope = scopeOf(rawScope), copy = checkedState(state, scope), values = inputsOf(inputs, scope);
  if (!Number.isSafeInteger(budget) || budget < 1 || budget > 4 || !Number.isSafeInteger(managedCapacity) || managedCapacity < 0 || managedCapacity >= budget || !Number.isSafeInteger(comparisonSlots) || comparisonSlots < 0 || comparisonSlots > budget) throw Error('Invalid scheduler book budget or headroom');
  const priorityManagedMarketIds = values.priorityManagedMarketIds, priorityManaged = new Set(priorityManagedMarketIds);
  const managedMarketIds = unique([...priorityManagedMarketIds, ...values.heldMarketIds, ...values.pendingMarketIds]);
  const excluded = new Set(values.excludedMarketIds), managed = new Set(managedMarketIds), reads = [], selected = new Set();
  const add = (marketId, reason) => {
    if (reads.length >= budget || selected.has(marketId) || excluded.has(marketId)) return false;
    selected.add(marketId);
    reads.push({...scope.markets.get(marketId), path: scope.core.has(marketId) ? 'sourceSnapshots' : 'supplementalDepth', reason});
    return true;
  };
  // Actual obligations must keep their slots even when shadow wallets hold many
  // markets. Retain persisted fairness inside each group and share one budget.
  for (const marketId of fairOrder(priorityManagedMarketIds, copy, scope, 'book')) add(marketId, 'priority_held_or_pending');
  for (const marketId of fairOrder(managedMarketIds.filter(value => !priorityManaged.has(value)), copy, scope, 'book')) add(marketId, 'held_or_pending');
  const availableCapacity = Math.max(0, managedCapacity - managed.size), entryMarketIds = [];
  for (const marketId of fairOrder(values.candidateMarketIds.filter(value => !managed.has(value)), copy, scope, 'book')) {
    if (entryMarketIds.length >= availableCapacity) break;
    if (add(marketId, 'eligible_candidate')) entryMarketIds.push(marketId);
  }
  let comparisonCount = 0;
  for (const marketId of fairOrder([...scope.core], copy, scope, 'book')) {
    if (comparisonCount >= comparisonSlots) break;
    if (add(marketId, 'comparison')) comparisonCount++;
  }
  for (const marketId of fairOrder(scope.marketIds.filter(value => !scope.core.has(value)), copy, scope, 'book')) add(marketId, 'discovery');
  return {
    state: copy, reads, managedCapacity, managedMarketIds, priorityManagedMarketIds, availableCapacity, entryMarketIds,
    overCapacity: managed.size > managedCapacity,
    unserviceableManagedMarketIds: managedMarketIds.filter(value => excluded.has(value)),
    deferredCandidateMarketIds: values.candidateMarketIds.filter(value => !managed.has(value) && !entryMarketIds.includes(value)),
  };
}

// Refresh a bounded active set instead of sweeping every history past its TTL.
// Every fourth history read discovers a market outside that set. On other reads,
// oldest-attempt-first refresh keeps every member moving even when a request
// fails. Callers should supply the book plan's selected candidates, not all leads.
function historyAllocation(raw, scope) {
  const fields = ['marketId', 'referenceReady', 'validUntil', 'receivedAt', 'identityKnown', 'quotePlausible', 'opportunityGapMicros'];
  if (!raw || raw.schemaVersion !== 1 || !canonicalClock(raw.at) || !Array.isArray(raw.markets) || raw.markets.length !== scope.marketIds.length || Object.keys(raw).some(key => !['schemaVersion', 'at', 'markets'].includes(key))) throw Error('Invalid history allocation');
  const now = Date.parse(raw.at), rows = new Map();
  for (const row of raw.markets) {
    if (!row || typeof row !== 'object' || Array.isArray(row) || Object.keys(row).length !== fields.length || Object.keys(row).some(key => !fields.includes(key)) || !scope.markets.has(row.marketId) || rows.has(row.marketId)
      || ['referenceReady', 'identityKnown', 'quotePlausible'].some(key => typeof row[key] !== 'boolean')
      || row.receivedAt !== null && (!canonicalClock(row.receivedAt) || Date.parse(row.receivedAt) > now)
      || row.validUntil !== null && !canonicalClock(row.validUntil)
      || row.opportunityGapMicros !== null && (!Number.isSafeInteger(row.opportunityGapMicros) || Math.abs(row.opportunityGapMicros) > 1000000)) throw Error('Invalid or foreign history allocation row');
    rows.set(row.marketId, {...row});
  }
  return {at: raw.at, now, rows};
}

function planAllocatedHistory(copy, rawScope, scope, values, raw, {budget, activeSetLimit, discoveryEvery}) {
  const allocation = historyAllocation(raw, scope), excluded = new Set(values.excludedMarketIds), chosen = new Set();
  if (Object.values(copy.historyLastAttemptAt ?? {}).some(at => Date.parse(at) > allocation.now)) throw Error('History allocation clock precedes attempt');
  const managed = unique([...values.priorityManagedMarketIds, ...values.heldMarketIds, ...values.pendingMarketIds]);
  const due = marketId => {
    const row = allocation.rows.get(marketId);
    return !row.referenceReady || row.validUntil === null || Date.parse(row.validUntil) <= allocation.now + 600000;
  };
  const receiptCooling = marketId => {
    const row = allocation.rows.get(marketId);
    // A successful response with no usable reference is not evidence that an
    // immediate second read can help. Missing receipts still get initial reads.
    return !row.referenceReady && row.receivedAt !== null && allocation.now - Date.parse(row.receivedAt) < 600000;
  };
  const repeatCooling = (marketId, state) => {
    const at = state.historyLastAttemptAt?.[marketId];
    // Eight intervening attempts OR ten elapsed minutes permits a retry. The
    // clock escape prevents a small due pool deadlocking its own attempt count.
    // Legacy records with no attempt clock get one retry; actual admission then
    // persists its clock. Never invent a successful receipt from an attempt.
    return at !== undefined && state.historyAttempts - state.historyLastAttempt[marketId] < 8 && allocation.now - Date.parse(at) < 600000;
  };
  const eligible = (marketId, state) => !excluded.has(marketId) && !chosen.has(marketId) && due(marketId) && !receiptCooling(marketId) && !repeatCooling(marketId, state);
  const knownOrder = (marketIds, state) => {
    const fair = fairOrder(marketIds, state, scope, 'history'), rank = new Map(fair.map((marketId, index) => [marketId, index]));
    return fair.sort((a, b) => {
      const x = allocation.rows.get(a), y = allocation.rows.get(b);
      return Number(y.quotePlausible) - Number(x.quotePlausible)
        || (x.quotePlausible ? (x.opportunityGapMicros ?? 1000001) - (y.opportunityGapMicros ?? 1000001) : 0)
        || rank.get(a) - rank.get(b);
    });
  };
  const knownDue = scope.marketIds.filter(marketId => allocation.rows.get(marketId).identityKnown && due(marketId) && !excluded.has(marketId));
  // This bounded display describes risk/candidate/due attention, not eight
  // unconditional refresh reservations. Fresh members never force a read.
  const activeMarketIds = unique([
    ...fairOrder(values.priorityManagedMarketIds, copy, scope, 'history'),
    ...fairOrder(managed, copy, scope, 'history'),
    ...fairOrder(values.candidateMarketIds, copy, scope, 'history'),
    ...knownOrder(knownDue, copy),
  ]).filter(marketId => !excluded.has(marketId)).slice(0, activeSetLimit);
  const reads = []; let simulated = copy;
  for (let slot = 0; slot < budget; slot++) {
    const global = fairOrder(scope.marketIds.filter(marketId => eligible(marketId, simulated)), simulated, scope, 'history');
    if (!global.length) break;
    const reservedDiscovery = (simulated.historyAttempts + 1) % discoveryEvery === 0;
    const groups = reservedDiscovery ? [] : [
      {marketIds: fairOrder(values.priorityManagedMarketIds, simulated, scope, 'history'), reason: 'due_official_history', priority: 0},
      {marketIds: fairOrder(managed, simulated, scope, 'history'), reason: 'due_managed_history', priority: 1},
      {marketIds: fairOrder(values.candidateMarketIds, simulated, scope, 'history'), reason: 'due_candidate_history', priority: 2},
      {marketIds: knownOrder(knownDue, simulated), reason: 'due_known_history', priority: 3},
    ];
    groups.push({marketIds: global, reason: 'fair_history_discovery', priority: 4});
    let selection;
    for (const group of groups) {
      const marketId = group.marketIds.find(value => eligible(value, simulated));
      if (marketId) { selection = {marketId, reason: group.reason, priority: group.priority}; break; }
    }
    if (!selection) break;
    const {marketId, reason, priority} = selection, row = allocation.rows.get(marketId);
    chosen.add(marketId);
    reads.push({...scope.markets.get(marketId), reason, priority, reservedDiscovery, referenceReady: row.referenceReady, validUntil: row.validUntil, opportunityGapMicros: row.opportunityGapMicros});
    simulated = recordSchedulerAttempts(simulated, rawScope, {historyMarketIds: [marketId], attemptedAt: allocation.at});
  }
  return {state: copy, reads, activeMarketIds, deferredManagedMarketIds: managed.filter(value => !activeMarketIds.includes(value)),
    allocationMode: 'due_aware_v1', allocationAt: allocation.at,
    dueMarketCount: scope.marketIds.filter(value => !excluded.has(value) && due(value)).length,
    receiptCooldownMarketIds: scope.marketIds.filter(value => !excluded.has(value) && due(value) && receiptCooling(value)),
    attemptCooldownMarketIds: scope.marketIds.filter(value => !excluded.has(value) && due(value) && repeatCooling(value, copy))};
}

export function planHistoryReads(state, rawScope, inputs = {}, {budget = 1, activeSetLimit = 8, discoveryEvery = 4} = {}) {
  const scope = scopeOf(rawScope), copy = checkedState(state, scope), values = inputsOf(inputs, scope);
  if (!Number.isSafeInteger(budget) || budget < 1 || budget > 2 || !Number.isSafeInteger(activeSetLimit) || activeSetLimit < 1 || activeSetLimit > 8 || !Number.isSafeInteger(discoveryEvery) || discoveryEvery < 2 || discoveryEvery > 8) throw Error('Invalid scheduler history budget');
  if (inputs.historyAllocation !== undefined) return planAllocatedHistory(copy, rawScope, scope, values, inputs.historyAllocation, {budget, activeSetLimit, discoveryEvery});
  const excluded = new Set(values.excludedMarketIds), managed = unique([...values.priorityManagedMarketIds, ...values.heldMarketIds, ...values.pendingMarketIds]);
  const priority = unique([
    ...fairOrder(values.priorityManagedMarketIds, copy, scope, 'history'),
    ...fairOrder(managed, copy, scope, 'history'),
    ...fairOrder(values.candidateMarketIds, copy, scope, 'book'),
    // Stable universe order keeps the retained usable set bounded across reads.
    ...scope.marketIds.filter(value => values.usableMarketIds.includes(value)),
  ]).filter(value => !excluded.has(value));
  const activeMarketIds = priority.slice(0, activeSetLimit), active = new Set(activeMarketIds), reads = [], chosen = new Set();
  let simulated = copy;
  for (let slot = 0; slot < budget; slot++) {
    const discover = !active.size || (simulated.historyAttempts + 1) % discoveryEvery === 0;
    const discovery = fairOrder(scope.marketIds.filter(value => !active.has(value) && !excluded.has(value)), simulated, scope, 'history');
    const refresh = fairOrder(activeMarketIds, simulated, scope, 'history');
    const marketId = (discover ? [...discovery, ...refresh] : [...refresh, ...discovery]).find(value => !chosen.has(value));
    if (!marketId) break;
    chosen.add(marketId);
    reads.push({...scope.markets.get(marketId), reason: active.has(marketId) ? 'active_history_refresh' : 'history_discovery'});
    simulated = recordSchedulerAttempts(simulated, rawScope, {historyMarketIds: [marketId]});
  }
  return {state: copy, reads, activeMarketIds, deferredManagedMarketIds: managed.filter(value => !active.has(value))};
}

// Persist this returned state immediately before each actual request attempt.
// Do not record a planned but unattempted read. A timeout or rejected response is
// still an attempt; otherwise one failing market can monopolize the rotation.
export function recordSchedulerAttempts(state, rawScope, {bookMarketIds = [], historyMarketIds = [], attemptedAt} = {}) {
  const scope = scopeOf(rawScope), copy = checkedState(state, scope);
  if (attemptedAt !== undefined && !canonicalClock(attemptedAt)) throw Error('Invalid scheduler attempt clock');
  for (const [kind, attempted] of [['book', bookMarketIds], ['history', historyMarketIds]]) {
    const ids = proposals(attempted, scope, kind + ' attempts');
    if (ids.length !== attempted.length) throw Error('Duplicate scheduler attempts');
    for (const marketId of ids) {
      const sequence = copy[kind + 'Attempts'] + 1;
      if (!Number.isSafeInteger(sequence)) throw Error('Scheduler attempt counter exhausted');
      copy[kind + 'Attempts'] = sequence;
      copy[kind + 'LastAttempt'][marketId] = sequence;
      copy[kind + 'Cursor'] = (scope.marketIds.indexOf(marketId) + 1) % scope.marketIds.length;
      if (kind === 'history') {
        if (attemptedAt !== undefined) (copy.historyLastAttemptAt ??= {})[marketId] = attemptedAt;
        else if (copy.historyLastAttemptAt) delete copy.historyLastAttemptAt[marketId];
      }
    }
  }
  return copy;
}
