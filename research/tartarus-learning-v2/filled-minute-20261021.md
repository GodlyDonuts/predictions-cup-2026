# One filled order per minute: execution objective and current gap

**R85 installed at 05:19:45 UTC; original-ledger restoration is in progress.** Sole worker1069937, binary `5222f0f999fa`, checked execution source `7fb6fee064521c8e2f9f1bb2419bb7e6050b3efa`, updater1069940. The replacement fixes the proved-no-HTTP cohort failure that stopped R83 and includes R84 source/preflight report deferral. It preserves all535 fills, the full2,691,159,025-byte original financial prefix, all prior engine approvals and known order20904. **Fresh trading is not yet verified.** [Installation evidence](r85-local-guard-cohort-installed-20261022.json), [repair and validation](local-guard-cohort-r85-20261022.md).

**One distinct filled order/minute remains unmet.** The completed 04:35–04:50 UTC window had **one first-filled order in 15 minutes (0.0667/minute)** and 14 empty UTC minutes. SELL 1,472 YES993 shares realized **66.24 SUSQies net** against original FIFO. The preceding window had two first-filled orders and +50.115 realized net. Partial receipts do not create additional trades. [Latest completed measurement](r83-post-delaware-cadence-20261022.json), [official-receipt/FIFO audit](r83-post-delaware-turnover-20261022.json).

At the 04:50:25 audit, cash was **14,147.655**, remaining original cost **82,036.345**, total attributed realized **604.085**, including unchanged inherited 76.52. The separate official 04:39 receipts, observed 04:41:49, mark the account at **94,293.565 SUSQies**, down 5.706435% from 100,000. These are dated, non-atomic marks; realized gains do not establish whole-account profit or executable liquidation. [Official aggregates](r83-account-after-delaware-20261022.json).

R83 examined multiple candidates in a sweep, but median completed-scope spacing was 125.53 seconds. Fresh incoming books were absent or stale in the native view at two measured timestamps. Qualified R84 reduces repeated full-report construction during source/preflight reads. It was never deployed and is included in R85; the latter reproduces and repairs the newer no-HTTP cohort failure. No restored-trading or end-to-end speedup claim is made. [Source optimization](source-projection-r84-20261022.md), [cohort repair](local-guard-cohort-r85-20261022.md).

The dated observations below remain historical evidence.

## Historical observations at21:16UTC

Observed October 21, 21:16 UTC. The human now targets at least one exchange-confirmed filled order per minute, together with money growth. Count a physical order once at its first execution. Subsequent partial receipts, acknowledgments, cancellations and invested principal do not count as additional trades or profit. The target is not achieved.

## Actual execution and money

The 21:08:58 receipt/FIFO audit has 458 fills and 12 distinct filled orders today UTC. The most recent new filled order began at 20:54:44.744. Rolling first-filled-order counts are 0/1 minute, 0/5 minutes, 1/15 minutes and 1/60 minutes. The last two rates are 0.0667 and 0.0167 orders/minute. The auditor now reports these windows explicitly; three checks cover partial-fill deduplication, exact UTC boundaries and rejection of simulated/future fills.

That latest sell, native20317 / physical54473043 on993YES, subsequently reached 4,189 shares in six receipts. Proceeds are 3,204.585 SUSQies, original FIFO cost is 2,983.230 and realized net gain is **221.355**. Original452fills and remaining FIFO match exactly; all new receipts match their original exchange records. Reconciled cash is27,660.060 and remaining original owned cost68,209.850. These are not a fresh official whole-account valuation. The last official20:16:40 marked equity remains94,780.155, down5.219845% from100,000. Earlier R70 sales realizedâˆ’105.925. [Actual audit](r72-turnover-20261021.json).

## Changes now operating

