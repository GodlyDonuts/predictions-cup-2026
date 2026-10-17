# R28: original binding expiry repair

Checked source is `069a85a478942eb8148c354df368e369e940eb7e`. R28 was installed at15:00:15.643UTC as the sole worker661886 after the original R27 failure at14:21:28UTC. The new process is restoring; fresh trading, fills and profit are not yet verified.

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

## Final release and controlled recovery

[Final build verification](final-build-root-verification.json) authenticates275 original members/all263 Git inputs and the optimized release with the same290 distinct checks. Binary SHA256 is `203462d82692c9a68c3303546271bc8b7fc60e3945b36fe4224a5252be779dfe`; private build archive7,332,007bytes/SHA091d3ed0ce95dd89246c549f2c01235959d49e0bb7f1ea3f2627c236d7e57b12. The earlier source-only packet remains an accurately dated release-pending artifact.

[Independent deployment verification](deployment-root-verification.json) authenticates23 original members and actual new executable/argv/MainPID, unchanged native identity, exact stopped492481/382fills, all six original review generations and15 original financial bridge bytes. The original natural STOP is archived byte-for-byte. The new control materializes the already committed sixth model generation and changes only activation.binarySha256. No model, rate, risk, source or expiry clock was renewed. Actual last cash3,613.785/owned91,851.730/attributedrealized−114.400 remain historical accounting, not current executable marks.

The finite observer661894 reads only existing files, with no account calls or runtime writes. It requires actual fresh trading/realtimeHealthy from new PID661886 before capturing the first300s. A process that is restoring does not establish current market connectivity, submissions or profitable operation.
