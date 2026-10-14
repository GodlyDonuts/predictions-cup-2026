import fs from 'node:fs';
import path from 'node:path';
import {createHash} from 'node:crypto';
import {pathToFileURL} from 'node:url';

const STATES = new Map(Object.entries({
  Alabama:'AL', Alaska:'AK', Arizona:'AZ', Arkansas:'AR', California:'CA', Colorado:'CO', Connecticut:'CT',
  Delaware:'DE', Florida:'FL', Georgia:'GA', Hawaii:'HI', Idaho:'ID', Illinois:'IL', Indiana:'IN', Iowa:'IA',
  Kansas:'KS', Kentucky:'KY', Louisiana:'LA', Maine:'ME', Maryland:'MD', Massachusetts:'MA', Michigan:'MI',
  Minnesota:'MN', Mississippi:'MS', Missouri:'MO', Montana:'MT', Nebraska:'NE', Nevada:'NV',
  'New Hampshire':'NH', 'New Jersey':'NJ', 'New Mexico':'NM', 'New York':'NY', 'North Carolina':'NC',
  'North Dakota':'ND', Ohio:'OH', Oklahoma:'OK', Oregon:'OR', Pennsylvania:'PA', 'Rhode Island':'RI',
  'South Carolina':'SC', 'South Dakota':'SD', Tennessee:'TN', Texas:'TX', Utah:'UT', Vermont:'VT',
  Virginia:'VA', Washington:'WA', 'West Virginia':'WV', Wisconsin:'WI', Wyoming:'WY'
}));
const PARTY = Object.freeze({Democratic:'D', Republican:'R'});
const OTHER = Object.freeze({D:'R', R:'D'});
const HASH = /^[a-f0-9]{64}$/;
const check = (condition, code) => { if (!condition) throw new Error(code); };
const sha256 = value => createHash('sha256').update(value).digest('hex');
const rounded = value => Math.round(value * 1e6) / 1e6;
const canonical = value => JSON.stringify(value && typeof value === 'object'
  ? Array.isArray(value) ? value.map(x => JSON.parse(canonical(x)))
    : Object.fromEntries(Object.keys(value).sort().map(k => [k, JSON.parse(canonical(value[k]))]))
  : value);

