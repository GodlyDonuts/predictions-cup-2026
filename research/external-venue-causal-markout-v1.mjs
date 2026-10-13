import {createHash} from 'node:crypto';

export const PROCESSING_LATENCY_MS = 3000;
export const SOURCE_AGE_LIMITS_MS = Object.freeze([30_000, 60_000, 120_000, 300_000]);
export const CENSOR_HORIZONS_MS = Object.freeze([0, 1_000, 5_000, 30_000, 60_000, 120_000, 300_000, 3_600_000]);
const fail = (condition, message) => { if (!condition) throw new Error(message); };
const isoMs = value => {
  const result = typeof value === 'number' ? value : Date.parse(value);
  fail(Number.isFinite(result), 'invalid timestamp');
  return result;
};
const sha256 = value => createHash('sha256').update(value).digest('hex');
const ownBody = (row, field) => Object.fromEntries(Object.entries(row).filter(([key]) => key !== field));
const canonical = value => value && typeof value === 'object'
  ? Array.isArray(value) ? `[${value.map(canonical).join(',')}]`
    : `{${Object.keys(value).sort().map(key => `${JSON.stringify(key)}:${canonical(value[key])}`).join(',')}}`
  : JSON.stringify(value);
const percentile = (values, p) => {
  if (!values.length) return null;
  const sorted = [...values].sort((a, b) => a - b);
  return sorted[Math.min(sorted.length - 1, Math.floor((sorted.length - 1) * p))];
};
const intervalStats = values => {
  const intervals = values.slice(1).map((value, index) => value - values[index]);
  return {count: intervals.length, p50Ms: percentile(intervals, 0.5), p90Ms: percentile(intervals, 0.9), maxMs: intervals.length ? Math.max(...intervals) : null};
};

export function createExternalAudit() {
  return {sequence: 0, previous: '', rows: 0, observations: [], identity: null, questionChanges: 0,
    conditionChanges: 0, noTokenChanges: 0, venueMarketMismatches: 0, noTokenMismatches: 0,
    twoSidedBooks: 0, marginDescriptionRows: 0, descriptionHashes: new Set(), firstJournalAt: null, lastJournalAt: null, lastReceiptAtMs: null};
}

export function consumeExternalRow(state, row) {
  fail(row && typeof row === 'object' && !Array.isArray(row), 'external row invalid');
  const body = ownBody(row, 'hash');
  fail(Number.isSafeInteger(row.sequence) && row.sequence === state.sequence + 1 && row.previous === state.previous,
    'external journal sequence/chain mismatch');
  fail(row.hash === sha256(JSON.stringify(body)), 'external journal hash mismatch');
  fail(row.event?.type === 'external_public_no_book_observed' && row.event.accountCredentialsIncluded === false,
    'external event type or credential marker invalid');
  const gammaReceipt = row.event.gammaReceipt, bookReceipt = row.event.bookReceipt;
  for (const [receipt, name] of [[gammaReceipt, 'Gamma'], [bookReceipt, 'venue book']]) {
    fail(receipt && typeof receipt.bodyText === 'string' && receipt.sourceBodyHash === sha256(receipt.bodyText), `${name} response hash mismatch`);
    fail(isoMs(receipt.requestStartedAt) <= isoMs(receipt.receivedAt), `${name} receipt clock invalid`);
  }
  fail(isoMs(row.at) >= isoMs(bookReceipt.receivedAt) && isoMs(row.at) >= isoMs(gammaReceipt.receivedAt), 'journal precedes source receipt');
  const gamma = JSON.parse(gammaReceipt.bodyText), venueBook = JSON.parse(bookReceipt.bodyText);
  const outcomes = JSON.parse(gamma.outcomes), tokenIds = JSON.parse(gamma.clobTokenIds);
  fail(Array.isArray(outcomes) && Array.isArray(tokenIds) && outcomes.length === tokenIds.length, 'venue outcomes/tokens invalid');
  const noIndex = outcomes.findIndex(value => String(value).toLowerCase() === 'no');
  fail(noIndex >= 0 && typeof tokenIds[noIndex] === 'string', 'venue NO token missing');
  const sourceMs = Number(venueBook.timestamp);
  fail(Number.isSafeInteger(sourceMs) && sourceMs > 0, 'venue source timestamp invalid');
  const identity = {question: gamma.question, slug: gamma.slug, conditionId: gamma.conditionId, noTokenId: tokenIds[noIndex]};
  if (!state.identity) state.identity = identity;
  else {
    if (identity.question !== state.identity.question || identity.slug !== state.identity.slug) state.questionChanges++;
    if (identity.conditionId !== state.identity.conditionId) state.conditionChanges++;
    if (identity.noTokenId !== state.identity.noTokenId) state.noTokenChanges++;
  }
  const matchedNo = venueBook.asset_id === identity.noTokenId;
  const matchedCondition = venueBook.market === identity.conditionId;
  const description = typeof gamma.description === 'string' ? gamma.description : '';
  state.descriptionHashes.add(sha256(description));
  if (/resolve according to the margin of victory/i.test(description) && /\bwin\b/i.test(String(gamma.question))) state.marginDescriptionRows++;
  if (!matchedNo) state.noTokenMismatches++;
  if (!matchedCondition) state.venueMarketMismatches++;
  if (Array.isArray(venueBook.bids) && venueBook.bids.length && Array.isArray(venueBook.asks) && venueBook.asks.length) state.twoSidedBooks++;
  const at = isoMs(row.at), receiptAt = isoMs(bookReceipt.receivedAt);
  fail(state.lastReceiptAtMs === null || receiptAt >= state.lastReceiptAtMs, 'external receipt clock rollback');
  state.sequence = row.sequence;
  state.previous = row.hash;
  state.rows++;
  state.lastReceiptAtMs = receiptAt;
  state.firstJournalAt ??= row.at;
  state.lastJournalAt = row.at;
  state.observations.push({journalAtMs: at, receiptAtMs: receiptAt, sourceAtMs: sourceMs,
    venueAgeAtReceiptMs: receiptAt - sourceMs, question: identity.question, conditionId: identity.conditionId,
    noTokenId: identity.noTokenId, descriptionHash: sha256(description), matchedNo, matchedCondition});
  return state;
}

