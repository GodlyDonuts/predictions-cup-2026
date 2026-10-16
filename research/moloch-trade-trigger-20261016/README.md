# Separate prospective public-trade sampling

## Actual deployment and present coverage, 15:12 UTC

Read-only observer **PID516505**, source bytes `4e4649c054a469baed4ba57651fae6c201aed8a479ba6f8551e5d28e0e0d891e`, started15:05:34 UTC in new `crossvenue-observer-trade-v1`. The [analysis protocol](analysis-protocol.json) was published/read back on main`4b0b2d2b` before activation; Git-LF SHA `dbf827a8cab904595b6173873949cd8c5f9666769754993b5c7636344bddc291`. Root independently passed all78 Python checks. The switch preserved the actual SIG PID516085 and its process start. Old v10's two gzip segments were closed, decompressed/JSON-checked and sealed. [Actual rollout](deployment/production-rollout.json).

At15:12:21 the observer has fresh process/status, matching source/policy and sampling enabled, but **zero eligible trade samples**. Counters report13,305 accepted SIG books,50 stale-SIG trigger skips, four old trades and one unavailable SIG scope. These are coverage counters, not outcomes; process health is not proof of current target books or a qualified model. [Dated actual verification](deployment/live-verification.json). No old source clock, age rule, v7 result or missing flow is rewritten. Active data is unsealed and has not been analyzed for returns. [Official coverage boundaries](unchanged-book-coverage.md).

At15:12:22 sole SIG production is fresh trading with24 successor ACKs and180 cumulative fill rows. A new6,503-share NO entry at.865 cost5,625.095; subsequent partial exits have brought total attributed realized to**−118.810000**. Cash69,236.720000/owned26,224.385000 and full95,579.915000 capital reconcile. Entry size/fill count is not return. [Actual SIG observation](deployment/sig-live-verification.json). Source-task-only statements below describe the handoff before root deployment.

This opt-in read-only observer mode samples information arrivals directly,
instead of waiting for a coincident SIG book update. It is source-only and has
not been deployed by this task. It adds no account requests, external client,
credentials, trading action or pricing authority. The default book sampler and
all sealed v7 captures/results retain their original meaning.

The [frozen protocol](protocol.json) records the sampling policy and SHA256
`031edcb2d4d6ca144e89d86b467e6b879113f1e172c3ef19457f2b577996717b`.
The module's computed policy hash is authoritative; the tests verify the file
against that exact declaration. This is a sampling protocol, not an approved
forecast, calibrated execution model or complete statistical analysis protocol.

## Admission and timing

`crossvenue_observer.py --sampling-mode trade` uses the existing local cache
polling and existing append-only SIG source. The default remains `book`.
After a complete WS envelope has been processed, original validated public
trade evidence may trigger a decision using a previously retained actual SIG
book. A new SIG book is not requested or synthesized for this purpose.

- Trade source/receipt ages must each be at most2,000 ms. Original processing
  availability must follow receipt and precede source export; original export
  must precede observer first availability, which precedes decision time.
- Source export age is at most2,000 ms. Original SIG source and receipt ages
  are each at most3,000 ms, with no future source/receipt/observer time. Actual
  scope gaps, invalid books and incomplete coverage withdraw the retained book;
  invalid books also censor pending future labels.
- Current selected external book age remains at most20,000 ms under the original
  source-aware selection rules. It must match the trigger ticker and direct
  party-reference mapping. Candidate analogs are skipped. A direct reference
  still has **unverified resolution equivalence**, explicitly retained in rows.
- Known direction, exact trade identity/body hash and full original evidence
  within at least one exported window are required. Partial-window evidence is
  not promoted to an exhaustive tape; it is skipped unless another fully
  evidenced window supplies that original record. Empty windows remain missing.
- Each market/ticker/trade identity is consumed on first observation, including
  stale/missing-book, old/unsigned or unavailable-external skips. A later book or
  poll cannot re-trigger it. At most one sample per scope/second is retained;
  additional first-observed identities are consumed with a spacing-skip count.

The emitted decision pins the original trade record (execution, receipt and
processing clocks, trade/body identity, SID/sequence/connection epoch), exact
source-snapshot hash/export clock, first observer availability, SIG observation
clock and current SIG journal row/hash/sequence. Its sample ID hashes the policy,
trigger proof and original book-sample identity. Trade labels carry the policy
hash; they cannot alias book-triggered cohorts. No original timestamp is renewed.

The original1/3/10/30/120-second horizons and3-second label slack are unchanged.
Only a later actual valid SIG book whose source and receipt both reach the
target horizon can label a sample. There are no labels at trigger time, synthetic
future prices, credited fills or implied maker executions. Counterfactual
one-share crossing labels retain their original explicitly assumed zero fees.

## Bounds, restart and diagnostics

The new cache holds at most237 SIG scopes; pending samples cap at128 per scope.
At most32,768 first-observed identities are retained. A conflicting identity/body
or a full identity table disables further trade sampling for the process; IDs
are never evicted to enable re-triggering. Existing genuine pending labels may
still complete, and status explicitly reports `triggerDisabled`. This does not
declare stream coverage healthy or authorize a financial recovery.

Trade mode requires a new output directory. A create-new, fsynced policy marker
is written before observation; an existing marker/status/capture refuses startup.
Seen identities are process-local, so an intentional restart needs a separately
named cohort. Cross-cohort analysis must also deduplicate original trade IDs.
No existing sealed capture is reopened, removed or automatically continued.

Counters distinguish duplicate/conflicting IDs, seen/cache/pending caps, old or
future trades, invalid/partial evidence, unknown direction, analog references,
missing/stale SIG, missing current external and sample-spacing skips. Original
SIG/flow validation counters remain available. Rejected triggers never become
zero-flow observations or fabricated healthy coverage.

## Selection and validation limits

This is an event-conditioned, poll-visible subset of known-direction prints
with current SIG and external books. Scarcity, stale SIG, delivery lag and
skipped identities can be informative selection. These samples cannot be
compared to v7's book-triggered observations as randomized or unconditional
returns. Existing controls need the same event cohort and causal availability;
a separate fit/held-out protocol must be frozen before outcome inspection.
The100-OOS/100-selected research thresholds confer no financial authority.
No prospective sample count, edge, fill rate or profit improvement is claimed.

Validation:11 new tests and33 existing observer tests pass locally. The default
deterministic book/label output matches its exact pre-change byte SHA256
`475a5499e3705ade2da67942c853b94ccd488be18c3d53211b84f99bd4b428cb`.
New tests use original-shaped book/trade evidence and cover first-availability
binding, delayed labels, duplicate/skip consumption, missing/stale/future inputs,
actual gap/invalid-book withdrawal, conflict/cap disable, analog denial and
fixed bounds/new-cohort refusal. No network, SSH or production operation was run.
