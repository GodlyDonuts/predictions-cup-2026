import {createHash} from 'node:crypto';

const uuid = value => typeof value === 'string' && /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i.test(value);
const types = new Set(['monotonic', 'complementary', 'mutually_exclusive', 'implication', 'boolean_and', 'boolean_or', 'boolean_not', 'conditional_if', 'boolean_expression']);
const statuses = new Set(['active', 'stale', 'disabled', 'settled', 'deleted']);
const roles = new Set(['member', 'antecedent', 'consequent', 'output', 'input']);
const directions = new Set(['antecedent_to_consequent', 'condition_consequence_to_output', 'inputs_to_output', 'symmetric', 'ordered_members']);
const fail = () => { throw Error('RELATION_RESPONSE_INVALID'); };
const plain = value => {
  if (!value || typeof value !== 'object' || Array.isArray(value)
    || ![Object.prototype, null].includes(Object.getPrototypeOf(value))
    || ['__proto__', 'constructor', 'prototype'].some(key => Object.hasOwn(value, key))) fail();
  return value;
};
const text = (value, nullable = false, max = 8192) => {
  if (nullable && value === null) return null;
  if (typeof value !== 'string' || value.length > max) fail();
  return value;
};
const id = (value, nullable = false) => {
  if (nullable && value === null) return null;
  if (typeof value !== 'string' || !/^[1-9]\d*$/.test(value) || !Number.isSafeInteger(Number(value))) fail();
  return value;
};
const member = (value, set) => { if (!set.has(value)) fail(); return value; };
const clock = (value, nullable = false) => {
  if (nullable && value === null) return null;
  if (typeof value !== 'string' || !/^\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d(?:\.\d{1,9})?(?:Z|[+-]\d\d:\d\d)$/.test(value)
    || !Number.isFinite(Date.parse(value)) || new Date(value.slice(0, 19) + 'Z').toISOString().slice(0, 19) !== value.slice(0, 19)) fail();
  return value;
};
const array = (value, max = 1000) => { if (!Array.isArray(value) || value.length > max) fail(); return value; };
const canonical = value => JSON.stringify(value && typeof value === 'object'
  ? Array.isArray(value) ? value.map(item => JSON.parse(canonical(item)))
    : Object.fromEntries(Object.keys(value).sort().map(key => [key, JSON.parse(canonical(value[key]))])) : value);
const freeze = value => { if (value && typeof value === 'object') { Object.values(value).forEach(freeze); Object.freeze(value); } return value; };

function expression(value, depth = 0, budget = {left: 1000}) {
  if (value === null) return null;
  if (depth > 32 || --budget.left < 0) fail();
  plain(value);
  const descend = item => expression(item, depth + 1, budget);
  if (value.kind === 'leaf') {
    if (!Number.isSafeInteger(value.exchangeId) || value.exchangeId < 1) fail();
    return {kind: 'leaf', exchangeId: value.exchangeId};
  }
  if (value.kind === 'and' || value.kind === 'or') {
    const children = array(value.children); if (!children.length) fail();
    return {kind: value.kind, children: children.map(descend)};
  }
  if (value.kind === 'not') return {kind: 'not', child: descend(value.child)};
  if (value.kind === 'cond') return {kind: 'cond', condition: descend(value.condition), consequence: descend(value.consequence)};
  fail();
}

function relationship(value, tournamentId) {
  plain(value);
  if (!uuid(value.id) || !Number.isSafeInteger(value.version) || value.version < 1 || typeof value.isExhaustive !== 'boolean'
    || (Object.hasOwn(value, 'tournamentId') && value.tournamentId !== tournamentId)) fail();
  const nodes = array(value.nodes).map(node => {
    plain(node);
    if (node.currentPrice !== null && (typeof node.currentPrice !== 'number' || !Number.isFinite(node.currentPrice) || node.currentPrice < 0 || node.currentPrice > 1)) fail();
    return {exchangeId: id(node.exchangeId), marketId: id(node.marketId, true), marketTitle: text(node.marketTitle, true),
      outcome: text(node.outcome, true), currentPrice: node.currentPrice, role: member(node.role, roles), contractId: text(node.contractId, true)};
  });
  const direction = plain(value.direction), constraint = plain(value.constraint);
  return {id: value.id, type: member(value.type, types), source: member(value.source, new Set(['auto_derived', 'admin_tagged'])),
    status: member(value.status, statuses), label: text(value.label, true), reason: text(value.reason, true), isExhaustive: value.isExhaustive,
    expression: expression(value.expression), nodes, direction: {kind: member(direction.kind, directions),
      fromExchangeIds: array(direction.fromExchangeIds).map(v => id(v)), toExchangeIds: array(direction.toExchangeIds).map(v => id(v)), description: text(direction.description)},
    constraint: {description: text(constraint.description), priceRule: text(constraint.priceRule), boundSemantics: text(constraint.boundSemantics), evaluationEndpoint: text(constraint.evaluationEndpoint)},
    createdAt: clock(value.createdAt, true), updatedAt: clock(value.updatedAt, true), version: value.version};
}

