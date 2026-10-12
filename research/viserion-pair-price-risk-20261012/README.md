# Pair economics at the permitted hedge price

Viserion/20261012-pair-price-risk audits a distinction in the dated funded-pair report. Its observed uniform first/hedge limits need not equal the planner's highest permitted hedge price. `fundedPairPlanV2` defaults to a 0.005 minimum package surplus and reserves up to 0.995 per pair. That upper price changes conditional gain and exception exposure, even if the first leg fills exactly at its limit.

| Historical illustrative budget | Quantity | Observed conditional surplus | Minimum permitted conditional surplus | No-exception completion must exceed: observed / permitted |
|---|---:|---:|---:|---:|
| 1 SUSQie | 1 | 0.020000 | 0.005000 | 0.428572 / 0.750000 |
| 10 SUSQies | 10 | 0.200000 | 0.050000 | 0.428572 / 0.750000 |
| 100 SUSQies | 92 | 0.920000 | 0.460000 | 0.714286 / 0.833334 |

These are scenario arithmetic on the original report, not current executable offers, account cash or estimated completion rates. A declared 1% zero-payout exception makes all three **maximum-permitted-price** cases fail positive expected return even at 100% ordinary completion. The ordinary payout premise and exception probability are not independently verified; no actual probability is estimated.

## Exact scenario price cap

For quantity `Q`, maximum pair principal `P`, unmatched unwind loss `L`, completion probability conditional on ordinary resolution `c`, and zero-payout exception probability `e`:

`Expected return = (1-e) * [c * (Q-P) - (1-c) * L] - e * P`.

`price-cap.mjs` solves this inequality for `P` at a strictly positive declared return target. It intersects the result with the existing gross reservation and ordinary package-surplus constraint, then floors the uniform package limit to the 0.005 tick. Subtracting the protected first-leg price gives the maximum hedge limit. Every monetary calculation and probability coefficient uses integer micros; negative decimal display rounds down. The numerical cap **does not validate the probabilities or authorize execution**.

For example, assume 90% completion, 0.5% exception, 0.015 unmatched loss, and first limit 0.575 for one pair. The observed 0.980 package has +0.011517 floor expected return, while the permitted 0.995 package has -0.001990. The same assumptions cap the package at 0.990 and hedge at 0.415, retaining +0.002512 floor expectation. At 1% exception the package cap falls to 0.985. These assumptions are not recommended production settings.

Run from the repository root:

```powershell
node --test research/viserion-pair-price-risk-20261012/price-cap.test.mjs research/pair-completion-economics-v1.test.mjs
node research/viserion-pair-price-risk-20261012/audit.mjs
```

Eleven checks pass: observed versus maximum-cost sign reversal, exact tick cap, probability sensitivity, capital/return targets, quantity scaling, impossible/invalid scenarios, conservative rounding and the existing completion-economics checks. The audit reproduces original report principal/surplus before computing its alternatives. It records hashes of the report and planner, preserves every 4-by-4 probability case, and makes no account requests. No original planner, source report, runtime, bank or activation was changed.

Next: the execution owner can use independently supported completion/exception evidence to select a price guard before any deployment. Future unwind depth remains uncertain; zero fees, independent rule verification, current full books, durable intents and ownership checks remain prerequisites. A favorable observed hedge price cannot justify a larger permission without evaluating the adverse permitted fill price.
