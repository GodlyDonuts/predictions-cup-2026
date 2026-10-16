# Entry economics: value the actual partial-fill exit lifecycle

Read-only review, October16. Recommendation: qualify ordinary maker entries against
their **quantity-dependent, partial-fill inventory/exit lifecycle**, rather than
raising the displayed microprice threshold. This is a measurement/selection proposal,
not demonstrated positive EV or authorization to change production.

## What the original evidence supports

The frozen [ordinary-fill study](../moloch-maker-markout-20261016/README.md) covers
50 actual ordinary resting-after-ACK BUY-fill observations across24 intents. Only
nine have full-depth1s marks; eight have120s marks. Their displayed marks less
entry/entry fees are−28.24 and−31.05 respectively. Most older fills have no capture
coverage. Own quoted depth was not removed, later exit fees are unknown, actual
venue liquidity role is unavailable, and these are not realized strategy returns.

The frozen [existing-threshold partition](../moloch-maker-markout-20261016/guard-study.json)
rejects two submission contexts and keeps seven: the kept observations are still
−3.90 at1s and−6.71 at120s. Immediately before execution, eight pass and only one
fails. This falsifies “passing this proxy demonstrates positive expectancy” in
these few observations; it does not establish a new profitable threshold or causal
losses avoided by cancelling. The quantities/intents are dependent and no outcome
is assigned to an order that did not actually fill.

Paper6's original private attachment hashes to
`be96a03a94886ac984d31f6d517a9cfee6de6054f2e3ed5d957c19dd227e136b`.
Its “Use the same signal both offensively and defensively” section explicitly
requires value **conditional on being filled**, with exit/capital/tail costs.
The public papers' figures and claims are research leads; they do not calibrate
that conditional distribution for our account.

## Why a passing entry can still lose

