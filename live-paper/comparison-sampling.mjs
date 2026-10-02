import {createHash} from 'node:crypto';
import {validateSchedulerState} from './book-scheduler.mjs';

// Operator collection treatment, independent of frozen strategy implementations.
// Two attempts per pair improve short follow-up clocks while retaining rotation.
// They do NOT promise independent engine versions, three-stage shock fills, or
// continuous outcome coverage. Confirmed pending/held risks remain host inputs.
export const COMPARISON_SAMPLING = Object.freeze({mode: 'paired_comparison_two_attempts_v1', attemptsPerMarket: 2,
  carryMs: 140000, panel: Object.freeze(['377', '354', '277', '305'])});
const hash = x => createHash('sha256').update(JSON.stringify(x)).digest('hex');
const canonical = x => typeof x === 'string' && Number.isFinite(Date.parse(x)) && new Date(x).toISOString() === x;
const need = (ok, message) => { if (!ok) throw Error(message); };
const object = x => x !== null && typeof x === 'object' && !Array.isArray(x);
const uuid = x => typeof x === 'string' && /^[a-f0-9]{8}(?:-[a-f0-9]{4}){3}-[a-f0-9]{12}$/.test(x);
const planHash = p => hash({sequence: p.sequence, sessionId: p.sessionId, at: p.at, phase: p.phase, phaseHash: p.phaseHash, marketIds: p.marketIds});

function context(input, scope) {
  const state = validateSchedulerState(input, scope), panel = [...COMPARISON_SAMPLING.panel];
  need(Array.isArray(scope.coreMarketIds) && scope.coreMarketIds.length === 4
    && panel.every(id => scope.coreMarketIds.includes(id)), 'Comparison sampling requires the fixed four-market panel');
  const markets = new Map(scope.universe.exchanges.map(x => [x.marketId, x]));
  let ledger = state.comparisonSampling;
  if (ledger === undefined) ledger = {schemaVersion: 1, mode: COMPARISON_SAMPLING.mode, scopeHash: state.scopeHash,
    phase: 0, cursor: 0, totalAttempts: 0, phaseAttemptStart: 0, selectedMarketIds: [], remainingAttempts: {},
    startedAt: null, expiresAt: null, lastAttemptAt: {}, lastPhase: null, sessionId: null, plans: 0, lastPlan: null};
  need(object(ledger) && ledger.schemaVersion === 1 && ledger.mode === COMPARISON_SAMPLING.mode && ledger.scopeHash === state.scopeHash,
    'Invalid comparison sampling identity');
  for (const key of ['phase', 'cursor', 'totalAttempts', 'phaseAttemptStart', 'plans']) need(Number.isSafeInteger(ledger[key]) && ledger[key] >= 0, 'Invalid comparison sampling counter');
  need(ledger.sessionId === null || uuid(ledger.sessionId), 'Invalid comparison session');
  need(ledger.cursor < panel.length && ledger.phaseAttemptStart <= ledger.totalAttempts
    && Array.isArray(ledger.selectedMarketIds) && [0, 2].includes(ledger.selectedMarketIds.length)
    && new Set(ledger.selectedMarketIds).size === ledger.selectedMarketIds.length
    && ledger.selectedMarketIds.every(id => panel.includes(id)) && object(ledger.remainingAttempts) && object(ledger.lastAttemptAt), 'Invalid comparison sampling phase');
  need(Object.keys(ledger.remainingAttempts).length === ledger.selectedMarketIds.length
    && Object.keys(ledger.remainingAttempts).every(id => ledger.selectedMarketIds.includes(id)), 'Invalid comparison remaining scope');
  for (const id of ledger.selectedMarketIds) need(Number.isSafeInteger(ledger.remainingAttempts[id]) && ledger.remainingAttempts[id] >= 0
    && ledger.remainingAttempts[id] <= 2, 'Invalid comparison remaining attempts');
  for (const [id, at] of Object.entries(ledger.lastAttemptAt)) need(panel.includes(id) && canonical(at), 'Invalid comparison attempt clock');
  if (ledger.selectedMarketIds.length) {
    need(ledger.phase > 0 && canonical(ledger.startedAt) && canonical(ledger.expiresAt)
      && Date.parse(ledger.expiresAt) - Date.parse(ledger.startedAt) === COMPARISON_SAMPLING.carryMs
      && ledger.totalAttempts - ledger.phaseAttemptStart === 4 - Object.values(ledger.remainingAttempts).reduce((a, n) => a + n, 0), 'Invalid comparison phase lineage');
  } else need(ledger.startedAt === null && ledger.expiresAt === null && ledger.phaseAttemptStart === ledger.totalAttempts, 'Invalid idle comparison phase');
  if (ledger.lastPhase !== null) need(object(ledger.lastPhase) && Number.isSafeInteger(ledger.lastPhase.phase)
    && ledger.lastPhase.phase > 0 && ledger.lastPhase.phase <= ledger.phase && ['completed', 'expired', 'collector_restart'].includes(ledger.lastPhase.reason)
    && canonical(ledger.lastPhase.at) && Array.isArray(ledger.lastPhase.marketIds) && ledger.lastPhase.marketIds.length === 2
    && new Set(ledger.lastPhase.marketIds).size === 2 && ledger.lastPhase.marketIds.every(id => panel.includes(id)), 'Invalid prior comparison phase');
  if (ledger.lastPlan !== null) {
    const p = ledger.lastPlan;
    need(object(p) && p.sequence === ledger.plans && p.sessionId === ledger.sessionId && canonical(p.at)
      && Number.isSafeInteger(p.phase) && p.phase <= ledger.phase && Array.isArray(p.marketIds) && p.marketIds.length <= 2
      && new Set(p.marketIds).size === p.marketIds.length && p.marketIds.every(id => panel.includes(id))
      && Array.isArray(p.attemptedMarketIds) && new Set(p.attemptedMarketIds).size === p.attemptedMarketIds.length
      && p.attemptedMarketIds.every(id => p.marketIds.includes(id)) && p.planId === planHash(p), 'Invalid comparison reservation plan');
  }
  state.comparisonSampling = structuredClone(ledger);
  return {state, ledger: state.comparisonSampling, panel, markets};
}

