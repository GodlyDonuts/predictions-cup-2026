import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import {createHash} from 'node:crypto';
import {fileURLToPath} from 'node:url';
import {journalLines} from '../live-paper/journal-lines.mjs';
import {engineBookPointsV1} from '../paper/maker-engine-book-point-v1.mjs';
import {screenComplementBookV1} from './same-market-complement-arb-v1.mjs';

export const POLL_MARKET_SCREEN_V1 = Object.freeze({
  id: 'primary-poll-market-screen-20261013-v1',
  capture: Object.freeze({
    file: 'realtime-2026-10-13T19-52-33-253Z-9ecf5d7b-070f-4c01-a991-28a67a9f4a29.jsonl',
    bytes: 279_458_961,
    sha256: 'fd6a9552a5f70365ee78706fa1efff70e2a36e7f3dd55c1a932cc3795e1979e7',
    rows: 76_207,
    acceptedBookPoints: 66_127,
    marketBatchRows: 70_894,
    markets: 237,
    from: '2026-10-13T19:52:33.254Z',
    through: '2026-10-13T20:40:59.415Z',
    tournamentId: 'bda92870-621e-47b0-bc3c-3602c5c26f55',
  }),
  tick: 0.005,
});

const TOPIC = /^tournament:([0-9a-f-]+):market:(\d+)$/;
const sourceStates = {
  IA: {
    office: 'Federal Senate and State Governor', stage: 'General', publishedAt: '2026-10-09',
    url: 'https://emersoncollegepolling.com/iowa-2026-poll-u-s-senate-race-tightens-sand-maintains-edge-for-governor/',
    contentHash: null, expectedHash: null, actualHash: null, digestState: 'unretained_null',
    hashStatus: 'publisher bytes were not retained in the verified local source ledger',
    fieldDates: ['2026-10-05', '2026-10-06'], sampleSize: 650, population: 'likely voters',
    marginOfError: '±3.8 percentage points',
    shares: {senate: {AshleyHinson: 47, JoshTurek: 45, ThomasLaehn: 2, undecided: 7},
      governor: {Sand: 49, Lahn: 45, undecided: 6}},
  },
  TX: {
    office: 'Federal Senate and State Governor', stage: 'General', publishedAt: '2026-10-13',
    url: 'https://sri.siena.edu/2026/10/13/talarico-maintains-small-lead-over-paxton/',
    contentHash: null, expectedHash: null, actualHash: null, digestState: 'unretained_null',
    hashStatus: 'publisher bytes and full fieldwork/methodology record were not retained in this offline task',
    availableNoLaterThan: '2026-10-13T13:00:00.000Z', fieldDates: null, sampleSize: null,
    population: null, shares: {senate: {JamesTalarico: 48, KenPaxton: 44},
      governor: {GregAbbott: 48, GinaHinojosa: 47}},
  },
  NY: {
    office: 'State Governor', stage: 'General', publishedAt: '2026-10-08',
    url: 'https://emersoncollegepolling.com/new-york-2026-poll-governor-hochul-leads-blakeman-by-12/',
    contentHash: null, expectedHash: null, actualHash: null, digestState: 'unretained_null',
    hashStatus: 'publisher bytes were not retained in the verified local source ledger',
    fieldDates: ['2026-10-03', '2026-10-05'], sampleSize: 900, population: 'likely voters',
    shares: {KathyHochul: 52, BruceBlakeman: 40, undecided: 8},
  },
};

function sha(bytes) { return createHash('sha256').update(bytes).digest('hex'); }
function sourceDigestState({contentHash, actualHash}) {
  if (actualHash === null || actualHash === undefined) {
    return contentHash === null || contentHash === undefined ? 'unretained_null' : 'digest_not_computed';
  }
  if (contentHash === null || contentHash === undefined) return 'computed_without_expected_hash';
  return actualHash === contentHash ? 'verified_match' : 'digest_mismatch';
}
function isoMs(value) { const n = Date.parse(value); assert(Number.isFinite(n), 'invalid timestamp'); return n; }
function marketIdFromTopic(topic) { return TOPIC.exec(topic ?? '')?.[2] ?? null; }
function tickPrice(value) { return Number((Math.ceil((Number(value) - 1e-10) / POLL_MARKET_SCREEN_V1.tick)
  * POLL_MARKET_SCREEN_V1.tick).toFixed(6)); }

