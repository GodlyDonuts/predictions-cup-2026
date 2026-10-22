# Tartarus implementation status — October 22, 02:31 UTC

R81 was installed at **02:31:01 UTC** as sole worker **1061072**, updater1061075, and is **restoring the original ledger; fresh trading is not yet verified**. Checked source `efb48634eff9bdd3bcd73630376d822cb418f263`, binary `fe8280811298`. All500 receipts, the full2,654,477,535-byte financial prefix and native engine were preserved. Known outstanding order20696 remains attributed and must reconcile. [Installation](r81-deployment-20261022.json).

**The one-distinct-filled-order-per-minute target remains unmet.** R80 has three original-receipt-confirmed sells:470YES1044 at0.85 (+7.05),10,000NO1051 at0.87 (+86.275), and1,217YES1067 at0.305 (−24.34). Combined realized change **+68.985SUSQies**. The last sell first executed02:16:25.832; its three partial receipts count as one order. Total500 receipts. The02:24 audit has two new first-filled orders in15minutes. [Receipt audit](r80-third-sell-20261022.json).

R80 excludes obsolete managed maximum-age nominations. Fresh sell reservations now reach later preparation but expire before physical dispatch. R81 eliminates repeated full-history audits of the same immutable SELL state, while keeping current book, original FIFO, request, review and unknown-acceptance checks. All396 Rust and8 updater checks pass. R81 is installed and restoring; execution-speed gains remain unmeasured. [Current expiration evidence](r80-preintent-expiry-20261022.json), [R80 change](managed-age-r80-20261022.md).

At02:05:32 UTC, bounded online archival retained16.196GB of cold SDK history losslessly in287.5MB of verified archives, leaving17.018GB available. Each original was decoded and rehashed before its raw duplicate was removed. The active worker was **not paused or restarted**; current/open generations and financial history were untouched. Seven real archival checks passed. [Archive evidence](r80-cold-archive-20261022.json). Earlier cleanup removed only unused reproducible test binaries. [Cleanup inventory](r80-disposable-tests-cleanup-20261022.json).

Reconciled cash is **9,761.315**, original remaining cost **86,177.58**, attributed realized **358.98** including inherited76.52. These are not equity. The last retained separate official account receipts at01:25:28–30 mark **94,232.46 SUSQies, −5.76754%** from100,000; they are not an atomic or executable liquidation snapshot. [Official marks](r79-latest-account-20261022.json).

## Running and queued work

GPU population037 remains active (process18116 verified at20:20UTC). A newly completed second member in island001 produced-10.56343% nominal marked ROI and+0.0030525% stressed marked ROI on the two-day development window. Its minimum daily-growth reward is-0.13955066; it is not a profitable usable checkpoint. The two stochastic cases took145/102decisions and produced15/2fills, with no realized FIFO gain. All new fills in those cases were BUY_NO, so effective exits remain a weakness. [Original-result digest and metrics](population-037-second-member-result-20261021.json).

Continuation040 is queued on helper2288 for the original trainer18116. It replaces039 before any039 launch. Each island's latest completed generation, learned hyperparameters and all original checkpoints are preserved before migration. New040 training has not started. [Exact queue/source pins](population-040-queued-20261021.json).

The queued source includes:

- Founder evaluation/retention, so a worse first trained generation cannot erase its stronger starting policy.
- Family-conditioned target attention: BUY and SELL can learn different market preferences. The3072-parameter zero-initialized extension preserves old behavior at installation and imposes no action/quantity restriction.
- 64-transition compute chunks and equivalent source-validation improvements.
- Ownership transfer of fresh built-in historical responses, avoiding redundant deep copies. Generic/custom providers retain defensive snapshots.39 focused checks pass; four matched fitting-hour runs have identical complete observation/action/request/receipt/financial hashes and1.040x measured collection speedup. [Benchmark](owned-history-replay-benchmark-20261021.json).

[Target-selection implementation and model audit](family-target-query-20261021.md), [founder retention](founder-retention-20261021.md).

Optional process-based evolution evaluation is checked on sourcee0d83619 but disabled.22 unique focused checks cover behavior, clocks, finance and isolation. In four interleaved CPU passes over a five-minute development slice, all observations/actions/complete results matched, but child processes took154.12s median versus116.24s locally (32.6% slower). This measured result does not justify changing active037 or queued040. No optimization or independent test was run. [Benchmark and exact limits](process-evaluation-benchmark-20261021.json).

## Results and limits

The2,768-update checkpoint completed the full October17–19 development window at-4.3489075% nominal/-4.93724% stressed marked ROI. Its minimum daily-growth reward remains worse than the matched founder. The first island's final2,773-update checkpoint is a different checkpoint and its final evaluation is pending. [Later-checkpoint result](population-037-later-result-20261021.json).

The separate fitting-hour behavior audit found large orders, long uninterruptible waits, and SELL choices against markets with zero delivered holdings. API-defined complement conversion turned those SELL attempts into BUYs; it was not learning effective exits. This motivated family-dependent target selection rather than restricting legal choices. [Behavior evidence](population-037-fit-behavior-20261021.json).

Three venues provide information; execution is SIG-only. Requests, receipts, virtual clocks, modeled inference latency and rate budgets are explicit. The model can select all implemented raw API action families, quantities, prices, builder operations and waits. Software checks do not establish full API parity, calibrated real execution or profitable trading skill.

Current detailed development is2 fitting days plus2 development days. Full competition execution history and the requested4+4 evaluation chronology remain incomplete. ProtectedOctober21–25 has not been consumed. No learned model is deployed or independently profitable.

Production is separate from ML. The latest official marked equity remains the 20:16:40 observation of 94,780.155 SUSQies, down 5.22% from the start. R70's last three filled sell orders realized a net loss of 105.925; no whole-account profit is established. [Current production state](../../docs/CURRENT_STATE.md).
