import { createHash } from 'node:crypto';

export const MI_COUNTIES = Object.freeze([
  'Alcona', 'Alger', 'Allegan', 'Alpena', 'Antrim', 'Arenac', 'Baraga', 'Barry', 'Bay', 'Benzie',
  'Berrien', 'Branch', 'Calhoun', 'Cass', 'Charlevoix', 'Cheboygan', 'Chippewa', 'Clare', 'Clinton', 'Crawford',
  'Delta', 'Dickinson', 'Eaton', 'Emmet', 'Genesee', 'Gladwin', 'Gogebic', 'Grand Traverse', 'Gratiot', 'Hillsdale',
  'Houghton', 'Huron', 'Ingham', 'Ionia', 'Iosco', 'Iron', 'Isabella', 'Jackson', 'Kalamazoo', 'Kalkaska',
  'Kent', 'Keweenaw', 'Lake', 'Lapeer', 'Leelanau', 'Lenawee', 'Livingston', 'Luce', 'Mackinac', 'Macomb',
  'Manistee', 'Marquette', 'Mason', 'Mecosta', 'Menominee', 'Midland', 'Missaukee', 'Monroe', 'Montcalm', 'Montmorency',
  'Muskegon', 'Newaygo', 'Oakland', 'Oceana', 'Ogemaw', 'Ontonagon', 'Osceola', 'Oscoda', 'Otsego', 'Ottawa',
  'Presque Isle', 'Roscommon', 'Saginaw', 'St. Clair', 'St. Joseph', 'Sanilac', 'Schoolcraft', 'Shiawassee', 'Tuscola', 'Van Buren',
  'Washtenaw', 'Wayne', 'Wexford',
]);

export const SOURCE_URLS = Object.freeze({
  current: Object.freeze([
    'https://www.michigan.gov/sos/elections/election-results-and-data/voter-participation-dashboard',
    'https://app.powerbigov.us/view?r=eyJrIjoiMTk3MjM4ZWQtMTlkZi00N2QxLTk5ZTktODdhYjk3YTVmMzk3IiwidCI6ImQ1ZmI3MDg3LTM3NzctNDJhZC05NjZhLTg5MmVmNDcyMjVkMSJ9',
  ]),
  historical: Object.freeze([
    'https://www.michigan.gov/sos/-/media/Project/Websites/sos/Press-releases/2026/documents/PRNov2022BallotStatsByJurisdiction_10_17_2022.xlsx?hash=D07DC7BEF7EA35DCCBFEFB6EF7C8AC18&rev=d8901c8bca2443a0abd01a0847ae95c7',
    'https://www.michigan.gov/sos/elections/election-results-and-data/voter-participation-dashboard',
    'https://app.powerbigov.us/view?r=eyJrIjoiMTk3MjM4ZWQtMTlkZi00N2QxLTk5ZTktODdhYjk3YTVmMzk3IiwidCI6ImQ1ZmI3MDg3LTM3NzctNDJhZC05NjZhLTg5MmVmNDcyMjVkMSJ9',
  ]),
  electionResults: 'https://www.michigan.gov/sos/elections/election-results-and-data',
});

const COUNTY_BY_KEY = new Map(MI_COUNTIES.map((name) => [countyKey(name), name]));
const EARLY_IN_PERSON_START = '2026-10-24';
const MICHIGAN_CUTOFF_UTC = Date.parse('2026-10-24T04:00:00.000Z');

function fail(code) {
  throw new Error(`MI_TURNOUT_${code}`);
}

function countyKey(value) {
  if (typeof value !== 'string') fail('COUNTY');
  return value.trim().replace(/\s+/g, ' ').toLowerCase();
}

function requireExactKeys(value, keys, code) {
  if (!value || typeof value !== 'object' || Array.isArray(value)) fail(code);
  const actual = Object.keys(value).sort();
  const expected = [...keys].sort();
  if (actual.length !== expected.length || actual.some((key, index) => key !== expected[index])) fail(code);
}

export function sha256Hex(text) {
  if (typeof text !== 'string' || text.length === 0) fail('CONTENT');
  return createHash('sha256').update(text, 'utf8').digest('hex');
}

