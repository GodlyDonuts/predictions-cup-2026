// Offline planner, not an executor or another account limiter. The caller must
// persist beginMakerDispatchV1 BEFORE using the existing serialized transport,
// then reconcile acceptance and all cancel/fill races before finishing it.
import {amount, price, instant} from '../paper/fixed.mjs';
import {payloadHash} from './ledger.mjs';

export const MAKER_SCHEDULER_POLICY_V1 = Object.freeze({
  maximumAccountAgeMs: 20000, maximumBookAgeMs: 10000,
  minimumQuoteAgeMs: 2500, minimumRepriceTicks: 1,
  minimumNetEntryEdge: '0.005000', maximumOpenQuotes: 8, maximumSubmitBatch: 8,
});
const need = (v, code) => { if (!v) throw Error('MAKER_SCHEDULER_' + code); };
const active = new Set(['open', 'partial']);
const terminal = new Set(['filled', 'canceled', 'expired', 'rejected']);
const slot = q => `${q.exchangeId}:${q.side}:${q.action}`;
function validateQuote(q) {
  need(typeof q.exchangeId === 'string' && /^[1-9]\d*$/.test(q.exchangeId), 'EXCHANGE');
  need(['yes', 'no'].includes(q.side) && ['buy', 'sell'].includes(q.action), 'SIDE');
  need(Number.isSafeInteger(q.quantity) && q.quantity > 0 && q.quantity <= 2147483647, 'QUANTITY');
  price(q.price);
}
function equivalent(q) {
  return {bid: (q.action === 'buy') === (q.side === 'yes'),
    price: q.side === 'yes' ? amount(q.price) : 1000000n - amount(q.price)};
}
function crosses(a, b) {
  if (a.exchangeId !== b.exchangeId) return false;
  const x = equivalent(a), y = equivalent(b);
  return x.bid !== y.bid && (x.bid ? x.price >= y.price : y.price >= x.price);
}
function validateState(s) {
  need(s?.schemaVersion === 1 && Number.isSafeInteger(s.revision) && s.revision >= 0, 'STATE');
  if (s.reconcileNotBefore !== null) instant(s.reconcileNotBefore);
  if (s.pending !== null) need(typeof s.pending?.dispatchKey === 'string' &&
    ['submit', 'submit_batch', 'cancel'].includes(s.pending.kind) && Number.isFinite(instant(s.pending.at)), 'PENDING');
}
export function createMakerSchedulerV1() {
  return {schemaVersion: 1, revision: 0, pending: null, reconcileNotBefore: null};
}

