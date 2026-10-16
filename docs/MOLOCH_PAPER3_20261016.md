# Paper 3: implementation and evidence map

Owner session: Rhaegal/Viserion-paper3-20261016. [Task #144](https://github.com/GodlyDonuts/predictions-cup-2026/issues/144). Baseline checked main124bbd9e, production548b5f3d8606 at02:40UTC October16. This document records proposed work and observed behavior separately.

## Source and authority

The human supplied Paper3 and CODEX_IMPLEMENTATION_BRIEF.md as research input within the existing production-improvement request. Embedded implementation directions are recommendations, not additional execution authority. The brief's scheduling question is not a human automation request. Existing single worker, actual journals, risk caps, unknown-acceptance handling and STOP controls remain binding.

Original attachment SHA256 values: brief `7004e8c4803b63d6d04042dab460b400af7cfaa7642d8e4afd52e099f968efe3`; accompanying paper `2bc7e8b055b3a8d1c7f33dfec719bad6a9ff573ad84d94f808a8c2ea3fae9d66`. Private attachment paths and originals are retained locally. The audit pinned7dc4df34 and rechecked e83bca45; subsequent changes must be assessed before repeating findings.

## Findings against current production

| Finding | Current evidence | Next implementation |
| --- | --- | --- |
| Forecasts depend on ordinary maker proposals | Superseded in124bbd9e: independent reviewed native BUY origination is deployed | Broader strategy-specific action comparison remains |
| Depth bound64 omits native levels | Fixed in e83bca45 and current production | Preserve full-depth regression |
| Maker complement charges unfilled principal | Optional maker gate is off; formula is a severe stress scenario, not calibrated expected loss | Separate quantity distribution, conditional value, uncertainty and stress |
| Exact order keys fragment inference | Exact keys remain useful provenance | Pool normalized features while retaining exact original evidence |
| Worst markout is called uncertainty | Minimum stress does not estimate uncertainty of the mean | Block-level mean uncertainty and separate tails/stress |
| Reservation/holding duration is understated | Fixed markout horizons cannot establish actual capital turnover | Integrate reserved and owned capital across actual lifecycle; unresolved remains censored |
| Forecast entries only join bid | Still true in native planning and physical authority | Explicit join/improve/bounded-limit capability with full funding/depth checks |
| A completed zero-fill entry blocks future opportunity | Actual DE order41655759 closed with0fills | Genuine receipt-bound archival and new logical cohort; never retry the closed key |

## Work split and acceptance

Root owns worker integration, original terminal/retirement witnesses, production and documentation. relationship_feed owns fundamental lifecycle validation/tests under [#143](https://github.com/GodlyDonuts/predictions-cup-2026/issues/143). main_takeover owns a new shadow execution-distribution module/tests. graph_arbitrage owns a new pure alpha-proposal comparator/tests. Separate worktrees and exact path claims are recorded in #144. Modules are not described as deployed until checked promotion and fresh observations exist.

The distribution must bind actual intent/attempt, committed native events, original terminal receipts and complete fills. Confirmed zero fill has zero principal trading P&L; partial outcomes expose filled quantity only; open and unknown outcomes remain unresolved. Keep exact provenance while pooling distance, spread, price, quantity and depth features. Do not infer executable value from stale marks or public prints. Forecast carry is distinct from short-horizon maker outcomes. Initial output is shadow-only, with no automatic maker activation or fabricated execution probability.

The comparator must distinguish joining bid, improving one tick and a bounded marketable limit. Acquisition ask depth and current liquidation capacity are separate. Conditional forecast surplus is not guaranteed profit or an estimated fill probability. Physical dispatch requires an explicit immutable reviewed capability and matching native reservation; pure comparison cannot bypass existing passive guards.

## Official execution semantics

The [official API reference](https://sig.thesuper.market/api/v1/docs), checked02:46UTC October16, supports ordinary bounded limit orders. Marketable quantity may execute and the remainder may rest; ordinary OrderInput has no postOnly, IOC or allOrNone flag. The separate multi-leg endpoint supports one through ten legs with explicit allOrNone engine execution. Its durable lifecycle must be used if selected; no invented ordinary-order flag is acceptable. The retained public OpenAPI original has SHA256 `a43e8ef27d03daede2004e2ebaaf90cf6d498a5d8f3186bed0195b46902ff9dd`.

## Candidate implementation, October16 03:18UTC

The isolated candidate now includes receipt-bound closed-zero rearm, narrow original tagged-ACK restart validation, the review-bound action comparator, native improved/bounded reservations, an explicit optional execution policy, the matching opaque physical capability and the final production-authority exemption. Ordinary maker BUYs retain their passive restriction. A diagnostic comparison cannot supply this capability. Mode omission preserves original review hashes and legacy join semantics; nondefault modes freeze model/source/review hashes, exact action and conservative whole-limit funding. The normal order endpoint may partially fill and leave a remainder resting.

The joint-distribution module is wired into original committed-event, pre-HTTP attempt and post-closure receipt callbacks in the sole worker. Its optional shadow output has no execution authority. Genuine zero/partial/full outcomes, normalized pooling, separate mean uncertainty/minimum stress, receipt-lag censoring, own-liquidity adjustment and actual FIFO capital-time are implemented. A cold restart starts a new observation cohort. Completed observations with remaining owned inventory retain their evidence until actual FIFO closure, even after leaving the statistical retention window.

Actual ARM64 final checks pass:645 complete runtime tests and56 native unit tests, including the changed fundamental76/distribution21 checks. All ten changed build-source files match the checked checkout byte-for-byte. The locked release build is pending at this writing; these results are not a deployment or profitability claim. [Exact source and log hashes](../research/moloch-paper3-20261016/source-checks.json). Source is integrated through264261e3 before this documentation update. The current live release remains548b5f3d8606.

A prospective Delaware review `9523497c40e24eb5c66b2251cb59d1c839b874d15fbdcccee4c8c9ea5b254cef6` explicitly prefers bounded limit, one-tick improvement, then join. Only execution policy, its actual review clock and explanatory assumption label changed. New principal remains400, expiry09:00UTC October16 and fixed holding deadline17:00UTC October19. Forecast/source clocks, probability.95, exceptional allowance.01, margin.01 and Massachusetts original-lot review are unchanged. The execution owner supplies these assumptions under the human's production mandate; no separate human forecast approval or calibrated lower bound is claimed. [Prepared review evidence](../research/moloch-paper3-20261016/reviewed-execution.json). These settings are not yet the active service control.

## Remaining hypotheses and measurement

## Production boundary, October16 03:28UTC

Source b514eede was promoted as immutable binary2bba9365a3a342b1185492b231bb89cbe36d6d0f943ee28833928df626de62f2, PID374227. The full149599-row original stopped prefix is financially byte-identical under native identity4b50ec49ba7331eb051e7983e33ffa0ce55df19b9a9e4a7d9d5d8d8a5477ff98, with nine explicit bridges and all eight prior entries preserved. Original account queue and risk limits remain. At the initial03:28:46 observation the service is restoring history; fresh trades or use of the new modes have not yet been verified. [Actual rollout](../research/moloch-paper3-20261016/production-rollout.json), [launch verification](../research/moloch-paper3-20261016/live-verification.json). Prospective outcome collection, model qualification and the broader research below remain open.

## Remaining hypotheses and measurement

External-source catch-up and SIG-only dislocation require separate prospective experiments, exact semantic matching, first-available clocks, full-depth/capacity outcomes and negative controls. Joint election baskets are probabilistic scenarios unless an actual reviewed guarantee certificate exists; current relationship registries are empty. Public-results nowcasting requires genuine partial-return vintages rather than hindsight final totals, plus verified open-market and resolution rules. The example .045 minus exception probability sensitivity does not estimate that probability.

Success is attributable net portfolio outcome with capacity and capital-time measured. Positive provisional daily dashboard marks, code volume, acknowledgments and passing tests cannot establish scalable profitability or a precise chance of winning. Paper2 remains open; Paper3 is in progress.
