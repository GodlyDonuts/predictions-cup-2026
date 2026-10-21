# Current-value execution repair — October 21, 2026

## Objective and measured baseline

The human's operational target is **97,000 marked SUSQies by 10:58:40 UTC on October 21**, measured from the 08:58:40 task start. This is a target, not a promised return. The existing official worker's 08:53:37 positions receipt reports 94,389.64 in marked positions; its account receipt reports 403.36 cash: **94,793.00 total**, leaving 2,207.00 to the target. These are contest marks, not executable liquidation proceeds. Almost all capital is already invested.

## Verified bottleneck

The installed typed review inspector validated the original pinned settings and found **zero current reviews out of fourteen**. Their forecast expirations are October 18–19. Meanwhile the existing laptop collectors are live: 237 Kalshi markets and 378 Polymarket outcomes. Fresh collection was not automatically refreshing the production review inputs. Raising feed throughput alone cannot fix that gap.

Worker R59, PID989530, is verified back in its trading loop at 09:38:37 UTC with healthy realtime. The first twelve-market generation `862b7c358f067c77fb7ee49276f0cb8893927434af92ba9cafeba9d7b5bbc1663` committed through its owner socket. Its already-published change removes a redundant 2,500 ms post-read exit delay. Counts remain396 fills/1660 acknowledged orders at the initial live check. The first legacy1042 attempt still expired during preparation/activation before submission; new orders and profit are not established by the reload.

## Implemented prospective changes

- Capture twelve existing V2 review scopes from the existing Kalshi cache. Require original body hashes, matching parsed bodies, exact retained SIG contract roots, two-sided books, quotes within 30 seconds and market metadata within one day.
- Use the conditional Kalshi party midpoint as an explicitly **uncalibrated** model input. Preserve source bytes and receipt times. Six of the initial twelve scopes also retain current exact-party Polymarket observations as context; those observations do not silently become model inputs or settlement-equivalence claims.
- Give the fresh forecasts a 15-minute lifetime. Retain original ballot and contract identity bytes and clocks with a declared 14-day identity lifetime; the existing worker still checks the exact current SIG contract before dispatch. This changes the identity-document retention policy, not the actual SIG book/account freshness limits.
- Replace FIFO-only exit pricing on **1067 YES (Alaska Senate)** and **872 YES (Nevada Governor)** with current modeled value. This can realize a loss when freeing capital is preferable to keeping an overpriced position. Original costs and realized losses remain in the ledger. Other existing FIFO profit-turnover policies remain.
- Retain legacy reviews **1042 and 1068** unchanged: their original V1 cohort deadlines cannot be silently reinterpreted by this V2 refresh. Their legacy exit behavior remains a limitation.
- Serve public reference captures on laptop localhost only. An authenticated SSH reverse tunnel exposes that endpoint only on VPS localhost. It carries no SIG key or account credentials.
- The VPS refresh process stages immutable reviews, then uses the existing worker's authenticated owner socket. Initial refresh covers all twelve scopes. Subsequent passes inspect fresh references every30seconds and update a review only when its executable SIG price ticks change or its forecast has less than two minutes left. Known open targets are deferred; the worker independently checks them again. This avoids spending account reads and invalidating pending plans merely to change source timestamps. The process validates the exact worker, original base control and generation before every request, and halts on an unresolved reload result or another generation writer.

The existing worker performs any account reads and actual trading through its original serialized queue. The refresh process creates no SIG client, clears no STOP, renews no execution activation and changes no account budget or position ledger. A financial-control comparison and the worker's own reload validation prevent unrelated control changes.

## Initial source comparison

These are the 09:19 UTC capture's conditional model points, not execution prices:

| Exchange / side | Previous point | Fresh point |
|---|---:|---:|
| 966 YES | 0.920000 | 0.961500 |
| 977 NO | 0.995000 | 0.983500 |
| 1067 YES | 0.503000 | 0.255000 |
| 865 YES | 0.954000 | 0.942000 |
| 872 YES | 0.782000 | 0.425000 |
| 993 YES | 0.840000 | 0.765000 |
| 885 YES | 0.862000 | 0.725000 |
| 1051 NO | 0.996000 | 0.958500 |
| 851 NO | 0.989000 | 0.965000 |
| 1043 NO | 0.933000 | 0.903500 |
| 1044 YES | 0.933000 | 0.907000 |
| 1077 NO | 0.999000 | 0.983500 |

