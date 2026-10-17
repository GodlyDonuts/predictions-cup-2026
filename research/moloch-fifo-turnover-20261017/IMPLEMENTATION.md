# Explicit FIFO-profit turnover

Source candidate `20f829f2544410416c99ca02f349eed0976f7a2a` combines the exit objective with reviewed passive exits, unchanged-reservation retry, verified dispatch-row caching and bounded realtime book catch-up. It is not a claim of deployment or profit. Consult the current production baseline and dated release evidence for installation and activation status. Tasks [169](https://github.com/GodlyDonuts/predictions-cup-2026/issues/169) and [170](https://github.com/GodlyDonuts/predictions-cup-2026/issues/170) record qualification and rollout.

The prior passive rule sells only at the reviewed conditional forecast value after fees. That can leave an accounting gain unrealized below the forecast. The new, separately reviewed objective permits such a sale to recycle capital. Forecast value remains unchanged; the exit objective changes. This choice has not been shown to outperform holding the same position or to improve tournament winning probability.

## Explicit review permission

The optional field below belongs inside a complete authenticated `ReviewInput`. It cannot approve itself, create a cohort or authorize an order by its presence alone.

```json
{
  "passiveValueExit": {
    "version": 1,
    "maximumQuantity": 10000,
    "pricingObjective": {
      "kind": "fifoProfitTurnover",
      "minimumNetGain": "0.005000"
    }
  }
}
```

Omitting `passiveValueExit` preserves historical behavior and canonical review bytes. Omitting its `pricingObjective` preserves the separately approved forecast-value passive behavior. The FIFO objective requires a version2 review for an authentic, closed original-entry cohort. Inherited inventory is excluded because fees incurred before adoption are unknown. It can still use its existing reviewed value or covered-exit routes.

Adding this permission changes the canonical review hash and requires a new owner-approved generation. It does not renew forecasts, source age limits, review expiry, holding schedules, entry windows, risk limits or the shared API budget.

## Pricing and quantity

The native engine reconstructs the remaining FIFO lots from original owned BUY/SELL fills and verifies them against the actual wallet. It includes each BUY fill's original fee through a conservative upper bound `ceil(fee / fillQuantity)` per unit. Those fees remain charged exactly once in financial accounting; the upper bound is only a pricing calculation.

For a proposed first-n FIFO prefix, the minimum net sale price is the maximum acquisition unit cost in that prefix plus the explicitly approved minimum gain. The limit rounds upward to a valid tick after configured SELL fees, including conservative one-share fee rounding. Every positive partial fill at or above that limit therefore satisfies the configured gain bound relative to the authenticated acquisition-cost bound. This depends on correct venue fill semantics and configured fee assumptions; a reservation is not execution evidence.

Quantity is bounded by actual owned units, the native per-order cap, the review cap and the passive cap. The planner first selects the longest contiguous profitable prefix at one tick inside the observed held-side ask. If that prefix is empty, it tries the unchanged ask. If neither price admits a profitable prefix, it may rest the capped prefix at its own fee-inclusive profit floor. It never skips an expensive leading lot to sell a cheaper later lot.

For NO holdings, held-side bid is `1 − YES ask` and ask is `1 − YES bid`. Competing ask quantity does not establish available sell depth. A limit may rest, fill partially, fill immediately or never fill. Cash and realized P&L change only through actual official fill feedback. The [historical census](README.md) provides conditional examples and the limits of those observations.

## Attribution and physical execution

The native reservation retains `expectedPayout` as the original conditional forecast. A distinct `reviewed_fifo_profit_turnover` reason, `forecastExpectedPayout`, `minimumNetSalePrice`, FIFO cost digest and complete pricing basis identify the new objective. Its separate model digest binds the forecast model, review and pricing basis. The native engine independently rebuilds the cost floor instead of accepting runtime cost assertions.

Original entry/exit acknowledgments, complete fill totals, terminal records, actual order IDs, native wallet FIFO and position hashes must agree. An open pledge, competing liability, unresolved physical outcome, foreign fill, package exposure or changed position blocks a new reservation. The existing managed-inventory exclusion prevents ordinary planning from creating a competing order.

The same single worker collects fresh preflight facts, binds the original request, checks current trusted review/source/expiry and produces the existing physical authorization token. A genuinely untouched reserved passive quote can retry preparation with its original ID and terms; an attempted or uncertain order cannot use that path. Known open quotes remain on the original fill/cancel/reconciliation path.

## Processing changes in the same candidate

The verified-row cache removes repeated parsing while retaining a complete original-file hash and current never-attempted proof on each hit. It grants no SDK retry authority. Its measured component improvement is documented [separately](../moloch-fundamental-not-sent-cache-20261017/README.md); whole-worker speed and profit do not follow from that measurement.

Book catch-up records and normalizes every original public frame. Within bounded book-only segments it can select the latest valid complete book for each exact scope. Nonbook events remain barriers. An observed account-generation change keeps all originals. Original receipt clocks remain unchanged, and native queue admission records actual processing time. This does not establish total ordering across separate public/private sockets or promise lower latency.

The release changes native financial code and therefore requires an explicit bridge. The unchanged native startup must validate every exact historical bridge checkpoint and the stopped financial ledger before constructing new order authority. Source/tests/release verification is distinct from this actual startup validation.

## Remaining economic uncertainty

The feature protects a configured accounting-profit floor; it supplies no passive fill-rate estimate, calibrated forecast, capital opportunity-cost model or empirical tournament-rank improvement. Selling below the forecast can sacrifice modeled terminal value. Future owned fills, exact fees, realized proceeds, inventory exposure and subsequent outcomes must decide whether this objective improves Moloch. Quote count and submitted volume are not substitutes for those results.
