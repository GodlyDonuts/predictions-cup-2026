# R31 startup: older base policy correctly rejected

Read-only diagnosis, October 17, 2026. R31 PID 690003 stopped at 18:38:15 UTC after restoring history, with `RELOAD_REBASE_NONBINARY_CONTROL_CHANGED`. This note does **not** claim a corrected deployment, resumed trading, new fill, or profit. The execution owner retains STOP and recovery authority.

The active generation was G8, `5350339f459a3651a3f8394abc40684b2be49dee6ab1f2c9b411a8a8fec752e1`. Its original immutable active overlay has raw SHA256 **`ec7421c0ece470e06919c0195f97858c1af28ab4761b3a6d3b8b8235a7f7132e`**. The installed R31 startup Control, SHA256 `8aa4db9d0dd51af45c2265bce365c8fa4c1b660e7172cca2ab7ac834eba073fb`, instead retained older base review settings. Comparing the parsed originals finds exactly four differing paths:

- `activation.binarySha256`
- `fundamentalPolicy.authorityPath`
- `fundamentalPolicy.authoritySha256`
- `fundamentalPolicy.reviews`

The guard permits only the binary difference for this recovery. It correctly refused to replace the committed G8 source/review settings with the older base. This is a configuration readiness barrier, not evidence of inadequate acquisition capital or an economic rejection of a FIFO-profit exit.

In actual source `86619d823de3391c0bd6cdb8514d12b936841a1b`, `worker.rs:2225` calls `Active::recover`. `fundamental_reload.rs:122` selects **only the latest committed generation**, not each historical overlay against today's base. Lines 129 and 37 compare that original active overlay with the proposed new base and reject nonbinary differences. The source-only audit also verified all eight original request-body hashes and their previous-generation chain. There is no justification for weakening that comparison or deleting/relabeling any generation.

The minimal recovery is a separately approved immutable startup Control equal to the **original G8 overlay in every parsed field except** `activation.binarySha256`, which must name the already qualified R31 binary. Preserve the old base, G8 overlay, all original requests/source bytes, review clocks, caps, holding deadlines, and sidecar. No new generation, source refresh, model change, native change, or bridge append is required merely to correct this startup mapping. The owner must independently bind and verify the corrected Control, binary, unit, stopped boundary, queue state, and incident STOP before a single recovery start; this research note authorizes none of those actions.

Root reports no ledger change from the failed startup: boundary **523602**, **382** original fill tokens, all **8** generations and all **16** bridges retained. The sealed deployment packet independently contains that stopped head/report and byte-identical stopped/current 16-entry bridge files. Native identity remains `8012cc9fb2c4feef68f1759ce65a37dc7afcf62a843cfb7a28493d8ac58e1ab6`. This diagnosis performs no financial Session replay and does not substitute for an after-incident owner boundary check.

## Original evidence pins

The private deployment ZIP is 9,062,887 bytes, SHA256 `9d32ccb0986eb9a1430a396df03bd2c8df7abce6475f302a0344c16afa3aa5e2`; all 47 original member byte/SHA pins were checked locally before comparison. Raw payloads remain private.

| Original artifact | SHA256 |
|---|---|
| Stopped head, sequence 523602 | `964ff762bf28d1a22ac2d93a8b2733e03efc1a368426837c7931818edc1f9e97` |
| Stopped report, 382 fill tokens | `3f95693ad82bba9e03863e12bffa2402d4c0b68a6e7afecb0388a76aa505f178` |
| Both stopped/current bridge files, 16 entries | `3ce54840aa80a31aa2d16baccc7a92b31a01d2178e8c61c672dc2353451a9861` |
| Actual `fundamental_reload.rs`, Git LF | `9cd4045967af2eb7ccf213a47444751f61eab245cd9dc5030f095f27fad3ca92` |
| Actual `worker.rs`, Git LF | `d1c1d3b1988e50eef70f918fadaa94bde11ed17f7108872ca6b42a10f052d35b` |

No code, account/API call, SSH operation, runtime file, STOP, ledger, or authority was changed by this audit. Recovery success must be recorded separately after actual owner verification.
