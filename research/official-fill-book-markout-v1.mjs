import {amount, money, SCALE, TICK} from '../paper/fixed.mjs';
import {payloadHash} from '../execution/core.mjs';
import {createBookPublicationState} from '../live-paper/book-publication-v3.mjs';

const HORIZONS = [1_000, 5_000, 30_000];
const need = (ok, code) => { if (!ok) throw Error('OFFICIAL_FILL_MARKOUT_' + code); };
const id = value => typeof value === 'string' && /^[1-9]\d*$/.test(value) && Number.isSafeInteger(Number(value));
const instant = value => typeof value === 'string' && Number.isFinite(Date.parse(value)) ? Date.parse(value) : null;
const keyOf = (marketId, exchangeId) => String(marketId) + ':' + String(exchangeId);

function micros(value, name) {
  let parsed;
  if (typeof value === 'number') {
    need(Number.isFinite(value), 'PRICE_TYPE_' + name);
    const scaled = value * Number(SCALE), rounded = Math.round(scaled);
    need(Math.abs(scaled - rounded) <= 0.001, 'PRICE_PRECISION_' + name);
    parsed = BigInt(rounded);
  } else {
    need(typeof value === 'string', 'PRICE_TYPE_' + name);
    parsed = amount(value);
  }
  need(parsed >= TICK && parsed <= SCALE - TICK && parsed % TICK === 0n, 'PRICE_RANGE_' + name);
  return parsed;
}

function inspectLevels(levels, descending, name) {
  need(Array.isArray(levels) && levels.length > 0 && levels.length <= 20, 'BOOK_LEVELS_' + name);
  const result = levels.map((row, index) => {
    need(row && typeof row === 'object' && Number.isSafeInteger(row.quantity) && row.quantity > 0, 'BOOK_QUANTITY_' + name);
    return {price: micros(row.price, name + '_' + index), quantity: row.quantity};
  });
  for (let i = 1; i < result.length; i += 1) {
    need(descending ? result[i].price < result[i - 1].price : result[i].price > result[i - 1].price, 'BOOK_ORDER_' + name);
  }
  return result;
}

function contractBook(book, side) {
  const yesBids = inspectLevels(book.bids, true, 'BID');
  const yesAsks = inspectLevels(book.asks, false, 'ASK');
  need(yesBids[0].price < yesAsks[0].price, 'CROSSED_BOOK');
  if (side === 'yes') return {bids: yesBids, asks: yesAsks};
  need(side === 'no', 'FILL_SIDE');
  const noBids = yesAsks.map(row => ({price: SCALE - row.price, quantity: row.quantity}));
  const noAsks = yesBids.map(row => ({price: SCALE - row.price, quantity: row.quantity}));
  noBids.sort((a, b) => a.price > b.price ? -1 : a.price < b.price ? 1 : 0);
  noAsks.sort((a, b) => a.price < b.price ? -1 : a.price > b.price ? 1 : 0);
  return {bids: noBids, asks: noAsks};
}