The pre-existing exceptional allowances and entry margins still apply to these points. Kalshi/Polymarket and SIG have different resolution conditions; this is conditional value trading, not guaranteed cross-exchange arbitrage. A lower modeled holding value does not itself create a profitable sale; current executable SIG depth determines whether the worker can act.

## Verification and limits

### Execution follow-up at 10:04 UTC

The 09:54:35 positions receipt and 09:54:39 account receipt report 94,378.96 marked holdings plus 403.36 cash: **94,782.32 total**. This is an asynchronous official mark, not executable liquidation value. At 09:59:42 the sole worker remained healthy with 396 fills and 1,660 acknowledged/terminal orders; refreshing valuations has not yet produced a new trade.

The next worker repair addresses three concrete execution problems. An untouched, non-passive reviewed SELL can now reuse its exact reservation after collecting seven fresh facts; the previous retry path handled passive orders only. If creating a reservation consumes the original packet's time window, the same call refreshes that reservation once without extending its expiry or changing its price/quantity. Ordinary entry, exit and defensive-cancel preflight packets now use the existing one-public-frame servicing budget, while ordinary stream drains remain at 64 frames. Finally, dispatch refreshes reconciliation when optional work has exhausted its ten-second permission window, and records the resulting admission decision. Current ownership, depth, pending-order and physical SDK checks still apply.

The ARM64 worker typecheck and focused regressions pass, including a genuine non-passive reservation, seven-fact refresh, identical qid/quantity/price/FIFO, and refusal after an intent exists. **R60 is installed at 10:07:01 UTC as sole PID994490 and is restoring original history; fresh trading or execution is not yet verified.** Its checked binary is `2bc2e8a3a49402f0bc2e99cb1528ba2b6715ec8749cd5e0583a26f998bc41bda`. The [installation proof](production-exit-refresh-installed-20261021.json) preserves all 396 fills, FIFO, the entire 2,492,994,478-byte financial prefix and twenty engine bridges. The reference updater994506 waits for the current worker and keeps the exact resolved generation, with only its binary identity rebased. The 10:07:01/03 official receipts show 94,392.17 marked positions plus 403.36 cash, totaling **94,795.53**. No account history or checkpoint has been removed. The separate matched CPU comparison of the completed 656-update learner is retained in [the exact comparison record](population-034-completed-656-comparison-20261021.json).

The standalone ARM64 typed helper builds against the deployed runtime library. Original settings load with fourteen valid historical review/source bindings and zero current reviews. A fresh twelve-review generation stages and validates with twelve current reviews. A separate two-market capture traverses the real localhost SSH tunnel and passes the same validation. Deliberately changing the midpoint away from its original book is rejected. Future-clock/stale quotes are rejected. Staging preserves every base financial-control field and sends zero reloads or official account requests.

Initial production activation is verified above; original subsequent fill evidence remains required. The first refresh implementation rotated timestamp-only changes and imposed avoidable runtime work. The adaptive follow-up passed three additional offline cases covering unchanged/exchanged executable ticks, NO complements, modeled exit ticks, expiry and open-target deferral; it does not require restarting the trading worker. Existing GPU training and retained checkpoints continue; live production reference observations are excluded from ML fitting and selection. The matched CPU evaluation of the completed656-update member finished with −6.687015% nominal and−6.540975% stressed marked ROI; worst human reward−0.086047. This regressed versus the84-update checkpoint's worst reward−0.041089, while still improving on the parent's−0.215753. More updates did not produce a profitable model. Neither development run used the independent window.

### Execution and learning observation at 10:40 UTC

R60 remains sole PID994490 and is still restoring its original 2.49GB journal; the old progress file belongs to PID989530 and is not evidence of current trading. At10:39 it had replayed2.14GB. No new fill has been verified. The latest retained official account mark remains94,795.53 at10:07, not a fresh liquidation value. The97,000 target deadline remains10:58:40UTC.

CPU profiling identified repeated active-order scans through a20,033-record retired prefix while only3 of20,040 native orders remain unretired. A derived-prefix optimization preserves every record and passes125 ARM64 checks; its full original-journal financial-parity replay and release build are still running offline. It has not been deployed, and no live speed gain is claimed.

