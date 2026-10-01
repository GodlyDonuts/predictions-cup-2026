# Offline paper replay foundation

An isolated, dependency-free Node.js 22+ experiment under `paper/`. It never imports the
existing scanner, reads credentials or portfolio files, makes network requests, or
places/cancels competition orders. `--live` is rejected. The starting paper bankroll
is a required scenario setting; it is unrelated to the five real competition positions.

**This is a tested simulator foundation, not a trained model, profitable strategy,
historical backtest, live feed adapter, or execution-ready bot.** The included data and
orders are invented deterministic fixtures. Their P&L is not evidence of an edge.

## Reproduce

No installation, credentials or external packages are needed. From the repository root:

```sh
node --test bot/*.test.mjs paper/*.test.mjs
node --test --experimental-test-coverage paper/*.test.mjs
node paper/cli.mjs --events paper/fixtures/synthetic.jsonl --config paper/fixtures/config.json
```

Or run `npm test` and `npm run replay` from `paper/`. The runtime version used for this
milestone was Node **24.19.0**; the existing scanner's Node 22+ convention is retained.
The package has no dependencies, so no dependency installation or lockfile is required.

The CLI accepts only local event/config files and prints one JSON report to stdout.
It exits nonzero without a partial success report on validation, chronology, accounting,
duplicate-conflict or ambiguous late-fill errors. Identical duplicate event IDs are
idempotent. A failed engine cannot be reused or report a completed result.

`fixtures/expected-report.json` is a committed golden result. Tests independently check
key ledger and fill values rather than relying only on regenerating that file. Source
and config byte checksums plus the canonical event checksum appear in every CLI result.

## What is modeled

- Independent outcome instruments, long-only integer shares, resting limit orders
- Decimal-string money parsed to exact `BigInt` micro-SUSQies; 0.005 limit/book/trade
  tick and 0.005..0.995 price range; marks can be 0..1 at micro precision
- Cash, inventory and outstanding-order reservations, position limits, FIFO cost basis,
  partial fills, cancel latency, expiry and configurable nonnegative fee scenarios
- Full external books, historical own quantity, timestamps, strictly increasing trade
  evidence sequences, snapshot versions, explicit gaps and authoritative recovery
- Realized P&L, reference unrealized P&L, tournament/last/reference marks and estimated
  depth-constrained liquidation values as separate values
- No-trade baseline; scripted-passive fill ratio, turnover, fees, realized P&L per
  simulated write request, and observed reference-equity drawdown
- Availability-time split manifest with explicit train/validation/test cuts, fixed
  label horizon, purging, embargo, complete source checksum and exclusion reasons

## Fill model and important limits

1. A snapshot, best quote, last-price mark, disappearing depth or a price touch NEVER
   establishes a fill. Only an explicit print at the exact order limit can consume
   queue/fill quantity. A price-through print at another level is not inferred volume.
2. A paper order joins behind all visible same-side depth (including existing real own
   quantity) and earlier paper orders. Absolute offsets are normalized to preserve FIFO
   nonoverlap. Across all paper orders, fills from one print cannot exceed its quantity.
3. New snapshot depth is conservatively placed ahead; canceled or disappearing depth is
   not credited as traded. Canceled paper-order offsets may remain as phantom queue
   ahead of later orders. This deliberately under-fills some paths.
4. Evidence needs a known aggressor direction and `externalAggressor: true`; known own
   aggressor prints cannot fill our paper orders. Unknown ownership MUST NOT be silently
   cast to true by a future adapter. The bundled synthetic fixture states it explicitly.
5. Placement uses receipt/decision time plus configured latency. Prints must occur
   strictly after activation; ambiguous same-time activation prints do not fill. Matching
   occurs at the limit with no price improvement. Crossing external or our paper resting
   liquidity is rejected, including a pending-cancel paper quote. This simulator's
   passive-only admission is NOT a claim that participant post-only orders are supported.
6. Cancels continue to expose orders until modeled cancel acknowledgement. Expiry and
   acknowledgement are effective before a print at the same time. Input order breaks
   receipt-time ties. A newly delivered pre-terminal print that could fill a canceled or
   expired order fails closed, even when stale; ordered recovery is required. Known
   identical evidence duplicates are ignored, conflicting evidence versions halt.