export function candidateContractIdentity(metadata) {
  const root = metadata?.root, d = root?.contract_details;
  if (!root || !d || d.contractType !== 'Election Outcome'
    || d.resolutionType !== 'Party Winner' || d.raceStage !== 'General'
    || !['Federal', 'State'].includes(d.officeLevel)
    || !['Democratic Party', 'Republican Party'].includes(d.winnerName)
    || !/^[A-Z]{2}$/.test(d.usState ?? '') || !d.raceId || !d.stageId
    || !/^\d{4}-\d{2}-\d{2}$/.test(d.electionDate ?? '')) return null;
  return {
    marketId: String(metadata.marketId), rootHash: metadata.rootHash,
    state: d.usState, office: d.raceName, officeLevel: d.officeLevel,
    stage: d.raceStage, raceId: String(d.raceId), stageId: String(d.stageId),
    electionDate: d.electionDate, winner: d.winnerName,
    party: d.winnerName === 'Democratic Party' ? 'democratic' : 'republican',
    title: root.title,
  };
}

export function compareBoundToQuote({ask, lowerBound, tick = POLL_MARKET_SCREEN_V1.tick,
  exactMapping = false, sourceHashVerified = false, calibrationScopeMatched = false}) {
  if (![ask, tick].every(Number.isFinite) || ask < 0 || ask > 1 || tick <= 0) {
    return {eligible: false, reason: 'invalid_quote_or_tick', oneTickEntry: null,
      lowerBound: null, edge: null};
  }
  const entry = tickPrice(ask + tick - 1e-10);
  if (!exactMapping || !sourceHashVerified || !calibrationScopeMatched
    || !Number.isFinite(lowerBound) || lowerBound < 0 || lowerBound > 1) {
    return {eligible: false, reason: 'no_verified_contract_level_probability_bound',
      oneTickEntry: entry, lowerBound: null, edge: null};
  }
  const edge = Number((lowerBound - entry).toFixed(6));
  return {eligible: edge >= 0, reason: edge >= 0 ? 'lower_bound_covers_one_tick_entry' : 'lower_bound_below_one_tick_entry',
    oneTickEntry: entry, lowerBound, edge};
}

