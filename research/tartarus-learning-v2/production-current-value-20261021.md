# Current-value execution repair — October 21, 2026

## Objective and measured baseline

The human's operational target is **97,000 marked SUSQies by 10:58:40 UTC on October 21**, measured from the 08:58:40 task start. This is a target, not a promised return. The existing official worker's 08:53:37 positions receipt reports 94,389.64 in marked positions; its account receipt reports 403.36 cash: **94,793.00 total**, leaving 2,207.00 to the target. These are contest marks, not executable liquidation proceeds. Almost all capital is already invested.

## Verified bottleneck

The installed typed review inspector validated the original pinned settings and found **zero current reviews out of fourteen**. Their forecast expirations are October 18–19. Meanwhile the existing laptop collectors are live: 237 Kalshi markets and 378 Polymarket outcomes. Fresh collection was not automatically refreshing the production review inputs. Raising feed throughput alone cannot fix that gap.

Worker R59, PID989530, is still restoring the original ledger at this report's preparation. Its already-published change removes a redundant 2,500 ms post-read exit delay. Neither restoration nor this staged valuation repair proves a new order, fill or profit.

## Implemented prospective changes

- Capture twelve existing V2 review scopes from the existing Kalshi cache. Require original body hashes, matching parsed bodies, exact retained SIG contract roots, two-sided books, quotes within 30 seconds and market metadata within one day.
- Use the conditional Kalshi party midpoint as an explicitly **uncalibrated** model input. Preserve source bytes and receipt times. Six of the initial twelve scopes also retain current exact-party Polymarket observations as context; those observations do not silently become model inputs or settlement-equivalence claims.
- Give the fresh forecasts a 15-minute lifetime. Retain original ballot and contract identity bytes and clocks with a declared 14-day identity lifetime; the existing worker still checks the exact current SIG contract before dispatch. This changes the identity-document retention policy, not the actual SIG book/account freshness limits.
- Replace FIFO-only exit pricing on **1067 YES (Alaska Senate)** and **872 YES (Nevada Governor)** with current modeled value. This can realize a loss when freeing capital is preferable to keeping an overpriced position. Original costs and realized losses remain in the ledger. Other existing FIFO profit-turnover policies remain.
- Retain legacy reviews **1042 and 1068** unchanged: their original V1 cohort deadlines cannot be silently reinterpreted by this V2 refresh. Their legacy exit behavior remains a limitation.
- Serve public reference captures on laptop localhost only. An authenticated SSH reverse tunnel exposes that endpoint only on VPS localhost. It carries no SIG key or account credentials.
- The VPS refresh process stages immutable reviews, then uses the existing worker's authenticated owner socket. Initial refresh covers all twelve scopes; subsequent passes rotate one scope every 30 seconds. An open target order can defer its refresh. The process validates the exact worker, original base control and generation before every request, and halts on an unresolved reload result or another generation writer.

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

The standalone ARM64 typed helper builds against the deployed runtime library. Original settings load with fourteen valid historical review/source bindings and zero current reviews. A fresh twelve-review generation stages and validates with twelve current reviews. A separate two-market capture traverses the real localhost SSH tunnel and passes the same validation. Deliberately changing the midpoint away from its original book is rejected. Future-clock/stale quotes are rejected. Staging preserves every base financial-control field and sends zero reloads or official account requests.

Production activation and original fill evidence will be recorded separately after the current worker finishes restoration. The existence of a prepared generation is not a deployment claim. Existing GPU training and retained checkpoints continue; live production reference observations are excluded from ML fitting and selection. A matched CPU evaluation of the completed 656-update member started at 09:19 UTC; no independently profitable ML checkpoint is established.
