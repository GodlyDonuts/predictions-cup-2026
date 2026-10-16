# Prospective stale-state sampling, without stale execution

Declared October 16 at 17:47:42 UTC, from `bfd1e368`. **Offline design only.**
[protocol.json](protocol.json) fixes a future **17:55–18:15 UTC** coverage window
inside the already running V5 original input trace. Derive it only after V5's
normal seal and successful exact-input replay. It changes research eligibility
only; the active V5 sampler, its protocol, historical replay, financial authority
and live freshness guards
remain unchanged. No sample values or economic outcomes were opened for this
design. The existing studies cannot support the proposed model fit.

## What the retained metadata establishes

| Original bounded probe | Books / exchanges | Source age >3s | Receipt age median / p90 | Target trade probes |
| --- | ---: | ---: | ---: | --- |
| [777-book report](../moloch-trade-trigger-20261016/source-age-probe/report.json) | 777 / 226 | 610 (78.5%) | 7,924 / 11,291 ms | 7 suffix-missing, 2 stale |
| [779-book report](../moloch-preflight-service-20261016/measurement/age-report.json) | 779 / 214 | 432 (55.5%) | 2,069 / 7,923 ms | 4 suffix-missing, 3 stale |
| [869-book report](../moloch-defensive-cancel-20261016/measurement/age-report.json) | 869 / 219 | 181 (20.8%) | 838 / 6,064 ms | 3 suffix-missing, 1 stale |

All books were accepted; all three probes observed **zero** same-sequence,
newer-original-time versions. Across these small windows, 14 of 20 trade probes
had no target book **in the newly captured suffix**. The six matched trade
observations were only 639–1,334 ms old at first probe availability, whereas
their target books had original source ages 4,427–10,511 ms and receipt ages
2,293–10,165 ms. Every matched target failed the 3-second source-age criterion.
These are metadata joins, not returns or independent statistical samples.
Absence from a fresh suffix does not establish absence from the full cache.

The [V3 partial status](../moloch-trade-trigger-20261016/witness/deployment/trace-v3-live-verification.json)
at 16:47:45 UTC recorded 114 stale-SIG skips and one sample. Root subsequently
read only the original final private status at **17:05:12.684 UTC**: 270
`tradeStaleSigSkip`, 95 `tradeOldSkip`, 3 `tradeNoCurrentSigSkip`, 2
`tradeNoCurrentExternalSkip`, 1 spacing skip, 3 samples, 7 labels and 7 missing
timely-label counts. Duplicate/invalid-flow/window counters overlap and have
different units; summing them is not a unique-trigger denominator. This final
counter observation was supplied by root, not independently hash-verified here.
The [original V3 seal](../moloch-defensive-cancel-20261016/deployment/prior-trace-v3-seal.json)
pins ten output rows; root reports exact-input replay of those ten rows and seven
labels. [V2](../moloch-trade-trigger-20261016/witness/actual-v2/README.md)
reproduced two rows and one label. Neither supplies a fit/holdout population.

The 3-second rule structurally selects trade times near a *received target-book
read*. A quiet target can therefore contribute no research row despite a timely
external trade. The evidence supports this eligibility bottleneck, but does
**not** establish quiet-market inactivity as its sole cause: delivery pauses,
observer scheduling, suffix warmup, sparse publication and best-effort drops
also matter. There is no append timestamp separating all these delays.

Official [retained book semantics](../moloch-book-freshness-semantics-20261016/README.md)
say `asOf.at` is engine **read time**, not last price-change time. Silent targets,
other-market updates and heartbeats are not fresh target witnesses. REST may be
cached for up to one second; request start does not repair engine time. The new
pilot must preserve that distinction even when a stale book is a useful feature.

## Separate future pilot

Commit the protocol before 17:55 UTC; if it misses that deadline, this interval
is void and requires a newly declared future interval, never a backdated one.
No new collector, process, account read or active-sampler change is needed.
After the existing V5 cohort seals normally, verify its entire input witness
and replay with its exact reviewed producer before deriving coverage. Pin that
seal and the full causal prefix, including pre-17:55 state/seen identities;
do not initialize an empty cache at the window boundary. Do not change V5's
sample policy, emitted outputs or interpretation. This is a separately named
metadata derivation from future inputs, not a replacement experiment.
Consume each original trade identity once. Keep the original
2-second trigger/export and 20-second external-book limits, verified observed
flow, known direction and explicit mapping. Record every valid mapped trade
unit in a disjoint denominator even if its SIG state is missing, stale or gapped.
This remains a poll-visible subset, not the venue's complete trade tape.

