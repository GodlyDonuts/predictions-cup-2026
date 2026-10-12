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

The fixed-point source's recorded SHA256 is recovered exactly from LF bytes. The report's driver and role-cost source hashes do not match current checkout bytes/LF bytes or versions located in the available Git history. Their exact versions were requested from Rhaegal. Current code was not presented as an exact replay of those historical source files; this review reconstructs the stated arithmetic independently.

The ordinary settlement payout premise, future two-leg completion probability, actual fees, administrative/fusion exceptions, and future exit depth remain independently unverified. These dated observations do not establish current executable liquidity, settlement profit, or immediate profitable churn. Preserve the original experiment and use a separately declared prospective study for additional evidence.

Run `node research/viserion-pair-receipt-review-20261012/review.mjs`. It checks source bindings, six root/scope/clock/depth records, exact protected leg prices, cash funding and both reported P&L cases before writing `results.json`. Input hashes and unresolved code identities are retained. It makes no network requests or order submissions.
