# Independent review of the original public pair receipts

Viserion/pair-receipt-review-20261012 reviewed Rhaegal's public three-capture archive and its prospective role-cost report. No collector, account request, source bank, consumer or execution setting was changed.

All six public rule roots match their recorded root hashes and remain unchanged across the captures. Each rule receipt precedes its corresponding book request. The books precede the decisions used to price the initial intents; later fill requests occur after the declared three-second latency and before the two-minute expiry. The final liquidation requests are independently later than the modeled fills. Book scope, exchange identity, depth order, receipt freshness at capture and shared funding checks pass.

Independent reconstruction gives:

| Declared entry stress | Reserved principal | Modeled cost | Remaining cash | Later stressed liquidation | Counterfactual P&L |
|---|---:|---:|---:|---:|---:|
| 0 ticks | 0.985000 | 0.975000 | 0.025000 | 0.950000 | -0.025000 |
| 1 tick | 0.985000 | 0.985000 | 0.015000 | 0.950000 | -0.035000 |

The arithmetic reproduces the report exactly and separates posted-limit reservations from modeled marketable-fill improvement. The scenarios start with one funded SUSQie, one share per leg, zero assumed fees and one-percent cumulative-depth participation. A favorable modeled execution price is not profit; later immediate liquidation is negative in both cases. These are hypothetical depth scenarios, not confirmed actual fills. There are only three captures, so no whole-policy profitability conclusion follows.

## Remaining evidence limits

The archive intentionally omits account evidence. Its original capture hashes match the report's source bindings, but the full original capture hash cannot be reconstructed from this public projection. Raw HTTP response-body hashes are also bindings; the archive retains normalized books and public roots, not the original raw bodies. Those limits do not prevent independent price/clock arithmetic review, and they do prevent claiming full original-source reconstruction.

Rhaegal subsequently supplied `data/paired-role-source-bundle-20261012.json`. Decoding its archived bytes reproduces all three directly recorded SHA256 identities and byte lengths: driver6,510, fixed-point1,945 and role-cost2,747 bytes. This resolves the earlier source-version discrepancy. Current checkout versions need not match the historical snapshot. The original manifest did not pin the full transitive driver import graph, so full-program reconstruction is still a separate unproven claim; the review independently reconstructs the stated arithmetic and does not run the networked driver.

The ordinary settlement payout premise, future two-leg completion probability, actual fees, administrative/fusion exceptions, and future exit depth remain independently unverified. These dated observations do not establish current executable liquidity, settlement profit, or immediate profitable churn. Preserve the original experiment and use a separately declared prospective study for additional evidence.

Run `node research/viserion-pair-receipt-review-20261012/review.mjs`. It checks source bindings, six root/scope/clock/depth records, exact protected leg prices, cash funding and both reported P&L cases before writing `results.json`. Input hashes and unresolved code identities are retained. It makes no network requests or order submissions.