export function verifyOrderAckReceiptChain(rows) {
  need(Array.isArray(rows), 'RECEIPT_ROWS');
  let sequence = 0;
  let previous = null;
  let acknowledged = 0;
  let conflicts = 0;
  const orders = new Map();
  const conflicted = new Set();
  for (const row of rows) {
    need(row && typeof row === 'object' && typeof row.hash === 'string', 'RECEIPT_SHAPE');
    const {hash, ...body} = row;
    need(body.sequence === sequence + 1 && body.previous === previous && payloadHash(body) === hash, 'RECEIPT_CHAIN');
    sequence = body.sequence;
    previous = hash;
    const receipt = body.receipt;
    if (receipt?.kind !== 'order_ack_principal' || receipt.method !== 'POST' || receipt.endpoint !== '/orders'
      || receipt.matchedIdentity !== true || receipt.executionAuthority !== false) continue;
    const orderId = String(receipt.orderId ?? '');
    if (!id(orderId) || !id(String(receipt.exchangeId ?? '')) || !['yes', 'no'].includes(receipt.canonicalSide)
      || !['buy', 'sell'].includes(receipt.canonicalAction)) continue;
    const identity = {
      tournamentId: receipt.tournamentId,
      exchangeId: String(receipt.exchangeId),
      side: receipt.canonicalSide,
      action: receipt.canonicalAction,
      ackAtMs: instant(receipt.receivedAt),
    };
    const prior = orders.get(orderId);
    if (prior && (prior.tournamentId !== identity.tournamentId || prior.exchangeId !== identity.exchangeId
      || prior.side !== identity.side || prior.action !== identity.action)) {
      orders.delete(orderId);
      conflicted.add(orderId);
      conflicts += 1;
      continue;
    }
    if (!conflicted.has(orderId)) {
      if (prior && prior.ackAtMs !== null) identity.ackAtMs = identity.ackAtMs === null
        ? prior.ackAtMs : Math.min(prior.ackAtMs, identity.ackAtMs);
      orders.set(orderId, identity);
      acknowledged += 1;
    }
  }
  return {orders, conflictedOrderIds: conflicted, receiptRowsVerified: sequence,
    matchedOrderAckRows: acknowledged, conflictingOrderIds: conflicted.size, conflictingRows: conflicts};
}

function walk(levels, quantity) {
  let remaining = quantity;
  let valueMicros = 0n;
  for (const level of levels) {
    const used = Math.min(remaining, level.quantity);
    valueMicros += level.price * BigInt(used);
    remaining -= used;
    if (remaining === 0) break;
  }
  return {coveredShares: quantity - remaining, valueMicros};
}

function median(values) {
  if (!values.length) return null;
  const sorted = [...values].sort((a, b) => a < b ? -1 : a > b ? 1 : 0);
  const middle = Math.floor(sorted.length / 2);
  return sorted.length % 2 ? sorted[middle] : (sorted[middle - 1] + sorted[middle]) / 2n;
}

function summarizeLags(values) {
  if (!values.length) return {count: 0, medianMs: null, p90Ms: null, maxMs: null};
  const sorted = [...values].sort((a, b) => a - b);
  const mid = Math.floor(sorted.length / 2);
  return {count: sorted.length, medianMs: sorted.length % 2 ? sorted[mid] : (sorted[mid - 1] + sorted[mid]) / 2,
    p90Ms: sorted[Math.floor((sorted.length - 1) * 0.9)], maxMs: sorted.at(-1)};
}

function formatRatio(numerator, denominator) {
  return denominator > 0n ? money(numerator / denominator) : null;
}

function exactKeys(value, keys) {
  return value && typeof value === 'object' && !Array.isArray(value)
    && Object.keys(value).length === keys.length && keys.every(key => Object.hasOwn(value, key));
}

