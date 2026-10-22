# R84: publish one full state report per source/preflight sequence

## Problem and change

The one-first-filled-order-per-minute target remains unmet. R83 completed two first-filled orders in its first 15-minute measurement and one in the next. The latter sold 1,472 YES993 shares, realizing 66.24 SUSQies against original FIFO cost. More acknowledgments or partial receipts are not additional trades. [Completed second window](r83-post-delaware-cadence-20261022.json), [official-receipt/FIFO audit](r83-post-delaware-turnover-20261022.json).

R83 spent a median 125.53 seconds between completed scopes in the initial window. Its full historical state report is approximately 50.5 MB. Live stack samples caught report construction during source admission; repeated full rendering occurs while collecting a source pair or seven preflight receipts. The SDK read budget was not saturated at the checked boundary. These samples identify a cost, not its unbiased percentage of runtime. [Evidence](r83-processing-bottleneck-20261022.md).

R84 defers construction and publication of that derived report until the bounded source/preflight sequence returns. Each original event still applies to the current engine, passes its financial audit, and persists its journal row and head. The worker continues to use current in-memory facts and original receipt timestamps. No order submission occurs inside either deferred scope. Outside these scopes, full report publication remains synchronous.

On a semantic error, the report is published for the committed prefix. Panic or I/O uncertainty poisons the session; restart recovers the original journal. Nested scopes are rejected. The report file can be older than the head while the scope is active, so readers must retain its sequence and timestamp rather than present it as current.

## Scope and preservation

The isolated candidate changes only `moloch/native/src/session.rs` and two wrappers in `moloch/live-runtime/src/worker.rs`. Unrelated unfinished rank changes are excluded. No sizing, pricing, limiter, execution window or financial ledger policy changes are included.

The retained engine approval chain already occupies about 252.6 MB. One more original 50.5 MB report exceeds its previous 256 MiB envelope, so the bounded file limit becomes 384 MiB. The 32-entry maximum, every prior approval, exact financial prefix, report, holdings, costs and unknown-order checks remain. This is storage capacity for retained approvals, not an increase in trading risk.

The installer requires the completed R83 observation, an exact published source commit, qualified artifact hashes and the original process identity. It drains that worker, retains the original bank, verifies the next engine approval, and starts one replacement. Existing approved positions and known outstanding orders remain on the same bank. A file-only observer independently checks the complete original financial prefix and all original fills before reporting fresh trading.

## Validation and production status

The isolated native suite passed 136 checks (three explicit experiments ignored). The worker suite passed 244 checks. New coverage proves serial and deferred execution produce byte-identical journal, head and final report, including fills, remaining FIFO and fees; no intermediate full projection is built. Error, panic, restart and projection-write failures are covered.

At preparation, R83 is still production and R84 has not been deployed. Release binary `a0ec22267f1a10ed56f4a97ebd5f949d1b127f8ecaf0a3910b12362ebc49aa9d` and its upgrade helper are now qualified. [Exact source/library/binary checks](r84-source-projection-qualified-20261022.json), [engine identity and helper](r84-upgrade-helper-20261022.json). The published source will match both compiled source files byte for byte, including the non-ASCII serialization fixture. No full-worker speedup, one-filled-order-per-minute result, or whole-account profit is claimed. The latest separate official mark is 94,293.565 SUSQies from the 04:39 UTC receipts, observed 04:41:49, down 5.706435% from 100,000. [Dated aggregates](r83-account-after-delaware-20261022.json).

The completed coverage capture also found fresh incoming books absent or stale in the native strategy view at two timestamps. Faster processing may improve that gap, but freshness, source recovery and actual fills must be measured after restart. [Corrected coverage interpretation](local-retirement-sweep-r83-20261022.md).