function collectQuotePoints(records, source, expected = POLL_MARKET_SCREEN_V1) {
  const config = new Map(), topicStates = new Map(), lastRevision = new Map(), lastBookVersion = new Map();
  const quotes = new Map(), diagnostics = {rows: 0, configRows: 0, marketBatchRows: 0,
    sourceBooks: 0, acceptedBookPoints: 0, rejectedBookPoints: 0, revisionGaps: 0,
    lifecycleInvalidations: 0, missingQuoteInCapture: 0};
  const deadline = isoMs(expected.capture.through);
  let quoteReceiptAt = null;
  let dataStarted = false, groupAt = null, group = [], nextIndex = 0;
  let captureFrom = null, captureThrough = null, scope = null;

  const finishScope = () => {
    if (scope) return scope;
    const exchanges = [...config.values()].flatMap(x => x.exchangeIds.map(exchangeId => ({exchangeId, marketId: x.marketId})));
    assert.equal(exchanges.length, expected.capture.markets, 'capture scope count');
    assert.equal(new Set(exchanges.map(x => x.exchangeId)).size, expected.capture.markets, 'duplicate exchange IDs');
    scope = {tournamentId: expected.capture.tournamentId, exchanges};
    return scope;
  };
  const flush = () => {
    if (!group.length) return;
    const invalidated = new Set(), updates = new Map(), events = [];
    const markInvalid = (topic, lifecycle = false) => {
      const id = marketIdFromTopic(topic); invalidated.add(id);
      const old = updates.get(topic) ?? topicStates.get(topic) ?? {epoch: null, ready: false};
      updates.set(topic, {...old, ready: false});
      if (lifecycle) diagnostics.lifecycleInvalidations++;
    };
    for (const row of group) {
      const topic = row.topic ?? null, id = marketIdFromTopic(topic);
      if (row.kind === 'config') continue;
      if (row.kind === 'subscribed') {
        if (id !== null) invalidated.add(id);
        updates.set(topic, {epoch: row.epoch, ready: false}); diagnostics.lifecycleInvalidations++;
        continue;
      }
      if (['invalidation', 'disconnected', 'recovery_started'].includes(row.kind)) {
        if (topic === null) {
          invalidated.add(null);
          for (const [key, state] of topicStates) updates.set(key, {...state, ready: false});
        } else markInvalid(topic, true);
        continue;
      }
      if (row.kind === 'recovery_finished') {
        const old = updates.get(topic) ?? topicStates.get(topic) ?? {epoch: null, ready: false};
        updates.set(topic, {...old, epoch: row.epoch, ready: row.booksReady === true});
        continue;
      }
      if (row.kind !== 'event' || row.event !== 'market_batch') continue;
      diagnostics.marketBatchRows++; diagnostics.sourceBooks += row.payload?.books?.length ?? 0;
      const state = topicStates.get(topic), payload = row.payload ?? {};
      if (!state || state.epoch !== row.epoch) { markInvalid(topic); continue; }
      const key = `${topic}#${row.epoch}`, revision = payload.delivery?.revision, previous = payload.delivery?.previousRevision;
      if (!Number.isSafeInteger(revision) || revision < 0 || !Number.isSafeInteger(previous) || previous < 0) {
        markInvalid(topic); continue;
      }
      const oldRevision = lastRevision.get(key);
      if (oldRevision !== undefined && revision <= oldRevision) continue;
      const gap = oldRevision !== undefined && previous > oldRevision;
      lastRevision.set(key, revision);
      if (gap) { diagnostics.revisionGaps++; markInvalid(topic); continue; }
      if (payload.resyncRequired !== false || payload.marketSettled?.length) { markInvalid(topic); continue; }
      if ((payload.bookDirty ?? []).some(x => !(payload.books ?? []).some(b => b.exchangeId === x.exchangeId))) {
        markInvalid(topic); continue;
      }
      events.push(row);
    }
    const candidates = new Map();
    for (const row of events) {
      const id = marketIdFromTopic(row.topic);
      if (invalidated.has(null) || invalidated.has(id)) continue;
      const pointResult = engineBookPointsV1(row, finishScope(), row.observedAt);
      diagnostics.rejectedBookPoints += pointResult.rejected.length;
      for (const point of pointResult.books) {
        const version = BigInt(point.sequence), sourceAt = isoMs(point.sourceAt), prior = lastBookVersion.get(point.exchangeId);
        if (prior && (version < prior.sequence || (version === prior.sequence && sourceAt <= prior.sourceMs))) continue;
        lastBookVersion.set(point.exchangeId, {sequence: version, sourceMs: sourceAt});
        try {
          const screened = screenComplementBookV1({marketId: point.marketId, exchangeId: point.exchangeId,
            sequence: point.sequence, availableAt: row.observedAt, sourceAt: point.sourceAt,
            nextExpiryAt: point.sourceBinding.nextExpiryAt, bids: point.bids, asks: point.asks});
          candidates.set(point.exchangeId, {marketId: point.marketId, exchangeId: point.exchangeId,
            receivedAt: row.receivedAt, observedAt: row.observedAt, sourceAt: point.sourceAt,
            sequence: point.sequence, bid: Number(screened.bid), bidSize: screened.bidSize,
            ask: Number(screened.ask), askSize: screened.askSize,
            sourceEventHash: point.sourceBinding.recordHash, rawBookHash: point.sourceBinding.rawBookHash});
        } catch { diagnostics.rejectedBookPoints++; }
      }
    }
    for (const quote of candidates.values()) {
      diagnostics.acceptedBookPoints++;
      if (deadline !== null && isoMs(quote.observedAt) <= deadline && !quotes.has(quote.marketId)) quotes.set(quote.marketId, quote);
    }
    for (const [topic, state] of updates) topicStates.set(topic, state);
    if (invalidated.has(null)) for (const [topic, state] of topicStates) topicStates.set(topic, {...state, ready: false});
    group = [];
  };

  for (const raw of records) {
    const row = typeof raw === 'string' ? JSON.parse(raw) : raw;
    assert(row && row.format === 'sig-feed-capture' && row.schemaVersion === 1 && row.captureIndex === nextIndex++, 'capture row chain');
    const observedMs = isoMs(row.observedAt);
    assert(captureThrough === null || observedMs >= isoMs(captureThrough), 'capture clock rollback');
    captureFrom ??= row.observedAt; captureThrough = row.observedAt; diagnostics.rows++;
    if (row.kind === 'config') {
      assert(!dataStarted, 'config after data');
      const rawScope = row.scope, parts = TOPIC.exec(row.topic ?? '');
      assert(row.format === 'sig-feed-capture' && rawScope?.tournamentId === expected.capture.tournamentId
        && parts?.[1] === expected.capture.tournamentId && parts?.[2] === rawScope.marketId
        && row.topic === rawScope.topic && Array.isArray(rawScope.exchangeIds), 'invalid capture config');
      assert(!config.has(rawScope.marketId), 'duplicate market config');
      const exchangeIds = rawScope.exchangeIds.map(String);
      assert(exchangeIds.length > 0 && exchangeIds.every(x => /^\d+$/.test(x)) && new Set(exchangeIds).size === exchangeIds.length, 'invalid exchange config');
      config.set(rawScope.marketId, {marketId: rawScope.marketId, exchangeIds}); diagnostics.configRows++;
    } else {
      dataStarted = true;
      if (quoteReceiptAt === null && row.kind === 'event' && row.event === 'market_batch') {
        quoteReceiptAt = row.receivedAt ?? row.observedAt;
      }
    }
    if (groupAt !== null && observedMs !== groupAt) flush();
    groupAt = observedMs; group.push(row);
  }
  finishScope(); flush();
  assert.equal(diagnostics.rows, expected.capture.rows, 'capture row count');
  assert.equal(diagnostics.marketBatchRows, expected.capture.marketBatchRows, 'market batch row count');
  assert.equal(diagnostics.acceptedBookPoints, expected.capture.acceptedBookPoints, 'accepted quote-point replay parity');
  assert.equal(captureFrom, expected.capture.from, 'capture start');
  assert.equal(captureThrough, expected.capture.through, 'capture through');
  diagnostics.missingQuoteInCapture = config.size - quotes.size;
  return {config, quotes, diagnostics, captureFrom, captureThrough, quoteReceiptAt};
}

