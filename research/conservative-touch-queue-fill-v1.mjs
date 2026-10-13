import { normalizeOfficialFillTick } from '../execution/api-tick-price.mjs';
import { instantNs } from '../feed/contract.mjs';

const NS_PER_MS = 1_000_000n;
const msNs = value => BigInt(value) * NS_PER_MS;
const ns = value => instantNs(value);
const key = value => `${value.marketId}|${value.exchangeId}`;

export const QUEUE_FILL_POLICY = Object.freeze({
  sampleEveryMs: 30_000,
  activationLatencyMs: 3_000,
  quoteTtlAfterActivationMs: 10_000,
  maximumBookAgeMs: 3_000,
  markoutHorizonsMs: Object.freeze([1_000, 5_000, 30_000]),
  fees: '0.000000',
  oneTickEachWayStress: '0.010000',
});

const spreadBin = ticks => ticks <= 1 ? '1 tick' : ticks === 2 ? '2 ticks' : '3+ ticks';
const queueBin = shares => shares <= 10 ? '1-10' : shares <= 50 ? '11-50' : shares <= 250 ? '51-250' : '251+';
const BINS = Object.freeze({
  bySpread: Object.freeze(['1 tick', '2 ticks', '3+ ticks']),
  byQueueAhead: Object.freeze(['1-10', '11-50', '51-250', '251+']),
});

function tickPrice(value) {
  const normalized = normalizeOfficialFillTick(value);
  return { tick: normalized.tick, price: normalized.price };
}

function clusterSummary(rows) {
  const perMarket = new Map();
  for (const row of rows) {
    const marketId = row.member?.marketId ?? row.marketId;
    if (typeof marketId === 'string') perMarket.set(marketId, (perMarket.get(marketId) ?? 0) + 1);
  }
  const sum = [...perMarket.values()].reduce((a, b) => a + b, 0);
  const squares = [...perMarket.values()].reduce((a, b) => a + b * b, 0);
  return {
    quoteMarketClusters: perMarket.size,
    effectiveIndependentQuoteMarkets: squares ? sum * sum / squares : null,
    fillOutcomeMarketClusters: 0,
    effectiveIndependentFillMarkets: null,
    clusteredStandardError: null,
    clustered95PercentInterval: null,
    uncertaintyStatus: 'unavailable_without_confirmed_queue_fill_outcomes',
  };
}