function finishPhase(ledger, reason, at) {
  ledger.lastPhase = {phase: ledger.phase, reason, at, marketIds: [...ledger.selectedMarketIds],
    remainingAttempts: {...ledger.remainingAttempts}};
  ledger.selectedMarketIds = []; ledger.remainingAttempts = {}; ledger.startedAt = null; ledger.expiresAt = null;
  ledger.phaseAttemptStart = ledger.totalAttempts;
}

function checkClock(ledger, now) {
  need(Number.isSafeInteger(now) && now >= 0 && now <= 8640000000000000, 'Invalid comparison sampling clock');
  const clocks = [...Object.values(ledger.lastAttemptAt), ledger.startedAt, ledger.lastPhase?.at, ledger.lastPlan?.at].filter(Boolean);
  need(clocks.every(at => Date.parse(at) <= now), 'Comparison sampling clock rollback');
}

export function planComparisonSampling(basePlan, scope, {now, sessionId} = {}) {
  const {state, ledger, panel, markets} = context(basePlan?.state, scope); checkClock(ledger, now);
  need(uuid(sessionId), 'Invalid comparison collector session');
  need(Array.isArray(basePlan.reads) && basePlan.reads.length <= 4, 'Invalid comparison read budget');
  // Basket overlay may reuse a comparison read without changing its reason.
  // Protect its selected leg identities as well as explicit protected reasons.
  const basketIds = basePlan.basketMeasurement?.marketIds ?? [];
  need(Array.isArray(basketIds) && new Set(basketIds).size === basketIds.length
    && basketIds.every(id => markets.has(id) && basePlan.reads.some(r => r.marketId === id)), 'Invalid basket reservation scope');
  const seen = new Set(), slots = [], protectedIds = new Set();
  for (const [index, read] of basePlan.reads.entries()) {
    need(markets.has(read.marketId) && markets.get(read.marketId).exchangeId === read.exchangeId && !seen.has(read.marketId)
      && read.path === (panel.includes(read.marketId) ? 'sourceSnapshots' : 'supplementalDepth'), 'Invalid comparison base read');
    seen.add(read.marketId);
    if (read.reason === 'comparison' && !basketIds.includes(read.marketId)) { need(panel.includes(read.marketId), 'Foreign comparison slot'); slots.push(index); }
    else protectedIds.add(read.marketId);
  }
  need(slots.length <= 2, 'Comparison sampling has only two replaceable slots');
  const output = {...structuredClone(basePlan), state};
  let expired = false, restarted = false;
  if (ledger.sessionId !== null && ledger.sessionId !== sessionId) {
    if (ledger.selectedMarketIds.length) finishPhase(ledger, 'collector_restart', new Date(now).toISOString());
    ledger.lastPlan = null; restarted = true;
  }
  ledger.sessionId = sessionId;
  if (ledger.selectedMarketIds.length && now >= Date.parse(ledger.expiresAt)) { finishPhase(ledger, 'expired', new Date(now).toISOString()); expired = true; }
  if (!ledger.selectedMarketIds.length && slots.length >= 2) {
    const order = ledger.phase === 0 ? slots.map(i => basePlan.reads[i].marketId)
      : panel.map((_, i) => panel[(ledger.cursor + i) % panel.length]);
    const selected = order.filter(id => !protectedIds.has(id)).slice(0, 2);
    if (selected.length === 2) {
      need(Number.isSafeInteger(ledger.phase + 1), 'Comparison phase exhausted');
      ledger.phase++; ledger.selectedMarketIds = selected; ledger.remainingAttempts = Object.fromEntries(selected.map(id => [id, 2]));
      ledger.startedAt = new Date(now).toISOString(); ledger.expiresAt = new Date(now + COMPARISON_SAMPLING.carryMs).toISOString();
      ledger.phaseAttemptStart = ledger.totalAttempts; ledger.cursor = (panel.indexOf(selected.at(-1)) + 1) % panel.length;
    }
  }
  // A partial cycle cannot reserve pass two while another member still needs
  // pass one. The host freezes one plan and reserves each member at most once.
  const passRemaining = Math.max(0, ...Object.values(ledger.remainingAttempts));
  const passMembers = ledger.selectedMarketIds.filter(id => ledger.remainingAttempts[id] === passRemaining && passRemaining > 0);
  const required = passMembers.filter(id => !protectedIds.has(id));
  const assigned = new Set(protectedIds), order = [...required, ...slots.map(i => basePlan.reads[i].marketId),
    ...panel.map((_, i) => panel[(ledger.cursor + i) % panel.length])];
  for (const index of slots) {
    const marketId = order.find(id => !assigned.has(id));
    need(marketId, 'Comparison slot could not remain unique'); assigned.add(marketId);
    const original = basePlan.reads[index], spec = markets.get(marketId);
    output.reads[index] = marketId === original.marketId ? structuredClone(original)
      : {...structuredClone(original), marketId, exchangeId: spec.exchangeId, option: spec.option, path: 'sourceSnapshots'};
  }
  const phaseIdentity = ledger.selectedMarketIds.length ? {scopeHash: ledger.scopeHash, mode: ledger.mode, phase: ledger.phase,
    marketIds: [...ledger.selectedMarketIds], startedAt: ledger.startedAt, expiresAt: ledger.expiresAt} : null;
  need(Number.isSafeInteger(ledger.plans + 1), 'Comparison plans exhausted');
  const reservationMarketIds = output.reads.filter(r => passMembers.includes(r.marketId)).map(r => r.marketId);
  const plan = {sequence: ++ledger.plans, sessionId, at: new Date(now).toISOString(), phase: ledger.phase,
    phaseHash: phaseIdentity ? hash(phaseIdentity) : null, marketIds: reservationMarketIds, attemptedMarketIds: []};
  plan.planId = planHash(plan); ledger.lastPlan = plan;
  output.measurement = {mode: COMPARISON_SAMPLING.mode, reason: phaseIdentity ? 'paired_comparison_sampling' : 'insufficient_comparison_slots',
    expiredCarry: expired, abandonedOnRestart: restarted, phase: ledger.phase, phaseHash: phaseIdentity ? hash(phaseIdentity) : null, planId: plan.planId,
    marketIds: [...ledger.selectedMarketIds], remainingAttempts: {...ledger.remainingAttempts},
    startedAt: ledger.startedAt, expiresAt: ledger.expiresAt,
    reservationMarketIds,
    persistBeforeDispatch: true, attemptMeaning: 'durable_pre_request_reservation_not_confirmed_admission_or_receipt', requestsAdded: 0, outcomesGuaranteed: false};
  return output;
}