1. [Native desired()](../../moloch/native/src/engine.rs#L837) plans ordinary
   `pairedSpread` legs from the combined YES/NO discount. Physical submission is
   separate; an ordinary pair of resting bids is not an atomic multi-leg fill.
   One-sided execution does not earn the combined redemption discount.
2. [The current side assessment](../../moloch/live-runtime/src/ordinary_side_edge.rs#L124)
   computes a displayed top-depth proxy minus the unchanged limit and entry fee.
   It is neither the future executable bid nor an expected value conditional on
   another trader choosing to fill us. The final
   [worker gate](../../moloch/live-runtime/src/worker.rs#L1334) is a submission
   assessment, not a continuously authenticated cancel signal for a resting order.
3. [Inventory management](../../moloch/native/src/engine.rs#L964) takes the current
   executable side at maximum inventory age, without requiring the fixed-gain
   target. Before maximum age it can seek the fixed gain; that target is not a
   guaranteed realized exit. The60s clock is a native planning trigger, not a
   promise of a physical sale60s after execution: source freshness, depth,
   reconciliation, reservations, activation and account authorization still apply.
4. [Entry size](../../moloch/native/src/engine.rs#L1347) can use100% of the relevant
   displayed top depth under the original pricing policy. The maximum-age exit
   targets at most the configured50% of then-current opposite executable top
   depth. Those are different sides/times; neither a large entry-depth count nor
   collateral/cash availability establishes inexpensive liquidation capacity.

For scale only, the execution owner's reported6503-share NO entry at.865 has
principal5625.095. A one-tick adverse move costs32.515 on that entire quantity;
the **minimum passing proxy threshold** .001 times6503 is only6.503. The actual
assessed edge may be larger;6.503 is not a verified expected gain. This arithmetic
does not attribute the subsequently reported unwind proceeds, fees or realized
loss. Those require the original matched fills, which this review did not fetch.

## The concrete qualification mismatch

The existing [empirical execution model](../../moloch/live-runtime/src/execution_edge.rs#L249)
accumulates original partial fills, but returns before making its mark until
`filled_quantity == requested quantity`. Its mark target is the **last completing
fill's execution time + markout_ms**. `first_executed` is retained for coverage;
it does not set that target. Full-order fill probability and a mark conditional on
full completion are meaningful quantities, but do not describe the entire actual
partial-fill lifecycle.

Native maximum-age planning instead uses the **oldest remaining new FIFO lot**.
For example, one partial share at t0 and another completing the order at t0+59s
can cause an inventory exit at t0+60s; the full-fill model's60s mark targets
t0+119s. The model need not label a permanently partial order's economic exit at
all. This is a source-level counterexample, not a claim about the actual6503 fill.

[maker_policy.rs](../../moloch/live-runtime/src/maker_policy.rs#L133) already has
opaque qualification, an empirical nonpositive-markout veto and bounded unqualified
exploration. Its non-full-fill principal charge is explicitly conservative policy,
**not** an estimate of partial-fill exit proceeds. Merely enabling that optional
path would not remove this horizon/coverage distinction. No current empirical
qualification or enabled-control state was independently checked for this review.

## One proposed improvement and the smallest faithful implementation

Extend ordinary-maker qualification to track an original attempt as a sequence of
actual FIFO inventory exposures, including partial, complete and terminal-zero-fill
outcomes. Each label must follow the native oldest-lot inventory policy actually
applicable at that time and the quantity it would need to dispose of. Keep these
lifecycle labels distinct from the existing full-fill labels; do not rename old
training observations or pool unlike target definitions.

The selection question becomes: **after actual entry costs and a conservative
quantity-dependent executable exit/capital/risk charge, is the prospective
fill-conditioned lifecycle value positive?** P(fill) alone cannot turn a negative
conditional filled return into positive EV. Cash available, a top-book imbalance
and the combined paired discount cannot supply missing expectation.

The narrow source scope would be `execution_edge.rs` plus focused actual-Session
tests, followed by `maker_policy.rs` only to consume a separately typed qualified
lifecycle estimate. Root-owned physical authority and worker integration must
still bind the opaque decision. Existing ordinary prices/quantities, budgets,
SELL/cancel/covered-hedge authority and unknown-acceptance fences remain intact.
Missing qualification should not pretend to be positive EV; any bounded exploration
remains explicitly unqualified and requires its existing reviewed capital/loss policy.
This review makes no code or control change.

## Measurements that distinguish improvement from another proxy

- Freeze the maker-specific policy/model/horizon and evaluation rules before the
  next evaluation cohort. Do not change the already frozen trade-trigger taker
  protocol or select a horizon/threshold after its results.
- Preserve every original submitted intent, planned quantity, quote price,
  seven-receipt preflight, source/receipt/processing clocks, actual ACK, partial
  execution time/quantity, cancellation request/response, terminal proof and full
  official fill tape. Nonfills require genuine terminal evidence; a touched book
  or public trade never becomes an own fill.
- Bind each original FIFO lot and its consumption to the native head/policy.
  Measure oldest-lot threshold→fresh source→native exit proposal→physical exit
  attempt→actual exit fills. Retain residual inventory and missing-depth cases;
  neither call60s a guaranteed deadline nor force a fictitious60s exit.
- At predeclared execution-age and actual-exit clocks, value the actual exposure
  through full executable side depth after removing known own resting liquidity.
  For NO use the complement of YES asks. Report insufficient depth, gap, delayed
  availability and unknown fees as unavailable, not zero proceeds. Separate
  displayed executable marks from actually realized FIFO/netting cash.
- Compare the frozen side-only criterion with this lifecycle qualification on the
  same prospectively observed attempts and independently held-out intent/market
  time clusters. Report partial/full/zero-fill coverage, actual inventory duration,
  net conditional marks, actual realized exits, worst/tail loss and capital-time.
  Cancelled/unsubmitted alternatives have no claimed counterfactual fill profit.
  Better prediction or lower loss alone does not establish profitable deployment.

The existing frozen sample cannot supply this comparison: it lacks most historical
books, is not an all-attempt/nonfill trial, and does not establish the proposed
native-lifecycle labels. No new statistics were fitted, outcomes searched, current
cohort opened, network/SSH/account request made, or runtime/source policy changed.
