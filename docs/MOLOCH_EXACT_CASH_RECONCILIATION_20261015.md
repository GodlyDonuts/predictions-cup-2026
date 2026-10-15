# Exact scoped cash reconciliation — October 15

Production task [#120](https://github.com/GodlyDonuts/predictions-cup-2026/issues/120). The existing guard compared exact native cash to the tournament summary's `myBalance`. The official saved OpenAPI explicitly documents that field as rounded to two decimals. A half-cent fill result therefore caused a false mismatch and repeatedly deferred otherwise eligible proposals.

At 11:03Z, the native expected account cash was 70,851.605000 while the tournament summary showed 70,851.61. The existing worker's independently stored scoped `/account?tournamentId=...` receipt at 10:21:09.204Z reported balance and buyingPower 70,851.605, with zero availableAdvance. All 95 owned orders were terminal and the 27-fill ledger was unchanged across these observations. This supports a precision-source diagnosis; no invented cash adjustment or reset is needed.

The patch adds a fresh scoped exact-cash receipt to `SnapshotSet`, checks its exact endpoint/tournament binding and existing 20-second freshness, validates the official cash/advance/buying-power identity, and compares gross cash to the unchanged native expectation using integer decimal units. The existing funded allocation, position attribution, contract/book freshness, passive-entry rule and uncertainty guards remain. Buying power or advance is never substituted for cash. A new bank seeds from exact scoped cash; existing seed files and financial histories are untouched.

Snapshot collection adds one serialized Account GET before the market/contract/book reads, leaving the executable book last. It uses the existing central limiter and durable receipt store. Seven reads plus network/queue time can still exceed a short exit's window; no expiry or freshness bound is extended. The separate one-preflight-per-pass scheduling bound prevents repeated failed candidates from monopolizing the worker.

Three added authority regressions cover exact half-cent authorization despite rounded display, rejection of a real sub-cent discrepancy even when the display matches, and rejection of wrong scope, stale/null cash or substitution of borrowing capacity. The full integrated runtime suite passes 118 checks on ARM64. Release and live outcome are recorded in CURRENT_STATE; a passing guard does not establish fills or profitability.

Primary contract: saved `runtime/moloch/api-spec-ioc-20261014.json`, SHA256 `6b5ff9a661c06f1a2d1dda131bf870a70f4f79699ac9937375a278861e1f81bf` (ignored private working receipt; actual SHA is documented in the same-binary audit). Relevant pointers are `/paths/~1tournaments~1{slug}/get/responses/200/content/application~1json/schema/properties/myBalance/description` and `/paths/~1account/get/description`. Raw account/profile response fields stay outside Git.

The combined bounded-preflight/reconciliation integration adds eighteen scheduling/rotation tests to the 118-check exact-cash suite. Promotion requires the combined Linux suite/build, not just the earlier exact-cash result. Broader ordered websocket draining remains unwired.

Final combined ARM64 validation: 136 runtime checks pass, locked release build passes. Candidate binary SHA256 is cf5821a7feb2b1d242a8c574728e76daf717a05985c159dce01ef719c86a4dcd. Native financial identity remains the checked V18 identity; no new engine bridge or financial history change is required.