// Save/install with recordSchedulerAttempts immediately before requesting a read.
// This reserves, rather than proves, network admission: failure or STOP during
// the client's wait may consume it. Planning/omitted slots consume no attempts.
// A duplicate plan/member reservation throws before a second caller dispatch.
export function recordComparisonAttempt(input, scope, {marketId, at, planId} = {}) {
  const {state, ledger, markets} = context(input, scope);
  need(markets.has(marketId) && canonical(at), 'Invalid comparison attempt'); checkClock(ledger, Date.parse(at));
  need(ledger.lastPlan && planId === ledger.lastPlan.planId, 'Comparison reservation plan mismatch');
  if (!ledger.lastPlan.marketIds.includes(marketId)) return state;
  need(!ledger.lastPlan.attemptedMarketIds.includes(marketId), 'Duplicate comparison plan reservation');
  // Even an expired/abandoned carry cannot authorize the same frozen plan's
  // member twice. Keep this marker distinct from consumed phase attempts.
  ledger.lastPlan.attemptedMarketIds.push(marketId);
  if (!ledger.selectedMarketIds.length) return state;
  if (Date.parse(at) >= Date.parse(ledger.expiresAt)) { finishPhase(ledger, 'expired', at); return state; }
  if (!ledger.selectedMarketIds.includes(marketId) || ledger.remainingAttempts[marketId] === 0) return state;
  need(Number.isSafeInteger(ledger.totalAttempts + 1), 'Comparison attempts exhausted');
  ledger.remainingAttempts[marketId]--; ledger.totalAttempts++; ledger.lastAttemptAt[marketId] = at;
  if (Object.values(ledger.remainingAttempts).every(n => n === 0)) finishPhase(ledger, 'completed', at);
  return state;
}