function sha256Base64(bytesBase64) {
  if (typeof bytesBase64 !== 'string'
    || !/^(?:[A-Za-z0-9+/]{4})*(?:[A-Za-z0-9+/]{2}==|[A-Za-z0-9+/]{3}=)?$/.test(bytesBase64)
    || bytesBase64.length === 0) fail('CONTENT_BASE64');
  const bytes = Buffer.from(bytesBase64, 'base64');
  if (bytes.toString('base64') !== bytesBase64) fail('CONTENT_BASE64');
  return createHash('sha256').update(bytes).digest('hex');
}

function hashNormalizedRows(rows) {
  if (!Array.isArray(rows)) fail('ROWS');
  const tuples = rows.map((row) => [
    row.county,
    row.asOfDate,
    row.mode,
    row.registrationDenominator ?? row.activeRegistered ?? row.registeredVoters,
    row.returnedMail,
  ]).sort((a, b) => {
    const left = countyKey(a[0]);
    const right = countyKey(b[0]);
    return left < right ? -1 : left > right ? 1 : 0;
  });
  return sha256Hex(JSON.stringify(tuples));
}

function hashRevision({ sourceUrl, receivedAt, asOfDate, contentSha256, rowsSha256 }) {
  return sha256Hex(JSON.stringify({ sourceUrl, receivedAt, asOfDate, contentSha256, rowsSha256 }));
}

function hashLeanRows(rows) {
  if (!Array.isArray(rows)) fail('LEAN_ROWS');
  const tuples = rows.map((row) => [row.county, row.demTwoPartyShare]).sort((a, b) => {
    const left = countyKey(a[0]);
    const right = countyKey(b[0]);
    return left < right ? -1 : left > right ? 1 : 0;
  });
  return sha256Hex(JSON.stringify(tuples));
}

function hashLeanRevision({ sourceUrl, receivedAt, electionDate, contentSha256, rowsSha256 }) {
  return sha256Hex(JSON.stringify({ sourceUrl, receivedAt, electionDate, contentSha256, rowsSha256 }));
}

/** Seal the raw source bytes, parser output, and local receipt metadata as one revision record. */
export function sealSourceRevision({ sourceUrl, receivedAt, asOfDate, contentBase64, rows }) {
  const contentSha256 = sha256Base64(contentBase64);
  if (!Array.isArray(rows)) fail('ROWS');
  const normalizedRows = rows.map((row) => {
    if (!row || typeof row !== 'object' || Array.isArray(row)) fail('ROW');
    return Object.freeze({ ...row });
  });
  const rowsSha256 = hashNormalizedRows(normalizedRows);
  const record = { sourceUrl, receivedAt, asOfDate, contentBase64, contentSha256, rowsSha256 };
  return Object.freeze({ ...record, rows: Object.freeze(normalizedRows), revisionSha256: hashRevision(record) });
}

/** Seal a prior-office county lean source and its locally received bytes. */
export function sealLeanRevision({ sourceUrl, receivedAt, electionDate, contentBase64, rows }) {
  const contentSha256 = sha256Base64(contentBase64);
  if (!Array.isArray(rows)) fail('LEAN_ROWS');
  const normalizedRows = rows.map((row) => {
    if (!row || typeof row !== 'object' || Array.isArray(row)) fail('LEAN_ROW');
    return Object.freeze({ ...row });
  });
  const rowsSha256 = hashLeanRows(normalizedRows);
  const record = { sourceUrl, receivedAt, electionDate, contentBase64, contentSha256, rowsSha256 };
  return Object.freeze({ ...record, rows: Object.freeze(normalizedRows), revisionSha256: hashLeanRevision(record) });
}

function parseUtcTimestamp(value, field) {
  if (typeof value !== 'string' || !/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d{1,3})?Z$/.test(value)) {
    fail(`${field}_UTC`);
  }
  const milliseconds = Date.parse(value);
  if (!Number.isFinite(milliseconds)) fail(`${field}_UTC`);
  const canonical = new Date(milliseconds).toISOString();
  const supplied = value.includes('.')
    ? value.replace(/\.(\d{1,3})Z$/, (_, fraction) => `.${fraction.padEnd(3, '0')}Z`)
    : value.replace(/Z$/, '.000Z');
  if (supplied !== canonical) fail(`${field}_UTC`);
  return milliseconds;
}

