# Conditional payout capacity evaluator

`node --test live-paper/payout-capacity.test.mjs`

`node live-paper/payout-capacity.mjs input.json` prints a JSON research report.
Import `evaluatePayoutCapacity(input)` to call it from an existing offline analysis.
No dependencies, network, credentials, collector, order placement or runtime state.
This module is not wired into an observer or executor. Existing frozen policies and
banks are untouched. A subsequent integration needs its own review.

## What this adds

Existing basket helpers identify candidate relationships and price gaps. This
standalone evaluator answers a narrower capital question: **how much equal-unit,
two-leg package capacity survives the full ask ladders, explicit costs and a
worst-case single-leg exposure limit?** It consumes explicitly reviewed payout
scenarios, rather than inferring a settlement theorem from party names.

It walks both ask ladders at their joint breakpoints, using integer micro-SUSQ
arithmetic. It stops when marginal conditional payout minus modeled cost ceases
to clear the hurdle, a leg runs out, the quantity limit binds, or total/single-leg
cost reaches its cap. Fixed fees are included once per leg. With ascending asks
and nonnegative constant per-share costs, cumulative profit increases throughout
the admitted marginal segments; the largest admitted quantity maximizes modeled
profit within these limits. A positive marginal edge does not ensure positive
total profit after fixed fees, which is checked separately.

The unmatched-leg bound is the larger individual-leg acquisition cost including
fees and adverse-price allowance, assuming that isolated leg ultimately pays zero.
It is **not** a liquidation estimate, collateral model, cancellation guarantee,
or a limit on the loss if both legs fill and the reviewed scenario set is wrong.

## Input contract

All monetary values are nonnegative decimal **strings**, at most six fractional
digits. Quantities are nonnegative safe integers (depth quantities strictly
positive); output quantities are strings. Timestamps use canonical UTC ISO with
milliseconds. IDs are strings. SHA-256 fields are lowercase 64-character hex.

- `now`: evaluation timestamp, supplied explicitly for reproducible offline work
- `policy`: `maxAgeMs`, `maxSkewMs`, `maxReviewAgeMs`, `maxQuantity`,
  `maxCost`, `maxUnmatchedCost`, `minMarginalEdge`, `minTotalProfit`
- `review`: `id`, `kind: "reviewed_conditional_scenarios"`, `tournamentId`,
  `evidenceHash`, `reviewedAt`, `expiresAt`, `completeUnderAssumptions: true`,
  nonempty `assumptions`, exactly two `legs`, nonempty `scenarios`
- each reviewed leg: `marketId`, `exchangeId`, `outcome: "YES" | "NO"`,
  `rulesHash` binding the exact inspected predicate
- each scenario: unique `id` and two binary `payouts`, one for each leg in order
- `books`: exactly two native outcome books in that same order, with matching
  tournament/market/exchange/outcome/rulesHash, `nativeOutcome: true`,
  `status: "active"`, `sourceHash`, `version`, `requestStartedAt`, `receivedAt`,
  `tickSize`, `adversePricePerShare`, `feePerShare`, `fixedFee`, and `asks`
- asks: ascending distinct prices strictly between 0 and 1 on the tick grid,
  each with `price` and positive integer `quantity`

Freshness uses the request start, not merely the response time. Skew bounds the
whole interval from the earliest request start to the latest response. Future
and reversed clocks are rejected. Prices adjusted for adverse movement cannot
exceed 1. Both outcomes must be supplied natively: this module never synthesizes
NO asks by blindly flipping YES asks, nor substitutes marks for depth.

Unknown fees are a blocker: missing fees fail validation. A zero must be backed
by the caller's evidence; this code cannot verify that assertion. Nonlinear or
quantity-dependent fees must be conservatively bounded by the supplied constant
per-share and fixed components, otherwise do not use this model.

## Minimal scenario example and caveats

For a hypothetical NO(D), NO(R) pair, the ordinary scenarios might be D=[0,1],
R=[1,0], neither=[1,1]. Those alone imply a conditional payout floor of 1.
For distinct exchanges, adding a possible fusion/both-party winner=[0,0] makes the floor zero and prevents
positive capacity. That is intentional. A same-exchange YES/NO pair whose asks
sum to more than 1 also has no positive capacity. Same-exchange YES/NO scenarios
cannot claim both outcomes pay one; a [0,0] conservative void stress is allowed.

The caller must review candidate identities, fusion/affiliation, administrative
changes, void/refund cases and proof expiry before defining the scenario set.
This binary-only evaluator cannot represent acquisition-cost refunds exactly:
exclude such analysis or use a conservative zero-payout stress scenario. Do not
label an ordinary-outcomes-only result guaranteed arbitrage.

`completeUnderAssumptions` is a caller attestation. Hashes bind the supplied
values, not their truth or external authenticity. The returned
`scenarioCompletenessVerified` is always false, `executionAuthority` is false,
`ordersSubmitted` and `networkRequests` are zero, and `fills` / `realizedPnl` are
null. `acceptedForResearch` means only positive conditional modeled capacity.
Do not execute directly from this report. Two fresh books are not an atomic
package, and partial fills or subsequent book movement may destroy the edge.

## Verification scope

Tests use synthetic inputs, exact budget boundary sweeps, depth breakpoints,
fees, stale/skewed/invalid clocks, scope mismatches, fusion stress, duplicate legs,
malformed decimal/quantity data, and quantities whose micro-costs exceed the
safe Number range. They establish behavior, not real-market opportunities or
profitability. No production capture or full repository suite is implied.