function readMetadataAt(metadataDir, quoteTimes, fallbackAt, ids) {
  const fallback = isoMs(fallbackAt), selected = new Map();
  function walk(dir) {
    for (const entry of fs.readdirSync(dir, {withFileTypes: true})) {
      const file = path.join(dir, entry.name);
      if (entry.isDirectory()) { walk(file); continue; }
      if (!entry.name.endsWith('.json') || entry.name.endsWith('.book.json')) continue;
      let item;
      try { item = JSON.parse(fs.readFileSync(file, 'utf8')); } catch { continue; }
      const id = String(item.marketId ?? ''), received = Date.parse(item.receivedAt);
      const cutoff = quoteTimes.has(id) ? isoMs(quoteTimes.get(id)) : fallback;
      if (!ids.has(id) || !item.root?.contract_details || !item.rootHash
        || !Number.isFinite(received) || received > cutoff) continue;
      const old = selected.get(id);
      if (!old || item.receivedAt > old.receivedAt) selected.set(id, {...item, sourceFile: file});
    }
  }
  walk(metadataDir);
  return selected;
}

function manifestSources(runtimeDir, manifestPath, sourceDirectory, repo) {
  const manifest = JSON.parse(fs.readFileSync(manifestPath, 'utf8'));
  return manifest.captures.map(source => {
    const bytes = fs.readFileSync(path.join(runtimeDir, sourceDirectory, source.file));
    const actualHash = sha(bytes);
    return {...source, expectedHash: source.contentHash ?? null,
      verified: actualHash === source.contentHash, actualHash,
      digestState: sourceDigestState({contentHash: source.contentHash, actualHash}),
      sourceDirectory, manifestFile: path.relative(repo, manifestPath),
      sourceType: source.contentType?.includes('pdf') ? 'primary-pollster-pdf' : 'primary-source-page'};
  });
}