function parseDate(value, field) {
  if (typeof value !== 'string' || !/^\d{4}-\d{2}-\d{2}$/.test(value)) fail(`${field}_DATE`);
  const milliseconds = Date.parse(`${value}T00:00:00.000Z`);
  if (!Number.isFinite(milliseconds) || new Date(milliseconds).toISOString().slice(0, 10) !== value) {
    fail(`${field}_DATE`);
  }
  return milliseconds;
}

function relativeHistoricalDate(currentDate) {
  const currentElection = Date.parse('2026-11-03T00:00:00.000Z');
  const historicalElection = Date.parse('2022-11-08T00:00:00.000Z');
  const daysBeforeElection = (currentElection - parseDate(currentDate, 'CURRENT_AS_OF')) / (24 * 60 * 60 * 1000);
  return new Date(historicalElection - daysBeforeElection * 24 * 60 * 60 * 1000).toISOString().slice(0, 10);
}

function validateRevision(revision, kind) {
  const label = kind.toUpperCase();
  if (!revision || typeof revision !== 'object') fail(`${label}_REVISION`);
  const allowedUrls = SOURCE_URLS[kind === 'current' ? 'current' : 'historical'];
  if (!allowedUrls.includes(revision.sourceUrl)) fail(`${label}_SOURCE`);
  const receivedAtMs = parseUtcTimestamp(revision.receivedAt, `${label}_RECEIVED_AT`);
  const asOfMs = parseDate(revision.asOfDate, `${label}_AS_OF`);
  if (receivedAtMs < asOfMs) fail(`${label}_BACKDATED_RECEIPT`);
  if (typeof revision.contentSha256 !== 'string' || !/^[a-f0-9]{64}$/.test(revision.contentSha256)) {
    fail(`${label}_CONTENT_HASH`);
  }
  if (sha256Base64(revision.contentBase64) !== revision.contentSha256) fail(`${label}_CONTENT_HASH_MISMATCH`);
  if (!Array.isArray(revision.rows) || revision.rows.length !== MI_COUNTIES.length) fail(`${label}_INCOMPLETE`);
  const expectedYear = kind === 'current' ? '2026' : '2022';
  if (!revision.asOfDate.startsWith(expectedYear)) fail(`${label}_YEAR`);

  const rowsByCounty = new Map();
  for (const row of revision.rows) {
    if (!row || typeof row !== 'object') fail(`${label}_ROW`);
    requireExactKeys(
      row,
      ['county', 'asOfDate', 'mode', kind === 'current' ? 'activeRegistered' : 'registeredVoters', 'returnedMail'],
      `${label}_ROW_FIELDS`,
    );
    const key = countyKey(row.county);
    const county = COUNTY_BY_KEY.get(key);
    if (!county) fail(`${label}_UNKNOWN_COUNTY`);
    if (rowsByCounty.has(key)) fail(`${label}_DUPLICATE_COUNTY`);
    if (row.county !== county) fail(`${label}_NONCANONICAL_COUNTY`);
    if (row.asOfDate !== revision.asOfDate) fail(`${label}_ROW_DATE`);
    if (row.mode !== 'mail') fail(`${label}_NON_MAIL_MODE`);
    if (row.asOfDate >= EARLY_IN_PERSON_START) fail(`${label}_POST_IN_PERSON`);
    const denominatorField = kind === 'current' ? 'activeRegistered' : 'registeredVoters';
    const registrationDenominator = row[denominatorField];
    if (!Number.isSafeInteger(registrationDenominator) || registrationDenominator <= 0) fail(`${label}_REGISTRATION`);
    if (!Number.isSafeInteger(row.returnedMail) || row.returnedMail < 0 || row.returnedMail > registrationDenominator) {
      fail(`${label}_RETURNS`);
    }
    rowsByCounty.set(key, {
      county,
      asOfDate: row.asOfDate,
      mode: row.mode,
      registrationDenominator,
      returnedMail: row.returnedMail,
    });
  }
  if (rowsByCounty.size !== MI_COUNTIES.length || MI_COUNTIES.some((name) => !rowsByCounty.has(countyKey(name)))) {
    fail(`${label}_INCOMPLETE`);
  }
  const normalizedRows = MI_COUNTIES.map((name) => rowsByCounty.get(countyKey(name)));
  const rowsSha256 = hashNormalizedRows(normalizedRows);
  if (revision.rowsSha256 !== rowsSha256) fail(`${label}_ROWS_HASH_MISMATCH`);
  const fingerprint = hashRevision({
    sourceUrl: revision.sourceUrl,
    receivedAt: revision.receivedAt,
    asOfDate: revision.asOfDate,
    contentSha256: revision.contentSha256,
    rowsSha256,
  });
  if (revision.revisionSha256 !== fingerprint) fail(`${label}_REVISION_HASH_MISMATCH`);
  return { receivedAtMs, normalizedRows, rowsSha256, fingerprint };
}