export function createCupAudit() {
  return {sequence: 0, previous: '', records: 0, targetBooks: [], firstAt: null, lastAt: null};
}

export function consumeCupRow(state, row, target) {
  fail(row && typeof row === 'object' && !Array.isArray(row), 'Cup public-book row invalid');
  const body = ownBody(row, 'hash');
  fail(Number.isSafeInteger(row.sequence) && row.sequence === state.sequence + 1 && row.previousHash === state.previous,
    'Cup public-book sequence/chain mismatch');
  fail(row.hash === sha256(canonical(body)), 'Cup public-book canonical hash mismatch');
  const recordAt = isoMs(row.at);
  fail(state.lastAt === null || recordAt >= state.lastAt, 'Cup public-book journal clock rollback');
  state.sequence = row.sequence;
  state.previous = row.hash;
  state.records++;
  state.firstAt ??= row.at;
  state.lastAt = recordAt;
  if (row.kind !== 'book') return state;
  const book = row.payload?.book;
  fail(book && typeof book === 'object', 'Cup public-book payload missing');
  if (book.marketId !== target.marketId || book.exchangeId !== target.exchangeId || book.tournamentId !== target.tournamentId) return state;
  const receivedAtMs = isoMs(book.receivedAt), requestStartedAtMs = isoMs(book.requestStartedAt), sourceAtMs = isoMs(book.sourceAt);
  fail(requestStartedAtMs <= receivedAtMs && receivedAtMs <= recordAt && receivedAtMs - requestStartedAtMs <= 20_000
    && recordAt - receivedAtMs <= 20_000 && sourceAtMs <= receivedAtMs + 2000, 'Cup book receipt chronology invalid');
  fail(book.instrument === `${target.exchangeId}:YES` && book.freshnessPolicy === 'authoritative_receipt_v1', 'Cup target book identity/freshness policy invalid');
  fail(Number.isSafeInteger(book.sourceEventAgeMs) && book.sourceEventAgeMs === receivedAtMs - sourceAtMs, 'Cup source-age field mismatch');
  fail(typeof book.sequence === 'string' && /^(0|[1-9]\d*)$/.test(book.sequence)
    && book.versionKey === `${book.sequence}:${book.sourceAt}`, 'Cup target source version invalid');
  const validateLevels = (levels, descending) => {
    fail(Array.isArray(levels) && levels.length > 0 && levels.length <= 20, 'Cup target depth missing or out of bounds');
    let previous = null;
    for (const level of levels) {
      const price = Number(level?.price);
      fail(Number.isFinite(price) && price > 0 && price < 1 && Number.isSafeInteger(level.quantity) && level.quantity > 0,
        'Cup target depth level invalid');
      fail(previous === null || (descending ? price < previous : price > previous), 'Cup target depth is not strictly price ordered');
      previous = price;
    }
  };
  validateLevels(book.bids, true);
  validateLevels(book.asks, false);
  const bestBid = Number(book.bids[0].price), bestAsk = Number(book.asks[0].price);
  fail(Number.isFinite(bestBid) && Number.isFinite(bestAsk) && bestBid > 0 && bestBid < bestAsk && bestAsk < 1,
    'Cup target top of book invalid');
  state.targetBooks.push({receivedAtMs, sourceAtMs, sourceAgeAtReceiptMs: book.sourceEventAgeMs,
    recordAtMs: recordAt, bestBid, bestAsk, bidLevels: book.bids.length, askLevels: book.asks.length,
    sessionId: row.sessionId, sequence: row.sequence});
  return state;
}

