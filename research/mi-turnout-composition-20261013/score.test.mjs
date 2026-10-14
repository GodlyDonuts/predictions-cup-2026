import test from 'node:test';
import assert from 'node:assert/strict';
import {
  MI_COUNTIES,
  SOURCE_URLS,
  sealLeanRevision,
  sealSourceRevision,
  scoreTurnoutComposition,
} from './score.mjs';

function rowsFor(year, asOfDate, returnedForFirst = 500, returnedForSecond = 500) {
  return MI_COUNTIES.map((county, index) => ({
    county,
    asOfDate,
    mode: 'mail',
    ...(year === '2026' ? { activeRegistered: 1000 } : { registeredVoters: 1000 }),
    returnedMail: index === 0 ? returnedForFirst : index === 1 ? returnedForSecond : 500,
  }));
}

function revision({ sourceUrl, receivedAt, asOfDate, rows, contentBase64 }) {
  const content = `captured source bytes ${sourceUrl} ${asOfDate}`;
  return sealSourceRevision({
    sourceUrl,
    receivedAt,
    asOfDate,
    contentBase64: contentBase64 ?? Buffer.from(content, 'utf8').toString('base64'),
    rows,
  });
}

function reseal(revisionRecord, overrides = {}) {
  return revision({
    ...revisionRecord,
    ...overrides,
    contentBase64: overrides.contentBase64 ?? revisionRecord.contentBase64,
    rows: overrides.rows ?? revisionRecord.rows,
  });
}

function leanRows(firstShare, secondShare) {
  return MI_COUNTIES.map((county, index) => ({
    county,
    demTwoPartyShare: index === 0 ? firstShare : index === 1 ? secondShare : 0.5,
  }));
}

function leanRevision(office, firstShare, secondShare, receivedAt = '2026-10-13T10:00:00.000Z', rows = leanRows(firstShare, secondShare)) {
  const electionDate = office === 'governor' ? '2022-11-08' : '2020-11-03';
  const content = `official ${office} results ${electionDate}`;
  return sealLeanRevision({
    sourceUrl: SOURCE_URLS.electionResults,
    receivedAt,
    electionDate,
    contentBase64: Buffer.from(content, 'utf8').toString('base64'),
    rows,
  });
}

function fixture() {
  const currentRevision = revision({
    sourceUrl: SOURCE_URLS.current[0],
    receivedAt: '2026-10-13T12:00:00.000Z',
    asOfDate: '2026-10-12',
    rows: rowsFor('2026', '2026-10-12', 600, 400),
  });
  const historicalRevision = revision({
    sourceUrl: SOURCE_URLS.historical[0],
    receivedAt: '2026-10-13T11:00:00.000Z',
    asOfDate: '2022-10-17',
    rows: rowsFor('2022', '2022-10-17'),
  });
  return {
    currentRevision,
    historicalRevision,
    governorLeanRevision: leanRevision('governor', 1, 0),
    senateLeanRevision: leanRevision('senate', 0, 1),
  };
}

test('scores a complete revision at local receipt time with separate office leans', () => {
  const input = fixture();
  const result = scoreTurnoutComposition(input);
  assert.equal(result.observedAt, input.currentRevision.receivedAt);
  assert.notEqual(result.observedAt, `${input.currentRevision.asOfDate}T00:00:00.000Z`);
  assert.equal(result.signalOnly, true);
  assert.equal(result.executionAuthorized, false);
  assert.equal(result.countyCount, 83);
  assert.equal(result.offices.governor.direction, 'democratic-leaning');
  assert.equal(result.offices.senate.direction, 'republican-leaning');
  assert.ok(Math.abs(result.offices.governor.value - 0.2 / 83) < 1e-12);
  assert.ok(Math.abs(result.offices.senate.value + 0.2 / 83) < 1e-12);
  assert.equal(result.currentRevision.contentSha256, input.currentRevision.contentSha256);
  assert.match(result.currentRevision.normalizedRowsSha256, /^[a-f0-9]{64}$/);
});

test('is deterministic and does not mutate input source rows', () => {
  const input = fixture();
  const before = structuredClone(input);
  const first = scoreTurnoutComposition(input);
  const second = scoreTurnoutComposition(input);
  assert.deepEqual(input, before);
  assert.deepEqual(first, second);
});

test('rejects mismatched source content hashes and non-official source URLs', () => {
  const input = fixture();
  input.currentRevision = {
    ...input.currentRevision,
    contentBase64: Buffer.from('tampered source bytes', 'utf8').toString('base64'),
  };
  assert.throws(() => scoreTurnoutComposition(input), /CONTENT_HASH_MISMATCH/);
  const wrongSource = fixture();
  wrongSource.currentRevision = { ...wrongSource.currentRevision, sourceUrl: 'https://example.invalid/data' };
  assert.throws(() => scoreTurnoutComposition(wrongSource), /CURRENT_SOURCE/);

  const changedReceipt = fixture();
  changedReceipt.currentRevision = { ...changedReceipt.currentRevision, receivedAt: '2026-10-13T11:59:59.000Z' };
  assert.throws(() => scoreTurnoutComposition(changedReceipt), /CURRENT_REVISION_HASH_MISMATCH/);
});