7. Same-sequence trade evidence is never spent twice. `bookSequence` must match the
   current pre-trade full snapshot; the trade sequence must exceed the instrument
   watermark. Snapshots are ordered by sequence, then occurrence time. A newer sequence
   with backward occurrence time invalidates the book. No assumption is made that
   consecutive exchange sequence numbers differ by one: adapters must emit explicit
   gap events from the source's documented semantics.
8. Both source age and receipt age must be within `maxDataAgeMs`; `nextExpiryAt` makes
   depth stale at the exact boundary. Missing, stale, mismatched and gap-invalidated
   evidence cannot fill or admit orders. Recovery retains a source-time and observed-
   sequence barrier independently from the last applied book, including gaps before
   the first book. An authoritative snapshot must be sourced strictly after the last
   invalidation/receipt of skipped evidence and have a sequence at least as high as
   every version observed during recovery. Its source time must also be at least the
   highest source timestamp observed on rejected recovery snapshots; an authoritative
   upgrade at the same sequence/time is allowed, a timestamp rollback is not.
   An in-flight pre-gap snapshot cannot
   restore freshness or roll back the watermark. Recovery of books does not
   reconstruct missed trades.

Pessimistic queue assumptions are **not a mathematical lower bound on P&L**: omitted
fills can omit losses as well as gains. Own paper orders never alter the historical
path or other traders' actions. Market impact, hidden orders, priority changes,
counterfactual liquidity, latency calibration and batch feed ordering remain unknown.
Skipped evidence/gaps and incomplete valuation counts must be inspected. A report
always sets `eligibleForStrategyPromotion: false` for this milestone.

## Accounting and valuation

Every actual simulated fill pays the configured bps charge, rounded UP to a micro,
plus the per-share charge. All costs are explicitly `unverified_scenario`; zero is a
scenario, never a verified fee claim or maker rebate. Buy reserves use worst-case
per-share rounding so fragmentation cannot overspend. Sell orders reserve inventory
and any possible net-negative fee cash. FIFO lots include entry costs; partial lot
allocation leaves the rounding remainder in the lot to conserve basis exactly.

Reference marks alone feed `referenceEquity` and reference unrealized P&L. Last trade and
tournament marks stay separately labeled. Missing/stale reference marks produce null
aggregate reference equity. `observedReferenceMaxDrawdown` uses only available mark
observations; `drawdownComplete` is false when observations were missing. This is not
a continuous, regular-return drawdown or Sharpe estimate.

Estimated liquidation walks fresh external bids, subtracts known historical own depth,
and uses conservative per-share fee rounding. Partial proceeds and unsold quantities
are explicit. `depthComplete` means enough fresh quoted depth. `executableComplete`
also requires sufficient unreserved cash for conservative net-negative fee funding;
`liquidationFeeFundingShortfall` is reported. Aggregate executable equity is null when
either requirement fails. A per-position `fullPositionValue` is a depth-only scenario,
not an admitted order. Values assume the historical depth stays available and existing
working sells can be reconciled/canceled without another fill. No taker implementation
or guaranteed execution is implied by this estimate.

No settlement, refunds, shorting, complementary YES/NO netting, collateral conversion,
cross-market relationships, correlated exposure or account-specific fee schedule is
implemented. Treat YES and NO as separate instrument IDs; do not infer complementary
settlement from naming. Production execution requires independent reconciliation.

## Schema contract, version 1

All events have `schemaVersion: 1`, a stable `eventId`, `type`, `occurredAt`, and
`receivedAt`. Timestamps are valid canonical `YYYY-MM-DDTHH:mm:ss.sssZ`, with
occurrence <= receipt. Replay must be nondecreasing in receipt time. IDs use letters,
digits, colon, dot, underscore or hyphen, start alphanumeric and are <=128 characters.
Monetary inputs are decimal strings (up to six places), quantities are safe positive
integer numbers, and sequences are unsigned integer strings. No implicit float money.