export function summarizeCoverage({external, cup, contractReview, processingLatencyMs = PROCESSING_LATENCY_MS}) {
  fail(Number.isSafeInteger(processingLatencyMs) && processingLatencyMs >= 0, 'processing latency invalid');
  const sourceTimes = external.observations.map(row => row.receiptAtMs);
  const cupTimes = cup.targetBooks.map(row => row.receivedAtMs);
  const books = [...cup.targetBooks].sort((a, b) => a.receivedAtMs - b.receivedAtMs || a.sequence - b.sequence);
  let cursor = 0;
  const joined = external.observations.map(observation => {
    const actionAtMs = observation.receiptAtMs + processingLatencyMs;
    while (cursor < books.length && books[cursor].receivedAtMs < actionAtMs) cursor++;
    const book = books[cursor] ?? null;
    return {observation, actionAtMs, book, delayAfterActionMs: book ? book.receivedAtMs - actionAtMs : null,
      sourceAgeAtActionMs: actionAtMs - observation.sourceAtMs,
      sourceAgeAtBookMs: book ? book.receivedAtMs - observation.sourceAtMs : null,
      cupBookAgeAtReceiptMs: book?.sourceAgeAtReceiptMs ?? null};
  });
  const horizons = CENSOR_HORIZONS_MS.map(horizonMs => ({horizonMs,
    matched: joined.filter(row => row.book && row.delayAfterActionMs <= horizonMs).length,
    censored: joined.filter(row => !row.book || row.delayAfterActionMs > horizonMs).length}));
  const freshness = SOURCE_AGE_LIMITS_MS.flatMap(sourceAgeLimitMs => SOURCE_AGE_LIMITS_MS.map(cupBookAgeLimitMs => ({
    sourceAgeLimitMs, cupBookAgeLimitMs, within30sAfterAction: joined.filter(row => row.book
      && row.sourceAgeAtActionMs >= 0 && row.sourceAgeAtActionMs <= sourceAgeLimitMs
      && row.cupBookAgeAtReceiptMs >= 0 && row.cupBookAgeAtReceiptMs <= cupBookAgeLimitMs
      && row.delayAfterActionMs <= 30_000).length
  })));
  const fit = contractReview?.contractEquivalenceEstablished === true && contractReview?.tiePolicyEquivalenceVerified === true;
  const agesAtReceipt = external.observations.map(row => row.venueAgeAtReceiptMs);
  const pollTimes = external.observations.map(row => row.receiptAtMs);
  const cupPollTimes = books.map(row => row.receivedAtMs);
  const actionAges = joined.map(row => row.sourceAgeAtActionMs);
  const bookSourceAges = joined.filter(row => row.book).map(row => row.sourceAgeAtBookMs);
  const cupAges = joined.filter(row => row.book).map(row => row.cupBookAgeAtReceiptMs);
  const maximumAge = values => values.length ? Math.max(...values) : null;
  const minimumAge = values => values.length ? Math.min(...values) : null;
  return {
    processingLatencyMs,
    sourceToCupContractFit: fit ? 'established_by_existing_review' : 'unresolved_blocker',
    markout: {status: 'not_run', observations: 0, reason: fit ? 'No markout implementation is in this audit-only scope.' : 'External and Cup payout predicates are not established as equivalent; outcome markout is suppressed.'},
    external: {observations: external.rows, question: external.identity?.question ?? null,
      venueDescriptionHashCount: external.descriptionHashes.size,
      descriptionSaysMarginWhileQuestionAsksWinner: external.marginDescriptionRows > 0,
      marginDescriptionReceipts: external.marginDescriptionRows,
      stableQuestion: external.questionChanges === 0, stableConditionId: external.conditionChanges === 0,
      stableNoTokenId: external.noTokenChanges === 0, conditionIdMatchesNoBook: external.venueMarketMismatches === 0,
      observedNoTokenMatches: external.noTokenMismatches === 0, twoSidedBookReceipts: external.twoSidedBooks,
      sourceAgeAtReceiptMs: {min: minimumAge(agesAtReceipt), p50: percentile(agesAtReceipt, 0.5), p90: percentile(agesAtReceipt, 0.9), max: maximumAge(agesAtReceipt),
        clockAhead: agesAtReceipt.filter(value => value < 0).length,
        within30s: agesAtReceipt.filter(value => value >= 0 && value <= 30_000).length,
        within60s: agesAtReceipt.filter(value => value >= 0 && value <= 60_000).length,
        within120s: agesAtReceipt.filter(value => value >= 0 && value <= 120_000).length,
        within5m: agesAtReceipt.filter(value => value >= 0 && value <= 300_000).length},
      pollIntervalMs: intervalStats(pollTimes), firstJournalAt: external.firstJournalAt, lastJournalAt: external.lastJournalAt},
    cup: {verifiedJournalRecords: cup.records, matchingMarketBooks: books.length,
      firstMatchingBookAt: books.length ? new Date(books[0].receivedAtMs).toISOString() : null,
      lastMatchingBookAt: books.length ? new Date(books.at(-1).receivedAtMs).toISOString() : null,
      targetBookIntervalMs: intervalStats(cupPollTimes),
      displayedDepthLevels: {minBids: books.length ? Math.min(...books.map(row => row.bidLevels)) : null,
        minAsks: books.length ? Math.min(...books.map(row => row.askLevels)) : null}},
    joins: {externalObservations: joined.length, laterBookAfterLatency: joined.filter(row => row.book).length,
      noLaterBook: joined.filter(row => !row.book).length,
      delayAfterActionMs: {p50: percentile(joined.filter(row => row.book).map(row => row.delayAfterActionMs), 0.5),
        p90: percentile(joined.filter(row => row.book).map(row => row.delayAfterActionMs), 0.9),
        max: maximumAge(joined.filter(row => row.book).map(row => row.delayAfterActionMs))},
      sourceAgeAtActionMs: {clockAhead: actionAges.filter(value => value < 0).length,
        within30s: actionAges.filter(value => value >= 0 && value <= 30_000).length,
        within60s: actionAges.filter(value => value >= 0 && value <= 60_000).length,
        within120s: actionAges.filter(value => value >= 0 && value <= 120_000).length,
        within5m: actionAges.filter(value => value >= 0 && value <= 300_000).length,
        p50: percentile(actionAges, 0.5), p90: percentile(actionAges, 0.9), max: maximumAge(actionAges)},
      externalSourceAgeAtObservedCupBookMs: {min: minimumAge(bookSourceAges), clockAhead: bookSourceAges.filter(value => value < 0).length,
        p50: percentile(bookSourceAges, 0.5), p90: percentile(bookSourceAges, 0.9), max: maximumAge(bookSourceAges)},
      cupBookSourceAgeAtReceiptMs: {min: minimumAge(cupAges), clockAhead: cupAges.filter(value => value < 0).length,
        p50: percentile(cupAges, 0.5), p90: percentile(cupAges, 0.9), max: maximumAge(cupAges)},
      censoringByDelayAfterLatency: horizons,
      ageSensitivity: freshness},
    decision: fit ? 'Contract gate passes only in the existing review; a separate frozen chronological markout would still be required.'
      : 'Blocked: contract/tie equivalence is unverified and documented predicate exceptions change NO payout. Do not treat the venue probability as a Cup outcome price.',
    evidenceLimits: ['Local hash chains and response-body hashes establish capture integrity, not venue authenticity or semantic equivalence.',
      'The captured candidate covers one Delaware Senate condition; it cannot provide independent market-group holdouts.',
      'Later-book timing is an observation-coverage diagnostic, not an executable fill, forecast, or profit estimate.']
  };
}

