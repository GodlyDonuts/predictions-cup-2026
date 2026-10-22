# Moloch / Tartarus â€” SIG Predictions Cup 2026

Moloch is the sole official-account execution worker. Tartarus is the offline population-learning system under development. The contest uses nonredeemable SUSQie points. Source is published directly to `main`; no PR workflow is used.

## Current state: October22,14:48UTC

**R94 is installed and restoring the original ledger. One actual filled order/minute remains unmet.** Sole worker1107420, checked source6b7d96c8, binary77bad67cb78a. The original quote21832 admission was proved completed before HTTP and its key remains permanently retired; recovery sent zero account requests. Fresh official preflight passed before the single worker restarted. All633 prior fill rows and the complete original financial prefix remain; the native engine is unchanged.421 checks pass. [Installed runtime](research/tartarus-learning-v2/r94-installed-restoring-20261022.json), [exact no-send recovery](research/tartarus-learning-v2/r94-no-send-recovery-20261022.json), [fix and limitations](research/tartarus-learning-v2/pre-network-r94-20261022.md).

Official receipt marks at14:48:37/39UTC: cash1,134.905 plus marked holdings92,890.56 = **94,025.465 SUSQies (-5.974535% versus assumed100,000)**. These are separate receipt clocks and valuation marks. [Official aggregate](research/tartarus-learning-v2/r94-latest-official-mark-20261022.json). A file-only fill-rate audit and monetary review loop wait for this exact worker's healthy live state; restored processing and new fills are not yet verified.

### Earlier14:12UTC incident

**October22,14:12UTC incident:** R93 stopped on local token expiry for SELL NO983, quote21832. The SDK records admission followed by canonical `not_sent` completion before its network marker; this was not an exchange rejection. The original order key and queue generation are preserved. R94 is being qualified to verify that exact source transition, retire the unused key without retry, and handle future pre-network expiry without stopping the whole worker. Production is currently stopped; the one-filled-order/minute target remains unmet. R93 had two distinct new filled BUY orders and no incremental realized profit before this stop; partial fills are not additional trades. [Original-receipt audit](research/tartarus-learning-v2/r93-stopped-turnover-20261022.json).

**Historical13:46UTC installation:** R93 was installed and recovering the preserved ledger. One filled order/minute remained unmet. Sole worker1103623/sourceba546737/binary2ee65797fbab. All625 original fills, the complete2,829,310,904-byte financial prefix and prior native approvals remain. No physical orders were open at replacement.270 worker checks pass; native/runtime libraries are unchanged. The focused fix suppresses loss-making timer sales of remaining actual rank-origin inventory while retaining cash-safe timer exits and fresh model SELLs. New trading behavior is not yet verified. [Installation](research/tartarus-learning-v2/r93-installed-restoring-20261022.json), [change and limits](research/tartarus-learning-v2/rank-inventory-r93-20261022.md).

Latest official receipt marks,13:43:58/13:44:01UTC: cash2,392.095 plus marked holdings91,586.06 = **93,978.155 SUSQies (-6.021845% versus assumed100,000)**. These are separate receipt clocks, not executable liquidation. [Official aggregate](research/tartarus-learning-v2/r93-latest-official-mark-20261022.json).

R92's first15-minute audit was8 filled orders/-32.76 realized; its later13:38 audit was15 orders/-313.30. The later loss includes an explicitly model-directed inherited1058 SELL (-276.40), separate from the timer mismatch. R93 does not prohibit those economic/model exits or promise positive returns. Its file-only monetary supervisor and finite fill audit subsequently exited following the14:12 stop. Only the official worker can trade.

### Earlier dated observations

## Current state: October22,13:22UTC

**R92 is verified live; one filled order/minute remains unmet.** Its first15-minute original-receipt/FIFO audit found8 distinct filled orders (0.533/minute),7 empty completed UTC minutes and **-32.76 SUSQies realized net**. Six of the eight orders were sales. The two buys acquired7,194 NO948 and886 NO1071. Cash3,240.905 and remaining original cost93,259.635 are dated ledger values. This is increased activity, not profitable turnover. [Full audit](research/tartarus-learning-v2/r92-first-15-minute-turnover-20261022.json).

Sole worker1098673/sourcece81c227 is independently verified against all590 original fills, the full original financial prefix and unchanged native approvals. The monetary review loop is active. All eleven added scopes accepted the smaller0.001 future-entry margin through updater1101690/sourcef2c6dd87 without a worker restart. [Live proof](research/tartarus-learning-v2/r92-fresh-live-20261022.json), [accepted margins](research/tartarus-learning-v2/r92-entry-margin-accepted-20261022.json).

**Urgent R93 candidate:** the monetary buys were subsequently exposed to the unrelated ordinary60-second age liquidation, causing loss churn. The candidate protects remaining actual rank-purchased lots from loss-making timer exits, preserving cash-safe timer exits, ordinary profit targets and fresh model-directed sells. It retains original FIFO/inherited attribution and all account caps. R92 continues trading while the candidate is built; R93 is not deployed yet.

