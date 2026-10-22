# R99: transfer strategy ownership without a needless sale

R97 remains the live worker at publication time. Its first completed 15-minute audit has four filled orders,2,452.635 SUSQies turnover and15.44 incremental realized profit. One filled order/minute is still unmet. See `r97-first-15-minute-turnover-20261022.json`.

The later17:25 audit supersedes the initial positive snapshot: six distinct orders,9,231.685 turnover and **-582.47 incremental realized net**; one filled order in the last15minutes. Original fills/FIFO still match. See `r97-turnover-1725-20261022.json`. The17:23 inherited YES885 sale realized-597.91 on10,000 shares; this remains an actual loss.

## Concrete economic issue

The old cohort-to-rank interface allowed a first covered SELL or a first BUY on an empty position. A first same-side BUY on an existing position was rejected, even when the same Moloch owner controlled the original cohort. The research compiler therefore required a preceding sale. On identical frozen inputs, the constrained optimizer sold731 YES1010 at0.685 and bought back more at0.700. Its weakest declared conditional gain was33.148501. A comparison removing only that ownership prerequisite found44.254701 with direct purchases. These are uncalibrated model comparisons, not executable profit, a backtest, or current authority.

R99 adds an explicit owner-produced same-side BUY handoff. The original cohort statement carries `sameSideBuyHandoffAllowed` and `sameSideBuyHandoffSide`, bound to its original cohort, generation, review, positions and source hash. Native and runtime checks require the stated side to match held inventory and reject opposite inventory. The held cohort writer independently compares the statement with its actual state before registration.

The handoff retains the original cohort body, FIFO positions and exact original fill prefix. Registering it creates neither a sale nor cash nor realized P&L. Subsequent real purchases append new lots. The original fundamental strategy pauses after the exact rank binding; it cannot compete for the transferred inventory. Historical nominations without the new fields retain their earlier semantics. The Python compiler removes the prior-sale requirement only for an original owner statement with a valid same-side position.

## Conditional monetary rotation

The distinct `declared_positive_expected_money_rotation` objective permits only a first owned SELL prerequisite. Every declared model must put the full conditional rotation above its explicit positive money floor. The first SELL may have negative expected value; that remains visible in the evaluation. Original FIFO, shared depth, actual-cash prefixes, market caps, partial fills and failure-to-replace branches remain checked. Future proceeds are never credited to the live wallet.

`prepare-current-money-rotation.py` recomputes the full plan and ordinary interval audit from matched originals. A frozen historical engineering run produced a first-leg expectation of−32.14 to−36.07 and full conditional gain33.15. That old engineering nomination is expired, unapproved and never submitted. Its purpose is schema/audit verification, not a fresh trade recommendation.

The publisher requires explicit rotation selection and a qualification proof for the exact actual worker binary. It rejects both an unselected rotation and an unsupported R97 binary before owner-file mutation. The ordinary autonomous supervisor remains direct-money-only: after a separately selected funding sale completes, residual opportunities are analyzed afresh. Automated repeated negative funding sales are not enabled by this release. Further live rotation selection and actual residual fills remain to be established.

## Execution and verification

The worker now reuses the existing exact-head order-render cache for duplicate monetary-review scans. A changed Session head invalidates the cache; no physical permission is reused. No end-to-end speedup is claimed yet.

Qualification:143 native tests passed (3 ignored),6 rank-source/approval tests,149 fundamental integration tests,7 ownership integration tests and270 worker tests. Seven Python portfolio tests also pass. The new ownership integration actually buys into a nonempty cohort, preserves all old lots and realized P&L, and verifies restart recovery. Exact compiled changed-source hashes and library/binary SHAs are in `r99-owner-handoff-qualified-20261022.json`.

Original build diagnostics are retained. A long temporary path caused14 Unix-socket fixture failures; the same binary passed149 tests with a shorter path. A postprocessor mistook the ending of `270 passed` for zero tests; the corrected numeric parser retained the genuine successful result. A predecessor standalone test link required thin LTO; the combined rank suite compiled and passed. Reused native/runtime artifacts are pinned to unchanged source, command and binary hashes; the changed worker was rebuilt.

The upgrade helper links the same native library and preserves existing snapshot/chain validation. A small fixture proves original/bounded proposer byte parity and rejects a malformed prior report. Real original-bank prefix parity and fresh R99 production processing still require installation and recovery evidence. No account client is created by qualification or the helper.

## Remaining cadence gap

The separate 15-sample R97 coverage audit saw a median146 raw fresh books but zero native books within the same strict10-second window; the maximum native fresh count was5. Raw book availability alone does not establish complete tape, eligibility or a missed profitable trade. This is a concrete remaining discovery/source-admission bottleneck, separate from the ownership fix. See `r97-book-coverage-20261022.json`.

R99 changes no funded cap, account limiter, activation window, original cost basis or unknown-acceptance rule. Deployment uses one serialized worker and preserves the complete old approval chain and financial prefix. Successful installation, actual fills, one/minute cadence and winning ROI require separate fresh evidence.
