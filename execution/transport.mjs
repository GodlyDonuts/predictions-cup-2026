// Contract source: the checked-in official openapi.json and api-guide.txt.
// This module owns no credentials, fetch function, timer, limiter or retry loop.
// dispatch MUST share the collector's serialized admission and durable cooldown,
// await beforeDispatch after waiting, then beforeNetwork after persisting rate
// admission, immediately before its network call. The latter is repeatable.
const numericId = value => typeof value === 'string' && /^[1-9]\d*$/.test(value) && Number.isSafeInteger(Number(value));
const orderId = value => numericId(value) || Number.isSafeInteger(value) && value > 0;
const object = value => value !== null && typeof value === 'object' && !Array.isArray(value);
const finite = value => typeof value === 'number' && Number.isFinite(value);
const clock = value => typeof value === 'string' && Number.isFinite(Date.parse(value));
const API_CODES = new Set(['MISSING_API_KEY', 'INVALID_API_KEY', 'API_KEY_REVOKED', 'API_KEY_EXPIRED', 'UNAUTHORIZED', 'INSUFFICIENT_SCOPES', 'ACCOUNT_BANNED', 'RESIDENCE_UPDATE_REQUIRED', 'ADMIN_REQUIRED', 'FORBIDDEN', 'TERMS_NOT_ACKNOWLEDGED', 'RATE_LIMITED', 'INVALID_JSON', 'INVALID_ID', 'INVALID_CURSOR', 'VALIDATION_ERROR', 'ORDER_CONFLICT', 'NOT_FOUND', 'CONFLICT', 'REQUEST_IN_FLIGHT', 'TERMINAL_ENGINE_SCOPE', 'RELATIONSHIP_VIOLATION', 'DATABASE_ERROR', 'INTERNAL_ERROR', 'ORDER_STATUS_UNKNOWN', 'TX_CONFLICT', 'SERVICE_UNAVAILABLE']);
const BODY_KEYS = ['idempotencyKey', 'exchangeId', 'side', 'action', 'quantity', 'price', 'expirationDate', 'tournamentId'];

export class OrderTransportError extends Error {
  constructor(code, {status = null, apiCode = null, outcome = 'blocked', mayHaveExecuted = false, retrySameRequestOnly = false, retryAfterMs = 0, semanticMismatch = false} = {}) {
    super(code);
    this.name = 'OrderTransportError';
    Object.assign(this, {code, status, apiCode, outcome, mayHaveExecuted, retrySameRequestOnly, retryAfterMs, semanticMismatch});
  }
}
const requireThat = (condition, code) => { if (!condition) throw new OrderTransportError(code); };

function scopeOf(scope) {
  requireThat(object(scope) && typeof scope.tournamentId === 'string' && /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i.test(scope.tournamentId) && typeof scope.tournamentSlug === 'string' && /^[a-z0-9]+(?:-[a-z0-9]+)*$/.test(scope.tournamentSlug) && Array.isArray(scope.exchanges) && scope.exchanges.length > 0, 'INVALID_EXECUTION_SCOPE');
  const byExchange = new Map(), byMarket = new Map();
  for (const value of scope.exchanges) {
    requireThat(numericId(value?.exchangeId) && numericId(value.marketId) && !byExchange.has(value.exchangeId), 'INVALID_EXECUTION_SCOPE');
    const spec = {exchangeId: value.exchangeId, marketId: value.marketId};
    byExchange.set(spec.exchangeId, spec);
    byMarket.set(spec.marketId, [...(byMarket.get(spec.marketId) || []), spec.exchangeId]);
  }
  return {tournamentId: scope.tournamentId, tournamentSlug: scope.tournamentSlug, byExchange, byMarket};
}

