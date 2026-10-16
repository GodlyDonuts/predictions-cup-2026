# Paper 7: execution economics and tournament decisions

Received October16,2026. Private attachment SHA256
`3b7199c71dea9d31da1119476e80481648c937e0519af47b5e0e797db99a5603`.
[Implementation task158](https://github.com/GodlyDonuts/predictions-cup-2026/issues/158).
The paper is research input. Its embedded source identifiers and schedules are
not verified API evidence, execution authority or measured deployment results.

## Corrected starting point at22:09UTC

The paper audited main27b4ad10 and historical worker540259. Actual production is
sole545165/source774f94fe/binary1180882d, six managed terminal models, native8acc
and twelve original bridges. At22:09:25UTC it was healthy/fresh trading with55
post-launch ACKs,21 new fill rows/235 cumulative, cash70,746.200000 and owned
cost24,680.835000. Combined realized−152.880000 includes actual trading losses;
increased order/fill counts are not evidence of a profitable strategy.

The current priority release, tracked in155, fixes the consumed-planning-packet
activation gate, indexes already closed startup ACKs, and adds opt-in profitable
cumulative-depth exits and same-side cumulative-budget top-ups. Source20543435
passes actual ARM64 runtime212/worker106 tests. Nativeb5 passes76 unit tests,
with one existing benchmark explicitly ignored. Fundamental96, value8 and depth14
also pass; fundamental uses corrected fixturea408 with the unchanged205 libraries.
Initial fixture book-version/signed-NO failures are retained. Optimized release,
prefix validation and deployment remain pending. Source publication is not activation.

## Paper 7 implementation split

| Work | Current state | Required useful result |
|---|---|---|
| Joint maker qualification | Child source work, default-off option | Actual zero/partial/full outcomes, block uncertainty and full capital-time economics determine qualification; old score remains diagnostic |
| Own-account realtime | Official semantics audited; prospective source work | Existing socket carries private account invalidations and prioritizes exact REST reconciliation; no WS financial booking |
| Acquisition distribution | Data/API audit | Distinguish acquisition quantity/timing from terminal or deadline value |
| Calibrated joint posterior | Historical/as-of dataset audit | Immutable, independently validated artifact with source/training fences and correlated outcomes; no claimed calibration yet |
| Kalshi innovations | Existing checked event-trigger observer; V7 resource stop | Complete original-input cohorts, causal source clocks, fixed out-of-sample comparison; no qualified alpha yet |
| Whole-account/rank objective | Existing research components, future integration | Complete holdings/refunds/debt and source-bound opponent scenarios before rank-based risk allocation |

Maker source work is isolated to maker_policy, focused tests and a read-only
execution_distribution config identity getter. Account source work is isolated
to realtime, account invalidation, authority generation and worker routing.
Root retains sole publication, build and deployment ownership. These changes
must not delay completing the current live entry repair.

## Verified account API facts

At22:01:28UTC a fresh public read of the official reference confirmed the private
`user:{profile_id}` channel and `account_batch` events. Current token parsing drops
that user topic; current protocol subscribes only to the market topic. Resting
fills can arrive without orderUpdates; window totals are not lifetime totals;
fill attribution/price can be null. Therefore account WS messages invalidate
facts and trigger reconciliation, while original REST receipts remain financial
evidence. Reconnect/gaps and quiet periods still require REST coverage.
[Official API reference](https://sig.thesuper.market/api/v1/docs#description/introduction).

The same reference confirms account-wide100 reads/30 writes per minute and up
to50 independent batch children per one HTTP batch write. Token POSTs are writes;
the existing SDK classifies token minting as a read. Current24-write limit leaves
headroom for its infrequent mint, but that mismatch is recorded for correction.
No batch executor, extra account client or additional rate allowance is activated
by this document. Official private API capabilities govern implementation.

## Economic objective

Measure after-cost increment, actual filled quantity and reserved/owned capital
time together. Terminal settlement value and executable deadline cash are
different targets. External candidate-win markets can be predictors without
being equivalent settlement hedges. More concentrated or negative-mean rank
candidates need a sourced, complete tournament comparison; the paper's example
sleeves do not change production risk limits automatically.

Paper7 is **not complete**. The current production objective and Paper6 remain
open; no overall profitability, calibrated posterior or qualified joint maker
model is claimed.