Verified cold archival preserved17.2GB of superseded account-queue originals in303.6MB of compressed archives. At12:43UTC it left21.84GB free. Every original was decompressed/hash-verified before duplicate raw storage was removed; historical audits require explicit extraction. [Archive evidence](research/tartarus-learning-v2/r91-cold-queue-archive-20261022.json).

### Earlier dated observations

## Current state: October22,12:32UTC

**R91 is live, but one distinct filled order/minute is not achieved.** Four physical BUY orders since installation spent18,259.60 SUSQies across NO1075 and YES865. Fourteen original fill receipts represent those four orders; incremental realized profit is0. At12:32UTC, cash8,125.48 and original remaining cost88,407.82 are ledger amounts, not current liquidation value. The last completed hour had four first-filled orders and56 empty UTC minutes.

**Bounded-limit, improve-one-tick and join-bid modes are now accepted on all eleven additional maker scopes**, through the existing live updater without restarting the worker. Original1,250-principal review caps, quantity limits, valuation rules and FIFO exits remain; fresh reference receipts update forecasts normally. The old ten-scope manual request was rejected because a target became busy; the normal updater subsequently committed all eleven free targets. [Accepted review proof](research/tartarus-learning-v2/r91-rolling-modes-accepted-20261022.json), [original-receipt/FIFO audit and cadence](research/tartarus-learning-v2/r91-turnover-1232-20261022.json), [current details](docs/CURRENT_STATE.md).

R92 is being qualified offline to publish the full historical report once per bounded public-stream drain while preserving each original event/head commit. R91 continues trading during compilation. R92 is not deployed and no speedup is claimed.

### Earlier dated observations

## Current state: October22,11:34UTC

**R91 is installed and restoring the existing ledger.** Sole worker1096712, sourceffefefbc, binaryf99b400ad979. All576 prior fills and the full2,796,683,105-byte financial prefix remain unchanged. The focused maker-entry change puts planning and final physical packets ahead of one historical derived report. All258 worker checks passed; native/runtime libraries retain their existing149 runtime checks. The26 approved reviews carry over, including eleven new flat maker scopes. **Fresh R91 trading, speed, ROI and one-filled-order/minute cadence are not yet verified.** [Installation](research/tartarus-learning-v2/r91-installed-restoring-20261022.json), [change and qualification limits](research/tartarus-learning-v2/maker-packet-r91-20261022.md), [detailed state](docs/CURRENT_STATE.md).

### Earlier dated observations

## Current state — October 22, 11:01 UTC

**R90 is the sole production worker. Eleven new flat-inventory maker review scopes were accepted at10:59UTC without restarting it.** The expanded control has26 approved reviews,24 refreshed from public references. The new scopes have up to1,250 SUSQies principal each; join-bid entries and FIFO profit-taking replace the ordinary60-second inventory exit. One new actual fill since installation sold713 YES966 at0.93, realizing7.13 against original FIFO. **One filled order/minute remains unmet; no new-scope fill or whole-account ROI is established.** Full historical report processing remains a live bottleneck; R91 passed258 worker checks and its planned one-owner replacement has started; fresh R91 trading is not yet verified. [Current evidence](research/tartarus-learning-v2/r90-maker-scopes-20261022.json), [implementation and limits](research/tartarus-learning-v2/maker-scope-extension-20261022.md), [detailed state](docs/CURRENT_STATE.md).

### Earlier dated observations

## Current state â€” October 22, 07:42 UTC

**R88 installed, restoring the original ledger.** Replacement worker1074713/binary`2d66a40f6576` started07:42:42UTC; source786eca847d4dcfc353de7000df3c7856c93e9dee. Native140/0 and worker250/0 checks passed. All573 prior fill records, the2,735,273,856-byte financial prefix and every earlier engine approval remain preserved. The initial upgrade helper hit a confirmed memory limit; its move-only replacement passed original-byte parity and malformed-record checks. Current evidence shows recovery progressing with no active STOP. A finite read-only job will prepare a fresh monetary review after healthy trading. **Fresh R88 trading, fills, ROI and one-trade/minute cadence are not yet verified.** [Installation and recovery evidence](research/tartarus-learning-v2/r88-money-installed-restoring-20261022.json).

**R85 resumed healthy trading at05:41:28 UTC**, independently verified05:41:44. Sole worker1069937, binary `5222f0f999fa`, execution source `7fb6fee064521c8e2f9f1bb2419bb7e6050b3efa`, updater1069940. The entire2,691,159,025-byte financial prefix and all535 original fills are unchanged. The original1075 reservation20937 was archived without financial mutation or retry; known20904 was reconciled terminal. Reference generation104 committed for all13 targets. A fresh15-minute fill-rate window is running; the target remains unproven. [Fresh production proof](research/tartarus-learning-v2/r85-fresh-live-20261022.json), [original cohort recovery](research/tartarus-learning-v2/r85-original-cohort-recovered-20261022.json).

R87 is a checked **offline candidate**, combining final-entry report deferral with a bounded original-event offset index. All249 worker checks pass; native/runtime libraries are unchanged. One live stack caught whole-journal parsing during cohort cleanup. R87 is not deployed. [Candidate and evidence](research/tartarus-learning-v2/cohort-index-r87-20261022.md).