function bodyOf(body, scope) {
  requireThat(object(body) && Object.keys(body).length === BODY_KEYS.length && Object.keys(body).every(key => BODY_KEYS.includes(key)), 'INVALID_LIMIT_ORDER');
  requireThat(typeof body.idempotencyKey === 'string' && body.idempotencyKey.length >= 1 && body.idempotencyKey.length <= 255 && !/[\x00-\x1f\x7f]/.test(body.idempotencyKey), 'INVALID_IDEMPOTENCY_KEY');
  requireThat(body.tournamentId === scope.tournamentId && scope.byExchange.has(body.exchangeId), 'ORDER_SCOPE_MISMATCH');
  requireThat(['yes', 'no'].includes(body.side) && ['buy', 'sell'].includes(body.action) && Number.isSafeInteger(body.quantity) && body.quantity > 0 && body.quantity <= 2147483647, 'INVALID_LIMIT_ORDER');
  requireThat(finite(body.price) && body.price >= 0.005 && body.price <= 0.995 && Math.abs(body.price * 200 - Math.round(body.price * 200)) < 1e-8, 'LIMIT_PRICE_REQUIRED');
  requireThat(clock(body.expirationDate) && /^\d{4}-\d{2}-\d{2}T.*(?:Z|[+-]\d{2}:\d{2})$/.test(body.expirationDate), 'LIMIT_EXPIRY_REQUIRED');
  return Object.fromEntries(BODY_KEYS.map(key => [key, body[key]]));
}

// Order payloads are flat scalars. This is byte-identical to core.canonicalPayload
// and ledger.canonicalJson; preserve these bytes through shared dispatch/retry.
const canonicalBody = body => JSON.stringify(Object.fromEntries(Object.keys(body).sort().map(key => [key, body[key]])));

function cursorSuffix(cursor) {
  if (cursor === undefined || cursor === null) return '';
  requireThat(typeof cursor === 'string' && cursor.length > 0 && cursor.length <= 4096 && !/[\x00-\x1f\x7f]/.test(cursor), 'INVALID_PAGINATION_CURSOR');
  return '&cursor=' + encodeURIComponent(cursor);
}

function encodedCursor(endpoint, base) {
  if (endpoint === base) return true;
  if (!endpoint.startsWith(base + '&cursor=')) return false;
  try { return endpoint === base + cursorSuffix(decodeURIComponent(endpoint.slice(base.length + 8))); }
  catch { return false; }
}

function statusError(value, method) {
  const status = Number.isInteger(value?.status) && value.status >= 400 && value.status <= 599 ? value.status : null;
  const suppliedCode = value?.apiCode ?? value?.code ?? value?.data?.error?.code;
  const apiCode = API_CODES.has(suppliedCode) ? suppliedCode : null;
  const wait = finite(value?.retryAfterMs) && value.retryAfterMs >= 0 ? value.retryAfterMs : 0;
  const writing = method !== 'GET';
  if (status === 429) return new OrderTransportError('RATE_LIMITED', {status, apiCode, outcome: 'rate_limited', retrySameRequestOnly: writing, retryAfterMs: Math.max(60000, wait)});
  if (status === 409 && apiCode === 'REQUEST_IN_FLIGHT') return new OrderTransportError('REQUEST_IN_FLIGHT', {status, apiCode, outcome: 'in_flight', mayHaveExecuted: writing, retrySameRequestOnly: writing, retryAfterMs: Math.max(90000, wait)});
  const uncertain = writing && (status === null || status >= 500);
  return new OrderTransportError(status === null ? 'REQUEST_UNCONFIRMED' : 'OFFICIAL_REQUEST_REJECTED', {status, apiCode, outcome: uncertain ? 'unknown' : status === 409 ? 'conflict' : 'rejected', mayHaveExecuted: uncertain, retrySameRequestOnly: uncertain, retryAfterMs: wait});
}

export class OrderTransport {
  #dispatch; #scope; #canWrite; #ownsOrder; #validatePermit; #validateLease; #validateNetworkPermit; #now;
  #logicalBodies = new Map(); #keyBodies = new Map(); #maxPages;

  constructor({dispatch, scope, canWrite, ownsOrder, validatePermit, validateLease, validateNetworkPermit = () => true, now = Date.now, maxPages = 100} = {}) {
    requireThat([dispatch, canWrite, ownsOrder, validatePermit, validateLease, validateNetworkPermit, now].every(value => typeof value === 'function') && Number.isSafeInteger(maxPages) && maxPages > 0 && maxPages <= 1000, 'INVALID_TRANSPORT_DEPENDENCIES');
    this.#dispatch = dispatch;
    this.#scope = scopeOf(scope);
    this.#canWrite = canWrite;
    this.#ownsOrder = ownsOrder;
    this.#validatePermit = validatePermit;
    this.#validateLease = validateLease;
    this.#validateNetworkPermit = validateNetworkPermit;
    this.#now = now;
    this.#maxPages = maxPages;
  }

