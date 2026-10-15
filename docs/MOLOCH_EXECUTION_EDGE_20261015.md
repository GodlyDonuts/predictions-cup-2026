# Moloch causal execution edge — October 15

This component estimates ordinary passive-entry execution quality from original
owned attempts, complete actual order lifecycles and subsequent executable books.
It also ranks independently checked ordinary/package plans by gain, gross funding
and capital lock time. It creates no orders, account client, credentials, funding
reservation, activation, payout certificate or execution authority.

The existing actual-source study has 86 owned fills and sparse new-BUY markout
coverage: the covered 30-second NO BUY mark was −0.005 per share on 716 shares;
the covered 1/5-second new BUY marks were flat. Missing books were unavailable,
not zero returns. Quote creation to execution averaged roughly 485–510 seconds
for those new BUY groups. None of this establishes positive alpha or a reliable
short capital lock horizon. See the [sealed-prefix study](../research/moloch-owned-fill-markouts-20261015/README.md)
and [aggregate](../research/moloch-owned-fill-markouts-20261015/actual-aggregate.json).

## Actual integration API

`execution_edge::Model::new(&bank, config)` pins the actual live bank manifest,
run ID and exact native policy hash. The model is deliberately non-Deserialize.
A cold process has no fitted confidence. Rebuild by passing the original durable
events in chronological order; do not import public prints, aggregate markout
summaries, current portfolio marks or a claimed fitted model as actual fills.

The execution owner registers the module in the runtime library and calls:

1. `observe_committed(&bank, source_id, sequence, &prepared_event)` after a
   successful `Session::accept`, or for each genuinely accepted event after a
   bounded batch succeeds. Pass the **actual prepared event including its
   original `processedAt`**, not a newly timestamped reconstruction. The model
   removes only `sourceRef` and checks its canonical hash against
   `Session::committed_source_hash(source_id, sequence)`, which reads the
   financial session's existing retained source-reference map. Batch-end bank
   state does not substitute a later book or later ownership state for an earlier event.
   The standard `moloch-live`/`moloch-package` cursors are pinned at construction;
   unknown writer streams must begin at a fresh first commit, not a historical suffix.
2. `track_attempt(&bank, &ordinary_dispatch, qid, now_ms)` immediately after the
   original durable ordinary physical-attempt record and before the SDK call.
   This snapshots features only. The exact current original quote must match
   the log's intent, have zero fills, remain unretired and have no existing ACK.
   Package legs, inventory SELLs and displayed-crossing entries are excluded
   from this ordinary passive-entry denominator. Actual maker status is not
   reported by these fields; “passive” means relative to the pre-attempt book.
3. Continue feeding actual ACK/fill/complete-terminal events through the same
   committed hook. Public trade events never become owned fills. Official
   receipt hashes, native order identity and unchanged financial fill facts
   remain required. Duplicate financial observations do not add samples.
4. `maker_candidate(&bank, qid, Some(&ExecutionCosts), now_ms)` evaluates a new
   native planned quote without sending it. Costs explicitly include inventory
   and incomplete-fill risk, plus an independently source-bound capital lock
   horizon. Passing `None` withholds allocation; a 30-second hypothetical mark
   does not prove that capital returns in 30 seconds.
5. `allocate(candidates, now_ms, gross_cash, capital_limit, shared_writes)` ranks
   planning candidates. Keep all existing account/STOP/unknown, portfolio,
   covered-position, current-book, physical request and durable-intent checks.
   Recheck freshness and economic proof at actual dispatch. Results always
   report `execution_authority: false`.

The owner must hook **all** actual writer paths, including ordinary reconciliation
and package actual fills, not only realtime books. An estimator error disables
its confidence; it must never drop or reject a valid official financial event,
undo a fill, bypass a cancellation or block a required liability reconciliation.
Policy/manifest changes require a fresh model or an explicitly checked replay.

## Causality and label definition

Buckets are exact exchange/market, YES/NO, BUY, original quantity, original limit,
spread and relative quote distance. No cross-market or inherited-SELL pooling is
assumed. Original features precede physical submission. Starting a cohort blocks
another cohort on the **same exchange**, across side/price/quantity buckets,
until its entire fill/mark/post-window/embargo interval ends. A burst of partial
fills cannot manufacture independent observations.