function validateLeanRevision(revision, office) {
  const field = `${office.toUpperCase()}_LEAN`;
  if (!revision || typeof revision !== 'object') fail(`${field}_REVISION`);
  if (revision.sourceUrl !== SOURCE_URLS.electionResults) fail(`${field}_SOURCE`);
  const expectedElectionDate = office === 'governor' ? '2022-11-08' : '2020-11-03';
  if (revision.electionDate !== expectedElectionDate) fail(`${field}_ELECTION_DATE`);
  const receivedAtMs = parseUtcTimestamp(revision.receivedAt, `${field}_RECEIVED_AT`);
  if (receivedAtMs < parseDate(revision.electionDate, `${field}_ELECTION`)) fail(`${field}_BACKDATED_RECEIPT`);
  if (typeof revision.contentSha256 !== 'string' || !/^[a-f0-9]{64}$/.test(revision.contentSha256)) {
    fail(`${field}_CONTENT_HASH`);
  }
  if (sha256Base64(revision.contentBase64) !== revision.contentSha256) fail(`${field}_CONTENT_HASH_MISMATCH`);
  if (!Array.isArray(revision.rows) || revision.rows.length !== MI_COUNTIES.length) fail(`${field}_INCOMPLETE`);
  const shares = new Map();
  for (const row of revision.rows) {
    if (!row || typeof row !== 'object') fail(`${field}_ROW`);
    requireExactKeys(row, ['county', 'demTwoPartyShare'], `${field}_ROW_FIELDS`);
    const key = countyKey(row.county);
    const county = COUNTY_BY_KEY.get(key);
    if (!county) fail(`${field}_UNKNOWN_COUNTY`);
    if (shares.has(key)) fail(`${field}_DUPLICATE_COUNTY`);
    if (row.county !== county || !Number.isFinite(row.demTwoPartyShare)
      || row.demTwoPartyShare < 0 || row.demTwoPartyShare > 1) {
      fail(`${field}_VALUE`);
    }
    shares.set(key, row.demTwoPartyShare);
  }
  if (shares.size !== MI_COUNTIES.length || MI_COUNTIES.some((name) => !shares.has(countyKey(name)))) {
    fail(`${field}_INCOMPLETE`);
  }
  const normalizedRows = MI_COUNTIES.map((county) => ({ county, demTwoPartyShare: shares.get(countyKey(county)) }));
  const rowsSha256 = hashLeanRows(normalizedRows);
  if (revision.rowsSha256 !== rowsSha256) fail(`${field}_ROWS_HASH_MISMATCH`);
  const fingerprint = hashLeanRevision({
    sourceUrl: revision.sourceUrl,
    receivedAt: revision.receivedAt,
    electionDate: revision.electionDate,
    contentSha256: revision.contentSha256,
    rowsSha256,
  });
  if (revision.revisionSha256 !== fingerprint) fail(`${field}_REVISION_HASH_MISMATCH`);
  return { receivedAtMs, shares, rowsSha256, fingerprint };
}

