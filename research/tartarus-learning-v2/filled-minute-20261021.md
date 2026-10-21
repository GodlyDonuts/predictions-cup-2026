# One filled order per minute: execution objective and current gap

R76 is verified live as sole worker 1041499. At 23:05:28 UTC its executable, original 481 fills and complete 2,601,300,909-byte financial prefix matched the installation evidence. Original no-network repeat entry 20449 was recovered without resubmission. [Live verification](r76-fresh-live-20261021.json), [recovery](r76-topup-recovery-20261021.json).

At 23:12:27 UTC, the original-receipt audit counted **one distinct first-filled order in 15 minutes**, four in 60 minutes and 16 today. **The one-trade-per-minute target remains unmet.** R76 order 20545/54709337 bought six 1043 NO shares for 4.86 SUSQies at 23:08:50.266. The preceding 385-share 966 buy was an R74 resting order filled during restoration, not a new R76 decision. Cash is 10,479.03 and remaining original position cost is 85,390.88; neither is current equity. Attributed realized net remains 289.995, including the unchanged 76.52 inherited portion. [Original receipts, FIFO and cadence](r76-turnover-20261021.json).

R76's 14-market exit-hint pass measured 505 ms at 23:11:39; completed scope work still takes roughly 60–120 seconds. Another order acknowledgment is not counted as a fill. Focused ChatGPT research is complete, and the finite file-only exit census is running. An isolated recorder optimization remains undeployed. [Execution evidence and limits](hint-batch-r76-20261021.md), [turnover economics](turnover-economics-20261021.md).

The dated observations below remain historical evidence.

## Historical observations at21:16UTC

Observed October 21, 21:16 UTC. The human now targets at least one exchange-confirmed filled order per minute, together with money growth. Count a physical order once at its first execution. Subsequent partial receipts, acknowledgments, cancellations and invested principal do not count as additional trades or profit. The target is not achieved.

## Actual execution and money

The 21:08:58 receipt/FIFO audit has 458 fills and 12 distinct filled orders today UTC. The most recent new filled order began at 20:54:44.744. Rolling first-filled-order counts are 0/1 minute, 0/5 minutes, 1/15 minutes and 1/60 minutes. The last two rates are 0.0667 and 0.0167 orders/minute. The auditor now reports these windows explicitly; three checks cover partial-fill deduplication, exact UTC boundaries and rejection of simulated/future fills.

That latest sell, native20317 / physical54473043 on993YES, subsequently reached 4,189 shares in six receipts. Proceeds are 3,204.585 SUSQies, original FIFO cost is 2,983.230 and realized net gain is **221.355**. Original452fills and remaining FIFO match exactly; all new receipts match their original exchange records. Reconciled cash is27,660.060 and remaining original owned cost68,209.850. These are not a fresh official whole-account valuation. The last official20:16:40 marked equity remains94,780.155, down5.219845% from100,000. Earlier R70 sales realized−105.925. [Actual audit](r72-turnover-20261021.json).

## Changes now operating

The same R72 worker1031505 continues trading. Version3 repeat-acquisition policies committed for851/872 at21:01:48 and1051 at21:03:56. They preserve original entries, exits and FIFO and allow a sold cohort to be considered for a later purchase. They do not establish a new fill or remove the16-entry/128-history bounds. 1043 and1044 subsequently committed at21:13:34 and21:15:42. Seven refreshed cohorts are still being activated. [Commit replies and exact generations](recurring-activation-batches-20261021.json).

The reference updater initially failed after activation because its old review helper rejected version3 with FUNDAMENTAL_TOP_UP_POLICY. The existing offline CLI was rebuilt against the already deployed native/runtime libraries. All14 typed review hashes remain identical; only its configuration path and binary hash changed. At21:11:01 the updater committed a fresh12-market forecast generation while preserving all three version3 policies. No trading-worker restart or extra SIG client occurred. [Verified helper repair](recurring-helper-upgrade-20261021.json), [reproducible repair](tools/upgrade-recurring-review-helper-20261021.py).

The opt-in coordinator also now waits for the authenticated existing worker to poll before creating the request's60-second lifetime. The request retains original source clocks and expiry. A genuine new policy revision advances reviewedAtMs. Confirmed committed or confirmed expired-uncommitted outcomes are required before another request; unknown outcomes are not blindly resent. Three Linux local-channel tests cover peer identity, STOP and absence of request bytes before the owner boundary.

## Prepared speed change and remaining economics

R73 replaces two lines in existing_target_busy: reuse the existing immutable order snapshot and its indexed quote lookup instead of rebuilding the complete20,000-order history for each checked market. The isolated source matches SHA85fa14cd49ee;48 relevant worker tests pass and optimized ARM64 binarya50c79969b9b builds. Native/runtime libraries are unchanged. **It is not deployed and no full-worker speedup is established.** A replacement currently costs about20minutes of original-ledger replay, so production remains running while this qualified change is prepared. [Exact candidate](r73-target-cache-qualified-20261021.json), [finite stack samples](r72-stack-samples-20261021.json), [component timing](r72-report-profile-20261021.json).

Current market work commonly takes60–120seconds per scope. Source indicates further full-history rendering and synchronous diagnostic work; the five stack samples do not quantify each component's total cost. Removing a repeated scan alone does not establish one filled order/minute.

Entry economics also matter: the currently installed external-reference model reduces ordinary probability by an explicit exceptional-outcome allowance before subtracting its entry margin. For example, the21:09 snapshot of993YES used ordinary probability0.755, allowance0.10 and margin0.01, producing an acquisition ceiling0.6695 while the stored SIG ask was0.765.872YES used0.345, allowance0.30 and margin0.01, giving0.2315 versus ask0.355.1043NO instead showed ordinary probability0.895 and an acquisition ceiling0.87605 versus stored ask0.825. These are dated conditional inputs, not current execution promises. Research must distinguish unjustified conservatism from actual contract-resolution mismatch before changing those assumptions.

ChatGPT Deep Research is complete. Its findings, the new file-only opportunity census and the boundary-aligned continuous updater correction are recorded in [the follow-up report](turnaround-research-results-20261021.md). That follow-up supersedes the five-market activation count above: eight are now activated. The original dated financial evidence here remains unchanged.

Next priorities: finish the recurring-policy opt-in, verify fresh references survive it, measure real first-fill cadence and net results, and implement the smallest supported execution/strategy changes. Trading volume alone is not evidence of economic improvement.

## October21,21:46UTC update

All12 recurring policies are now confirmed by the original worker at21:42:35UTC; [completion evidence](recurring-complete-20261021.json). The21:45:49 original-receipt audit still has458partial-fill records and12distinct filled orders today, with0first-filled orders in the last15minutes and1in the last60minutes. Profit and remainingFIFO are unchanged. Continuous forecasts committed after the boundary fix. The next dispatch patch addresses a measured1.765–3.756seconds of duplicate post-intent validation; it is compiling and not yet deployed. [Research and repair details](turnaround-research-results-20261021.md).
