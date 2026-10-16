# Ordinary side-edge runtime rollout — private review draft

The private draft `C:/Users/csram/AppData/Local/Moloch/ops/paper6_side_edge_rollout.py` currently hashes to `6016ec266e6271884b711b1d40f0877b0b3d9391444cb414367d22a31a802e61`. **It refuses every rollout mode while actual check counts remain unfrozen.** Candidate source and exact hashes are now pinned; test/release work is still running under the execution owner. No SSH, build, service, queue, account/API or rollout mode was executed by this task.

The existing inter-read operator is unchanged, SHA256 `4619c47fc7639f18a468bbb3f4fd8d85136cfe5f6dbb4ddf3cd31b4b334e75fd`. This is a separate draft and output namespace. Root remains sole execution owner/publisher; local synthetic guard checks are not operational deployment evidence or a reason to interrupt the healthy worker.

## Preserved authority and provenance

Linux CLI remains `python3 SCRIPT stage|stop|promote ABS_MANIFEST APPROVED_MANIFEST_SHA256`, without a default action. Before loading the approved manifest or helper, `freeze_ready()` rejects unset source/build/check constants. The current exact rejection is `CHECK_COUNTS_NOT_FROZEN`. Import is inert. Explicit exceptions enforce guards even with Python optimization.

Native engine identity must remain `8acc461bc02fb9236100058fe464b203d5da005375632af05c176223c7246754`. There is **no bridge, detached bank copy or prefix-replay operation**. Original twelve bridge entries and bytes are captured and checked unchanged. Bank, actual positions/fills, principal, dispatch/unknown state and original account budgets remain authoritative.

The original245-input R2 source manifest must hash to `2f53e211cd1c5d9476f71f94b596fb75bc1bbcb541789f8750f4d62da25fbefa`. Frozen candidate source is `870ce889d51a04c654a93785277c47ea33b52bea`, builder `/tmp/moloch-side-edge-20261016-r1`. Candidate248 inputs must be its exact union with three new runtime files; every native/other input remains byte-identical. The exact six allowed changes are:

- `moloch/live-runtime/src/lib.rs`
- `moloch/live-runtime/src/worker.rs`
- `moloch/live-runtime/src/prize_utility.rs`
- `moloch/live-runtime/src/stream_token_retry.rs`
- `moloch/live-runtime/tests/stream_token_retry.rs`
- `moloch/live-runtime/src/ordinary_side_edge.rs`

There is no separate ordinary-side-edge test file: its15 tests are inline, with four additional worker tests. Runtime/lib/worker/token module/token tests/side-edge module are required in the frozen map. Hashes cannot be substituted with a purported commit from a message: actual builder bytes, original source manifest, validation evidence, compiled binary and engine identity must agree. Exact required positive passed counts and zero failures are independently frozen for worker, prize utility, token retry and side-edge checks, plus a successful release build. At least the prior81 worker and12 prize checks must remain represented. None are currently asserted passed for a candidate.

| Runtime-relative path | Exact frozen raw SHA256 |
| --- | --- |
| `src/lib.rs` | `920e074ac0166bbabeb886c0ad6a67c0fa0cfc96b649e5d3301cf61ad58989c0` |
| `src/ordinary_side_edge.rs` | `4e7aa70f6a6acef7070fce2affed5b7d2129108961993254e64080ad03ce4715` |
| `src/prize_utility.rs` | `a5997c6beaf1adfa0f9e9aef6bc1c804d30fb1f0aad9867a3609bd7ed23ee747` |
| `src/stream_token_retry.rs` | `a305003b39ae1a0433f08fc68adc62f66a807502c1bf1935525fdd21e494906a` |
| `src/worker.rs` | `520b845af5515467bddcab5238f59e7e51b734344db08cc1d0432d8bfa62d680` |
| `tests/stream_token_retry.rs` | `8f184abe8faa5c2b6f22200f440c8724941263b22b799cc75208cddc95a4f14f` |

All six hashes independently match the named Git commit's LF bytes locally. This verifies the source pin, not remote builder fidelity or build success. Root independently checked the full native input map unchanged; stage must still check all actual builder bytes.

The prior receipt is exactly `/tmp/moloch-paper6-inter-read-token-recovery-rollout.json`. Its **actual raw hash** must match the execution-owner-approved manifest. It must bindPID510591, source`4ee163561b8018e58d945b38b4f4eea5cd7b09ee`, the unchanged native identity and exact old release/binary/control values. Stage/stop also verify actual `/proc` executable path/hash and exact `release run control` arguments. No abbreviated binary/hash from a message becomes a passing pin.