The fill model means **full original requested quantity filled within a fixed
window**. Partial official rows aggregate their unchanged quantities, principal
and fees; the model starts its markout only after that complete quantity exists
within the window. Individual partial depth cannot justify scaling to the whole
request. Early cancellation without a full fill is censored, not a false
zero-fill trial. A still-unresolved mature order withholds coverage. The complete
official terminal/tape path is required to finish the denominator; ACK alone is
never a fill or completed exposure.

The mark target is full-fill completion time plus the configured horizon. The
first observed same-exchange book whose source time reaches the target decides
the label. A shallow, stale, invalid, uncovered or late first book remains an
unavailable label; a later favorable/deeper book cannot replace it. A gap
censors a pending label, even if a later source recovery succeeds. A fill first
observed after its target is unavailable rather than retrospectively matched to
a convenient prior book. Source time, request time, receipt time and original
processing time have separate freshness checks. Complete contiguous trade-window
coverage must include the first component fill through the selected source
book, with sufficient projected sequence. Missing coverage does not become zero.

YES liquidation walks YES bids. NO liquidation walks `1 − YES ask` with the
original level quantities. Every modeled close covers the entire actual cohort. Known owned resting quantities are removed from displayed
depth. The model maintains its own committed ACK/terminal shadow: it does not
read a later batch-end fill/terminal state to improve an earlier label. At cold
start, historical bound orders without a known complete terminal are removed
conservatively, which can reduce coverage rather than create alpha.
Conservative execution surplus subtracts the greater of actual principal and
the original full limit notional, actual fees, and the independently specified
future close fee. Historical price improvement is not assumed to repeat.
Negative division rounds down. No hypothetical book close is credited as cash,
an actual sale, realized P&L or a completed portfolio round trip.

The online estimate exposes the equal-cohort mean, worst sample, coverage counts,
censoring and latest label availability time. Allocation uses the **worst observed
per-share result minus an explicit stress buffer**, multiplied by empirical full
fill probability after a probability haircut, less supplied inventory/partial
risk cost. This is an empirical stress rule, not a statistical confidence bound,
stationarity proof, calibrated ML model or guaranteed expected value. The API
explicitly reports `statistical_confidence_claimed: false`. Calendar-expired marks and exposure labels cannot supply confidence. Minimum independent
sample counts and coverage thresholds must pass before a positive estimate can
become a planning candidate. Asking a fitted model about an earlier clock is
unavailable; no future labels can enter a past decision.

## Portfolio economics and allocation

The allocator orders positive candidates by exact cross-multiplied
`net_gain / (gross_funding × source-bound holding_horizon)` with deterministic
ID tie-breaking. It checks separate gross cash, maximum capital lock, shared
write budget and overlapping risk groups. BUY funding is never reduced by
proposed SELL proceeds or collateral advances. This greedy ranking is bounded,
not a claim of globally optimal portfolio selection.

Eligible bases remain distinct: `OwnFillConditionalEstimate` is an empirical
execution estimate; `StatewiseGuaranteedIncrementalGain` requires an independent
checked minimum of **the change in wealth in every state**, including all costs.
An input hash is provenance, not an authenticated certificate.

`DownsideFloorUtility`, `LockedVsLiquidationBenchmark` and `ExecutableCashGain`
are diagnostic metrics and are excluded from positive-alpha allocation. For
example, values `[0,100] → [10,90]` increase the portfolio floor by 10, while the
minimum statewise change is **−10**. Selling an owned asset releases cash without
automatically earning that cash as profit. Neither number may be relabeled as
risk-free incremental gain. Graph plans must supply the correct economic basis,
source proof, full gross funding, maximum locked capital, explicit campaign/
settlement horizon and validity deadline.

## Bounds, restart and verification

Configuration validates windows, source ages, an explicit maximum training age, known fee provenance, key/pending
limits and retained samples. Defaults are intentionally supplied by the owner;
this module does not silently select a profitable-looking horizon or policy.
At most 1,024 keys, 4,096 pending attempts, 256 retained samples per bucket,
64 levels per side, 4,096 own-order shadow records and 128 actual fill rows per cohort are allowed. Source
identity is limited to eight existing writer streams. Exceeding source/model
integrity bounds withholds confidence rather than expanding memory or creating
another collector. Source cursor conflicts or changed policies disable the model.