**One distinct filled order/minute remains unmet.** The completed05:41–05:56 UTC window had **one first-filled order in15minutes (0.0667/minute)** and14empty UTC minutes. SELL3,473 YES865 at0.88 realized **42.69 SUSQies net** against original FIFO. Five partial receipts belong to that single physical order. [Completed measurement](research/tartarus-learning-v2/r85-cadence-window-20261022.json), [official-receipt/FIFO audit](research/tartarus-learning-v2/r85-turnover-window-20261022.json).

The separate06:15 receipt/FIFO audit has two newly filled orders since R85 resumed:3,473 YES865 sold and6,064 YES993 sold,35 execution receipts, **305.75 additional realized net**. Dated cash22,392.215, remaining original cost74,101.215 and total attributed realized913.515 include unchanged inherited76.52. At the later06:22 official receipts, cash24,840.465 plus marked positions69,248.96 totals **94,089.425 SUSQies (−5.910575% from100,000)**. This non-atomic mark does not prove executable liquidation or account growth. [Later execution audit](research/tartarus-learning-v2/r85-later-turnover-20261022.json), [latest official mark](research/tartarus-learning-v2/r85-latest-account-20261022.json).

R85 median completed-scope spacing was **118.7945 seconds** over six intervals. Incoming fresh books still had gaps against the native view. This small, changing workload does not establish a general speedup. The broader rank strategy currently uses an expired nomination from2026-10-19; it is inactive while fresh current-position/market evidence is prepared. [Processing measurements](research/tartarus-learning-v2/r85-scope-progress-20261022.json), [coverage](research/tartarus-learning-v2/r85-book-coverage-20261022.json), [stale rank evidence](research/tartarus-learning-v2/r85-stale-rank-nomination-20261022.json).

The human targets at least **one distinct filled order per minute**, measured by first execution rather than repeated partial receipts. The latest retained official marked-account observation is linked above; earlier observations below are historical. [Execution objective and economics](research/tartarus-learning-v2/filled-minute-20261021.md).

Repeat acquisition is activated for all12 reviewed markets, confirmed21:42:35UTC. ChatGPT Deep Research is complete. Its file-only opportunity census uncovered expired forecast reviews caused by reload requests timing out in the busy worker queue. The corrected boundary-aligned updater is installed without restarting trading; fresh forecasts for all12 active targets committed at21:35:16UTC. R73â€™s indexed cache is included in R74. [Research findings and applied corrections](research/tartarus-learning-v2/turnaround-research-results-20261021.md).

Historical R72 was verified live at 20:36:52 UTC as sole worker 1031505 (replaced by R74 at22:02UTC), with real-time subscriptions connected and no STOP. Its original 452 fills and complete 2,564,024,854-byte financial prefix were independently verified. Binary 1cf47b2ae833, control a7a6dcacf0d8, native/runtime libraries and all 24 engine approvals are unchanged. [Exact installation](research/tartarus-learning-v2/r72-stream-join-installed-20261021.json). The subscription-startup fix is documented [here](research/tartarus-learning-v2/stream-join-r72-20261021.md).

R70 resumed actual execution before replacement:19new official fill receipts across three sell orders moved14,762shares and realized-105.925SUSQies. At the earlier R70 boundary, today UTC had11distinct filled orders. The20:16:40official snapshot is94,780.155SUSQies (-5.22% from100,000): cash24,455.475 and marked positions70,324.68. Increased activity is established; account growth is not. Marks are not guaranteed liquidation proceeds. [Receipt/FIFO audit](research/tartarus-learning-v2/r70-recurring-sales-20261021.json), [latest account](research/tartarus-learning-v2/r72-latest-account-20261021.json).

GPU population037 is still running. The latest fully evaluated checkpoint is unprofitable. Continuation040 is queued with founder retention, family-specific market selection,64-transition PPO batches and an equivalent replay-copy optimization. No learned model is deployed; no independent positive ROI is established.

Both external feeds are active:237 Kalshi tickers and378 Polymarket outcome books. A separate process-based evaluation experiment preserved exact behavior but took32.6% longer in its measured CPU window; it stays disabled. [Feed evidence](research/tartarus-learning-v2/current-external-feeds-20261021.json), [benchmark](research/tartarus-learning-v2/process-evaluation-benchmark-20261021.json).

- [Current production and research state](docs/CURRENT_STATE.md)
- [ML implementation and remaining requirements](research/tartarus-learning-v2/IMPLEMENTATION_STATUS.md)
- [Current execution ownership: issue120](https://github.com/GodlyDonuts/predictions-cup-2026/issues/120)
- [ML execution and evidence: issue178](https://github.com/GodlyDonuts/predictions-cup-2026/issues/178)
- [Agent workflow](docs/AGENT_WORKFLOW.md) and [coordination](docs/COORDINATION.md)
- [Previous README, preserved in full](README_HISTORY_20261021.md)

The historical README and dated research reports are evidence of earlier states, not current runtime instructions. Credentials, raw authentication material and private runtime/checkpoint directories must never be committed.