// input.account is a complete authoritative OWNED-order projection; desired
// quotes must come from the caller's funded, fee-aware strategy and risk checks.
// availableWrites/nextWriteAt are hints from the shared client, never admission.
export function planMakerQuoteV1(state, input, policy = MAKER_SCHEDULER_POLICY_V1) {
  validateState(state);
  const now = instant(input.at), account = input.account;
  for (const key of ['maximumAccountAgeMs', 'maximumBookAgeMs', 'minimumQuoteAgeMs', 'minimumRepriceTicks', 'maximumOpenQuotes', 'maximumSubmitBatch'])
    need(Number.isSafeInteger(policy[key]) && policy[key] >= (key === 'minimumQuoteAgeMs' ? 0 : 1), 'POLICY');
  need(policy.maximumSubmitBatch <= 50, 'BATCH_LIMIT');
  const minimumEdge = amount(policy.minimumNetEntryEdge); need(minimumEdge > 0n, 'EDGE_POLICY');
  const hold = reason => ({kind: 'hold', reason, revision: state.revision});
  if (state.pending) return hold('write_outcome_pending_reconciliation');
  need(account?.complete === true && Array.isArray(account.orders), 'COMPLETE_OWNED_ACCOUNT');
  const observed = instant(account.observedAt);
  if (observed > now || now - observed > policy.maximumAccountAgeMs ||
      (state.reconcileNotBefore !== null && observed < instant(state.reconcileNotBefore))) return hold('account_refresh_required');
  need(Array.isArray(input.desired), 'DESIRED');
  need(Number.isSafeInteger(input.budget?.availableWrites) && input.budget.availableWrites >= 0 && input.budget.availableWrites <= 24, 'BUDGET');
  if (!input.budget.availableWrites || instant(input.budget.nextWriteAt) > now) return hold('shared_write_budget_wait');
  const ids = new Set(), resting = [];
  for (const q of account.orders) {
    need(typeof q.orderId === 'string' && /^[1-9]\d*$/.test(q.orderId) && !ids.has(q.orderId), 'ORDER_ID'); ids.add(q.orderId);
    if (!active.has(q.status) && !terminal.has(q.status)) return hold('owned_order_outcome_unknown');
    if (!active.has(q.status)) continue;
    validateQuote(q); instant(q.createdAt);
    need(instant(q.createdAt) <= observed && Number.isSafeInteger(q.remainingQuantity) && q.remainingQuantity > 0 && q.remainingQuantity <= q.quantity, 'OPEN_ORDER');
    resting.push(q);
  }
  const desired = new Map(), rejected = [];
  for (const q of input.desired) {
    validateQuote(q); need(!desired.has(slot(q)) && !rejected.includes(slot(q)), 'DUPLICATE_DESIRED');
    const source = instant(q.sourceAt), until = instant(q.validUntil);
    const edge = amount(q.netEdgePerShare);
    need(q.action !== 'sell' || q.reduceOnly === true, 'COVERED_SELL_REQUIRED');
    if (source > now || now - source > policy.maximumBookAgeMs || until <= now ||
        (q.action === 'buy' && (q.reduceOnly === true || edge < minimumEdge))) rejected.push(slot(q));
    else desired.set(slot(q), q);
  }
  const decision = (kind, reason, fields) => {
    const value = {kind, reason, revision: state.revision, at: input.at,
      accountObservedAt: account.observedAt, accountHash: payloadHash(account), ...fields};
    return {...value, dispatchKey: payloadHash(value)};
  };
  const cancel = (q, reason) => input.cancelEnabled === true
    ? decision('cancel', reason, {orderId: q.orderId, slot: slot(q)}) : hold('cancellation_disabled');
  resting.sort((a, b) => instant(a.createdAt) - instant(b.createdAt) || a.orderId.localeCompare(b.orderId));
  // Withdraw obsolete quotes immediately; do not wait out a minimum quote age
  // when fresh evidence no longer supports them or the operator stops entries.
  const seenSlots = new Set();
  for (const q of resting) {
    const target = desired.get(slot(q));
    if (seenSlots.has(slot(q))) return cancel(q, 'duplicate_owned_quote');
    seenSlots.add(slot(q));
    if (!target || (q.action === 'buy' && input.entriesEnabled !== true)) return cancel(q, 'withdraw_obsolete_quote');
    if (q.remainingQuantity > target.quantity) return cancel(q, 'reduce_quote_exposure');
    if ((q.action === 'buy' && amount(q.price) > amount(target.price)) ||
        (q.action === 'sell' && amount(q.price) < amount(target.price))) return cancel(q, 'withdraw_more_aggressive_quote');
  }
  // Even complementary YES/NO quotes can cross each other. Cancel the resting
  // conflicting leg first and require confirmed terminal status before entry.
  const candidates = [...desired.values()].sort((a, b) => {
    if (a.action !== b.action) return a.action === 'sell' ? -1 : 1;
    const score = amount(a.netEdgePerShare) * amount(b.price) - amount(b.netEdgePerShare) * amount(a.price);
    return score > 0n ? -1 : score < 0n ? 1 : slot(a).localeCompare(slot(b));
  });
  const submissions = [];
  for (const q of candidates) {
    if (q.action === 'buy' && input.entriesEnabled !== true) continue;
    if (q.action === 'sell' && input.exitsEnabled !== true) continue;
    const existing = resting.find(x => slot(x) === slot(q));
    if (existing) {
      const delta = amount(q.price) - amount(existing.price);
      const changed = (delta < 0n ? -delta : delta) >= BigInt(policy.minimumRepriceTicks) * 5000n ||
        existing.remainingQuantity !== q.quantity;
      if (changed && now - instant(existing.createdAt) >= policy.minimumQuoteAgeMs) return cancel(existing, 'reprice_after_confirmed_cancel');
      continue; // Retain queue position instead of sending identical writes.
    }
    const conflict = resting.find(x => crosses(q, x));
    if (conflict) return cancel(conflict, 'prevent_owned_quote_cross');
    if (submissions.some(x => crosses(q, x))) continue;
    if (resting.length + submissions.length >= policy.maximumOpenQuotes) {
      if (q.action === 'sell') {
        const entry = [...candidates].reverse().find(x => x.action === 'buy' && resting.some(r => slot(r) === slot(x)));
        const toCancel = entry && resting.find(r => slot(r) === slot(entry));
        if (toCancel) return cancel(toCancel, 'free_quote_slot_for_inventory_exit');
      }
      continue;
    }
    if (submissions.length < policy.maximumSubmitBatch) submissions.push(q);
  }
  if (submissions.length === 1) {
    const q = submissions[0];
    return decision('submit', q.action === 'sell' ? 'inventory_exit_priority' : 'net_edge_per_capital_priority',
      {quote: structuredClone(q), slot: slot(q)});
  }
  if (submissions.length > 1) return decision('submit_batch', 'ranked_quotes_one_shared_write',
    {quotes: structuredClone(submissions), endpoint: '/orders/batch', atomicAcceptance: false});
  return hold('retain_queue_or_no_eligible_quote');
}

export function beginMakerDispatchV1(state, decision) {
  validateState(state);
  need(!state.pending && decision.revision === state.revision && ['submit', 'submit_batch', 'cancel'].includes(decision.kind), 'STALE_DISPATCH');
  const {dispatchKey, ...body} = decision;
  need(dispatchKey === payloadHash(body), 'DISPATCH_CHANGED');
  instant(decision.at);
  return {...structuredClone(state), revision: state.revision + 1, pending: structuredClone(decision)};
}
// "confirmed" means the caller has resolved exact order identity and fills;
// "rejected" means a definitive no-write response, never a timeout/HTTP guess.
export function finishMakerDispatchV1(state, {dispatchKey, outcome, reconciledAt}) {
  validateState(state);
  need(state.pending?.dispatchKey === dispatchKey, 'DISPATCH_ID');
  need(['confirmed', 'rejected', 'unknown'].includes(outcome), 'OUTCOME');
  if (outcome === 'unknown') return structuredClone(state);
  need(instant(reconciledAt) >= instant(state.pending.at), 'RECONCILIATION_CLOCK');
  return {...structuredClone(state), revision: state.revision + 1, pending: null, reconcileNotBefore: reconciledAt};
}
