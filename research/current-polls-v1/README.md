# Current polling inputs and executable price diagnostics

This adds independent Republican and Democratic party valuations for Iowa and
Kansas, using the existing corrected historical V2. It does not change the frozen
Tennessee experiment, an active bank, the production window or accounting.

Primary sources: [Emerson Kansas](https://emersoncollegepolling.com/kansas-2026/),
[Emerson Iowa](https://emersoncollegepolling.com/iowa-2026-poll-hinson-leads-turek/),
[Marist Iowa](https://maristpoll.marist.edu/polls/2026-midterm-elections-in-iowa-september-2026/),
[Emerson national context](https://emersoncollegepolling.com/september-2026-national-poll-democrats/),
[Iowa official candidates](https://elections.marshallcountyia.gov/elections/info/2026_general_election_2026_11_03/)
and [Kansas official general-election notice](https://meadeco.org/367/2026-General-Election-Information).
Only selected facts, URLs and content hashes are published. Original HTML stays
in the restricted local runtime, with immutable observation receipts.

Both Emerson state surveys have750 likely voters. Iowa fieldwork ended
September1; Kansas fieldwork ended September16. Marist's newer Iowa survey is
registered voters, so it remains contradictory evidence rather than being
silently converted to a likely-voter forecast. Iowa's unspecified remaining
headline support is unallocated; it is not labeled a measured5% undecided share
or an observed zero for the Libertarian candidate.

The sensitivity model preserves the previous error/transport assumptions,
allows rounding in both directions and computes each party separately. A
discounted Republican valuation cannot simply be complemented into a Democratic
contract valuation: third-party outcomes and the judgment haircut matter.
These ranges and any uniform scenario mean are uncalibrated, not confidence
bounds, a polling consensus, guaranteed payout or evidence of trading profit.
Broader error stresses can move an underdog toward50%, rather than widening
every probability endpoint.

Reproduce the existing immutable artifacts without changing them:

```powershell
node research/current-polls-v1/capture.mjs
node research/current-polls-v1/capture.mjs --ballots
node --test research/current-polls-v1/assessment.test.mjs
```

`build.mjs` and `compare.mjs` are one-time builders. They use exclusive creation
and refuse to overwrite an existing assessment or comparison. Do not rerun them
to make unchanged fieldwork look fresh; introduce a named version with genuine
new evidence or a substantive model change.

Comparison uses exact official contract bindings and actual subsequent books
captured through the existing single collector. Each metadata/book request
replaces an existing auxiliary slot. The comparison clock is the historical book
receipt; computation time and quote age remain separate. No fill is fabricated.