The same R72 worker1031505 continues trading. Version3 repeat-acquisition policies committed for851/872 at21:01:48 and1051 at21:03:56. They preserve original entries, exits and FIFO and allow a sold cohort to be considered for a later purchase. They do not establish a new fill or remove the16-entry/128-history bounds. 1043 and1044 subsequently committed at21:13:34 and21:15:42. Seven refreshed cohorts are still being activated. [Commit replies and exact generations](recurring-activation-batches-20261021.json).

The reference updater initially failed after activation because its old review helper rejected version3 with FUNDAMENTAL_TOP_UP_POLICY. The existing offline CLI was rebuilt against the already deployed native/runtime libraries. All14 typed review hashes remain identical; only its configuration path and binary hash changed. At21:11:01 the updater committed a fresh12-market forecast generation while preserving all three version3 policies. No trading-worker restart or extra SIG client occurred. [Verified helper repair](recurring-helper-upgrade-20261021.json), [reproducible repair](tools/upgrade-recurring-review-helper-20261021.py).

The opt-in coordinator also now waits for the authenticated existing worker to poll before creating the request's60-second lifetime. The request retains original source clocks and expiry. A genuine new policy revision advances reviewedAtMs. Confirmed committed or confirmed expired-uncommitted outcomes are required before another request; unknown outcomes are not blindly resent. Three Linux local-channel tests cover peer identity, STOP and absence of request bytes before the owner boundary.

## Prepared speed change and remaining economics

R73 replaces two lines in existing_target_busy: reuse the existing immutable order snapshot and its indexed quote lookup instead of rebuilding the complete20,000-order history for each checked market. The isolated source matches SHA85fa14cd49ee;48 relevant worker tests pass and optimized ARM64 binarya50c79969b9b builds. Native/runtime libraries are unchanged. **It is not deployed and no full-worker speedup is established.** A replacement currently costs about20minutes of original-ledger replay, so production remains running while this qualified change is prepared. [Exact candidate](r73-target-cache-qualified-20261021.json), [finite stack samples](r72-stack-samples-20261021.json), [component timing](r72-report-profile-20261021.json).

Current market work commonly takes60â€“120seconds per scope. Source indicates further full-history rendering and synchronous diagnostic work; the five stack samples do not quantify each component's total cost. Removing a repeated scan alone does not establish one filled order/minute.

Entry economics also matter: the currently installed external-reference model reduces ordinary probability by an explicit exceptional-outcome allowance before subtracting its entry margin. For example, the21:09 snapshot of993YES used ordinary probability0.755, allowance0.10 and margin0.01, producing an acquisition ceiling0.6695 while the stored SIG ask was0.765.872YES used0.345, allowance0.30 and margin0.01, giving0.2315 versus ask0.355.1043NO instead showed ordinary probability0.895 and an acquisition ceiling0.87605 versus stored ask0.825. These are dated conditional inputs, not current execution promises. Research must distinguish unjustified conservatism from actual contract-resolution mismatch before changing those assumptions.

ChatGPT Deep Research is complete. Its findings, the new file-only opportunity census and the boundary-aligned continuous updater correction are recorded in [the follow-up report](turnaround-research-results-20261021.md). That follow-up supersedes the five-market activation count above: eight are now activated. The original dated financial evidence here remains unchanged.

Next priorities: finish the recurring-policy opt-in, verify fresh references survive it, measure real first-fill cadence and net results, and implement the smallest supported execution/strategy changes. Trading volume alone is not evidence of economic improvement.

## October21,21:46UTC update

All12 recurring policies are now confirmed by the original worker at21:42:35UTC; [completion evidence](recurring-complete-20261021.json). The21:45:49 original-receipt audit still has458partial-fill records and12distinct filled orders today, with0first-filled orders in the last15minutes and1in the last60minutes. Profit and remainingFIFO are unchanged. Continuous forecasts committed after the boundary fix. The next dispatch patch addresses a measured1.765â€“3.756seconds of duplicate post-intent validation; it is compiling and not yet deployed. [Research and repair details](turnaround-research-results-20261021.md).