The [fit-only opportunity census](fit-opportunity-census-20261021.json) examined2,542,717 SIG book records, of which2,542,103 were valid within the declared clock/depth rules. It found327 exchange-sides across190 contracts with a positive hindsight one-share entry/exit spread over3 seconds to1 hour. These are overlapping perfect-hindsight opportunities, not a causal portfolio or achievable ROI. No development/independent data, GPU, account client or execution was used. This establishes available price movement, not model skill. Population034's completed members remain loss-making or idle; member3 continues and all checkpoints are retained.

### Corrective installation at 10:57 UTC

R60 did not return to trading. It exited10:50:05 after restoring history because its startup control used old fundamental settings rather than the exact already committed live overlay (`RELOAD_REBASE_NONBINARY_CONTROL_CHANGED`). This deployment error is corrected by retaining the exact active overlay and changing only its executable identity.

Checked source25420da8 and binary153a4347eac54946053601d3e28d5e253b05392cb5aeb767933936107a7b7766 are installed as sole worker996893. The native retired-prefix optimization passes125 checks and full original743,614-event replay with the same sealed financial report,20,040 orders and396 fills. All20 prior native bridges are preserved, with one explicit21st bridge. The complete original2.49GB journal, FIFO, physical account queue, financial caps, windows and rate budget remain. The original active generation is preserved; reference updater996896 waits for this exact worker. [Installation proof](production-active-overlay-prefix-installed-20261021.json).

At10:53:13/15 the official receipts report94,562.96 marked positions plus403.36cash, totaling**94,966.32**:173.32 above the08:53 baseline and2,033.68 below the97,000 target. This mark change is not a new realized trading profit. Counts remain396fills/1660acknowledged and terminal orders. The worker is restoring and no new physical trade is verified. The deadline remains10:58:40UTC.

Population034 continues; the [checkpoint-preserving cooperative handoff](cooperative-continuation-20261021.md) waits for its complete generation. No independent profitable learned model is deployed.

### Fixed target deadline: 10:58:40 UTC

The97,000 target was not achieved in verified evidence by the two-hour deadline. The sole new worker996893 remains alive, no STOP is present, and it is restoring. Its10:57:10/12 original account receipts show94,562.32 marked positions and403.36cash, totaling**94,965.68**. That is172.68 above the94,793.00 baseline and2,034.32 below the target. No new physical order or fill is verified. Restoration, tests and mark changes are not execution success. The missed target and failed R60 handoff remain part of this report; the deadline is not rolled forward.

### Live loop and next bottleneck at 11:23 UTC

R61/996893 is now verified trading with healthy realtime and allowed dispatch. The reference updater has committed generation23 through this exact worker; subsequent unchanged price ticks correctly avoid reload churn. Original receipts at11:22:50/51 show94,667.48 marked positions plus403.36cash, totaling**95,070.84**. This is277.84 above the08:53 mark baseline, not newly realized trading profit. Counts remain396fills/1660acknowledged and terminalorders. [Whitelisted current-process evidence](production-r61-live-20261021.json).

An actual bounded stack sample found `FilePin::validate` hashing the full27MB dispatch journal inside `Cohorts::check -> entry_cancellations`. Ordinary repeated cache hits are being changed to validate held inode, size and nanosecond mtime/ctime at the same dispatch head. Cache misses or changed heads retain full original chain verification; physical preparation retains full-byte pins. This targets time spent validating unchanged history, not account freshness, ownership or uncertain execution. The candidate is not yet deployed. The separate native shared-review change passed full original financial replay but also remains undeployed; production stays running during preparation.

### Runtime-only replacement at11:37UTC

The dispatch-history cache fix is installed as sole R62 PID998119 with binary204a4746dff3a2157eab6c8a76dcf0c74a8498025abf2f1e544ecee788ff69a8. It changes only runtime cache reuse and retains the exact R61 native engine and production SDK. The existing21 bridges,396fills/FIFO, quiet original account queue, financial controls and exact active reference overlay were preserved. The replacement is restoring its original ledger; reference updater998122 waits for it. New fills and faster live execution are not yet verified. The [installation proof](production-r62-installed-20261021.json) also records a corrected deployment-only head-envelope comparison, with original financial files unchanged.
