# R77: stop repeatedly scanning the same dispatch history

Observed October 21, 23:34 UTC. R76 remains the sole live worker while the checked R77 replacement is prepared. The one-filled-order-per-minute target remains unmet.

## What changed and why

Twelve spaced stack samples found four consecutive observations in the original closed-order reader, two in cohort checking, and others in full report rendering and stream work. Sampling briefly suspended the process for 6.07 seconds in total; these observations are not exact CPU-time fractions. [Original profile](r76-cycle-profile-20261021.json).

The old reader cached exact order-ID selections. When a different market requested another selection from the same dispatch head, it parsed and hashed the entire journal again. R77 retains a bounded row-offset index after the first full chain validation and reads the original selected rows through the held descriptor. A changed head invalidates the index. Descriptor identity, file size and nanosecond modification/change clocks remain checked, recent-file versions retain full hashing, and physical dispatch plans still validate their original files. Membership and duplicate lifecycle rules remain checked on every selection. More than 65,536 indexed rows falls back to the original full scan. No account, book, clock or financial fact is cached by this change.

Two worker readers also reuse the existing immutable order snapshot instead of rendering all historical orders for each managed-market lookup. Selection and output fields are unchanged. Other unfinished worker/native changes in the working directory are excluded.

## Measured result and qualification

On an unchanged 27,436,363-byte copy of the original journal, 30 selections across three alternating baseline/candidate pairs took a median **14.8446 seconds before and 1.0228 seconds after**, a **93.11% reader-component reduction**. Every selected original row and prefix hash matched. This does not establish a 93% whole-worker gain or a fill-rate improvement. [Reproducible benchmark](r77-closed-index-benchmark-20261021.json), [driver](tools/benchmark-closed-index-r77-20261021.py).

The focused reader suite passed 11 checks, with one optional check ignored. Full fundamental integration passed 146 checks; the worker passed 233. The optimized ARM64 binary loads and hashes to `c1afb6bf248e2b31c48b1025326b186c859a6113f330b4da802ab83a029f668c`. The native engine/library is unchanged. [Exact qualification](r77-closed-index-qualified-20261021.json), [build](tools/build-closed-index-r77-20261021.py).

## Trading and money before replacement

At 23:25:01 UTC, original receipts establish 18 distinct first-filled orders today, two in 15 minutes and five in 60 minutes. R76's three newly filled orders bought 6 shares of 1043 NO, 1,263 shares of 1044 YES, and 10,000 shares of 1077 NO. The last order has eight receipts but counts once. The 966 buy during startup belonged to the old R74 worker. All original fills and remaining FIFO match. Cash is 895.22, remaining original cost is 94,974.69, and attributed realized net is 289.995 including the unchanged 76.52 inherited portion. No new realized gain from the R76 purchases is established. [Receipt/FIFO audit](r76-turnover-later-20261021.json).

Separate existing-owner account receipts at 23:26:10–12 show marked positions 93,503.68 plus cash 895.22, totaling **94,398.90 SUSQies**, **−5.6011%** from 100,000. This is not an atomic snapshot or executable liquidation value. No extra account client was used. [Marked-account evidence](r76-latest-account-20261021.json).

The finite exit census has observed profitable bid depth for parts of 993 and 1044 inventory, but no demonstrated ask-relative pricing miss in its accepted observations. Quotes are not fills and repeated snapshots are not additive profit. Most capital is now deployed, so sustained turnover needs inventory exits. A resting 865 sell acknowledgment is not counted as execution.

## Deployment and remaining work

The prepared replacement drains the original worker, preserves original FIFO/fills, dispatch history, known open orders and unknown-acceptance handling, and starts one successor with the same native engine, monetary limits and account limiter. The previous replacement needed about 21 minutes to restore history; this cost is material. [Deployment](tools/deploy-closed-index-r77-20261021.py), [finite verification](tools/observe-closed-index-r77-20261021.py).

R77 is qualified but not yet claimed live in this report. Verify the actual executable, complete original financial prefix, fresh trading and real first-filled-order cadence after replacement. Neither tests, invested principal nor ACKs establish profitability or completion.