// Captures public market relationship evidence only. No relationship in this
// result is promoted to a settlement proof or an order authorization.
export function createBasketRelationshipReader({tournamentId, dispatch, canOperate = () => true, now = Date.now, maxPages = 10} = {}) {
  if (!uuid(tournamentId) || typeof dispatch !== 'function' || typeof canOperate !== 'function' || typeof now !== 'function'
    || !Number.isSafeInteger(maxPages) || maxPages < 1 || maxPages > 10) throw Error('BASKET_RELATION_DEPENDENCIES');
  const base = '/relationships?tournamentId=' + encodeURIComponent(tournamentId) + '&limit=200', permits = new WeakMap();
  let busy = false;
  const allowed = () => { try { return canOperate() === true; } catch { return false; } };
  const stamp = () => { const value = now(); if (!Number.isSafeInteger(value) || value < 0 || value > 8640000000000000) throw Error('BASKET_RELATION_CLOCK'); return new Date(value).toISOString(); };
  function authorizes(spec) {
    if (!allowed() || !spec || spec.method !== 'GET' || spec.body !== undefined || spec.bodyJson !== undefined || spec.headers !== undefined
      || spec.beforeDispatch !== spec.beforeNetwork || typeof spec.beforeNetwork !== 'function') return false;
    return permits.has(spec.beforeNetwork) && permits.get(spec.beforeNetwork) === spec.endpoint;
  }
  async function capture() {
    if (busy) throw Error('BASKET_RELATION_CAPTURE_BUSY');
    busy = true;
    const snapshots = [], relationships = [], seenIds = new Set(), cursors = new Set();
    let cursor = null, complete = false, reasonCode = 'PAGE_LIMIT', startedAt;
    try {
      startedAt = stamp();
      for (let page = 0; page < maxPages; page++) {
        if (!allowed()) { reasonCode = 'STOPPED'; break; }
        const endpoint = base + (cursor === null ? '' : '&cursor=' + encodeURIComponent(cursor));
        const fence = () => { if (!allowed() || permits.get(fence) !== endpoint) throw Error('BASKET_RELATION_STOPPED'); return true; };
        permits.set(fence, endpoint);
        let response;
        try { response = await dispatch({method: 'GET', endpoint, beforeDispatch: fence, beforeNetwork: fence}); fence(); }
        catch { reasonCode = allowed() ? 'DISPATCH_UNAVAILABLE' : 'STOPPED'; break; }
        finally { permits.delete(fence); }
        try {
          plain(response);
          if (response.endpoint !== endpoint || response.status !== 200) fail();
          const requestStartedAt = clock(response.requestStartedAt), receivedAt = clock(response.receivedAt), data = plain(response.data);
          if (Date.parse(requestStartedAt) < Date.parse(snapshots.at(-1)?.receivedAt ?? startedAt)
            || Date.parse(receivedAt) < Date.parse(requestStartedAt) || Date.parse(receivedAt) > now()
            || (Object.hasOwn(data, 'tournamentId') && data.tournamentId !== tournamentId)) fail();
          const rows = array(data.data, 200).map(value => relationship(value, tournamentId)), pagination = plain(data.pagination);
          if (pagination.limit !== 200 || typeof pagination.hasMore !== 'boolean'
            || !(pagination.nextCursor === null || typeof pagination.nextCursor === 'string')
            || (pagination.nextCursor !== null && (!pagination.nextCursor || pagination.nextCursor.length > 4096 || /[\u0000-\u001f\u007f]/.test(pagination.nextCursor)))
            || (pagination.hasMore && pagination.nextCursor === null)) fail();
          if (pagination.nextCursor !== null) encodeURIComponent(pagination.nextCursor); // Reject unencodable Unicode before the next dispatch.
          const pageIds = new Set();
          for (const row of rows) { const key = row.id.toLowerCase(); if (seenIds.has(key) || pageIds.has(key)) throw Error('DUPLICATE_RELATIONSHIP'); pageIds.add(key); }
          snapshots.push({endpoint, requestStartedAt, receivedAt, status: 200, data: {data: rows, pagination: {
            limit: pagination.limit, hasMore: pagination.hasMore, nextCursor: pagination.nextCursor}}});
          rows.forEach(row => { seenIds.add(row.id.toLowerCase()); relationships.push(row); });
          if (!pagination.hasMore) { complete = true; reasonCode = 'COMPLETE'; break; }
          if (cursors.has(pagination.nextCursor)) { reasonCode = 'CURSOR_CYCLE'; break; }
          cursors.add(pagination.nextCursor); cursor = pagination.nextCursor;
        } catch (error) { reasonCode = error?.message === 'DUPLICATE_RELATIONSHIP' ? 'DUPLICATE_RELATIONSHIP' : 'INVALID_RESPONSE'; break; }
      }
      const result = {schemaVersion: 1, kind: 'basket_relationship_capture', tournamentId, startedAt, finishedAt: stamp(),
        complete, reasonCode, snapshots, relationships, orderEligible: false, proofConstructed: false};
      return freeze({...result, evidenceHash: createHash('sha256').update(canonical(result)).digest('hex')});
    } finally { busy = false; }
  }
  return Object.freeze({authorizes, capture});
}
