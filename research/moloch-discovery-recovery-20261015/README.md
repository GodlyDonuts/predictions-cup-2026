# Discovery recovery on actual production capture — October 15

The V26 runtime fixes a circular REST-discovery gate: the old selector required a fresh cached book and recent prints before scheduling the REST reads needed to recover a dirty or gapped market. Current authenticated activity can now nominate bounded REST recovery reads. Financial entry decisions still require the existing fresh executable facts and account authority.

The independent comparator verifies every hash-linked row and exact frozen byte prefix, then feeds the same 207 official frames (44,815,263 bytes) into the frozen old Universe and candidate. It preserves the same 18 owned markets and maximum 31 managed / three discovery reads. At frame 132 (20:04:45.424 UTC) and 140 (20:06:26.697 UTC), old discovery is zero and new discovery is three; frame 120 improves two to three and frame 160 one to three. At the endpoint both select three. This is recovery coverage evidence, not a fill or profit forecast. Intermediate probes and the exact prefix digest are in `actual-comparison.json`; private feed payloads and account IDs are excluded.

All probes preserve owned coverage and caps. Recovery activity expires after 60 seconds; the financial freshness gate remains ten seconds. The new selector prioritizes genuine fresh two-sided Study flow, then rotates recovery activity; genuine top-ranked flow can retain the same three slots, as the endpoint test demonstrates. Stream reset, malformed scope, future timestamps and settlement suppressions have focused regressions.

The runtime-only candidate also reduces cancellation preflight from seven logical endpoint operations to four account operations, retaining real cash/positions/open-orders/fill receipts. SDK pagination or recovery can add physical HTTP operations.

## Checked artifact

Source commit `e960eb39` on `codex/moloch-v26-discovery` contains the runtime-only candidate. Its 229 runtime checks and locked offline ARM64 release pass. Independently executed compiled native identity is `8520add404082af22ae484409cfcb972a895ffe612dec014c9d668c70b7ca282`, identical to the existing bank. Binary SHA-256: `38d35f3a28a925ba995148311d6d6d670496c94d2880353136eaa9d97298c73ec`. No fifth engine bridge, risk increase, second collector or account client is introduced. Production rollout evidence belongs to task #120 and CURRENT_STATE; staging alone is not deployment.

## Reproduce

Build the matching runtime and compile `replay.rs` with its emitted `moloch_live_runtime`, `moloch_replay`, `serde_json`, and `sha2` rlibs and dependency directory, using the same Rust toolchain. `legacy-universe.rs` freezes the previous selector from main `c9fa5571`. Run the comparator with the private scoped input and sealed capture paths. It makes zero account requests and never opens a bank. It verifies row chain, prefix SHA/bytes/frame count, clocks, owned coverage, read caps and expired activity before emitting aggregate JSON. The original input/capture remain private on the VPS; no synthetic profitable result is substituted.

## Production verification

The first full healthy cycle completed20:44:34 UTC with410ACK/86fills. Four subsequent official fill rows bought and sold123NO, respectively at0.350 and0.345, realizing−0.615000 with zero fees. The private original tape Receipt digests, matched IDs, signed quantities, prices and execution clocks verify; the derived report seal verifies but is not a full bank-chain replay. At20:53:59 UTC new-trading realized is−116.020000, inherited gross+38.075000 and actual fills90. [Cohort proof](first-live-cohort.json). This confirms trading resumed; it does not establish profitability.

V26 launched20:24:28 UTC as PID227437. At20:34:55 UTC its exact running binary published healthy fresh trading progress with395 acknowledged orders,393 terminals and86 actual fills; two acknowledgments are new after launch, not fills. STOP is absent, full first cycle pending. The stopped boundary was393/393 acknowledged attempts, unknown0/activefalse/manualReviewfalse. See release-checks.json, rollout.json and live-verification.json. Existing financial identity, four-entry bridge, caps and daily mandate are preserved.
