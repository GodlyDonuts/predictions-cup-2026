# Exit lifecycle rendering correction — October 21

R63 is running, but its latest exit refresh at 12:53:19 UTC still reported
`EXIT_REFRESH_FACTS_STALE` after book planning. No new fill is established:
396 original fills and 1,660 acknowledged/terminal orders remain the baseline.
A live stack sample located repeated SHA-256 work inside `refresh_safety`.

This patch reuses the exact lifecycle fingerprint only while the held native
Session, manifest, durable head and dispatch proof are unchanged. It also uses
the existing immutable order snapshot for exit reservation scans. Current clocks,
STOP state, realtime health, liabilities, reservations and account facts are still
checked on every call. The cached value is the same full JSON digest used before;
it is invalidated by a native event or dispatch-journal change.

Fundamental physical decisions now share the existing immutable, head-bound
ledger snapshot with production authority. Original-allocation comparisons avoid
repeated serialization; independent copies still require the full digest. A
49,948,406-byte production report measured about 176 ms per repeated complete
hash on the VPS. This is a component measurement, not end-to-end dispatch speed.

The cache regressions also exposed same-timestamp file rewrites. Recently written
dispatch files now receive full content verification; settled files retain the
existing descriptor/version cache, and physical-plan validation remains complete.

ARM64 verification: 30 authority, 22 fundamental-cache, one lifecycle-cache,
21 exit-refresh, 133 fundamental integration, 23 refresh-worker and 12 reserved-exit
checks passed. Two pre-existing benchmark cases remain ignored. The new lifecycle
fixture initially used an attempt without its corresponding intent; corrected
to the actual intent ID, it verifies unchanged-state reuse and invalidation by
native and dispatch events. No production order was involved.

The candidate is built as a surgical runtime patch on the installed R63 source,
reusing its exact native and transport libraries. Unrelated local rank/native
experiments are excluded. Installation and fresh trading require separate runtime
evidence; passing these checks does not establish fills or profitability.