function stripHtml(value) {
  return value.replace(/<!--[\s\S]*?-->/g, '').replace(/<[^>]*>/g, ' ')
    .replace(/&nbsp;|&#160;/gi, ' ').replace(/&amp;/gi, '&').replace(/&quot;/gi, '"')
    .replace(/&#39;|&apos;/gi, "'").replace(/&#x([0-9a-f]+);/gi, (_, n) => String.fromCodePoint(parseInt(n, 16)))
    .replace(/&#(\d+);/g, (_, n) => String.fromCodePoint(Number(n))).replace(/\s+/g, ' ').trim();
}
const one = (regex, text, code) => { const match = regex.exec(text); check(match, code); return match; };

/** Parse only explicit published model margin, win percentage, polling margin and update date. */
export function parseOnPointGovernorForecast({state, url, html, fetchedAt, contentHash}) {
  check(typeof state === 'string' && /^[A-Z]{2}$/.test(state) && STATES.has([...STATES].find(([, code]) => code === state)?.[0]), 'FORECAST_STATE');
  check(typeof url === 'string' && url === `https://www.tpsielections.com/forecast/${state.toLowerCase()}/governor`, 'FORECAST_URL');
  check(typeof html === 'string' && html.length > 100 && HASH.test(contentHash), 'FORECAST_SOURCE');
  const ledeHtml = one(/<p\b[^>]*class=["'][^"']*\blede\b[^"']*["'][^>]*>([\s\S]*?)<\/p>/i, html, 'FORECAST_LEDE')[1];
  const lede = stripHtml(ledeHtml);
  const lead = one(/model has (.+?) ahead by ([\d.]+) points?, winning (\d{1,3}) percent of ([\d,]+) simulated elections/i, lede, 'FORECAST_WIN_LINE');
  const modelHtml = one(/<span>\s*Model\s*<b\s+class=["']mono["']>([DR])\s*([+-])\s*([\d.]+)<\/b>\s*<\/span>/i, html, 'FORECAST_MODEL_MARGIN');
  const pollHtml = /<span>\s*Polls\s*<b\s+class=["']mono["']>([DR])\s*([+-])\s*([\d.]+)<\/b>\s*<\/span>/i.exec(html);
  const updated = one(/<span>\s*Updated\s*<b>([^<]+)<\/b>\s*<\/span>/i, html, 'FORECAST_UPDATED');
  const modelSign = modelHtml[2] === '-' ? -1 : 1;
  const modelMagnitude = Number(modelHtml[3]);
  check(Number.isFinite(modelMagnitude) && modelMagnitude >= 0 && modelMagnitude <= 100, 'FORECAST_MODEL_MARGIN_VALUE');
  const favoriteParty = modelSign > 0 ? modelHtml[1] : OTHER[modelHtml[1]];
  const displayedWinPct = Number(lead[3]);
  check(displayedWinPct >= 50 && displayedWinPct <= 100, 'FORECAST_WIN_PCT');
  const pollParty = pollHtml ? (pollHtml[2] === '-' ? OTHER[pollHtml[1]] : pollHtml[1]) : null;
  const pollMagnitude = pollHtml ? Number(pollHtml[3]) : null;
  const pollSignedForModelBase = pollHtml ? (pollParty === modelHtml[1] ? pollMagnitude : -pollMagnitude) : null;
  const modelSignedForBase = modelSign * modelMagnitude;
  const pollSignedForD = pollHtml ? (modelHtml[1] === 'D' ? pollSignedForModelBase : -pollSignedForModelBase) : null;
  const modelSignedForD = modelHtml[1] === 'D' ? modelSignedForBase : -modelSignedForBase;
  const updatedText = updated[1].trim();
  const updatedAt = Date.parse(`${updatedText} 00:00:00 UTC`);
  check(Number.isFinite(updatedAt), 'FORECAST_UPDATED_DATE');
  const electionCount = Number(lead[4].replaceAll(',', ''));
  check(Number.isSafeInteger(electionCount) && electionCount > 0, 'FORECAST_SIM_COUNT');
  return {
    state, url, fetchedAt, contentHash, modelUpdateDate: updatedText,
    modelFavoriteParty: favoriteParty, modelFavoriteCandidate: stripHtml(lead[1]),
    displayedFavoriteWinPct: displayedWinPct, simulationCount: electionCount,
    modelMarginForDPoints: modelSignedForD,
    pollingMarginForDPoints: pollSignedForD,
    modelMinusPollPoints: pollSignedForD === null ? null : modelSignedForD - pollSignedForD,
    displayRoundingOnly: {low: Math.max(0, displayedWinPct - 0.5), high: Math.min(100, displayedWinPct + 0.5)},
    probabilityCalibratedForSIG: false
  };
}

export function parseCallTheMapMichiganForecast({url, html, fetchedAt, contentHash}) {
  check(url === 'https://www.callthemap.com/governors/michigan' && typeof html === 'string' && HASH.test(contentHash), 'CROSSCHECK_SOURCE');
  const description = one(/<meta\s+name=["']description["']\s+content=["']([^"']+)["']/i, html, 'CROSSCHECK_DESCRIPTION')[1]
    .replace(/&amp;/gi, '&').replace(/&quot;/gi, '"').replace(/&#39;|&apos;/gi, "'");
  const forecast = one(/forecast as of ([^:]+):[\s\S]*?ahead of John James by D\+([\d.]+), with an (\d{1,3}) in 100 chance to win\.[\s\S]*?poll average is D\+([\d.]+); (\d+) public polls/i, description, 'CROSSCHECK_FORECAST');
  const modified = one(/"dateModified"\s*:\s*"([^"]+)"/i, html, 'CROSSCHECK_DATE_MODIFIED')[1];
  const updateAt = Date.parse(forecast[1]);
  check(Number.isFinite(updateAt) && Number.isFinite(Date.parse(modified)), 'CROSSCHECK_DATE');
  const pct = Number(forecast[3]);
  check(pct >= 50 && pct <= 100, 'CROSSCHECK_PCT');
  return {publisher:'Call the Map',state:'MI',favoriteParty:'D',favoriteCandidate:'Jocelyn Benson',
    displayedWinPct:pct,displayRoundingOnly:{low:Math.max(0,pct-0.5),high:Math.min(100,pct+0.5)},
    marginForDPoints:Number(forecast[2]),pollAverageForDPoints:Number(forecast[4]),pollCount:Number(forecast[5]),
    forecastAsOf:new Date(updateAt).toISOString().slice(0,10),sourceModifiedAt:modified,url,fetchedAt,contentHash,
    probabilityCalibratedForSIG:false};
}

export function compareIndependentModelToScreens(screens, crossCheck) {
  return screens.filter(row => row.state === crossCheck.state).map(row => {
    const yes = rounded(row.party === crossCheck.favoriteParty
      ? crossCheck.displayedWinPct / 100 : 1 - crossCheck.displayedWinPct / 100);
    const low = row.party === crossCheck.favoriteParty
      ? crossCheck.displayRoundingOnly.low / 100 : 1 - crossCheck.displayRoundingOnly.high / 100;
    const probability = rounded(row.side === 'YES' ? yes : 1 - yes);
    const lower = row.side === 'YES' ? low : 1 - (row.party === crossCheck.favoriteParty
      ? crossCheck.displayRoundingOnly.high / 100 : 1 - crossCheck.displayRoundingOnly.low / 100);
    return {marketId:row.marketId,party:row.party,side:row.side,topAsk:row.topAsk,oneTickAdverseAsk:row.oneTickAdverseAsk,
      externalModelProbability:probability,externalModelDisplayRoundingLow:rounded(lower),
      externalModelRoundingFloorMinusOneTickAsk:rounded(lower-row.oneTickAdverseAsk),
      publishedProbabilityDifferenceVsOnPoint:rounded(probability-row.modelProbability),
      label:'independent-model-sensitivity_only_not_trade_or_profit'};
  });
}

export function parseGovernorContracts(metadata) {
  check(metadata && Array.isArray(metadata.data) && metadata.pagination, 'MARKET_METADATA');
  const contracts = [];
  const seen = new Set();
  for (const market of metadata.data) {
    if (!market || market.status !== 'open' || typeof market.title !== 'string') continue;
    const match = /^Will the (Democratic|Republican) Party win the (.+) Governor\?$/i.exec(market.title.trim());
    if (!match) continue;
    const stateName = match[2], state = STATES.get(stateName);
    if (!state) continue;
    const partyName = match[1][0].toUpperCase() + match[1].slice(1).toLowerCase();
    const exchange = Array.isArray(market.exchanges) ? market.exchanges.find(row => row?.option === 'YES') ?? market.exchanges[0] : null;
    const context = (market.contexts ?? []).find(row => row?.tournament?.id)?.tournament;
    check(typeof market.id === 'string' && /^\d+$/.test(market.id) && typeof exchange?.id === 'string' && /^\d+$/.test(exchange.id), 'MARKET_ID');
    const key = `${state}:${PARTY[partyName]}`;
    check(!seen.has(market.id) && !contracts.some(row => `${row.state}:${row.party}` === key), 'DUPLICATE_MARKET_CONTRACT');
    seen.add(market.id);
    contracts.push({marketId: market.id, exchangeId: exchange.id, state, stateName, party: PARTY[partyName], marketTitle: market.title,
      settlementDate: market.settlementDate ?? null, tournamentId: context?.id ?? null});
  }
  return {contracts, metadataPageSize: metadata.data.length, totalMarkets: metadata.pagination.total,
    hasMoreMetadata: metadata.pagination.hasMore === true, metadataCoverageComplete: metadata.pagination.hasMore !== true};
}

function cents(value) {
  check(typeof value === 'string' && /^(?:0|1)(?:\.\d{1,6})?$/.test(value), 'BOOK_PRICE_FORMAT');
  const n = Math.round(Number(value) * 1e6);
  check(Number.isSafeInteger(n) && n >= 0 && n <= 1e6, 'BOOK_PRICE_RANGE');
  return n;
}
function validateBook(book, record, asOfMs, maxAgeMs) {
  check(book && typeof book.marketId === 'string' && typeof book.exchangeId === 'string' && book.instrument === `${book.exchangeId}:YES`, 'BOOK_IDENTITY');
  check(book.freshnessPolicy === 'authoritative_receipt_v1' && Array.isArray(book.bids) && Array.isArray(book.asks), 'BOOK_POLICY');
  const received = Date.parse(book.receivedAt), started = Date.parse(book.requestStartedAt), source = Date.parse(book.sourceAt), published = Date.parse(record.at);
  check([received, started, source, published].every(Number.isFinite), 'BOOK_TIME');
  // Match the repository's validated authoritative receipt policy: source clocks may lead
  // local receipt by at most 2s. Preserve the signed age so the tolerance is visible.
  check(started <= received && source <= received + 2000 && received <= published && published - received <= 20000, 'BOOK_CAUSALITY');
  check(Number.isFinite(book.sourceEventAgeMs) && book.sourceEventAgeMs >= -2000 && book.sourceEventAgeMs <= 20000
    && book.sourceEventAgeMs === received - source, 'BOOK_SOURCE_AGE');
  check(received <= asOfMs && asOfMs - received <= maxAgeMs, 'BOOK_STALE');
  const levels = (rows, desc) => {
    check(rows.length >= 1 && rows.length <= 20, 'BOOK_DEPTH');
    let prev = desc ? Number.POSITIVE_INFINITY : Number.NEGATIVE_INFINITY;
    return rows.map(row => {
      const p = cents(row.price);
      check(Number.isSafeInteger(row.quantity) && row.quantity > 0, 'BOOK_QUANTITY');
      check(desc ? p < prev : p > prev, 'BOOK_ORDER');
      prev = p;
      return {priceMicros: p, quantity: row.quantity};
    });
  };
  const bids = levels(book.bids, true), asks = levels(book.asks, false);
  check(bids[0].priceMicros < asks[0].priceMicros, 'BOOK_CROSSED');
  return {receivedAt: book.receivedAt, sourceAt: book.sourceAt, sourceEventAgeMs: book.sourceEventAgeMs, publishedAt: record.at, bids, asks};
}

export function analyzeOnPointBooks({contracts, forecasts, books, asOf, maxBookAgeMs = 60000, tick = 0.005}) {
  const asOfMs = Date.parse(asOf);
  check(Number.isFinite(asOfMs) && Array.isArray(contracts) && Array.isArray(forecasts) && Array.isArray(books), 'SCREEN_INPUT');
  check(Number.isFinite(maxBookAgeMs) && maxBookAgeMs > 0 && Number.isFinite(tick) && tick > 0 && tick < 0.05, 'SCREEN_POLICY');
  const forecastByState = new Map(forecasts.map(row => [row.state, row]));
  const latestBook = new Map();
  for (const entry of books) {
    const {book, record} = entry;
    if (!book || !record || record.kind !== 'book') continue;
    const prior = latestBook.get(book.marketId);
    if (!prior || Date.parse(book.receivedAt) > Date.parse(prior.book.receivedAt)) latestBook.set(book.marketId, entry);
  }
  const screens = [];
  const excluded = {missingForecast: 0, missingBook: 0, staleOrInvalidBook: 0, forecastAfterQuote: 0, noTopDepth: 0, tickOutOfRange: 0};
  for (const contract of contracts) {
    const forecast = forecastByState.get(contract.state);
    if (!forecast) { excluded.missingForecast++; continue; }
    const entry = latestBook.get(contract.marketId);
    if (!entry) { excluded.missingBook++; continue; }
    if (entry.book.marketId !== contract.marketId || entry.book.exchangeId !== contract.exchangeId
      || (contract.tournamentId && entry.book.tournamentId !== contract.tournamentId)) {
      excluded.staleOrInvalidBook++; continue;
    }
    let quote;
    try { quote = validateBook(entry.book, entry.record, asOfMs, maxBookAgeMs); }
    catch { excluded.staleOrInvalidBook++; continue; }
    if (Date.parse(forecast.fetchedAt) > asOfMs || Date.parse(forecast.modelUpdateDate) > Date.parse(quote.receivedAt)) {
      excluded.forecastAfterQuote++; continue;
    }
    const yesProb = rounded(contract.party === forecast.modelFavoriteParty
      ? forecast.displayedFavoriteWinPct / 100 : 1 - forecast.displayedFavoriteWinPct / 100);
    const yesLow = contract.party === forecast.modelFavoriteParty
      ? forecast.displayRoundingOnly.low / 100 : 1 - forecast.displayRoundingOnly.high / 100;
    const yesHigh = contract.party === forecast.modelFavoriteParty
      ? forecast.displayRoundingOnly.high / 100 : 1 - forecast.displayRoundingOnly.low / 100;
    const bestBid = quote.bids[0], bestAsk = quote.asks[0];
    const sides = [
      {side: 'YES', modelProbability: yesProb, roundingLow: yesLow, roundingHigh: yesHigh,
        askMicros: bestAsk.priceMicros, topAskQuantity: bestAsk.quantity},
      {side: 'NO', modelProbability: 1 - yesProb, roundingLow: 1 - yesHigh, roundingHigh: 1 - yesLow,
        askMicros: 1e6 - bestBid.priceMicros, topAskQuantity: bestBid.quantity}
    ];
    for (const candidate of sides) {
      if (candidate.askMicros <= 0 || candidate.askMicros >= 1e6) { excluded.noTopDepth++; continue; }
      const tickMicros = Math.round(tick * 1e6), stressedAskMicros = candidate.askMicros + tickMicros;
      if (stressedAskMicros >= 1e6) { excluded.tickOutOfRange++; continue; }
      screens.push({marketId: contract.marketId, exchangeId: contract.exchangeId, state: contract.state,
        stateName: contract.stateName, party: contract.party, marketTitle: contract.marketTitle, side: candidate.side,
        quoteReceivedAt: quote.receivedAt, sourceAt: quote.sourceAt, sourceEventAgeMs: quote.sourceEventAgeMs,
        bookPublishedAt: quote.publishedAt,
        quoteAgeMs: asOfMs - Date.parse(quote.receivedAt), topAsk: candidate.askMicros / 1e6,
        topAskQuantity: candidate.topAskQuantity, zeroFees: true, oneTickAdverseAsk: stressedAskMicros / 1e6,
        modelProbability: rounded(candidate.modelProbability), displayRoundingProbabilityRange: [rounded(candidate.roundingLow), rounded(candidate.roundingHigh)],
        rawProbabilityMinusAsk: rounded(candidate.modelProbability - candidate.askMicros / 1e6),
        roundingFloorMinusOneTickAsk: rounded(candidate.roundingLow - stressedAskMicros / 1e6),
        modelMinusPollPointsForD: forecast.modelMinusPollPoints,
        modelUpdateDate: forecast.modelUpdateDate, forecastUrl: forecast.url, forecastContentHash: forecast.contentHash,
        forecastProbabilityCalibratedForSIG: false, classification: 'descriptive_external_forecast_gap_not_trade_or_profit'});
    }
  }
  return {asOf, maxBookAgeMs, tick, fees: 0, contractCount: contracts.length, forecastCount: forecasts.length,
    screenCount: screens.length, excluded, screens, executionAuthority: false, orderCandidates: 0,
    profitabilityEstablished: false, note: 'External rounded forecast probabilities are uncalibrated for SIG. Rounding bounds are display precision only, not forecast uncertainty. Screens do not account for settlement error, correlated positions, capital limits, fill probability, or competition fills.'};
}

export function readFixedHashChainedBookTail(file, {maxBytes = 64 * 1024 * 1024} = {}) {
  check(path.isAbsolute(file) && Number.isSafeInteger(maxBytes) && maxBytes >= 1024, 'CAPTURE_INPUT');
  const fd = fs.openSync(file, 'r');
  try {
    const stat = fs.fstatSync(fd), byteLengthAtRead = stat.size;
    const offset = Math.max(0, byteLengthAtRead - maxBytes), length = byteLengthAtRead - offset;
    const bytes = Buffer.allocUnsafe(length);
    let read = 0;
    while (read < length) {
      const n = fs.readSync(fd, bytes, read, length - read, offset + read);
      check(n > 0, 'CAPTURE_CHANGED_DURING_SNAPSHOT');
      read += n;
    }
    const rawWindowHash = sha256(bytes);
    let start = 0;
    if (offset > 0) { const newline = bytes.indexOf(0x0a); check(newline >= 0, 'CAPTURE_NO_LINE_BOUNDARY'); start = newline + 1; }
    const end = bytes.lastIndexOf(0x0a);
    check(end >= start, 'CAPTURE_NO_COMPLETE_LINES');
    const books = new Map(), counts = {book: 0, session_start: 0, session_stop: 0, invalidation: 0};
    let cursor = start, lineCount = 0, first = null, previous = null, last = null;
    while (cursor < end) {
      const next = bytes.indexOf(0x0a, cursor);
      if (next < 0 || next > end) break;
      const line = bytes.toString('utf8', cursor, next); cursor = next + 1;
      if (!line) continue;
      let record;
      try { record = JSON.parse(line); } catch { throw new Error('CAPTURE_JSONL_INVALID'); }
      check(record && Number.isSafeInteger(record.sequence) && typeof record.hash === 'string' && HASH.test(record.hash), 'CAPTURE_RECORD');
      const {hash, ...body} = record;
      check(hash === sha256(canonical(body)), 'CAPTURE_RECORD_HASH');
      if (previous) check(record.sequence === previous.sequence + 1 && record.previousHash === previous.hash, 'CAPTURE_CHAIN_GAP');
      first ??= {sequence: record.sequence, at: record.at};
      previous = {sequence: record.sequence, hash}; last = {sequence: record.sequence, at: record.at, hash}; lineCount++;
      if (Object.hasOwn(counts, record.kind)) counts[record.kind]++;
      if (record.kind === 'session_start') books.clear();
      else if (record.kind === 'session_stop') books.clear();
      else if (record.kind === 'invalidation') {
        const marketId = record.payload?.marketId;
        if (marketId === null) books.clear(); else books.delete(marketId);
      } else if (record.kind === 'book' && record.payload?.book) {
        const b = record.payload.book;
        books.set(b.marketId, {record: {kind: record.kind, at: record.at, sequence: record.sequence}, book: b});
      } else check(false, 'CAPTURE_RECORD_KIND');
    }
    check(lineCount > 0 && last, 'CAPTURE_EMPTY');
    return {file, byteLengthAtRead, offset, windowBytes: length, rawWindowHash, first, last, lineCount, counts,
      malformedCompleteLines: 0, books: [...books.values()],
      chainIntegrity: 'every analyzed complete row hash-verified; adjacent rows in window sequence/hash-verified; prefix anchor outside window'};
  } finally { fs.closeSync(fd); }
}

export async function fetchOnPointForecasts(states, {fetchImpl = fetch, now = () => new Date().toISOString(), pause = ms => new Promise(resolve => setTimeout(resolve, ms))} = {}) {
  const out = [];
  for (const state of states) {
    const url = `https://www.tpsielections.com/forecast/${state.toLowerCase()}/governor`;
    const response = await fetchImpl(url, {headers: {'user-agent': 'Baal-quant-research/1.0'}, signal: AbortSignal.timeout(15000)});
    check(response.ok, `FORECAST_HTTP_${state}_${response.status}`);
    const bytes = Buffer.from(await response.arrayBuffer()), fetchedAt = now();
    out.push(parseOnPointGovernorForecast({state, url, html: bytes.toString('utf8'), fetchedAt, contentHash: sha256(bytes)}));
    await pause(120);
  }
  return out;
}

export async function fetchCallTheMapMichigan({fetchImpl = fetch, now = () => new Date().toISOString()} = {}) {
  const url = 'https://www.callthemap.com/governors/michigan';
  const response = await fetchImpl(url, {headers: {'user-agent':'Baal-quant-research/1.0'}, signal: AbortSignal.timeout(15000)});
  check(response.ok, `CROSSCHECK_HTTP_${response.status}`);
  const bytes = Buffer.from(await response.arrayBuffer());
  return parseCallTheMapMichiganForecast({url,html:bytes.toString('utf8'),fetchedAt:now(),contentHash:sha256(bytes)});
}

async function run() {
  const root = path.resolve(process.argv[2] ?? process.cwd());
  const runtime = process.env.LOCALAPPDATA ? path.join(process.env.LOCALAPPDATA, 'SigTradingCup', 'paper-runtime') : null;
  check(runtime && fs.existsSync(path.join(runtime, 'public-books.jsonl')), 'LOCAL_BOOK_CAPTURE_UNAVAILABLE');
  const metadataPath = path.join(root, 'markets.json'), metadataBytes = fs.readFileSync(metadataPath);
  const metadata = JSON.parse(metadataBytes.toString('utf8'));
  const {contracts, ...coverage} = parseGovernorContracts(metadata);
  const states = [...new Set(contracts.map(row => row.state))].sort();
  const forecasts = await fetchOnPointForecasts(states);
  const independentCrossCheck = await fetchCallTheMapMichigan();
  const capture = readFixedHashChainedBookTail(path.join(runtime, 'public-books.jsonl'));
  const asOf = new Date().toISOString();
  const result = analyzeOnPointBooks({contracts, forecasts, books: capture.books, asOf});
  const report = {
    schemaVersion: 1, generatedAt: asOf, method: 'read_only_onpoint_governor_model_vs_captured_SIG_book_v1',
    metadata: {path: 'markets.json', fileMtimeUtc: fs.statSync(metadataPath).mtime.toISOString(), sha256: sha256(metadataBytes), ...coverage},
    source: {publisher: 'OnPoint Politics / Public Sentiment Institute', forecastCount: forecasts.length, forecasts},
    independentModelCrossCheck: {forecast:independentCrossCheck,
      comparisons:compareIndependentModelToScreens(result.screens,independentCrossCheck)},
    marketData: {file: path.basename(capture.file), byteLengthAtRead: capture.byteLengthAtRead, offset: capture.offset,
      windowBytes: capture.windowBytes, rawWindowSha256: capture.rawWindowHash, first: capture.first, last: capture.last,
      lineCount: capture.lineCount, recordCounts: capture.counts, booksAvailableAtWindowEnd: capture.books.length,
      chainIntegrity: capture.chainIntegrity, malformedCompleteLines: capture.malformedCompleteLines},
    screen: result, freshnessMeaning: 'up-to-60s observation for research only; quotes are not revalidated for execution',
    accountOrRuntimeReads: false, contestOrders: 0
  };
  const output = path.join(root, 'data', 'onpoint-model-market-screen-20261014.json');
  fs.writeFileSync(output, `${JSON.stringify(report, null, 2)}\n`, {flag: 'w'});
  console.log(JSON.stringify({output, generatedAt: report.generatedAt, forecasts: forecasts.length,
    marketsInMetadataPage: coverage.metadataPageSize, governorContracts: contracts.length,
    freshScreens: result.screenCount, exclusions: result.excluded, bookWindowMiB: Math.round(capture.windowBytes / 1048576),
    rawWindowSha256: capture.rawWindowHash, profitabilityEstablished: false}));
}

if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) {
  run().catch(error => { console.error(error.message); process.exitCode = 1; });
}
