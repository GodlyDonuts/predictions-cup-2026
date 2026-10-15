# Maximum-age sell preflight scheduling

Task: [#130](https://github.com/GodlyDonuts/predictions-cup-2026/issues/130). The production owner retains worker integration and deployment in [#120](https://github.com/GodlyDonuts/predictions-cup-2026/issues/120). This source change supplies a pure scheduling helper; it performs no network request, creates no intent and does not change native clocks or execution authority.

## Observed problem and narrow change

Native inventoryAON exits expire at creation plus configured activation latency and book age. Current production uses three seconds of activation latency and ten seconds of book age: a maximum-age SELL draft has thirteen seconds total. The prior worker waits until activation before beginning its existing six account/contract/book GETs, then requires more than three seconds of remaining expiry before dispatch. With a 7.5-second preflight, starting at second3 ends at second10.5 and fails the final expiry margin. Starting those same reads at second0 ends at second7.5 and fits, while the actual submission remains after second3 activation.

`dispatch_window::preflight_stage` permits only snapshot reads before activation, only for an exact native `action:sell`, `executionStyle:inventoryAON`, `inventoryExitReason:maximumAge` quote with zero fills, no retirement/cancel, no attempted submission and no existing durable intent. Creation must already have occurred, activation must be at most three seconds away and the original expiry must retain more than five seconds of headroom. Ordinary BUY and other SELL policies continue waiting for activation. Existing stable intent and uncertainty protections remain required.

The parent wires this helper into the worker's initial quote-selection guard. It must then accept the freshly read book and reread the current native quote before checking `final_native_window_ready`. If the current native activation is still in the future, no intent, attempt or order may be created; proceed with other work without sleeping or extending the quote. A current retirement, cancellation, existing intent/attempt, malformed clock or final expiry margin rejects dispatch.

The final helper is only a necessary timing guard. The existing production authority still checks the current native activation, immutable control/binary, ownership/stop state, exact request and stable key, available cash, covered physical quantity, fresh account/contract/book receipts, book source clock and all other execution conditions immediately before submission. No prefetch result or helper stage grants trading authority. The original activation and expiration values are used unchanged.

## Limits and validation

This recovers at most the three seconds previously spent waiting for activation. It does not solve every short-exit failure: the read-only own-fill audit identified a draft already expired before dispatch began because websocket batch processing continued beyond its thirteen-second lifetime. That case remains ineligible under this helper. Bounded callback processing or urgent scheduling requires a separate checked change; no freshness window is weakened here.

The existing worker can retire an expired, neverattempted quote during its cancellation/cleanup pass. That durable native event may replan a fresh exit before the subsequent quote-list snapshot. The helper can prefetch for that newly created eligible exit without rewriting or renewing the expired quote.

Thirteen focused tests cover the 7.5-second six-GET timeline, completion before activation, ordinary policy behavior, already attempted/durable intents, retirement/cancel/partial fills, nullable cancel fields, lookahead bounds, original expiry margins, post-prefetch native changes and the already-expired batch-starvation case. The helper can expose its categorical stage for private diagnostics; a prefetch-eligible stage is not a submission or fill.

Local Windows has no Rust compiler. Linux tests and release-build results are recorded by the parent before integration/deployment. These scheduling tests do not establish actual production fills or profitable exits.