export function createConservativeTouchQueueFillV1({ members, policy = {} }) {
  const config = { ...QUEUE_FILL_POLICY, ...policy };
  if (!Array.isArray(members) || !members.length) throw Error('QUEUE_FILL_SCOPE');
  if (![config.sampleEveryMs, config.activationLatencyMs, config.quoteTtlAfterActivationMs, config.maximumBookAgeMs].every(Number.isSafeInteger)
    || config.sampleEveryMs <= config.activationLatencyMs + config.quoteTtlAfterActivationMs
    || config.activationLatencyMs < 0 || config.quoteTtlAfterActivationMs < 1 || config.maximumBookAgeMs < 1
    || !Array.isArray(config.markoutHorizonsMs) || !config.markoutHorizonsMs.length
    || !config.markoutHorizonsMs.every(Number.isSafeInteger) || new Set(config.markoutHorizonsMs).size !== config.markoutHorizonsMs.length) {
    throw Error('QUEUE_FILL_POLICY');
  }

  const memberByKey = new Map();
  const membersByTopic = new Map();
  for (const member of members) {
    if (!member || typeof member.marketId !== 'string' || typeof member.exchangeId !== 'string'
      || typeof member.topic !== 'string' || typeof member.tournamentId !== 'string') throw Error('QUEUE_FILL_MEMBER');
    const memberKey = key(member);
    if (memberByKey.has(memberKey)) throw Error('QUEUE_FILL_DUPLICATE_MEMBER');
    memberByKey.set(memberKey, member);
    const topicMembers = membersByTopic.get(member.topic) ?? [];
    topicMembers.push(member);
    membersByTopic.set(member.topic, topicMembers);
  }

  const stateByKey = new Map([...memberByKey.keys()].map(k => [k, { lastBook: null, lastSlot: null }]));
  const candidates = [];
  const openByKey = new Map([...memberByKey.keys()].map(k => [k, new Set()]));
  const tradeSeen = new Map();
  const revisionByTopicEpoch = new Map();
  const epochByTopic = new Map();
  let lastObservedNs = -1n;
  const counts = {
    marketBatchRecords: 0, acceptedBookPoints: 0, rejectedBookPoints: 0,
    tradeReportsSeen: 0, uniqueScopedTradeReports: 0, duplicateTradeReports: 0,
    invalidTradeReports: 0, candidateQuotes: 0, activatedQuotes: 0,
    staleCensoredQuotes: 0, priceMoveCensoredQuotes: 0, gapCensoredQuotes: 0,
    invalidationCensoredQuotes: 0, captureBoundaryCensoredQuotes: 0,
    expiredQuotes: 0, observedSamePriceQueueIncreases: 0,
    observedDisplayReductionsIgnored: 0, touchPriceTradeReportsUnknownSide: 0,
    touchPriceTradeQuantityUnknownSide: 0,
  };
  let currentFold = 'development';
  let currentSourceDay = 'unknown';

  function endCandidate(candidate, status, endAtNs = null) {
    if (candidate.status !== 'pending' && candidate.status !== 'active') return;
    candidate.status = status;
    candidate.endAtNs = endAtNs;
    openByKey.get(candidate.memberKey)?.delete(candidate);
    if (status === 'censored_stale') counts.staleCensoredQuotes++;
    else if (status === 'censored_price_move') counts.priceMoveCensoredQuotes++;
    else if (status === 'censored_gap') counts.gapCensoredQuotes++;
    else if (status === 'censored_invalidation') counts.invalidationCensoredQuotes++;
    else if (status === 'censored_capture_boundary') counts.captureBoundaryCensoredQuotes++;
    else if (status === 'expired') counts.expiredQuotes++;
  }

  function boundary(topic, reason = 'invalidation', at = null, exchangeId = null) {
    const atNs = at === null ? lastObservedNs : ns(at);
    if (exchangeId === null) {
      revisionByTopicEpoch.delete(topic);
      epochByTopic.delete(topic);
    }
    for (const member of membersByTopic.get(topic) ?? []) {
      if (exchangeId !== null && member.exchangeId !== exchangeId) continue;
      const memberKey = key(member);
      const state = stateByKey.get(memberKey);
      state.lastBook = null;
      if (exchangeId === null) state.lastSlot = null;
      for (const candidate of [...(openByKey.get(memberKey) ?? [])]) {
        const reasonMap = {
          gap: 'censored_gap', invalidation: 'censored_invalidation',
          capture_boundary: 'censored_capture_boundary',
        };
        endCandidate(candidate, reasonMap[reason] ?? 'censored_invalidation', atNs);
      }
    }
  }

  function advance(at, topic = null) {
    const atNs = ns(at);
    const memberKeys = topic === null
      ? [...memberByKey.keys()]
      : (membersByTopic.get(topic) ?? []).map(member => key(member));
    for (const memberKey of memberKeys) {
      const state = stateByKey.get(memberKey);
      const latest = state.lastBook;
      for (const candidate of [...(openByKey.get(memberKey) ?? [])]) {
        if (!latest || atNs < latest.receivedNs || atNs - latest.receivedNs > msNs(config.maximumBookAgeMs)) {
          endCandidate(candidate, 'censored_stale', atNs);
          continue;
        }
        if (candidate.status === 'pending' && atNs >= candidate.activationNs) {
          candidate.status = 'active';
          candidate.activated = true;
          counts.activatedQuotes++;
        }
        if (candidate.status === 'active' && atNs >= candidate.expiryNs) endCandidate(candidate, 'expired', candidate.expiryNs);
      }
    }
  }

  function observeBook(book, { observedAt } = {}) {
    const memberKey = key(book ?? {});
    const member = memberByKey.get(memberKey);
    if (!member || book.tournamentId !== member.tournamentId || typeof book.sequence !== 'string' || !/^(0|[1-9]\d*)$/.test(book.sequence)) {
      counts.rejectedBookPoints++;
      return false;
    }
    let sourceNs, receivedNs, observedNs, seq;
    let bid, ask, bidQty, askQty;
    try {
      sourceNs = ns(book.sourceAt);
      receivedNs = ns(book.receivedAt);
      observedNs = ns(observedAt);
      seq = BigInt(book.sequence);
      bid = tickPrice(book.bids?.[0]?.price);
      ask = tickPrice(book.asks?.[0]?.price);
      bidQty = book.bids?.[0]?.quantity;
      askQty = book.asks?.[0]?.quantity;
      if (!Number.isSafeInteger(bidQty) || bidQty < 1 || !Number.isSafeInteger(askQty) || askQty < 1
        || bid.tick >= ask.tick || receivedNs > observedNs || observedNs < sourceNs
        || observedNs - sourceNs > msNs(config.maximumBookAgeMs)) throw Error('BOOK_INVALID');
    } catch {
      counts.rejectedBookPoints++;
      boundary(member.topic, 'invalidation', observedAt, member.exchangeId);
      return false;
    }

    const state = stateByKey.get(memberKey);
    const prior = state.lastBook;
    if (prior && (seq <= prior.seq || sourceNs <= prior.sourceNs || receivedNs < prior.receivedNs)) {
      counts.rejectedBookPoints++;
      boundary(member.topic, 'invalidation', observedAt, member.exchangeId);
      return false;
    }

    counts.acceptedBookPoints++;
    const next = { seq, sourceNs, receivedNs, bid, ask, bidQty, askQty };
    if (prior) {
      for (const candidate of [...(openByKey.get(memberKey) ?? [])]) {
        if (bid.tick !== candidate.initialBidTick || ask.tick !== candidate.initialAskTick) {
          endCandidate(candidate, 'censored_price_move', sourceNs);
          continue;
        }
        const displayedQty = candidate.action === 'buy' ? bidQty : askQty;
        const delta = displayedQty - candidate.lastDisplayedQty;
        if (delta > 0) {
          candidate.queueAheadShares += delta;
          candidate.queueIncreaseShares += delta;
          counts.observedSamePriceQueueIncreases++;
        } else if (delta < 0) {
          candidate.displayReductionsIgnored++;
          counts.observedDisplayReductionsIgnored++;
        }
        candidate.lastDisplayedQty = displayedQty;
        candidate.lastBookSourceAt = book.sourceAt;
        candidate.lastBookReceivedAt = book.receivedAt;
      }
    }
    state.lastBook = next;

    const slot = Math.floor(Number(receivedNs / NS_PER_MS) / config.sampleEveryMs);
    if (state.lastSlot !== slot) {
      state.lastSlot = slot;
      const spreadTicks = ask.tick - bid.tick;
      for (const action of ['buy', 'sell']) {
        const limitTick = action === 'buy' ? bid.tick : ask.tick;
        const initialQueue = action === 'buy' ? bidQty : askQty;
        candidates.push({
          id: candidates.length + 1, memberKey, member, action, limitTick,
          limitPrice: action === 'buy' ? bid.price : ask.price, sourceDay: currentSourceDay,
          initialBidTick: bid.tick, initialAskTick: ask.tick, fold: currentFold,
          spreadTicks, initialQueueShares: initialQueue, queueAheadShares: initialQueue,
          queueIncreaseShares: 0, displayReductionsIgnored: 0,
          lastDisplayedQty: initialQueue, lastBookSourceAt: book.sourceAt,
          lastBookReceivedAt: book.receivedAt,
          decisionReceiptAt: book.receivedAt,
          activationAt: new Date(Number((receivedNs + msNs(config.activationLatencyMs)) / NS_PER_MS)).toISOString(),
          activationNs: receivedNs + msNs(config.activationLatencyMs),
          expiryNs: receivedNs + msNs(config.activationLatencyMs + config.quoteTtlAfterActivationMs),
          status: 'pending', endAtNs: null,
          activated: false,
          touchPriceReportsUnknownSide: 0, touchPriceQuantityUnknownSide: 0,
        });
        openByKey.get(memberKey).add(candidates.at(-1));
        counts.candidateQuotes++;
      }
    }
    return true;
  }

  function observeTrade(trade, { receivedAt, observedAt } = {}) {
    counts.tradeReportsSeen++;
    const member = trade && memberByKey.get(key(trade));
    if (!member || trade.tournamentId !== member.tournamentId || typeof trade.id !== 'string' || !trade.id
      || !Number.isSafeInteger(trade.sequence) || trade.sequence < 0
      || !Number.isSafeInteger(trade.quantity) || trade.quantity < 1) {
      counts.invalidTradeReports++;
      return false;
    }
    let executedNs, receiptNs, observedNs, price;
    try {
      executedNs = ns(trade.executedAt);
      receiptNs = ns(receivedAt);
      observedNs = ns(observedAt);
      price = tickPrice(trade.price);
      if (executedNs > receiptNs || receiptNs > observedNs) throw Error('TRADE_CLOCK');
    } catch {
      counts.invalidTradeReports++;
      return false;
    }
    const identity = `${key(trade)}|${trade.id}`;
    const signature = `${trade.sequence}|${trade.executedAt}|${price.tick}|${trade.quantity}`;
    const prior = tradeSeen.get(identity);
    if (prior !== undefined) {
      if (prior !== signature) throw Error('QUEUE_FILL_TRADE_ID_CONFLICT');
      counts.duplicateTradeReports++;
      return false;
    }
    tradeSeen.set(identity, signature);
    counts.uniqueScopedTradeReports++;

    for (const candidate of openByKey.get(key(trade)) ?? []) {
      if (candidate.status !== 'active' || price.tick !== candidate.limitTick) continue;
      if (executedNs < candidate.activationNs || executedNs >= candidate.expiryNs || receiptNs < candidate.activationNs || receiptNs >= candidate.expiryNs) continue;
      // A same-price print is only an unknown-side observation. Captured trade
      // rows have no documented aggressor-side or book-queue attribution.
      candidate.touchPriceReportsUnknownSide++;
      candidate.touchPriceQuantityUnknownSide += trade.quantity;
      counts.touchPriceTradeReportsUnknownSide++;
      counts.touchPriceTradeQuantityUnknownSide += trade.quantity;
    }
    return true;
  }

  function observeBatch({ topic, observedAt, receivedAt = observedAt, books = [], trades = [], revision = null, previousRevision = null, resyncRequired = false, settled = false }) {
    const observedNs = ns(observedAt);
    if (observedNs < lastObservedNs) throw Error('QUEUE_FILL_CLOCK_ROLLBACK');
    lastObservedNs = observedNs;
    counts.marketBatchRecords++;
    advance(receivedAt, topic);
    if (!membersByTopic.has(topic)) return;
    const epochKey = topic;
    const priorRevision = revisionByTopicEpoch.get(epochKey);
    if (priorRevision !== undefined && previousRevision !== priorRevision) {
      boundary(topic, 'gap', observedAt);
    }
    if (!Number.isSafeInteger(revision) || !Number.isSafeInteger(previousRevision)) {
      boundary(topic, 'gap', observedAt);
      return;
    }
    revisionByTopicEpoch.set(epochKey, revision);
    if (resyncRequired || settled) {
      boundary(topic, 'invalidation', observedAt);
      return;
    }
    for (const book of books) observeBook(book, { observedAt });
    for (const trade of trades) observeTrade(trade, { receivedAt, observedAt });
  }

  function observeRecord(record, books = []) {
    const receivedAt = record.receivedAt ?? record.observedAt;
    const topic = record.topic;
    advance(receivedAt, topic);
    if (!membersByTopic.has(topic)) return;
    if (['subscribed', 'disconnected', 'stopped', 'rejected', 'recovery_started', 'recovery_finished'].includes(record.kind)) {
      boundary(topic, 'invalidation', record.observedAt);
      return;
    }
    if (record.kind === 'invalidation') {
      boundary(topic, 'invalidation', record.observedAt, record.exchangeId ? String(record.exchangeId) : null);
      return;
    }
    if (record.kind !== 'event' || record.event !== 'market_batch') {
      if (record.kind === 'event') boundary(topic, 'invalidation', record.observedAt);
      return;
    }
    const payload = record.payload;
    const delivery = payload?.delivery;
    if (!Number.isSafeInteger(record.epoch) || record.epoch < 1) {
      boundary(topic, 'gap', record.observedAt);
      return;
    }
    const priorEpoch = epochByTopic.get(topic);
    if (priorEpoch !== undefined && record.epoch !== priorEpoch) boundary(topic, 'gap', record.observedAt);
    if (Number.isSafeInteger(record.epoch)) epochByTopic.set(topic, record.epoch);
    if (payload?.resyncRequired !== false || payload.marketSettled?.length) {
      boundary(topic, 'invalidation', record.observedAt);
      return;
    }
    const dirtyExchanges = new Set((payload.bookDirty ?? []).map(item => String(item.exchangeId)));
    for (const exchangeId of dirtyExchanges) boundary(topic, 'invalidation', record.observedAt, exchangeId);
    observeBatch({
      topic, observedAt: record.observedAt, receivedAt,
      // A dirty hint censors prior quotes first. A valid versioned book in
      // this same packet can then seed a new quote from that fresh snapshot.
      books, trades: payload.trades ?? [],
      revision: delivery?.revision, previousRevision: delivery?.previousRevision,
      resyncRequired: payload.resyncRequired === true, settled: !!payload.marketSettled?.length,
    });
  }

  function endCapture(at) {
    advance(at);
    for (const topic of membersByTopic.keys()) boundary(topic, 'capture_boundary', at);
  }

  function report() {
    const rows = [];
    for (const fold of ['development', 'untouched_held_out']) {
      const selectedFold = candidates.filter(x => x.fold === fold);
      for (const action of ['buy', 'sell']) {
        const selected = selectedFold.filter(x => x.action === action);
        const touchMarkets = new Set(selected.filter(x => x.touchPriceReportsUnknownSide > 0).map(x => x.member.marketId));
        const base = {
          candidateQuotes: selected.length,
          activatedQuotes: selected.filter(x => x.activated).length,
          initialQueueAheadShares: selected.reduce((sum, x) => sum + x.initialQueueShares, 0),
          observedQueueIncreaseSharesAdded: selected.reduce((sum, x) => sum + x.queueIncreaseShares, 0),
          displayReductionsIgnored: selected.reduce((sum, x) => sum + x.displayReductionsIgnored, 0),
          touchPriceTradeReportsUnknownSide: selected.reduce((sum, x) => sum + x.touchPriceReportsUnknownSide, 0),
          touchPriceTradeQuantityUnknownSide: selected.reduce((sum, x) => sum + x.touchPriceQuantityUnknownSide, 0),
          touchPriceReportMarkets: touchMarkets.size,
          confirmedQueueFills: 0,
          markoutSamples: 0,
          meanExecutableNet: null,
          oneTickEachWayStressMean: null,
          marketClusterUncertainty: clusterSummary(selected),
        };
        for (const horizonMs of config.markoutHorizonsMs) {
          rows.push({ fold, action, horizonMs, ...base });
        }
      }
    }
    const table = (fold, action, horizonMs) => rows.find(x => x.fold === fold && x.action === action && x.horizonMs === horizonMs);
    const cells = [];
    for (const fold of ['development', 'untouched_held_out']) for (const action of ['buy', 'sell']) for (const horizonMs of config.markoutHorizonsMs) {
      const subset = candidates.filter(x => x.fold === fold && x.action === action);
      const cellMetrics = selected => ({
        candidateQuotes: selected.length,
        touchPriceTradeReportsUnknownSide: selected.reduce((sum, x) => sum + x.touchPriceReportsUnknownSide, 0),
        confirmedQueueFills: 0,
        markoutSamples: 0,
        meanExecutableNet: null,
        oneTickEachWayStressMean: null,
        marketClusterUncertainty: clusterSummary(selected),
      });
      cells.push({ fold, action, horizonMs, family: 'overall', bin: 'all', ...cellMetrics(subset) });
      for (const [family, bins, classify] of [
        ['bySpread', BINS.bySpread, x => spreadBin(x.spreadTicks)],
        ['byQueueAhead', BINS.byQueueAhead, x => queueBin(x.initialQueueShares)],
      ]) for (const bin of bins) {
        cells.push({ fold, action, horizonMs, family, bin, ...cellMetrics(subset.filter(x => classify(x) === bin)) });
      }
    }
    return {
      schemaVersion: 1,
      mode: 'conservative_displayed_queue_touch_audit_v1',
      status: 'indeterminate_no_confirmed_queue_fills',
      policy: {
        quote: 'one paired hypothetical YES best-bid/best-ask quote per stable exchange per 30-second receipt-time slot',
        activationLatencyMs: config.activationLatencyMs,
        quoteTtlAfterActivationMs: config.quoteTtlAfterActivationMs,
        maximumBookAgeMs: config.maximumBookAgeMs,
        initialQueueAhead: 'all displayed top-level quantity at the quote price',
        displayedIncreases: 'add each observed same-price top-level increase to an observed queue-ahead proxy; snapshots can miss intervening additions',
        displayedReductions: 'never reduce queue ahead for displayed-size decreases; no cancellation is treated as a trade',
        tradeSide: 'not inferred; captured trade reports have no aggressor side and sequence comparison does not establish queue-side attribution; trade id and sequence are retained as distinct fields',
        fillRule: 'no print is allowed to decrement queue or confirm a fill without a supported aggressor-side/queue relationship',
        markoutHorizonsMs: config.markoutHorizonsMs,
        fees: config.fees,
        oneTickEachWayStress: config.oneTickEachWayStress,
      },
      counts: {
        ...counts,
        confirmedQueueFills: 0,
        markoutSamples: 0,
        quotesByStatus: Object.fromEntries([...new Set(candidates.map(x => x.status))].sort().map(status => [status, candidates.filter(x => x.status === status).length])),
      },
      outcomeTables: rows,
      prespecifiedCells: cells,
      actualFillsProven: false,
      profitabilityEstablished: false,
      contestOrders: 0,
      networkRequests: 0,
      limitations: [
        'Captured trade rows carry price, quantity, execution time and a sequence/id, but no aggressor side; the capture contract does not document a sequence join that proves a trade consumed the displayed bid or ask queue.',
        'A sequence audit found that no trade in the three frozen prefixes was exactly one sequence after its immediately preceding captured book point; current same-batch book sequence was greater than every trade sequence. This is descriptive only and does not establish a shared ordering domain or aggressor side.',
        'Book snapshots do not expose order IDs or queue priority, and continuity between snapshots cannot prove that all intervening cancellations, additions, and executions are known.',
        'Observed same-price size increases are conservatively added, while observed decreases are ignored; unseen intermediate changes mean the displayed queue-ahead proxy is not a full queue history.',
        'Same-price print reports are shown only as unknown-side observations. They do not reduce queue-ahead shares and are not fill proxies.',
        'Every 1s/5s/30s outcome cell is empty because no queue fill can be established from these archived fields; fees/stress and P&L are therefore not estimable.',
        'Quote observations and unknown-side print counts are descriptive, clustered by market for coverage only; no outcome uncertainty interval or profitability claim is available.',
      ],
    };
  }

  function setFold(fold) {
    if (!['development', 'untouched_held_out'].includes(fold)) throw Error('QUEUE_FILL_FOLD');
    currentFold = fold;
  }

  function setSourceDay(day) {
    if (typeof day !== 'string' || !/^2026-10-\d{2}$/.test(day)) throw Error('QUEUE_FILL_SOURCE_DAY');
    currentSourceDay = day;
  }

  function queueAheadSnapshotRows() {
    const columns = [
      'sourceDay', 'fold', 'marketId', 'exchangeId', 'action', 'decisionReceiptAt', 'activationAt',
      'lastBookReceivedAt', 'status', 'endAt', 'spreadTicks', 'initialQueueAheadShares',
      'observedQueueIncreaseSharesAdded', 'observedDisplayReductionsIgnored', 'finalQueueAheadShares',
      'touchPriceTradeReportsUnknownSide', 'touchPriceTradeQuantityUnknownSide',
    ];
    return {
      columns,
      rows: candidates.map(candidate => [
        candidate.sourceDay, candidate.fold, candidate.member.marketId, candidate.member.exchangeId,
        candidate.action, candidate.decisionReceiptAt, candidate.activationAt, candidate.lastBookReceivedAt,
        candidate.status, candidate.endAtNs === null ? null : new Date(Number(candidate.endAtNs / NS_PER_MS)).toISOString(),
        candidate.spreadTicks, candidate.initialQueueShares, candidate.queueIncreaseShares,
        candidate.displayReductionsIgnored, candidate.queueAheadShares,
        candidate.touchPriceReportsUnknownSide, candidate.touchPriceQuantityUnknownSide,
      ]),
    };
  }

  function noteRejectedBookPoints(count) {
    if (!Number.isSafeInteger(count) || count < 0) throw Error('QUEUE_FILL_REJECTED_COUNT');
    counts.rejectedBookPoints += count;
  }

  return { observeRecord, observeBatch, observeBook, observeTrade, boundary, advance, endCapture, setFold, setSourceDay, noteRejectedBookPoints, report, queueAheadSnapshotRows, get candidates() { return candidates; } };
}