function scoreOffice(currentRows, baselineRows, demShares) {
  const baselineByCounty = new Map(baselineRows.map((row) => [countyKey(row.county), row]));
  const totalRegistration = currentRows.reduce((total, row) => total + row.registrationDenominator, 0);
  let statewideRateResidual = 0;
  const countyValues = currentRows.map((row) => {
    const baseline = baselineByCounty.get(countyKey(row.county));
    const weight = row.registrationDenominator / totalRegistration;
    const rawRateDifference = row.returnedMail / row.registrationDenominator
      - baseline.returnedMail / baseline.registrationDenominator;
    statewideRateResidual += weight * rawRateDifference;
    return { row, weight, rawRateDifference };
  });
  const score = countyValues.reduce((total, value) => {
    const demeanedDifference = value.rawRateDifference - statewideRateResidual;
    const demLean = 2 * demShares.get(countyKey(value.row.county)) - 1;
    return total + value.weight * demLean * demeanedDifference;
  }, 0);
  return {
    value: score,
    direction: score > 0 ? 'democratic-leaning' : score < 0 ? 'republican-leaning' : 'neutral',
    statewideRateResidual,
  };
}

/**
 * Score a locally captured source revision. This function performs no I/O and
 * returns a research signal only; its observation timestamp is the local
 * receipt timestamp, never the source's as-of date.
 */
export function scoreTurnoutComposition({
  currentRevision,
  historicalRevision,
  governorLeanRevision,
  senateLeanRevision,
}) {
  const current = validateRevision(currentRevision, 'current');
  const historical = validateRevision(historicalRevision, 'historical');
  if (historical.receivedAtMs > current.receivedAtMs) fail('FUTURE_HISTORICAL_RECEIPT');
  if (current.receivedAtMs >= MICHIGAN_CUTOFF_UTC) fail('POST_IN_PERSON_RECEIPT');
  if (currentRevision.asOfDate >= EARLY_IN_PERSON_START) fail('POST_IN_PERSON');
  if (historicalRevision.asOfDate !== relativeHistoricalDate(currentRevision.asOfDate)) {
    fail('RELATIVE_DAY_MISMATCH');
  }

  const governorLean = validateLeanRevision(governorLeanRevision, 'governor');
  const senateLean = validateLeanRevision(senateLeanRevision, 'senate');
  if (governorLean.receivedAtMs > current.receivedAtMs || senateLean.receivedAtMs > current.receivedAtMs) {
    fail('FUTURE_LEAN_RECEIPT');
  }
  const governor = scoreOffice(current.normalizedRows, historical.normalizedRows, governorLean.shares);
  const senate = scoreOffice(current.normalizedRows, historical.normalizedRows, senateLean.shares);

  return {
    schema: 'mi_turnout_composition_research_signal_v1',
    signalOnly: true,
    executionAuthorized: false,
    observedAt: currentRevision.receivedAt,
    currentRevision: {
      sourceUrl: currentRevision.sourceUrl,
      asOfDate: currentRevision.asOfDate,
      receivedAt: currentRevision.receivedAt,
      contentSha256: currentRevision.contentSha256,
      normalizedRowsSha256: current.rowsSha256,
      fingerprint: current.fingerprint,
    },
    historicalRevision: {
      sourceUrl: historicalRevision.sourceUrl,
      asOfDate: historicalRevision.asOfDate,
      receivedAt: historicalRevision.receivedAt,
      contentSha256: historicalRevision.contentSha256,
      normalizedRowsSha256: historical.rowsSha256,
      fingerprint: historical.fingerprint,
    },
    leanRevisions: {
      governor: {
        sourceUrl: governorLeanRevision.sourceUrl,
        electionDate: governorLeanRevision.electionDate,
        receivedAt: governorLeanRevision.receivedAt,
        contentSha256: governorLeanRevision.contentSha256,
        normalizedRowsSha256: governorLean.rowsSha256,
        fingerprint: governorLean.fingerprint,
      },
      senate: {
        sourceUrl: senateLeanRevision.sourceUrl,
        electionDate: senateLeanRevision.electionDate,
        receivedAt: senateLeanRevision.receivedAt,
        contentSha256: senateLeanRevision.contentSha256,
        normalizedRowsSha256: senateLean.rowsSha256,
        fingerprint: senateLean.fingerprint,
      },
    },
    countyCount: MI_COUNTIES.length,
    baselineRelativeDayOffset: 5,
    offices: { governor, senate },
  };
}