function pollSourceLedger(runtimeDir, repo) {
  const v2 = JSON.parse(fs.readFileSync(path.join(repo, 'research/current-polls-v2/assessments-20261005-v2.json'), 'utf8'));
  const v2Definition = JSON.parse(fs.readFileSync(path.join(repo, 'research/current-polls-v2/definition.json'), 'utf8'));
  const v2Sources = [
    ...manifestSources(runtimeDir, path.join(repo, 'research/current-polls-v2/sources.json'), 'research-sources-current-marist-20261005-v2', repo),
    ...manifestSources(runtimeDir, path.join(repo, 'research/current-polls-v2/comparator-sources.json'), 'research-sources-current-marist-20261005-v2', repo),
  ];
  const v1 = JSON.parse(fs.readFileSync(path.join(repo, 'research/current-polls-v1/assessments-20261005-v1.json'), 'utf8'));
  const v1Sources = [
    ...manifestSources(runtimeDir, path.join(repo, 'research/current-polls-v1/sources.json'), 'research-sources-current-polls-20261005', repo),
    ...manifestSources(runtimeDir, path.join(repo, 'research/current-polls-v1/ballot-sources.json'), 'research-sources-current-polls-20261005-ballots', repo),
  ];
  const byName = new Map([...v2Sources, ...v1Sources].map(s => [s.file, s]));
  return {v2, v2Definition, v2Sources, v1, v1Sources, byName,
    modelsReviewed: [
      {file: 'research/poll-horizon-v2.mjs', summary: '29-day Senate general-election poll-margin model; 29–59 day source window; historical population labels/release clocks unavailable; not used for a different contract target.'},
      {file: 'research/poll-calibration-v1/definition.json', summary: '45-day Senate general-election D–R final-margin error model, 45–75 day window; not extended outside scope.'},
      {file: 'research/governor-error-v1/results-20261005-v1.json', summary: '45-day Governor general-election poll-margin model; not transported to late-October-field polling.'},
    ]};
}

function knownNewerEvidenceForRace(contract) {
  if (contract.state === 'IA' && (contract.office.includes('Senate') || contract.office.includes('Governor'))) return sourceStates.IA;
  if (contract.state === 'TX' && (contract.office.includes('Senate') || contract.office.includes('Governor'))) return sourceStates.TX;
  if (contract.state === 'NY' && contract.office.includes('Governor')) return sourceStates.NY;
  return null;
}

function exclusionReasons({metadata, contract, quote, localSources, recentEvidence, screen}) {
  const reasons = [];
  if (!metadata) reasons.push('root_contract_metadata_missing_at_quote');
  if (metadata && !contract) reasons.push(metadata.root?.contract_type === 'Freeform'
    || metadata.root?.contractType === 'Freeform' ? 'freeform_contract_has_no_exact_candidate_mapping' : 'contract_not_party_winner_general_election');
  if (!quote) reasons.push('no_valid_fresh_quote_within_sealed_receipt_deadline');
  if (recentEvidence) reasons.push('newer_primary_poll_known_but_source_hash_or_full_record_unverified');
  if (contract && !recentEvidence && !localSources.length) reasons.push('no_exact_primary_poll_source_in_verified_local_ledger');
  if (localSources.length && localSources.some(s => !s.verified)) reasons.push('primary_source_content_hash_mismatch');
  if (contract?.officeLevel !== 'Federal' || !contract?.office.includes('Senate')) reasons.push('no_exact_scope_matched_error_model_for_this_office');
  if (localSources.length && !recentEvidence) reasons.push('candidate_party_winner_mapping_or_other_party_outcome_not_closed');
  if (!screen?.lowerBound) reasons.push('no_valid_contract_level_probability_lower_bound');
  return [...new Set(reasons)];
}