The new tests use actual live `Session` commits and original ordinary dispatch
journals, with no account calls. They include chronological partial-fill maturity,
first-shallow-book selection, gaps/recovery, delayed observations, duplicate
receipts, exact source-byte rejection, early-terminal censoring, two disjoint
full-fill cohorts, unknown cost/turnover, full-quantity scaling and allocation
counterexamples for floor utility, gross cash, risk overlap and overflow.
Parent Linux compilation and publication are recorded in task
[#143](https://github.com/GodlyDonuts/predictions-cup-2026/issues/143). Source alone
does not establish current coverage, deployment, profitable trading or completion
of Paper 2.

## October15 unavailable-attempt and transport correction

The execution owner inspected frozen V28 suffix317931441 without a new account
request. Its first original attempt was BUY5 NO/exchange1023 at0.730 at
23:18:39.610Z. The last admitted book was processed23:18:39.161, with
source23:18:38.976233, request23:18:38.777 and receipt23:18:39.109: all were fresh.
The most recent complete trade window ended23:18:27.583 with projected sequence
172430631, behind the book's172432504 by1873 revisions; the last gap was17:59.918.
This is owner-read retained evidence, not independently fetched by this task.
The confirmed model defect was that a normal key eligibility error permanently
disabled the observer **before recording the original attempt**. The observed
cause was missing past continuity, not stale timestamps or an old WS subscription.

Two explicit changes are separate:

* Current quote-only features (spread, distance, quantity, original limit) require
  the exact original durable intent/attempt and fresh authoritative native depth.
  They do not consume public historical trades. Consequently lagging **past**
  public-trade coverage is no longer a prerequisite for capturing those features.
  This adds no OFI/trade-history feature and manufactures no public prints.
  Native invalidity, scope/source membership and clock guards remain. Every
  future actual-fill→first-markout-book coverage/projection check and gap censor
  remains mandatory; later favorable books/tapes cannot replace an unavailable
  first horizon observation.
* Missing/stale/insufficient current depth or unavailable passive eligibility is
  recorded as a bounded censored original entry attempt with exact quote digest,
  coarse known quote identity, time and reason. No later book backfills its
  unknown features or labels. Observer processing continues. Original entry
  counts include these cases; feature-qualified independent cohorts are counted
  separately. Invalid bank/policy/source cursor/bytes/original dispatch binding
  and exhausted memory/integrity bounds still disable model confidence only.

Censored attempts match estimates conservatively by known exchange/market/side/
quantity/limit, without inventing unknown spread/distance. They enter the sample
coverage denominator; actually full-filled censored attempts also enter missing
conditional-mark coverage after their actual fill horizon. Unclosed mature
attempts remain open. They do not count as zero-fill labels or positive samples.
The conservative probability lower score includes the censored denominator before
its configured haircut. Later genuine independent samples can meet the configured
coverage ratio (for example9 observed plus1 censored at90%); one missing attempt
is not a permanent coarse-key veto. These remain empirical coverage rules, not
statistical confidence or adversarial all-state EV guarantees. Retention is the
configured training window, with the existing pending bound covering both known-
feature and unavailable attempts; bounds never grow silently.

Transport provenance is retained: WS effective freshness uses ORIGINAL source,
receipt and processing clocks; REST requires ORIGINAL source, request, receipt
and processing freshness. Processing delivery remains bounded separately. An old
WS update cannot become fresh merely because it arrived now, and old REST
`asOf.at` is not waived. The actual realtime producer currently normalizes
requestStartedAt=receivedAt; a test with an older subscription request is a
semantic safeguard, not an assertion about the observed V28 failure.

Cold-start own-liquidity shadows stay conservative for retired native orders.
Retirement alone never proves physical closure. At the next genuine original
ordinary attempt, only a matching actual original DispatchLog.closed entry,
original ACK actual ID/native binding, immutable intent identity and retired
native quote permits terminal tombstoning/removal. No future batch-end lookup or
invented terminal receipt is used. Tombstones retain their4096 bound. This repairs
historical closed-order shadows while ambiguous/unknown physical orders remain.