export function createPublicBookJournalVerifier(universe) {
  const scope = createBookPublicationState(universe);
  const contracts = new Map(scope.scope.exchanges.map(row => [keyOf(row.marketId, row.exchangeId), row]));
  const sourceVersions = new Map();
  const sessions = new Set();
  const reasons = new Set(['BOOK_READ_FAILED', 'INVALID_BOOK', 'ACCOUNT_OUTAGE', 'STOP', 'COLLECTOR_FAILURE', 'MANUAL_INVALIDATION']);
  const stopReasons = new Set(['STOP', 'COLLECTOR_FAILURE', 'NORMAL_SHUTDOWN']);
  const recordKeys = ['schemaVersion', 'kind', 'sequence', 'previousHash', 'sessionId', 'tournamentId', 'scopeHash', 'at', 'payload', 'hash'];
  const bookKeys = ['tournamentId', 'marketId', 'exchangeId', 'instrument', 'receivedAt', 'requestStartedAt', 'sourceAt',
    'sequence', 'versionKey', 'bids', 'asks', 'freshnessPolicy', 'sourceEventAgeMs'];
  let sequence = 0, previousHash = '', lastAt = null, sessionId = null, closed = true, books = 0;
  let firstAt = null, lastAtText = null;
  const verifyLevels = (levels, descending) => {
    need(Array.isArray(levels) && levels.length > 0 && levels.length <= 20, 'SOURCE_LEVEL_COUNT');
    let prior = null;
    for (const row of levels) {
      need(exactKeys(row, ['price', 'quantity']) && Number.isSafeInteger(row.quantity) && row.quantity > 0, 'SOURCE_LEVEL');
      const parsed = micros(row.price, 'SOURCE');
      need(prior === null || (descending ? parsed < prior : parsed > prior), 'SOURCE_LEVEL_ORDER');
      prior = parsed;
    }
  };
  function observe(record) {
    need(exactKeys(record, recordKeys), 'SOURCE_RECORD_SHAPE');
    const {hash, ...body} = record;
    need(body.schemaVersion === 1 && Number.isSafeInteger(body.sequence) && body.sequence === sequence + 1
      && body.previousHash === previousHash && hash === payloadHash(body), 'SOURCE_CHAIN');
    need(body.tournamentId === scope.tournamentId && body.scopeHash === scope.scopeHash
      && typeof body.sessionId === 'string' && /^[A-Za-z0-9][A-Za-z0-9:_.-]{0,127}$/.test(body.sessionId), 'SOURCE_SCOPE');
    const atMs = instant(body.at);
    need(atMs !== null && (lastAt === null || atMs >= lastAt), 'SOURCE_CLOCK');
    if (body.kind === 'session_start') {
      need(exactKeys(body.payload, ['scope']) && payloadHash(body.payload.scope) === payloadHash(scope.scope)
        && !sessions.has(body.sessionId), 'SOURCE_SESSION_START');
      sessions.add(body.sessionId); sessionId = body.sessionId; closed = false;
    } else {
      need(!closed && body.sessionId === sessionId, 'SOURCE_SESSION_CLOSED');
      if (body.kind === 'book') {
        need(exactKeys(body.payload, ['book']) && exactKeys(body.payload.book, bookKeys), 'SOURCE_BOOK_SHAPE');
        const book = body.payload.book;
        need(book.tournamentId === scope.tournamentId && contracts.has(keyOf(book.marketId, book.exchangeId))
          && book.instrument === book.exchangeId + ':YES' && book.freshnessPolicy === 'authoritative_receipt_v1', 'SOURCE_BOOK_SCOPE');
        const received = instant(book.receivedAt), started = instant(book.requestStartedAt), source = instant(book.sourceAt);
        need(received !== null && started !== null && source !== null && started <= received && received <= atMs
          && received - started <= 20_000 && atMs - received <= 20_000 && source <= received + 2_000
          && Number.isSafeInteger(book.sourceEventAgeMs) && book.sourceEventAgeMs === received - source, 'SOURCE_BOOK_CLOCK');
        need(typeof book.sequence === 'string' && /^(0|[1-9]\d*)$/.test(book.sequence)
          && Number.isSafeInteger(Number(book.sequence)) && book.versionKey === book.sequence + ':' + book.sourceAt, 'SOURCE_BOOK_VERSION');
        verifyLevels(book.bids, true); verifyLevels(book.asks, false);
        need(amount(book.bids[0].price) < amount(book.asks[0].price), 'SOURCE_BOOK_CROSSED');
        const key = keyOf(book.marketId, book.exchangeId), old = sourceVersions.get(key);
        const depthHash = payloadHash({bids: book.bids, asks: book.asks});
        if (old) need(Number(book.sequence) >= old.sequence && source >= old.sourceMs
          && received >= old.receivedMs && (book.sequence !== old.sequenceText
            || (book.versionKey === old.versionKey && depthHash === old.depthHash)), 'SOURCE_BOOK_ROLLBACK');
        sourceVersions.set(key, {sequence: Number(book.sequence), sequenceText: book.sequence, sourceMs: source,
          receivedMs: received, versionKey: book.versionKey, depthHash});
        books += 1;
      } else if (body.kind === 'invalidation') {
        need(exactKeys(body.payload, ['marketId', 'reason']) && reasons.has(body.payload.reason)
          && (body.payload.marketId === null || scope.scope.exchanges.some(row => row.marketId === body.payload.marketId)), 'SOURCE_INVALIDATION');
      } else if (body.kind === 'session_stop') {
        need(exactKeys(body.payload, ['reason']) && stopReasons.has(body.payload.reason), 'SOURCE_SESSION_STOP');
        closed = true;
      } else need(false, 'SOURCE_KIND');
    }
    sequence = body.sequence; previousHash = hash; lastAt = atMs; firstAt ??= body.at; lastAtText = body.at;
    return body.kind === 'book' ? record.payload.book : null;
  }
  return {observe, stats: () => ({records: sequence, books, sessions: sessions.size, firstAt, lastAt: lastAtText, tailHash: previousHash})};
}

