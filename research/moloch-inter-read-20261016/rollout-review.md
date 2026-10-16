# Inter-read runtime rollout draft — review only

The private draft is `C:/Users/csram/AppData/Local/Moloch/ops/paper6_inter_read_rollout.py`, SHA256 `4619c47fc7639f18a468bbb3f4fd8d85136cfe5f6dbb4ddf3cd31b4b334e75fd`. It has **not been executed**. No SSH, live reads, service action, activation or production mutation was performed for this task. Root remains the sole execution owner and publisher.

This candidate changes runtime only. Native engine identity remains `8acc461bc02fb9236100058fe464b203d5da005375632af05c176223c7246754`. There is no bridge command, proposal, installation, detached bank copy or full-prefix replay in the draft. Existing twelve bridge entries and original bridge bytes are captured and checked unchanged.

## Exact CLI and reviewed inputs

Linux usage is `python3 SCRIPT stage|stop|promote ABS_MANIFEST APPROVED_MANIFEST_SHA256`. There is no default operation. Importing the script performs no rollout operations. Explicit exceptions enforce guards even under Python optimization.

The original helper `/tmp/moloch-throughput-rollout.py` must hash to `1e0da4b462dd0627dc597b725fc592530d6ae3c91babe09d4ecb641d2ce87133`; only its reviewed definitions before `command=sys.argv[1]` are loaded. Its existing account-queue quiet and original dispatch validation are reused. No account client is constructed.

The execution owner must supply a complete separately reviewed JSON manifest. Unknown actual artifact hashes are deliberately absent from this document; the script accepts no placeholders. Required fields are:

| Field | Required binding |
| --- | --- |
| `sourceCommit` | `4ee163561b8018e58d945b38b4f4eea5cd7b09ee` |
| `engineIdentity` | Same8acc identity above |
| `buildSource` | `/tmp/moloch-inter-read-20261016-r2` |
| `sourceHashes` | Exact245 raw-byte input map, checked against actual builder files |
| `baseSourceManifest`, `sourceManifest`, `validationEvidence`, `binary`, `priorRollout` | Each an absolute `{path,sha256}` original artifact pin |
| `checks` | `runtimeWorker:{passed:81,failed:0}`, `prizeUtility:{passed:12,failed:0}`, `release:{passed:true}` |
| `priorSourceCommit` | `31aefba5c22b529d60acab65659876294f2e0c8d` |
| `priorBinarySha256`, `priorReleasePath`, `priorControlPath` | Exact actual prior rollout values, also checked against installed/process bytes |
| `priorControlSha256`, `priorUnitSha256`, `bankManifestHash` | Exact execution-owner-reviewed existing artifacts/bank seal |
| `releasePath` | New immutable binary under the existing release root |

The prior rollout is specifically `/tmp/moloch-paper6-buffered-json-production-rollout.json`; its actual bytes must match the manifest pin. It must identify priorPID503120 and the same native engine. Its source, binary, release and control must match the manifest, rather than a partial hash copied from a message.

The244-input base manifest must hash to `0e3afb5a72e392704b803c7f5ab05ed95c4ac8b98e7e546aded1e33406932ad8`. The245-input union permits only these changes:

| Path | Candidate raw SHA256 |
| --- | --- |
| `moloch/live-runtime/src/lib.rs` | `20e0e6123dd2bf5c29b71c81e4623a4037488e7428a53e97c45190ce1430335b` |
| `moloch/live-runtime/src/worker.rs` | `fa826c8782be4db4774c4556df1f695f6fe66cec6e6e8db55837e464d87ecf87` |
| NEW `moloch/live-runtime/src/prize_utility.rs` | `bd2d1dc41759f4d39d89d5c6c920dcbaa5ec3afe23b267c82b4a1920b685305c` |

All native and other source inputs must remain byte-identical. Validation evidence must bind the same source manifest, exact check summary, compiled binary hash and engine identity. This draft does not itself establish that the build/tests succeeded.

The originalR1 candidate had78 old worker checks pass and three new fixture failures. Root suppliedR2 with corrected fixture UUIDs and the changed worker pin above; the production hook and native inputs were unchanged. R1 failure evidence is preserved, not relabeled as success. R2 must satisfy the original81-check requirement before staging.

## Operation boundaries

`stage` verifies the live oldPID/binary/control/unit, absence of both STOPs, exact artifacts and validation evidence. Actual `/proc` executable path/hash and exact `release run control` command arguments must match. It stages a new immutable binary/control/unit candidate, verifies the unit, and backs up original unit/control/restart configuration. Only `activation.binarySha256` changes in the copied control; all other controls, caps, funding configuration, daily authorization and rates compare exactly. Restart drop-in hash must remain `ddbe320f3fcd657d719a2ec28f67282ce2eb0c90ff8bf03eff9308871121936e`, with no extra drop-in files.

`stop` writes an owned request and STOP, waits at most120 seconds cooperatively, then requiresPID0, oldprocess absence, the original capture lock, no global STOP and zero queue unknowns/active request/manual review. No termination, lock deletion or unknown clearance occurs. It captures exact bank head/report/manifest, full original event-prefix length/hash, unchanged twelve bridge entries/hash, original dispatch verification/hash, fundamental journal/marker and package journal hashes. It does not copy those histories or rewrite them.

`promote` requires the same stopped boundary and artifacts, fresh quiet verification and a separate owner-created `/tmp/moloch-paper6-inter-read-owned-stop-approval.json` with this exact shape:

```json
{
  "operator": "paper6_inter_read_rollout_v1",
  "executionOwnerReviewed": true,
  "oldPid": 503120,
  "manifestSha256": "EXACT_APPROVED_MANIFEST_SHA256",
  "requestSha256": "EXACT_ORIGINAL_REQUEST_SHA256",
  "stopSha256": "EXACT_STOPPED_BOUNDARY_STOP_SHA256",
  "workerStatusSha256": "EXACT_STOPPED_BOUNDARY_STATUS_SHA256",
  "reason": "controlled-worker-stop-only-not-human-stop"
}
```

The witness is generated by the execution owner's independent review, not this script. It binds the **actual** STOP/status after shutdown: the worker may have replaced the initial request with generic `STOPPED_OR_OWNER_LOST`. Request-token equality is not assumed. A human STOP must not be relabeled by this witness.

Only that reviewed STOP is archived. The global STOP must remain absent. The candidate unit is installed, oldprocess absence is checked again, and there is exactly one service start. Failed archival/start preserves or writes a STOP and does not retry. The output receipt reports `freshTradingVerified:false`, `profitabilityEstablished:false`, and `bridgeWritten:false`; restoration/fresh trading must be separately observed by root.

## Checks and remaining review limits

Twenty-three pure guard checks passed locally: exact source union/native tamper, wrong runtime hash, missing/new input, control cap/rate/daily changes, failing/wrong/type-confused test counts, priorPID/engine/binary mismatch, and missing/altered independent STOP/status witness. JSON/source input fixtures are synthetic validation inputs, not account evidence. Syntax and import checks passed without running a mode. No Linux service or end-to-end operational success is claimed.

Root independently reread the pinned helper: dispatch verification validates the original chain and resolved attempts while allowing acknowledged still-open orders; queue quiet requires zero global unknowns, no unknown flag, no active request and no manual review. The earlier buffered rollout retained nine open orders. This task did not fetch remote files or independently perform those live checks. Actual manifest artifacts remain execution-owner review dependencies. The script never releases reservations, cancels orders or changes their attribution. Any failure leaves the rollout for the execution owner to inspect; it does not infer permission to clear a stop or retry.
