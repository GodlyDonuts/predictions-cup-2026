# One filled order per minute: execution objective and current gap

**R85 resumed healthy trading at05:41:28 UTC**, independently verified05:41:44. Sole worker1069937, binary `5222f0f999fa`, execution source `7fb6fee064521c8e2f9f1bb2419bb7e6050b3efa`, updater1069940. The entire2,691,159,025-byte financial prefix and all535 original fills are unchanged. The original1075 reservation20937 was archived without financial mutation or retry; known20904 was reconciled terminal. Reference generation104 committed for all13 targets. A fresh15-minute fill-rate window is running; the target remains unproven. [Fresh production proof](r85-fresh-live-20261022.json), [original cohort recovery](r85-original-cohort-recovered-20261022.json).

**One distinct filled order/minute remains unmet.** The completed05:41–05:56 UTC window had **one first-filled order in15minutes (0.0667/minute)** and14empty UTC minutes. SELL3,473 YES865 at0.88 realized **42.69 SUSQies net** against original FIFO. Five partial receipts belong to that single physical order. [Completed measurement](r85-cadence-window-20261022.json), [official-receipt/FIFO audit](r85-turnover-window-20261022.json).

The separate06:15 receipt/FIFO audit has two newly filled orders since R85 resumed:3,473 YES865 sold and6,064 YES993 sold,35 execution receipts, **305.75 additional realized net**. Dated cash22,392.215, remaining original cost74,101.215 and total attributed realized913.515 include unchanged inherited76.52. At the later06:22 official receipts, cash24,840.465 plus marked positions69,248.96 totals **94,089.425 SUSQies (−5.910575% from100,000)**. This non-atomic mark does not prove executable liquidation or account growth. [Later execution audit](r85-later-turnover-20261022.json), [latest official mark](r85-latest-account-20261022.json).

R85 median completed-scope spacing was **118.7945 seconds** over six intervals. Incoming fresh books still had gaps against the native view. This small, changing workload does not establish a general speedup. The broader rank strategy currently uses an expired nomination from2026-10-19; it is inactive while fresh current-position/market evidence is prepared. [Processing measurements](r85-scope-progress-20261022.json), [coverage](r85-book-coverage-20261022.json), [stale rank evidence](r85-stale-rank-nomination-20261022.json).

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