export function createOfficialFillBookTracker({fillRows, fillPageHasMore, orderAcknowledgements}) {
  need(Array.isArray(fillRows) && typeof fillPageHasMore === 'boolean' && orderAcknowledgements?.orders instanceof Map, 'TRACKER_INPUT');
  const seenFills = new Set();
  const rowsByMarketExchange = new Map();
  const excluded = {duplicateFillIds: 0, missingOrderId: 0, noBaalOrderAck: 0, conflictingOrderAck: 0, orderScopeMismatch: 0, invalidFill: 0};
  let duplicateFillIds = 0;
  let missingOrderId = 0;
  let noBaalOrderAck = 0;
  let conflictingOrderAck = 0;
  let orderScopeMismatch = 0;
  let invalidFill = 0;
  let matched = 0;
  const matchedOrders = new Set();
  const matchedMarkets = new Set();

  for (const fill of fillRows) {
    if (!fill || typeof fill !== 'object' || !id(String(fill.id ?? ''))) { invalidFill += 1; continue; }
    const fillId = String(fill.id);
    if (seenFills.has(fillId)) { duplicateFillIds += 1; continue; }
    seenFills.add(fillId);
    const orderId = fill.orderId == null ? '' : String(fill.orderId);
    if (!orderId || !id(orderId)) { missingOrderId += 1; continue; }
    const ack = orderAcknowledgements.orders.get(orderId);
    if (!ack) {
      if (orderAcknowledgements.conflictedOrderIds?.has(orderId)) conflictingOrderAck += 1;
      else noBaalOrderAck += 1;
      continue;
    }
    const marketId = String(fill.marketId ?? '');
    const exchangeId = String(fill.exchangeId ?? '');
    const atMs = instant(fill.at);
    let fillPrice;
    try { fillPrice = micros(fill.price, 'FILL'); } catch { invalidFill += 1; continue; }
    if (!id(marketId) || !id(exchangeId) || exchangeId !== ack.exchangeId || fill.side !== ack.side
      || atMs === null || !Number.isSafeInteger(fill.quantity) || fill.quantity <= 0
      || ack.tournamentId == null || ack.tournamentId === '') {
      orderScopeMismatch += 1;
      continue;
    }
    const row = {
      orderId,
      marketId,
      exchangeId,
      tournamentId: ack.tournamentId,
      side: ack.side,
      action: ack.action,
      quantity: fill.quantity,
      fillPrice,
      fillMs: atMs,
      ackAtMs: ack.ackAtMs,
      pre: null,
      nextCausal: null,
      nearbyBookReceipts: {sameMarketBefore: 0, sameMarketAfter: 0, otherMarketBefore: 0, otherMarketAfter: 0},
      post: new Map(HORIZONS.map(horizon => [horizon, null])),
    };
    const key = keyOf(marketId, exchangeId);
    if (!rowsByMarketExchange.has(key)) rowsByMarketExchange.set(key, []);
    rowsByMarketExchange.get(key).push(row);
    matched += 1;
    matchedOrders.add(orderId);
    matchedMarkets.add(marketId);
  }
  const allFills = [...rowsByMarketExchange.values()].flat();

  function observeBook(book) {
    if (!book || typeof book !== 'object' || book.instrument !== String(book.exchangeId) + ':YES') return;
    const receivedMs = instant(book.receivedAt);
    const sourceMs = instant(book.sourceAt);
    if (receivedMs === null || sourceMs === null || sourceMs > receivedMs + 2_000) return;
    const normalized = {book, receivedMs, sourceMs};
    for (const fill of allFills) {
      if (book.tournamentId !== fill.tournamentId || Math.abs(receivedMs - fill.fillMs) > 30_000) continue;
      const sameMarket = String(book.marketId) === fill.marketId;
      const side = receivedMs <= fill.fillMs ? 'Before' : 'After';
      const category = (sameMarket ? 'sameMarket' : 'otherMarket') + side;
      fill.nearbyBookReceipts[category] += 1;
    }
    const rows = rowsByMarketExchange.get(keyOf(book.marketId, book.exchangeId));
    if (!rows) return;
    for (const fill of rows) {
      if (book.tournamentId !== fill.tournamentId) continue;
      if (receivedMs <= fill.fillMs && sourceMs <= fill.fillMs
        && (!fill.pre || receivedMs > fill.pre.receivedMs)) fill.pre = normalized;
      if (receivedMs > fill.fillMs && sourceMs > fill.fillMs
        && (!fill.nextCausal || receivedMs < fill.nextCausal.receivedMs)) fill.nextCausal = normalized;
      for (const horizon of HORIZONS) {
        const target = fill.fillMs + horizon;
        if (receivedMs < target || sourceMs < target || sourceMs > target + 5_000 || receivedMs > target + 10_000) continue;
        const current = fill.post.get(horizon);
        if (!current || receivedMs < current.receivedMs) fill.post.set(horizon, normalized);
      }
    }
  }

  function finish() {
    const buckets = {missing: 0, atMost1s: 0, oneToFiveSeconds: 0, fiveToTwentySeconds: 0, olderThanTwentySeconds: 0};
    let freshContext = 0;
    let preTouchImprovementMicros = 0n;
    let preTouchShares = 0n;
    for (const fill of allFills) {
      if (!fill.pre) { buckets.missing += 1; continue; }
      const age = fill.fillMs - fill.pre.receivedMs;
      if (age <= 1_000) buckets.atMost1s += 1;
      else if (age <= 5_000) buckets.oneToFiveSeconds += 1;
      else if (age <= 20_000) buckets.fiveToTwentySeconds += 1;
      else buckets.olderThanTwentySeconds += 1;
      if (age <= 5_000) {
        const priorBook = contractBook(fill.pre.book, fill.side);
        const touch = fill.action === 'buy' ? priorBook.asks[0].price : priorBook.bids[0].price;
        const improvement = fill.action === 'buy' ? touch - fill.fillPrice : fill.fillPrice - touch;
        preTouchImprovementMicros += improvement * BigInt(fill.quantity);
        preTouchShares += BigInt(fill.quantity);
        freshContext += 1;
      }
    }

    const laterLags = allFills.flatMap(fill => fill.nextCausal ? [fill.nextCausal.receivedMs - fill.fillMs] : []);
    const laterSourceLags = allFills.flatMap(fill => fill.nextCausal ? [fill.nextCausal.sourceMs - fill.fillMs] : []);
    const ackResponseLags = allFills.flatMap(fill => fill.ackAtMs === null ? [] : [fill.fillMs - fill.ackAtMs]);
    const positiveAckLags = ackResponseLags.filter(lag => lag > 0);
    const negativeAckLags = ackResponseLags.filter(lag => lag < 0).map(lag => -lag);
    const ackResponseBuckets = {missingAckTimestamp: allFills.filter(fill => fill.ackAtMs === null).length,
      fillBeforeOrAtAckReceipt: ackResponseLags.filter(lag => lag <= 0).length,
      fillAfterAckReceiptAtMost1s: 0, fillAfterAckReceiptOneToThirtySeconds: 0,
      fillAfterAckReceiptThirtySecondsToFiveMinutes: 0, fillAfterAckReceiptFiveMinutesToOneHour: 0,
      fillAfterAckReceiptOverOneHour: 0};
    for (const lag of positiveAckLags) {
      if (lag <= 1_000) ackResponseBuckets.fillAfterAckReceiptAtMost1s++;
      else if (lag <= 30_000) ackResponseBuckets.fillAfterAckReceiptOneToThirtySeconds++;
      else if (lag <= 300_000) ackResponseBuckets.fillAfterAckReceiptThirtySecondsToFiveMinutes++;
      else if (lag <= 3_600_000) ackResponseBuckets.fillAfterAckReceiptFiveMinutesToOneHour++;
      else ackResponseBuckets.fillAfterAckReceiptOverOneHour++;
    }
    const continuityBuckets = {missing: 0, atMost30Seconds: 0, thirtySecondsToFiveMinutes: 0,
      fiveToSixtyMinutes: 0, oneToTwentyFourHours: 0, olderThanTwentyFourHours: 0};
    for (const fill of allFills) {
      if (!fill.nextCausal) { continuityBuckets.missing++; continue; }
      const delay = fill.nextCausal.receivedMs - fill.fillMs;
      if (delay <= 30_000) continuityBuckets.atMost30Seconds++;
      else if (delay <= 300_000) continuityBuckets.thirtySecondsToFiveMinutes++;
      else if (delay <= 3_600_000) continuityBuckets.fiveToSixtyMinutes++;
      else if (delay <= 86_400_000) continuityBuckets.oneToTwentyFourHours++;
      else continuityBuckets.olderThanTwentyFourHours++;
    }
    const sameMarketWindows = allFills.filter(fill => fill.nearbyBookReceipts.sameMarketBefore
      + fill.nearbyBookReceipts.sameMarketAfter > 0).length;
    const otherMarketWindows = allFills.filter(fill => fill.nearbyBookReceipts.otherMarketBefore
      + fill.nearbyBookReceipts.otherMarketAfter > 0).length;
    const bookCoverageNearFill = {
      windowMsEachSide: 30_000,
      observationUnit: 'Validated book receipt events counted per matched fill window; overlapping windows count the same event more than once.',
      matchedFillRows: allFills.length,
      fillsWithSameMarketBookReceipt: sameMarketWindows,
      fillsWithOtherMarketBookReceipt: otherMarketWindows,
      fillsWithOtherMarketButNoSameMarketBookReceipt: allFills.filter(fill => fill.nearbyBookReceipts.sameMarketBefore
        + fill.nearbyBookReceipts.sameMarketAfter === 0 && fill.nearbyBookReceipts.otherMarketBefore
        + fill.nearbyBookReceipts.otherMarketAfter > 0).length,
      fillsWithNoBookReceiptInWindow: allFills.filter(fill => fill.nearbyBookReceipts.sameMarketBefore
        + fill.nearbyBookReceipts.sameMarketAfter + fill.nearbyBookReceipts.otherMarketBefore
        + fill.nearbyBookReceipts.otherMarketAfter === 0).length,
      receiptsPerFillWindow: {
        sameMarket: allFills.reduce((sum, fill) => sum + fill.nearbyBookReceipts.sameMarketBefore
          + fill.nearbyBookReceipts.sameMarketAfter, 0),
        otherMarket: allFills.reduce((sum, fill) => sum + fill.nearbyBookReceipts.otherMarketBefore
          + fill.nearbyBookReceipts.otherMarketAfter, 0),
      },
      receiptDirectionCounts: {
        sameMarketBeforeFill: allFills.reduce((sum, fill) => sum + fill.nearbyBookReceipts.sameMarketBefore, 0),
        sameMarketAfterFill: allFills.reduce((sum, fill) => sum + fill.nearbyBookReceipts.sameMarketAfter, 0),
        otherMarketBeforeFill: allFills.reduce((sum, fill) => sum + fill.nearbyBookReceipts.otherMarketBefore, 0),
        otherMarketAfterFill: allFills.reduce((sum, fill) => sum + fill.nearbyBookReceipts.otherMarketAfter, 0),
      },
    };

    const horizonResults = {};
    for (const horizon of HORIZONS) {
      let bookMatched = 0;
      let depthMatched = 0;
      let partialDepth = 0;
      let markedShares = 0n;
      let executableMicros = 0n;
      let midTwiceMicros = 0n;
      let positiveFills = 0;
      let fullFills = 0;
      let actualSourceDelay = 0;
      let actualReceiptDelay = 0;
      const perFillExecutable = [];
      const perFillMidTwice = [];
      const orders = new Set();
      const markets = new Set();
      const candidateShares = allFills.reduce((total, fill) => total + BigInt(fill.quantity), 0n);
      for (const fill of allFills) {
        const observation = fill.post.get(horizon);
        if (!observation) continue;
        bookMatched += 1;
        actualSourceDelay += observation.sourceMs - (fill.fillMs + horizon);
        actualReceiptDelay += observation.receivedMs - (fill.fillMs + horizon);
        const book = contractBook(observation.book, fill.side);
        const exitLevels = fill.action === 'buy' ? book.bids : book.asks;
        const walked = walk(exitLevels, fill.quantity);
        if (!walked.coveredShares) continue;
        depthMatched += 1;
        if (walked.coveredShares < fill.quantity) partialDepth += 1;
        else fullFills += 1;
        const covered = BigInt(walked.coveredShares);
        const entryValue = fill.fillPrice * covered;
        const signedExecutable = fill.action === 'buy' ? walked.valueMicros - entryValue : entryValue - walked.valueMicros;
        executableMicros += signedExecutable;
        markedShares += covered;
        const executablePerShare = signedExecutable / covered;
        perFillExecutable.push(executablePerShare);
        if (signedExecutable > 0n) positiveFills += 1;
        const midTwice = book.bids[0].price + book.asks[0].price;
        const signedMidTwice = (fill.action === 'buy' ? 1n : -1n)
          * (midTwice - 2n * fill.fillPrice) * covered;
        midTwiceMicros += signedMidTwice;
        perFillMidTwice.push(signedMidTwice / covered);
        orders.add(fill.orderId);
        markets.add(fill.marketId);
      }
      horizonResults[String(horizon)] = {
        candidateFillRows: matched,
        observedLaterBooks: bookMatched,
        depthPricedFillRows: depthMatched,
        fullDepthFillRows: fullFills,
        partialDepthFillRows: partialDepth,
        unmarkedShares: Number(candidateShares - markedShares),
        markedShares: Number(markedShares),
        uniqueOrders: orders.size,
        uniqueMarkets: markets.size,
        meanSourceDelayMs: bookMatched ? Math.round(actualSourceDelay / bookMatched) : null,
        meanReceiptDelayMs: bookMatched ? Math.round(actualReceiptDelay / bookMatched) : null,
        weightedExecutableMarkoutPerShare: formatRatio(executableMicros, markedShares),
        medianExecutableMarkoutPerFillShare: median(perFillExecutable) == null ? null : money(median(perFillExecutable)),
        positiveExecutableFillFraction: depthMatched ? positiveFills / depthMatched : null,
        weightedMidMarkoutPerShare: markedShares ? formatRatio(midTwiceMicros, 2n * markedShares) : null,
        medianMidMarkoutPerFillShare: median(perFillMidTwice) == null ? null : money(median(perFillMidTwice) / 2n),
      };
    }
    return {
      schemaVersion: 1,
      mode: 'official_baal_fill_book_markout_v1',
      coverage: {
        officialFillRowsReceived: fillRows.length,
        officialFillPageHasMore: fillPageHasMore,
        fullFillHistoryAvailable: !fillPageHasMore,
        uniqueFillRows: seenFills.size,
        duplicateFillIds,
        matchedBaalFillRows: matched,
        uniqueMatchedOrders: matchedOrders.size,
        uniqueMatchedMarkets: matchedMarkets.size,
        excluded: {missingOrderId, noBaalOrderAck, conflictingOrderAck, orderScopeMismatch, invalidFill},
        preFillBookReceiptAgeBuckets: buckets,
        preFillContextAtMostFiveSeconds: freshContext,
        preFillPriceImprovementPerShare: formatRatio(preTouchImprovementMicros, preTouchShares),
      },
      bookContinuity: {
        definition: 'First same-contract book with both source time and receipt time strictly after each matched official fill.',
        fillsWithAnyLaterCausalBook: laterLags.length,
        fillsWithNoLaterCausalBook: allFills.length - laterLags.length,
        firstLaterBookReceiptLagMs: summarizeLags(laterLags),
        firstLaterBookSourceLagMs: summarizeLags(laterSourceLags),
        laterBookReceiptLagBuckets: continuityBuckets,
      },
      bookCoverageNearFill,
      fillVsAckResponse: {
        definition: 'Official fill timestamp minus the local acknowledged-order response receipt timestamp; this is not time since request submission.',
        fillsWithAckReceiptTimestamp: ackResponseLags.length,
        lagMs: {negativeMeansFillPrecededAckResponse: summarizeLags(negativeAckLags),
          positiveMeansFillFollowedAckResponse: summarizeLags(positiveAckLags)},
        buckets: ackResponseBuckets,
      },
      horizons: horizonResults,
      assumptions: {
        feesPerShare: '0.000000',
        queueFillModelUsed: false,
        anonymousTradeProxyUsed: false,
        actualOfficialFillsOnly: true,
        realizedPortfolioPnl: false,
        markoutDefinition: 'Each official fill is hypothetically liquidated at the first causal public book at or after the horizon; depth is walked and incomplete depth is left unmarked.',
        profitabilityEstablished: false,
        executionAuthority: false,
        contestOrdersPlaced: 0,
        apiRequests: 0,
      },
      limitations: [
        fillPageHasMore ? 'The cached official fill page is incomplete; results are a partial recent sample.' : 'Fill page reports complete coverage.',
        'Order-level clustering and incomplete cached history limit inference; markouts are not realized strategy P&L.',
        'Public-book receipt time is not the exchange fill instant; only causal books received at or after each horizon are used.',
      ],
      privacy: {aggregateOnly: true, rawAccountRowsWritten: false, identifiersWritten: false},
    };
  }

  return {
    observeBook,
    finish,
    diagnostics: {
      fillRows: fillRows.length,
      fillPageHasMore,
      receiptRowsVerified: orderAcknowledgements.receiptRowsVerified,
      orderAckRowsMatched: orderAcknowledgements.matchedOrderAckRows,
      orderAckConflicts: orderAcknowledgements.conflictingOrderIds,
      matchedBaalFillRows: matched,
      excluded: {duplicateFillIds, missingOrderId, noBaalOrderAck, conflictingOrderAck, orderScopeMismatch, invalidFill},
    },
  };
}

export {HORIZONS};