test('rejects duplicate, missing, unknown, or invalid county rows', () => {
  const duplicate = fixture();
  duplicate.currentRevision = reseal(duplicate.currentRevision, {
    rows: duplicate.currentRevision.rows.map((row, index) => index === 1 ? { ...duplicate.currentRevision.rows[0] } : row),
  });
  assert.throws(() => scoreTurnoutComposition(duplicate), /DUPLICATE_COUNTY/);

  const incomplete = fixture();
  incomplete.historicalRevision = reseal(incomplete.historicalRevision, {
    rows: incomplete.historicalRevision.rows.slice(0, -1),
  });
  assert.throws(() => scoreTurnoutComposition(incomplete), /INCOMPLETE/);

  const unknown = fixture();
  unknown.currentRevision = reseal(unknown.currentRevision, {
    rows: unknown.currentRevision.rows.map((row, index) => index === 0 ? { ...row, county: 'Atlantis' } : row),
  });
  assert.throws(() => scoreTurnoutComposition(unknown), /UNKNOWN_COUNTY/);

  const invalidCount = fixture();
  invalidCount.currentRevision = reseal(invalidCount.currentRevision, {
    rows: invalidCount.currentRevision.rows.map((row, index) => index === 0 ? { ...row, returnedMail: 1001 } : row),
  });
  assert.throws(() => scoreTurnoutComposition(invalidCount), /RETURNS/);

  const validEdit = fixture();
  validEdit.currentRevision = {
    ...validEdit.currentRevision,
    rows: validEdit.currentRevision.rows.map((row, index) => index === 0 ? { ...row, returnedMail: 601 } : row),
  };
  assert.throws(() => scoreTurnoutComposition(validEdit), /ROWS_HASH_MISMATCH/);
});

test('rejects non-mail, early-in-person, and post-cutoff observations', () => {
  const nonMail = fixture();
  nonMail.currentRevision = reseal(nonMail.currentRevision, {
    rows: nonMail.currentRevision.rows.map((row, index) => index === 0 ? { ...row, mode: 'early-in-person' } : row),
  });
  assert.throws(() => scoreTurnoutComposition(nonMail), /NON_MAIL_MODE/);

  const postDate = fixture();
  postDate.currentRevision = reseal(postDate.currentRevision, {
    receivedAt: '2026-10-24T03:00:00.000Z',
    asOfDate: '2026-10-24',
    rows: rowsFor('2026', '2026-10-24'),
  });
  assert.throws(() => scoreTurnoutComposition(postDate), /POST_IN_PERSON/);

  const postReceipt = fixture();
  postReceipt.currentRevision = reseal(postReceipt.currentRevision, { receivedAt: '2026-10-24T04:00:00.000Z' });
  assert.throws(() => scoreTurnoutComposition(postReceipt), /POST_IN_PERSON_RECEIPT/);
});

test('rejects backdated, future, and misaligned source revisions', () => {
  const backdated = fixture();
  backdated.currentRevision = reseal(backdated.currentRevision, { receivedAt: '2026-10-11T23:00:00.000Z' });
  assert.throws(() => scoreTurnoutComposition(backdated), /BACKDATED_RECEIPT/);

  const futureBaseline = fixture();
  futureBaseline.historicalRevision = reseal(futureBaseline.historicalRevision, { receivedAt: '2026-10-13T13:00:00.000Z' });
  assert.throws(() => scoreTurnoutComposition(futureBaseline), /FUTURE_HISTORICAL_RECEIPT/);

  const misaligned = fixture();
  misaligned.historicalRevision = reseal(misaligned.historicalRevision, {
    asOfDate: '2022-10-16',
    rows: rowsFor('2022', '2022-10-16'),
  });
  assert.throws(() => scoreTurnoutComposition(misaligned), /RELATIVE_DAY_MISMATCH/);
});

test('accepts valid UTC timestamps without milliseconds and rejects invalid calendar timestamps or row dates', () => {
  const wholeSecond = fixture();
  wholeSecond.currentRevision = reseal(wholeSecond.currentRevision, { receivedAt: '2026-10-13T12:00:00Z' });
  assert.equal(scoreTurnoutComposition(wholeSecond).observedAt, '2026-10-13T12:00:00Z');

  const impossibleTime = fixture();
  impossibleTime.currentRevision = {
    ...impossibleTime.currentRevision,
    receivedAt: '2026-02-31T12:00:00Z',
  };
  assert.throws(() => scoreTurnoutComposition(impossibleTime), /CURRENT_RECEIVED_AT_UTC/);

  const staleRowDate = fixture();
  staleRowDate.currentRevision = reseal(staleRowDate.currentRevision, {
    rows: staleRowDate.currentRevision.rows.map((row, index) => index === 0
      ? { ...row, asOfDate: '2026-10-11' }
      : row),
  });
  assert.throws(() => scoreTurnoutComposition(staleRowDate), /CURRENT_ROW_DATE/);
});

test('rejects incomplete or malformed independent office lean inputs', () => {
  const missing = fixture();
  missing.governorLeanRevision = leanRevision(
    'governor', 1, 0, '2026-10-13T10:00:00.000Z', missing.governorLeanRevision.rows.slice(0, -1),
  );
  assert.throws(() => scoreTurnoutComposition(missing), /GOVERNOR_LEAN_INCOMPLETE/);

  const invalid = fixture();
  invalid.senateLeanRevision = leanRevision(
    'senate', 0, 1, '2026-10-13T10:00:00.000Z',
    invalid.senateLeanRevision.rows.map((row, index) => index === 0 ? { ...row, demTwoPartyShare: 1.01 } : row),
  );
  assert.throws(() => scoreTurnoutComposition(invalid), /SENATE_LEAN_VALUE/);

  const future = fixture();
  future.governorLeanRevision = leanRevision('governor', 1, 0, '2026-10-13T13:00:00.000Z');
  assert.throws(() => scoreTurnoutComposition(future), /FUTURE_LEAN_RECEIPT/);
});