export function buildMarketRows({config, quotes, metadata, sourceLedger, asOf, cutoff}) {
  const rows = [];
  for (const [marketId, configured] of config) {
    const quote = quotes.get(marketId) ?? null;
    const rootRecord = metadata.get(marketId) ?? null;
    const contract = candidateContractIdentity(rootRecord);
    const localAssessment = contract?.office.includes('Senate') && sourceLedger.v2.assessments.find(a =>
      a.state === contract.state && a.candidates && a.sources?.some(s => sourceLedger.byName.get(s.file)?.verified));
    const localKansas = contract?.state === 'KS' && contract.office.includes('Senate') && sourceLedger.v1.assessments.find(a =>
      a.state === 'KS' && a.candidates && a.sources?.some(s => sourceLedger.byName.get(s.file)?.verified));
    const localSources = [...(localAssessment?.sources ?? []), ...(localKansas?.sources ?? [])]
      .map(s => sourceLedger.byName.get(s.file)).filter(Boolean);
    const localPollFacts = localAssessment ? {candidates: localAssessment.candidates,
      polls: localAssessment.polls.map(p => ({pollster: p.pollster, population: p.population,
        democratic: p.democratic, republican: p.republican, unallocated: p.unallocated,
        fieldEndDate: p.fieldEndDate}))}
      : localKansas ? {candidates: localKansas.candidates,
        polls: [{pollster: 'Emerson', population: localKansas.poll.population,
          democraticPercent: localKansas.poll.democratic * 100, republicanPercent: localKansas.poll.republican * 100,
          otherPercent: localKansas.poll.other * 100, unallocatedPercent: localKansas.poll.unallocated * 100,
          fieldStartDate: localKansas.poll.fieldStartDate, fieldEndDate: localKansas.poll.fieldEndDate,
          publicationDate: localKansas.poll.publicationDate}]} : null;
    const recentEvidence = contract ? knownNewerEvidenceForRace(contract) : null;
    const ask = quote?.ask ?? null;
    const exactMapping = false; // Current poll records do not prove an exhaustive candidate-to-party settlement map.
    const sourceHashVerified = localSources.length > 0 && localSources.every(s => s.verified) && !recentEvidence;
    const calibrationScopeMatched = false; // Available models target D–R margins, not all party-winner outcomes in these mapped races.
    const comparison = ask === null ? {eligible: false, reason: 'no_quote', oneTickEntry: null, lowerBound: null, edge: null}
      : compareBoundToQuote({ask, lowerBound: null, exactMapping, sourceHashVerified, calibrationScopeMatched});
    const reasons = exclusionReasons({metadata: rootRecord, contract, quote, localSources, recentEvidence, screen: comparison});
    rows.push({marketId, exchangeIds: configured.exchangeIds,
      contract: contract ? {title: contract.title, state: contract.state, office: contract.office,
        officeLevel: contract.officeLevel, stage: contract.stage, raceId: contract.raceId, stageId: contract.stageId,
        electionDate: contract.electionDate, winner: contract.winner, rootHash: contract.rootHash,
        rootReceivedAt: rootRecord.receivedAt} : rootRecord ? {title: rootRecord.root?.title ?? null,
        contractType: rootRecord.root?.contract_type ?? rootRecord.root?.contractType ?? null,
        rootHash: rootRecord.rootHash, rootReceivedAt: rootRecord.receivedAt} : null,
      quote: quote ? {receivedAt: quote.receivedAt, observedAt: quote.observedAt, sourceAt: quote.sourceAt,
        sourceAgeMs: isoMs(quote.observedAt) - isoMs(quote.sourceAt), yesBid: quote.bid,
        yesBidSize: quote.bidSize, yesAsk: quote.ask, yesAskSize: quote.askSize,
        oneTickEntry: comparison.oneTickEntry, quoteIsDisplayOnly: true} : null,
      sourceEvidence: [...(recentEvidence ? [{...recentEvidence, status: 'hash_unverified_not_used_as_probability_input'}] : []),
        ...localSources.map(s => ({file: s.file, url: s.url, publishedNoLaterThan: s.observedAt,
          fieldDates: localAssessment?.polls?.filter(p => p.file === s.file).map(p => p.fieldEndDate)
            ?? (localKansas ? [localKansas.poll.fieldStartDate, localKansas.poll.fieldEndDate] : null),
          sampleHash: s.contentHash, expectedHash: s.expectedHash, actualHash: s.actualHash,
          digestState: s.digestState, verifiedHash: s.verified, contentType: s.contentType,
          sourceDirectory: s.sourceDirectory, manifestFile: s.manifestFile,
          contentHash: s.contentHash, status: recentEvidence ? 'superseded_by_known_newer_source' : 'source_bytes_hash_verified'}))],
      pollFacts: localPollFacts ? {...localPollFacts, interpretation: 'vote shares only; not a contract win probability'} : null,
      probabilityBound: null,
      comparison: {...comparison, askMinusBound: null, result: 'not_computable_without_verified_contract_level_bound'},
      eligible: false, exclusionReasons: reasons.length ? reasons : ['no_verified_contract_level_probability_lower_bound']});
  }
  assert.equal(rows.length, config.size, 'one output row per captured market');
  assert.equal(new Set(rows.map(x => x.marketId)).size, rows.length, 'duplicate market row');
  return rows;
}

