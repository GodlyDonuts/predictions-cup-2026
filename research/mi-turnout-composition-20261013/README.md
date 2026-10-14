# Michigan early-vote composition scorer

This is an offline, deterministic research scorer for one prospective hypothesis: whether the counties returning more mail ballots than their same-days-before-election 2022 comparison are disproportionately Democratic-leaning or Republican-leaning. It produces a descriptive direction by office. It does not estimate election probability, market value, execution, or profit, and it has no order or platform integration.

## Primary sources

- Michigan SOS, [2026 voter participation dashboard](https://www.michigan.gov/sos/elections/election-results-and-data/voter-participation-dashboard), with its [official Power BI dashboard](https://app.powerbigov.us/view?r=eyJrIjoiMTk3MjM4ZWQtMTlkZi00N2QxLTk5ZTktODdhYjk3YTVmMzk3IiwidCI6ImQ1ZmI3MDg3LTM3NzctNDJhZC05NjZhLTg5MmVmNDcyMjVkMSJ9). The dashboard describes participation data from QVF and notes records can be corrected retroactively.
- Michigan SOS, [jurisdiction-level 2022 absentee ballot workbook dated October 17](https://www.michigan.gov/sos/-/media/Project/Websites/sos/Press-releases/2026/documents/PRNov2022BallotStatsByJurisdiction_10_17_2022.xlsx?hash=D07DC7BEF7EA35DCCBFEFB6EF7C8AC18&rev=d8901c8bca2443a0abd01a0847ae95c7).
- Michigan SOS, [October 13, 2026 statewide early-vote release](https://www.michigan.gov/sos/resources/news/2026/10/13/three-weeks-before-election-day-6-percent-of-voters-have-already-cast-their-ballot). Its statewide figures are stated as of 07:00 ET on October 13; the release does not supply an exact publication timestamp. That as-of time is never used as a local receipt time.
- Michigan SOS, [official election-results archive](https://www.michigan.gov/sos/elections/election-results-and-data), for distinct prior county lean inputs: 2022 Governor and 2020 Senate. Each is a separately receipt-time/hash-sealed input; the scorer does not fetch or infer them.

## Input and receipt rules

`scoreTurnoutComposition` accepts four already captured source-revision records: 2026 participation, 2022 participation comparator, prior Governor results, and prior Senate results. Each record carries the raw source bytes, local receipt time, normalized rows, and hashes that bind the receipt metadata and parsed rows:

```js
{
  sourceUrl,                 // exact official URL allowlisted in score.mjs
  receivedAt,                // local receipt timestamp, UTC ISO-8601 ending in Z
  asOfDate,                  // source data's Michigan calendar date, YYYY-MM-DD
  contentBase64,             // exact captured bytes (UTF-8 dashboard export or binary XLSX)
  contentSha256,             // SHA-256 of decoded bytes, verified by the scorer
  rows: [{ county, asOfDate, mode: 'mail', returnedMail, activeRegistered }], // current
  rowsSha256,                 // canonical normalized row digest
  revisionSha256              // digest binds source URL, receipt time, date, and both hashes
}
```

Current participation rows use `activeRegistered`; historical rows use `registeredVoters`, matching their source-specific denominator definitions. Lean rows have `{ county, demTwoPartyShare }` with values from 0 to 1. Create turnout records with `sealSourceRevision({ sourceUrl, receivedAt, asOfDate, contentBase64, rows })` and prior-lean records with `sealLeanRevision({ sourceUrl, receivedAt, electionDate, contentBase64, rows })`; both compute hashes and freeze the returned record and rows. The caller must retain the original bytes, receipt record, parser/version, and sealed rows together in immutable local storage. The scorer verifies the raw-content hash and row/revision digests and validates all 83 Michigan counties, but deliberately does not perform source fetching, filesystem writes, or source-format parsing. These digests bind the exact source-content hash, normalized rows, and local receipt metadata for audit; they are integrity checks, not source signatures. A source's reported as-of timestamp must not be substituted for `receivedAt`.

The Michigan dashboard reports aggregate QVF voter-history counts, not tabulator totals, and displays values of 10 or fewer as “Less than 10.” It can also revise prior counts when clerks correct records. This scorer requires an exact numeric row for every county; if a source export contains a suppressed value, it must be rejected rather than imputed or reconstructed. Preserve each exact downloaded revision and its local receipt time so later source corrections do not rewrite an earlier observation.

Current observations must be dated before October 24, 2026, contain only `mail` rows, and have been received before 00:00 Michigan time on October 24 (04:00 UTC). Any early-in-person mode or post-cutoff row/revision fails closed. The 2022 and 2026 participation snapshots must be on the same relative day to their respective November 8 and November 3 elections; therefore the historical as-of date is exactly five calendar days after the 2026 as-of date. Each prior-lean revision must be locally received no later than the current participation revision; its election date is fixed to November 8, 2022 for Governor or November 3, 2020 for Senate. Every input must contain exactly one valid row per Michigan county. Duplicate, missing, unknown, invalid, or mismatched rows are rejected rather than silently dropped.

`demTwoPartyShare` in each separately sealed lean revision is the prior Democratic share of the two-party vote, between zero and one. Governor and Senate inputs are independently validated and scored; the scorer does not assume one office's lean represents the other.

## Calculation

For county (c), let (r_{c,t}) be returned mail ballots divided by active registered voters in the 2026 snapshot, and (r_{c,b}) returned mail ballots divided by the 2022 source's registration denominator in the relative-day historical snapshot. Let (w_c) be the county's share of 2026 active registration. The scorer first removes the statewide weighted change:

```text
delta[c] = (r[c,t] - r[c,b]) - sum_j(w[j] * (r[j,t] - r[j,b]))
lean[c,office] = 2 * prior_dem_two_party_share[c,office] - 1
score[office] = sum_c(w[c] * lean[c,office] * delta[c])
```

A positive score is labelled `democratic-leaning`, a negative score `republican-leaning`, and exactly zero `neutral`. These labels describe the sign of this ecological composition statistic only; they are not forecasts or trade instructions. `observedAt` is copied from the current snapshot's local `receivedAt`, never its earlier source as-of date.

## Run focused tests

```powershell
node --test research/mi-turnout-composition-20261013/score.test.mjs
```

The tests cover hash verification, deterministic scoring, timestamp use, complete county coverage, independent office leans, and fail-closed duplicate/invalid/post-cutoff inputs. Passing unit tests establish code behavior only. This one-state aggregate statistic remains unvalidated; it can be confounded by registration changes, mail-return timing, local election administration, and ecological bias. No edge or profitability is claimed.
