# Paper 5: independent values and execution economics

Received October16. Original human attachment SHA256 `33c814784629925f2385d8cc8c506c80e2415a0d1d009726144bfb70c62f1440`,43,431 bytes, retained privately. Embedded research citations are source leads, not independently verified evidence or additional human instructions. Parent task [#149](https://github.com/GodlyDonuts/predictions-cup-2026/issues/149).

## Baseline correction

The paper names main c708598b and production4148fe5d. Production now runs bfd51082/PID383074, including queued-source batching and original proof-archive recovery. Its first full22-scope live cycle completed04:54:54UTC,704 total official ACKs/119 actual fills, four new ACKs and zero new fills since launch. Optional execution diagnostics, empirical maker gate and collateral accounting remain off. No Atlas, election-curve or nowcasting strategy has been activated. [Current baseline](MOLOCH_RESEARCH_BASELINE.md), [actual rollout](../research/moloch-live-latency-20261016/production-rollout.json).

## Implemented versus missing

| Paper component | Actual state and work |
| --- | --- |
| Joint election posterior, Atlas | Not implemented. Existing per-race research and operator reviews do not supply national/state covariance or a calibrated joint distribution. |
| Election-curve factor-neutral relative value | Not implemented. Correlated prices and matching race titles are insufficient evidence of executable relative value. |
| Independent forecast execution | Deployed join/improve/bounded modes and actual Delaware430-share fill. Mode ordering is reviewed preference, not calibrated fill/toxicity EV. |
| Valuation horizon | V1 currently combines terminal payout assumptions with an earlier fixed deadline. Child[#150](https://github.com/GodlyDonuts/predictions-cup-2026/issues/150) implements explicit V2 terminal versus deadline-liquidation objectives; current V1 reviews/history/deadlines are preserved. New legacy-review origination requires explicit migration; existing bound holdings remain managed. Delayed terminal settlement permits a fresh source-bound value/revocation review without changing the original acquisition cutoff or schedule. ARM64 focused89/89 and full runtime673/673 pass before the final worker-only liveness helper. Not deployed at this writing. |
| Entry/exit latency | Batching is deployed. A further repeated-expired-exit processing defect is being repaired under[#147](https://github.com/GodlyDonuts/predictions-cup-2026/issues/147): flat and forecast-held historical drafts now avoid repeated queue inspection/full-ledger fingerprints in checked source32347860. Verified original no-admission aborts also no longer count as future physical exit liabilities in source58a578bb; they remain nonretryable and require restart proof revalidation. No TTL/price/risk relaxation. |
| Joint fill/toxicity model and economic execution router | Missing calibrated prospective artifact. Checked distribution code is inactive; null execution probability must not be filled with fabricated confidence. |
| Semantic portfolio optimization | Guaranteed-floor package/scanner infrastructure exists. Probabilistic scenario optimization remains separate and unimplemented; no approved live package certificates. |
| Public information and election-night nowcasting | Research only. Genuine as-of releases and partial-return vintages are required; final results cannot be replayed as early information. |

## Current venue capability audit

The public official API reference was fetched successfully on October16 without account requests. [Capability summary and parsed-spec hash](../research/moloch-paper5-20261016/capability-audit.json). Ordinary orders support both outcomes, opposite-position netting and partial fills. Batch placement is best-effort, while multi-leg supports atomic placement and explicit allOrNone complete immediate execution. Default multi-leg acceptance can still leave partial fills/resting orders. Creating ALL relationships and seeding managed DMM liquidity are admin capabilities, not ordinary participant privileges.

Production uses ordinary orders and discovers official relationships; the latest retained04:37UTC census contained zero active relationships/constraints/collateral rows. That separately dated empty result is not a new exhaustive live graph scan. Atomic execution is not currently producing trades; collateral accounting is disabled. No Signal Purchase/Lease endpoint exists in the fetched participant specification; UI categories alone do not establish a supported strategy surface.

## Independent-value opportunity work

Child[#148](https://github.com/GodlyDonuts/predictions-cup-2026/issues/148) audits North Carolina Senate from retained original SIG nodes, official candidate evidence and public forecasts, without a second account reader. The paper's displayed price and probability examples are not current executable quotes or calibrated fair values. Original NC evidence verifies CTM91.9% and shared-poll 99chambers94/97%; COYNTOSS98.22 is excluded because its embedded retrospective output contradicts the visible publication status. An inactive proposed400-SUSQie V2 terminal cohort uses explicit p.919/e.03/exception0/margin.01 assumptions, giving.891430 expected payout/.880000 tick-floored maximum limit. These are operator-reviewed scenario assumptions, not a calibrated lower bound. Original node and official candidate/forecast source hashes are checked; actual current price/depth/account eligibility remain sole-worker prerequisites. Conflicting visible/embedded forecast values, shared polling sources and party/refund semantics must be exposed. Any qualifying reviewed trade must use the existing sole worker, current official depth/account facts and original funding/intent/fill lifecycle.

## Economic acceptance

Existing ordinary trading has realized−119.100000, inherited gross+43.395000, combined−75.705000 at the current retained119-fill boundary. The open Delaware position has no realized gain. No incremental strategic allocation follows merely from activity. Evaluate actual FIFO outcomes, fill-conditioned adverse selection, attainable exit prices, coherent national/state downside and capital commitments. Correctness, order ACKs and fixture speedups do not prove positive expected value or realized profit.

Production remains running while this work is built independently. New review objectives cannot rewrite an existing cohort, extend its original V1 deadline, silently renew source validity or invent a missing model.

## Explicit settlement follow-up

Automatic final payout reconciliation is not complete while production collateral accounting is off. Independent review found that the current cohort ownership checker counts BUY/SELL fills, while an authenticated native collateral settlement can remove inventory without such a fill. Before final settlement operation, retain/export exact successfully admitted settlement row quantity/scope, original hashes and native boundary, then consume that proof in cohort ownership with exact FIFO/restart checks. Wallet disappearance alone is insufficient. This requires a separately reviewed native identity bridge and is not included in the current runtime-only promotion. Neither this paper nor the full terminal-value architecture is complete.