async function hashFile(file) {
  const hash = createHash('sha256'); let bytes = 0;
  for await (const part of fs.createReadStream(file)) { bytes += part.length; hash.update(part); }
  return {bytes, sha256: hash.digest('hex')};
}

export async function runPrimaryPollMarketScreenV1({runtimeDir = path.join(process.env.LOCALAPPDATA ?? '', 'SigTradingCup', 'paper-runtime'),
  repo = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')} = {}) {
  assert(fs.existsSync(runtimeDir), 'paper-runtime not found');
  const capture = POLL_MARKET_SCREEN_V1.capture, captureFile = path.join(runtimeDir, capture.file);
  const before = fs.statSync(captureFile);
  assert.equal(before.size, capture.bytes, 'sealed capture byte count changed');
  const hashBefore = await hashFile(captureFile);
  assert.deepEqual(hashBefore, {bytes: capture.bytes, sha256: capture.sha256}, 'sealed capture hash changed');
  const replay = collectQuotePoints(journalLines(captureFile), POLL_MARKET_SCREEN_V1);
  assert.equal(replay.config.size, capture.markets, 'sealed market count');
  const ids = new Set(replay.config.keys());
  const metadataDir = path.join(runtimeDir, 'production-metadata');
  const quoteTimes = new Map([...replay.quotes].map(([id, quote]) => [id, quote.receivedAt]));
  const metadata = readMetadataAt(metadataDir, quoteTimes, replay.quoteReceiptAt ?? capture.from, ids);
  const sourceLedger = pollSourceLedger(runtimeDir, repo);
  const rows = buildMarketRows({config: replay.config, quotes: replay.quotes, metadata,
    sourceLedger, asOf: capture.from, cutoff: capture.through});
  const after = fs.statSync(captureFile), hashAfter = await hashFile(captureFile);
  assert.equal(after.size, before.size, 'capture changed during screen');
  assert.equal(after.dev, before.dev, 'capture device changed during screen');
  assert.equal(after.ino, before.ino, 'capture identity changed during screen');
  assert.deepEqual(hashAfter, hashBefore, 'capture changed during screen');

  const reasonCounts = {};
  for (const row of rows) for (const reason of row.exclusionReasons) reasonCounts[reason] = (reasonCounts[reason] ?? 0) + 1;
  const contractKinds = {};
  for (const row of rows) {
    const k = row.contract?.officeLevel ? `${row.contract.officeLevel}|${row.contract.stage}|${row.contract.contractType ?? 'Election Outcome'}`
      : row.contract?.contractType ?? (row.contract ? 'unclassified_root' : 'missing_root');
    contractKinds[k] = (contractKinds[k] ?? 0) + 1;
  }
  const recentEvidence = Object.fromEntries(Object.entries(sourceStates).map(([state, item]) => [state, item]));
  const retainedPrimaryPageDigests = [...sourceLedger.byName.values()].map(s => ({
    file: s.file, sourceDirectory: s.sourceDirectory, manifestFile: s.manifestFile, url: s.url,
    observedAt: s.observedAt, contentType: s.contentType,
    expectedHash: s.expectedHash, actualHash: s.actualHash, digestState: s.digestState,
  }));
  const locallyHashVerifiedPrimaryPages = retainedPrimaryPageDigests.filter(s => s.digestState === 'verified_match').length;
  return {
    schemaVersion: 1, id: POLL_MARKET_SCREEN_V1.id, asOf: new Date().toISOString(),
    method: 'offline_primary_source_to_sealed_quote_screen_v1',
    source: {kind: 'sealed_local_realtime_capture', file: capture.file, bytes: capture.bytes,
      sha256: capture.sha256, rows: replay.diagnostics.rows, from: replay.captureFrom,
      through: replay.captureThrough, quoteReceiptAt: replay.quoteReceiptAt,
      tournamentId: capture.tournamentId,
      marketCount: replay.config.size, verifiedBeforeAndAfter: true,
      quoteSelection: 'first valid fresh captured book point for each contract during the sealed 48-minute holdout; no price-based selection',
      rootMetadataSelectedAtOrBeforeEachQuote: true,
      quotesAreDisplayedDepthNotActualOrdersOrFills: true},
    captureDiagnostics: replay.diagnostics,
    sourceRecords: {digestAlgorithm: 'SHA-256', locallyHashVerifiedPrimaryPages,
      retainedPrimaryPageDigests,
      knownNewerPrimarySources: recentEvidence,
      verifiedLedgerLimit: 'Each retained page records its manifest expectedHash and computed actualHash with an explicit digestState; later releases have null digests because source bytes were not retained, so they are not probability inputs.'},
    calibration: {modelsReviewed: sourceLedger.modelsReviewed,
      existingSenateModel: {file: 'data/poll-horizon-results-20261005-v2.json', horizonDays: 29,
        allowedPollHorizonDays: [29, 59], races: 240, trainedThroughYear: 2022,
        standardDeviationPoints: 7.595541706884344, distribution: 'fixed_t4',
        populationLabelsAvailable: false, unchanged: true},
      existingGovernorModel: {file: 'research/governor-error-v1/results-20261005-v1.json',
        horizonDays: 45, allowedPollHorizonDays: [45, 75], races: 231,
        standardDeviationPoints: 8.179237418654745, distribution: 'fixed_t4', unchanged: true},
      usedToCreateContractProbability: false,
      reason: 'The reviewed models estimate two-major-party vote-margin errors; current party-winner contracts need exact exhaustive candidate/settlement mapping and a scoped unconditional winner-probability bound. Known newer releases lack retained, hash-verified primary-source bytes.'},
    universe: {capturedMarkets: replay.config.size, rootsAvailableAtQuote: metadata.size,
      rootsMissingAtQuote: replay.config.size - metadata.size,
      marketsWithQuote: replay.quotes.size,
      marketsWithoutQuote: replay.config.size - replay.quotes.size,
      rootCategories: contractKinds},
    exclusions: {counts: reasonCounts, countsOverlap: true, eligibleCandidates: 0,
      rule: 'A contract is eligible only when its exact market/office/stage/candidate settlement, primary source bytes/hash, existing scope-matched error model, numerical lower bound and captured offer all match. Poll percentages alone never become win probabilities.'},
    candidates: rows,
    priceVsBound: {eligibleCandidates: 0, numericComparisons: 0,
      result: 'No defensible contract-level probability bound was available. For every captured quote, the report retains bid/ask, size, source age, and ask plus one adverse tick; lower-bound edge is null rather than inferred from poll share.'},
    limitations: [
      'The Iowa Emerson October 9 release has poll shares and fieldwork dates in the task evidence but its primary page bytes were not retained locally; the source hash is therefore null and it is not used as a probability input.',
      'The newer Texas Siena October 13 and New York Emerson October 8 source records are listed with null hashes; these are not used as probability inputs.',
      'The local poll ledger is not a guarantee of exhaustive internet-wide poll discovery. Other market contracts are explicitly excluded where no exact verified primary source is in the captured ledger.',
      'No poll share is treated as a probability. No uncalibrated horizon transport, national prior, complement assumption, or pollster average is added.',
      'Captured book snapshots are display quotes only. No continuity, queue priority, actual fill, account state, order or profit is inferred.',
      'This is an offline source/quote screen, not an execution or profitability finding.',
    ],
    networkRequests: 0, accountStateRead: false, runtimeChanges: 0, contestOrders: 0,
    credentialsRead: false, eligibleTrades: 0, profitabilityEstablished: false,
  };
}

async function main() {
  const result = await runPrimaryPollMarketScreenV1();
  const target = path.resolve('data/primary-poll-market-screen-20261013.json');
  fs.writeFileSync(target, JSON.stringify(result, null, 2) + '\n', {flag: 'wx'});
  console.log(JSON.stringify({report: target, capturedMarkets: result.universe.capturedMarkets,
    roots: result.universe.rootsAvailableAtQuote, quotes: result.universe.quoteWithinReceiptDeadline,
    eligibleCandidates: result.exclusions.eligibleCandidates, numericPriceVsBound: result.priceVsBound.numericComparisons,
    exclusionCounts: result.exclusions.counts}, null, 2));
}

if (process.argv[1] && fileURLToPath(import.meta.url) === path.resolve(process.argv[1])) await main();
