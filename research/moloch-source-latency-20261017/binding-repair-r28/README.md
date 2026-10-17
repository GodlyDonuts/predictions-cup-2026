# R28: original binding expiry repair

Checked source is `069a85a478942eb8148c354df368e369e940eb7e`. Production remains stopped after the original R27 failure at14:21:28UTC; an optimized release and controlled sole-worker recovery are pending. Source publication does not establish a deployment, fill or profit.

The original generic `FUNDAMENTAL_NOT_ELIGIBLE` originates in `bind_quantity`, before R27's physical-decision catch. The new pre-intent helper wraps **both actual binding and physical token minting**. It defers only the exact stale-book codes after proving the current quote has no intent, physical attempt, acknowledgment, binding or fill; the SDK queue must positively have zero unknowns/no active operation/no manual review, and the actual target native book must be unavailable under the unchanged ten-second policy. Fresh ineligibility, file/proof errors, queue uncertainty and any post-intent failure retain their original stop behavior.

The same candidate replaces one full-history rendering in `assess_quantity` with the existing exact head-bound order snapshot. Its liability predicate still examines every relevant row against the current dispatch state; no account facts, review, eligibility decision or authority token are cached. This removes repeated rendering, not the liability checks.

[Original incident](../incident-r27/README.md) authenticates untouched quote15612, all382 prior fill tokens and six model generations. The prior R27 claim that the original shutdown was fully repaired is superseded by the actual callgraph and regression here.

## Root-verified source qualification

[Original source/test aggregate](source-tests-root-verification.json) verifies273 original packet members, all263 Git inputs (223 exact/40 equivalent line endings), and exactly two changed source files relative to qualified R27: `worker.rs` and `fundamental_policy.rs`. Financial engine identity remains `5b61d3cbb38eadf5170fafae5c91cb5b75858346b2771031084224bfa2c3d6d4`.

| Actual ARM64 target | Passed |
|---|---:|
| Fundamental library unit checks |8|
| Snapshot library unit checks |2|
| Real-Session snapshot integration |6|
| Fundamental integration |128|
| Worker |146|
| Pre-intent filter, already within worker |5|

There are290 distinct checks, not295. Actual regressions exercise the real binding call with a book expiring before binding, expiry between binding and token minting, fresh success, and fresh ineligibility/unknown queue/prior operation failures. The first run exposed an out-of-order ACK in a negative test fixture; its timestamp was corrected against the actual native clock, with production logic unchanged. Original failed qualification evidence remains private and retained.

Independent targets were compiled with three jobs on the six-core VPS; source and release settings stayed fixed. The source/test packet explicitly records release pending and no binary hash. No optimized-release success or installation is claimed by that packet. A final release proof and owner recovery must preserve the exact stopped492481 boundary, six original review generations, fifteen financial bridges and current account queue before restarting the sole service.

The separate [measured dispatch-proof component](../not-sent-bench/README.md) and [reviewed value-limit exit strategy](https://github.com/GodlyDonuts/predictions-cup-2026/issues/169) are follow-up work. Neither is silently included in this candidate.