The script additionally pins original prior receipt SHA`3525d580edea47480c284c4c8d36b477111383a8abc3e9d2d7c6903cfb82c865`, control SHA`7e7cb15ab9dc853887edec5768690fc682f01753d59c9109f102dd6f20641fa2` and unit SHA`edc4bd56a7d307f46db642e869e4c119359740072fce92a118a59bf00270fdfb`. The reviewed manifest and actual files must both match these exact values.

The only accepted restart-directory entries are:

| Entry | Exact raw SHA256 |
| --- | --- |
| `10-bounded-restart.conf` | `ff4237fb5e0b5f2a68159e4fa43d737eefc68ebdd422f56eea068c4ffba0e795` |
| `10-bounded-restart.conf.before-exec-condition-20261016` | `ddbe320f3fcd657d719a2ec28f67282ce2eb0c90ff8bf03eff9308871121936e` |

The backup does not end in `.conf`, is explicitly pinned, and is not treated as a second active drop-in. Extra/missing/changed files reject. Both original configuration files are backed up with the unit and control; none is deleted or silently replaced by the operator.

## Exact control difference

The approved manifest must contain `ordinarySideEdgeMode`: `null`, `"disabled"`, `"shadow"` or `"enforce"`. `null` preserves the existing field exactly, including absence. An explicit string changes only top-level `ordinarySideEdge` to that value. There is no default enabling behavior. Root decides the mode only after source/authority/preservation review; the operator does not judge the retrospective study sufficient for enforcement.

Apart from `activation.binarySha256` and the optional explicit enum, the complete new control must equal the original control. Capital, funding, daily windows, activation end, sizing, inventory management, rate pacing, reviews and all other fields remain unchanged. Hidden/unfamiliar fields are preserved by exact deep comparison rather than discarded through a partial whitelist.

## Stage, stop and promotion boundaries

`stage` requires the old process and artifacts to match, both STOPs absent, and the frozen source/build/test evidence. It stages an immutable new binary/control/unit candidate, validates the unit and preserves exact backups. It does not stop or start the service.

`stop` writes an owned request/STOP and waits cooperatively for at most120seconds. It does not kill another process. Before capturing the boundary it requiresPID0, oldprocess absence, the actual capture lock, no global STOP, and the original quiet queue proof with zero global unknowns, no active request and no manual review. The pinned reviewed helper permits known acknowledged still-open liabilities; they need not be closed solely for a same-ledger runtime replacement.

The stopped boundary captures exact bank head/report/manifest, original event-prefix byte length/SHA, all twelve bridge entries/SHA, original dispatch verification plus raw journal SHA, fundamental journal/marker SHA and package journal SHA. It neither copies those histories nor resets funding, leases, reservations, fills or unknowns.

`promote` first rechecks the same boundary, quiet state and staged/original artifacts. It requires a separate execution-owner witness at `/tmp/moloch-paper6-side-edge-owned-stop-approval.json` binding operator`paper6_side_edge_rollout_v1`, actual oldPID510591, approved manifest hash, original request hash, **actual stopped STOP hash** and worker status hash, with reason`controlled-worker-stop-only-not-human-stop` and `executionOwnerReviewed:true`. The worker can rewrite the initial STOP to generic owner-lost status, so request-token equality is never assumed. The script cannot generate its own independent witness or convert a human STOP into an owned stop.

Only that witnessed STOP is archived. Global STOP remains absent. The candidate unit is installed; original process absence and STOP conditions are checked again before exactly one service start. Archival/start failure preserves or writes a STOP, with no automatic retry. Output explicitly reports fresh trading unverified and profitability unestablished. Root must separately verify actual restoration and fresh trading.

## Local checks and remaining freeze

Thirty-four pure guard/import/syntax checks passed: unfrozen refusal, exact binary/enum differences, existing-mode preservation under null, cap/rate/daily/activation tampering, source union/native tampering, wrong/missing source/check counts, priorPID/engine/binary mismatch and independent actual-STOP/status witness changes. Synthetic in-memory frozen values were used only to exercise guard branches; they never populate the on-disk draft or run a mode. The prior operator hash remains exact.

Pending root inputs are actual test/release results and exact required counts, compiled binary hash, source/build/validation artifact hashes, and full old release/binary/control paths for the approved manifest. Until those are supplied and reviewed, this document is an intentionally non-executable rollout review. No additional human authorization flow is introduced: root is the existing execution owner and supplies the actual review witness.