At each original decision callback in the trace, derive the *already available*
last verified
SIG book and its original hash, version, source/request/receipt/first-availability
clocks. Store separate ages and continuity/conflict flags. A prior valid book
before a gap may be retained as historical state, but cannot furnish current
depth, cross-gap labels or an executable quote. Conflicted/future/missing state
has no numeric book features. Predeclare source-age bands: **0–3, >3–10,
>10–60, >60–300 and >300 seconds**, plus missing/invalid. Do not reset old clocks,
forward-fill freshness, infer an unchanged book, invent direction, or impute a
missing price. No probabilities or age-dependent uncertainty curve are supplied
by this pilot.

The primary measurements are denominators, stage-specific exclusions, age-band
coverage and time to the next genuine target observation. Record the old
3-second *book eligibility flag* on the same future inputs; it is not a replay
or amendment of the active experiment. Apply the original one-second per-scope
spacing only to label nominations; retain every skipped unit and reason in the
denominator. Stop enrollment at 18:15 UTC or 5,000 valid mapped units, whichever
comes first; consult already recorded pending-horizon metadata only through
18:17:03 UTC. If the normal V5 seal ends earlier, record right censoring; do not
extend the live trace for this design. Preserve identity/pending bounds and
count capacity censoring explicitly. If an original source witness required
by the derivation is absent, abstain rather than repair its clock or infer it.

For this pilot evaluate only **label availability**, never price changes or
returns. Fixed-horizon availability retains 1/3/10/30/120-second horizons and
3-second slack.
Only an original target book whose source, receipt and observer availability are
all at/after the target time and within that slack can supply a label; its
source/receipt age at availability must also be at most 3 seconds. A late book
does not backdate a missed horizon. A later, separately preregistered model
study could predict original bid/ask/midpoint **levels**, not a return from a
stale assumed entry. Any change from the stale reference
contains unobserved pre-trigger movement as well as later movement and is not
identified causal trade impact. A separate next-observation endpoint reports
its variable delay, right-censored at 123 seconds; it is not a fixed-horizon
return. The coverage derivation must not inspect V5 sample/label values or
select intervals using them. No touch, spread or displayed quantity is credited
as a maker fill.

This pilot can establish whether removing the *research* book-age exclusion
recovers valid historical states with measurable future observation coverage.
It cannot establish that those features predict prices. Could-be-quiet targets
are only targets with no retained updates, not proven unchanged engine books.
The pilot cannot manufacture timely labels.
Missingness is likely informative: event-driven target updates select active
markets and shocks. Report label availability by original age band, market and
time block, including all failures; observed-label statistics describe that
selected population. Do not impute zero changes or use unestimated inverse
propensity weights. If timely labels remain scarce, stop: a separately approved,
signal-independent randomized/scheduled label-read study through the existing
sole worker is needed before population-wide price/EV claims. It is not
authorized or implemented by this document.

## Boundary before any fitted or live strategy

No model is fitted in the pilot. A later pre-outcome fit protocol must specify
chronological training/heldout blocks, race/market clustering, overlap embargo
of at least 123 seconds, stale-state-only and age-only baselines, sparse-band
abstention and missingness reporting. More correlated rows are not more
independent evidence. No automatic model or financial qualification follows.

A later prediction can nominate a target for refresh. Its executable decision
must occur **after** the sole worker obtains genuine fresh target depth and the
unchanged account/rules/ownership/risk facts. Record trigger-to-refresh and
refresh-to-decision delays, including abandoned candidates. Reassess value and
quantity against that later book; do not value an entry at the old lagged quote
or select successful refreshes and call them all-trigger EV. Mappings remain
reviewed references, not certified resolution equivalence. The immediate next
action is root review of this finite coverage pilot, not a production guard
change or another unbounded capture.