| Type | Required additional fields |
| --- | --- |
| `snapshot` | `instrument`, `sequence`, `authoritative` boolean, `nextExpiryAt` timestamp or null, `bids`, `asks` |
| `trade` | `instrument`, `sequence`, `bookSequence`, `price`, `quantity`, `aggressor` buy/sell, `evidence: explicit_trade`, `externalAggressor` boolean |
| `submit` | `instrument`, `orderId`, `action` buy/sell, `quantity`, `limitPrice`, `expiresAt` timestamp or null |
| `cancel` | `orderId` |
| `gap` | `instrument`, nonempty `reason` |
| `mark` | `instrument`, `source` reference/last/tournament, `price` |
| `clock` | none; advances pending actions and freshness without market evidence |

Each book level is `{price, quantity, ownQuantity}`. Quantity is total displayed size;
own quantity must be explicitly known, nonnegative, and no larger than total. Duplicate
prices and crossed/locked snapshots are rejected. Snapshot sides may be empty. Paper
submits/cancels have occurrence equal to receipt: they cannot backdate a decision.
This internal schema is NOT a verified one-to-one mapping of the supported feed.

Config requires `schemaVersion`, positive decimal `initialCash`, positive integer
`maxDataAgeMs` and `maxPositionShares`, nonnegative integer `orderLatencyMs` and
`cancelLatencyMs`, and `costs: {feeBps, feePerShare, assumption: unverified_scenario}`.
Bps are integers 0..10000. CLI additionally requires the four `split` options shown
in `fixtures/config.json`. `schema.mjs` is the executable validation specification.

## Chronological split and evaluation boundary

`chronologicalSplit(events, {trainEnd, validationEnd, labelHorizonMs, embargoMs})`
assigns by **receivedAt**, the first time information was available to the strategy.
Train is before trainEnd; validation is [trainEnd, validationEnd); test starts at
validationEnd. Timestamp groups stay intact. Embargo excludes [cut−embargo,cut+embargo)
around each cut. Train/validation rows with receipt+labelHorizon >= their next nominal
cut are purged. Invalid/ambiguous cuts, durations and duplicate IDs are rejected.

The CLI uses unique events after the engine has checked retransmission identity;
the raw file checksum still covers the exact original input. The manifest covers all
deduplicated event payloads, including excluded ones. It reports IDs and reasons so a
later model runner can select the appropriate data without a random split.

**The scripted-passive report spans the whole fixture and is not held-out model
performance.** This milestone does not fit, select or tune a model. Split assignment
does not verify label maturity, feature availability, fold-specific fitting, source
revisions or adequate sample size. Later experiments must fit scalers/models only on
training windows, choose hyperparameters on validation, freeze the test set, use only
as-of features and mature labels, and add walk-forward folds. A checksum is lineage,
not proof that a dataset is unbiased or complete.

Sharpe, fixed-horizon adverse selection and fill calibration are deliberately null,
with the missing evidence stated. No profitable ML result is asserted.

## Data needed before meaningful ML or paper-performance conclusions

- Authorized, timestamped/versioned full books, depth and explicit trade records from
  the supported feed, with true receipt/source clocks and gap/recovery lineage
- Verified interpretation of feed sequence/revision and one-second batch ordering;
  an adapter must not invent aggressor direction, queue position or ownership
- Matching own order/fill records to calibrate the simulator and detect ownership;
  strict preservation/redaction of account-sensitive records and no credentials in Git
- Actual latency observations, known fees and expiry/cancel acknowledgement semantics
- Independent reference signals and exact resolution mappings with as-of revisions
- Sufficient chronological observations, stress regimes, mature labels and held-out
  evaluation against simple/no-trade baselines after costs and correlated risk

Feed/reconciliation integration belongs in a separate coordinated milestone. Current
bot/, source research, execution ownership and schedules are unchanged. This draft
does not authorize live connections, API writes, merges or competition execution.

See the repository's [paper strategy plan](../research/fast_strategy_plan_2026-10-01.md)
for the documented requirements and source references. Recheck platform documentation
before a future live adapter; this offline simulator does not validate its current API.