  #owned(id) {
    requireThat(orderId(id), 'INVALID_ORDER_ID');
    let owned = false;
    try { owned = this.#ownsOrder(String(id)) === true; } catch {}
    requireThat(owned, 'ORDER_NOT_OWNED');
    return String(id);
  }

  // Bind this method for the shared dispatch allowlist. It never widens request()
  // itself. Unknown methods, URLs, query keys and unsupported order routes fail.
  authorizeRequest(spec) {
    requireThat(object(spec) && Object.keys(spec).every(key => ['method', 'endpoint', 'body', 'bodyJson', 'beforeDispatch', 'beforeNetwork'].includes(key)) && typeof spec.endpoint === 'string', 'REQUEST_OUTSIDE_ALLOWLIST');
    const {method, endpoint} = spec, scope = this.#scope;
    if (method === 'POST' && endpoint === '/orders') {
      bodyOf(spec.body, scope);
      requireThat(spec.bodyJson === undefined || spec.bodyJson === canonicalBody(spec.body), 'FROZEN_ORDER_BODY_MISMATCH');
      requireThat(typeof spec.beforeDispatch === 'function' && typeof spec.beforeNetwork === 'function', 'WRITE_GUARD_REQUIRED');
      return true;
    }
    const own = /^\/orders\/([1-9]\d*)$/.exec(endpoint);
    if (method === 'DELETE' && own) {
      requireThat(spec.body === undefined && spec.bodyJson === undefined && typeof spec.beforeDispatch === 'function' && typeof spec.beforeNetwork === 'function', 'WRITE_GUARD_REQUIRED');
      this.#owned(own[1]);
      return true;
    }
    requireThat(method === 'GET' && spec.body === undefined && spec.bodyJson === undefined, 'REQUEST_OUTSIDE_ALLOWLIST');
    if (endpoint === '/tournaments/' + scope.tournamentSlug || endpoint === '/tournaments/' + scope.tournamentSlug + '/portfolio/positions') return true;
    const market = /^\/markets\/([1-9]\d*)(\/orderbook)?\?/.exec(endpoint);
    if (market && scope.byMarket.has(market[1]) && endpoint === '/markets/' + market[1] + (market[2] || '') + '?tournamentId=' + scope.tournamentId + (market[2] ? '&depth=20' : '')) return true;
    for (const status of ['all', 'open', 'closed', 'expired']) if (encodedCursor(endpoint, '/orders?status=' + status + '&limit=200&tournamentId=' + scope.tournamentId)) return true;
    if (own) { this.#owned(own[1]); return true; }
    const fills = /^\/orders\/([1-9]\d*)\/fills\?/.exec(endpoint);
    if (fills && encodedCursor(endpoint, '/orders/' + fills[1] + '/fills?limit=200')) { this.#owned(fills[1]); return true; }
    throw new OrderTransportError('REQUEST_OUTSIDE_ALLOWLIST');
  }

  async #request(spec) {
    this.authorizeRequest(spec);
    let response;
    try { response = await this.#dispatch(spec); }
    catch (error) { if (error instanceof OrderTransportError) throw error; throw statusError(error, spec.method); }
    if (!object(response) || !Number.isInteger(response.status) || response.status < 200 || response.status >= 300) throw statusError(response, spec.method);
    if (!clock(response.requestStartedAt) || !clock(response.receivedAt) || Date.parse(response.receivedAt) < Date.parse(response.requestStartedAt) || !object(response.data)) throw new OrderTransportError('INVALID_OFFICIAL_RESPONSE', {status: response.status, outcome: spec.method === 'GET' ? 'rejected' : 'unknown', mayHaveExecuted: spec.method !== 'GET', semanticMismatch: spec.method !== 'GET'});
    return response;
  }

  #scopedExchange(exchangeId) { requireThat(this.#scope.byExchange.has(exchangeId), 'RESPONSE_SCOPE_MISMATCH'); }
  #coverage(coverage, required = false) {
    if (coverage === undefined && !required) return;
    requireThat(object(coverage) && coverage.complete === true && (coverage.projectedThroughSequence === null || Number.isSafeInteger(coverage.projectedThroughSequence) && coverage.projectedThroughSequence >= 0), 'INCOMPLETE_OFFICIAL_COVERAGE');
  }
  #order(row, expectedId) {
    requireThat(object(row) && orderId(row.id) && (expectedId === undefined || String(row.id) === expectedId) && row.tournamentId === this.#scope.tournamentId, 'RESPONSE_SCOPE_MISMATCH');
    this.#scopedExchange(row.exchangeId);
    requireThat(['yes', 'no'].includes(row.side) && ['buy', 'sell'].includes(row.action) && Number.isSafeInteger(row.quantity) && row.quantity >= 0 && typeof row.open === 'boolean' && (row.priceLimit === null || finite(row.priceLimit) && row.priceLimit >= 0 && row.priceLimit <= 1) && clock(row.createdAt) && (row.expirationDate === null || clock(row.expirationDate)), 'INVALID_OFFICIAL_ORDER');
  }
  #marketContext(data, marketId, book) {
    requireThat(object(data) && (!book ? data.id === marketId : true) && Array.isArray(data.contexts) && data.contexts.length === 1 && data.contexts[0]?.type === 'tournament' && data.contexts[0]?.tournament?.id === this.#scope.tournamentId, 'RESPONSE_SCOPE_MISMATCH');
    const exchanges = book ? data.contexts[0].orderbook?.exchanges : data.exchanges;
    requireThat(Array.isArray(exchanges) && exchanges.length === this.#scope.byMarket.get(marketId).length, 'RESPONSE_SCOPE_MISMATCH');
    const seen = new Set();
    for (const value of exchanges) {
      const exchangeId = book ? value.exchangeId : value.id;
      requireThat(this.#scope.byExchange.get(exchangeId)?.marketId === marketId && !seen.has(exchangeId), 'RESPONSE_SCOPE_MISMATCH');
      seen.add(exchangeId);
    }
  }

  async tournament() {
    const response = await this.#request({method: 'GET', endpoint: '/tournaments/' + this.#scope.tournamentSlug});
    const data = response.data;
    requireThat(data.id === this.#scope.tournamentId && data.slug === this.#scope.tournamentSlug && finite(data.myBalance) && data.myBalance >= 0 && typeof data.status === 'string' && typeof data.isPendingEnrolment === 'boolean', 'RESPONSE_SCOPE_MISMATCH');
    return response;
  }
  async market(marketId) {
    const response = await this.#request({method: 'GET', endpoint: '/markets/' + marketId + '?tournamentId=' + this.#scope.tournamentId});
    this.#marketContext(response.data, marketId, false);
    return response;
  }
  async orderbook(marketId) {
    const response = await this.#request({method: 'GET', endpoint: '/markets/' + marketId + '/orderbook?tournamentId=' + this.#scope.tournamentId + '&depth=20'});
    this.#marketContext(response.data, marketId, true);
    return response;
  }
  async positions() {
    const response = await this.#request({method: 'GET', endpoint: '/tournaments/' + this.#scope.tournamentSlug + '/portfolio/positions'});
    // Official tournament positions is an unpaginated complete array. Inventing
    // limit/cursor parameters would silently risk truncation or wrong scope.
    const {positions, summary, pagination} = response.data;
    requireThat(Array.isArray(positions) && object(summary) && ['totalMarketValue', 'totalCostBasis', 'totalUnrealizedPnl'].every(key => finite(summary[key])) && pagination === undefined, 'INCOMPLETE_OFFICIAL_POSITIONS');
    const seen = new Set();
    for (const row of positions) {
      this.#scopedExchange(row.exchangeId);
      requireThat(this.#scope.byExchange.get(row.exchangeId).marketId === row.marketId && !seen.has(row.exchangeId) && Number.isSafeInteger(row.quantity) && Array.isArray(row.lots), 'INVALID_OFFICIAL_POSITION');
      seen.add(row.exchangeId);
    }
    return response;
  }
  async order(id) {
    id = this.#owned(id);
    const response = await this.#request({method: 'GET', endpoint: '/orders/' + id});
    this.#order(response.data, id);
    return response;
  }

  async #pages(base, validate, identity) {
    const rows = [], pages = [], cursors = new Set(), seen = new Set();
    let cursor, first, last;
    for (let page = 0; page < this.#maxPages; page++) {
      const response = await this.#request({method: 'GET', endpoint: base + cursorSuffix(cursor)}), data = response.data;
      if (!first) first = response;
      validate(data, first.data);
      requireThat(Array.isArray(data.data) && object(data.pagination) && Number.isInteger(data.pagination.limit) && data.pagination.limit >= 1 && data.pagination.limit <= 200 && typeof data.pagination.hasMore === 'boolean', 'INCOMPLETE_OFFICIAL_PAGINATION');
      for (const row of data.data) {
        const key = identity(row);
        requireThat(!seen.has(key), 'INCONSISTENT_OFFICIAL_PAGINATION');
        seen.add(key); rows.push(row);
      }
      pages.push({receivedAt: response.receivedAt, coverage: data.coverage || null});
      last = response;
      const next = data.pagination.nextCursor;
      if (!data.pagination.hasMore) {
        requireThat(next === null, 'INCOMPLETE_OFFICIAL_PAGINATION');
        return {...last, requestStartedAt: first.requestStartedAt, data: {...last.data, data: rows, pagination: {limit: 200, hasMore: false, nextCursor: null}, pages}};
      }
      cursorSuffix(next);
      requireThat(typeof next === 'string' && !cursors.has(next) && data.data.length > 0, 'INCOMPLETE_OFFICIAL_PAGINATION');
      cursors.add(next); cursor = next;
    }
    throw new OrderTransportError('PAGINATION_LIMIT_EXCEEDED');
  }
  orders(status = 'all') {
    requireThat(['all', 'open', 'closed', 'expired'].includes(status), 'INVALID_ORDER_STATUS');
    return this.#pages('/orders?status=' + status + '&limit=200&tournamentId=' + this.#scope.tournamentId, data => {
      this.#coverage(data.coverage);
      requireThat(Array.isArray(data.data), 'INCOMPLETE_OFFICIAL_PAGINATION');
      data.data.forEach(row => this.#order(row));
    }, row => String(row.id));
  }
  fills(id) {
    id = this.#owned(id);
    return this.#pages('/orders/' + id + '/fills?limit=200', (data, first) => {
      requireThat(String(data.orderId) === id && data.tournamentId === this.#scope.tournamentId, 'RESPONSE_SCOPE_MISMATCH');
      this.#scopedExchange(data.exchangeId);
      this.#coverage(data.coverage, true);
      requireThat(finite(data.totalQuantityFilled) && (data.avgFillPrice === null || finite(data.avgFillPrice) && data.avgFillPrice >= 0 && data.avgFillPrice <= 1) && data.totalQuantityFilled === first.totalQuantityFilled && data.avgFillPrice === first.avgFillPrice && data.exchangeId === first.exchangeId, 'INCONSISTENT_OFFICIAL_PAGINATION');
      requireThat(Array.isArray(data.data), 'INCOMPLETE_OFFICIAL_PAGINATION');
      for (const row of data.data) requireThat(orderId(row.id) && clock(row.filledAt) && Number.isSafeInteger(row.quantity) && (row.side === 'yes' && row.quantity > 0 || row.side === 'no' && row.quantity < 0) && (row.price === null || finite(row.price) && row.price >= 0 && row.price <= 1), 'INVALID_OFFICIAL_FILL');
    }, row => JSON.stringify([row.id, row.filledAt, row.side, row.quantity, row.price]));
  }

  async #write(spec, authority) {
    requireThat(object(authority?.permit) && typeof authority.permit.intentId === 'string' && authority.permit.intentId.length > 0 && authority.lease !== null && authority.lease !== undefined, 'PERMIT_AND_LEASE_REQUIRED');
    const {permit, lease} = structuredClone(authority);
    let admitted = false;
    spec.beforeDispatch = async () => {
      requireThat(!admitted, 'DUPLICATE_WRITE_ADMISSION');
      try {
        requireThat(await this.#canWrite() === true, 'WRITE_STOPPED_OR_OWNER_LOST');
        requireThat(await this.#validateLease(lease, spec) === true, 'INVALID_EXECUTION_LEASE');
        requireThat(await this.#validatePermit(permit, spec) === true, 'INVALID_EXECUTION_PERMIT');
        // Validators may await IO. Check the owner/STOP fence again afterwards.
        requireThat(await this.#canWrite() === true, 'WRITE_STOPPED_OR_OWNER_LOST');
        if (spec.method === 'DELETE') this.#owned(spec.endpoint.slice('/orders/'.length));
      } catch (error) {
        if (error instanceof OrderTransportError) throw error;
        throw new OrderTransportError('EXECUTION_AUTHORITY_UNAVAILABLE');
      }
      if (spec.method === 'POST') {
        const identity = String(permit.runId || '') + ':' + permit.intentId, encoded = spec.bodyJson;
        requireThat(!this.#logicalBodies.has(identity) || this.#logicalBodies.get(identity) === encoded, 'LOGICAL_REQUEST_CHANGED');
        requireThat(!this.#keyBodies.has(spec.body.idempotencyKey) || this.#keyBodies.get(spec.body.idempotencyKey) === encoded, 'IDEMPOTENCY_PAYLOAD_CHANGED');
        requireThat(this.#logicalBodies.has(identity) || permit.retry === true || Date.parse(spec.body.expirationDate) > this.#now(), 'LIMIT_ORDER_EXPIRED');
        // The persistent permit validator must verify the same immutable body on
        // process restart; local bindings additionally catch accidental changes.
        this.#logicalBodies.set(identity, encoded);
        this.#keyBodies.set(spec.body.idempotencyKey, encoded);
      }
      admitted = true;
      return true;
    };
    spec.beforeNetwork = async () => {
      // No intent transition here: the first guard already durably recorded it.
      // This fence catches STOP/owner loss during shared rate-state persistence.
      try {
        requireThat(admitted, 'WRITE_NOT_ADMITTED');
        requireThat(await this.#canWrite() === true, 'WRITE_STOPPED_OR_OWNER_LOST');
        requireThat(await this.#validateLease(lease, spec) === true, 'INVALID_EXECUTION_LEASE');
        requireThat(await this.#validateNetworkPermit(permit, spec) === true, 'INVALID_NETWORK_EXECUTION_PERMIT');
        requireThat(await this.#canWrite() === true, 'WRITE_STOPPED_OR_OWNER_LOST');
        if (spec.method === 'DELETE') this.#owned(spec.endpoint.slice('/orders/'.length));
      } catch (error) {
        if (error instanceof OrderTransportError) throw error;
        throw new OrderTransportError('EXECUTION_AUTHORITY_UNAVAILABLE');
      }
      return true;
    };
    Object.freeze(spec);
    const response = await this.#request(spec);
    if (!admitted) throw new OrderTransportError('DISPATCH_GUARD_NOT_CALLED', {outcome: 'unknown', mayHaveExecuted: true, retrySameRequestOnly: true});
    return response;
  }

  async place(body, authority) {
    const payload = Object.freeze(bodyOf(body, this.#scope));
    // A parsed success is evidence of a placement response, not proof of the
    // requested canonical action. Core owns semantic checks and must retain any
    // returned order ID even on mismatch, then reconcile without blind retries.
    return this.#write({method: 'POST', endpoint: '/orders', body: payload, bodyJson: canonicalBody(payload)}, authority);
  }
  async cancel(id, authority) {
    id = this.#owned(id);
    const response = await this.#write({method: 'DELETE', endpoint: '/orders/' + id}, authority);
    if (String(response.data.orderId) !== id || response.data.tournamentId !== this.#scope.tournamentId) throw new OrderTransportError('INVALID_CANCEL_RESPONSE', {status: response.status, outcome: 'unknown', mayHaveExecuted: true, semanticMismatch: true});
    return response;
  }
}
