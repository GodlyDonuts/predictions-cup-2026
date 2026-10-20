# SIG data-only historical probe: prepared, not executed

The existing SDK supports typed historical candle reads through its original serialized account queue. A new isolated `sig-history-read` binary is built and tested on the VPS; production services, controls and financial state have not been changed. [Actual preflight/build proof](sig-historical-read-probe-20261020.json).

The proposed first request is one GET of up to500one-minute candles for exchange840, ending **October12,05:00UTC exclusive**, explicitly scoped to the original tournament. It tests whether the official history endpoint can supply older SIG prices. It does not reconstruct historical order books, establish original publication latency or start the ideal training experiment. Empty or rejected history is not success evidence.

## Fixed read boundary

`backend-rust/transport-service/src/bin/sig-history-read.rs` derives account scope, exchange eligibility, queue path and existing credential path from the exact hash-pinned original production control. It requires the real canonical `/home/archon/apps/moloch/shared/live-production-v1` root, preventing a copied control from selecting an empty account queue or unrelated stop files. It reuses original admission/backoff with100reads/minute and600ms pacing. Configuration disables writes and caps pages at1. The command constructs only `Endpoint::History`; no raw endpoint, order, token-mint or trading command is accepted.

Reads require the shared `NETWORK_DISABLED`, shared global `EXECUTION_STOP` and job STOP to be absent, while the live worker's `EXECUTION_STOP` must remain a regular file. Stop symlinks, including dangling network-stop symlinks, block reads. The exact original control hash and scope are rechecked by the SDK authority at its admission/network boundaries. Every write authorization is rejected. Binary bytes must match the plan before executing a read; original secure credential storage is reused without copying its value.

Default invocation is a preflight: it creates no SDK transport, opens no account queue and loads no credential. `--execute-read-only` is separate and still cannot bypass the above conditions. The binary does not remove or archive any stop, activate production or grant itself permission. The data-only network decision is pending human confirmation; historical-data task intent has not silently cleared the existing network stop.

## Exact build and actual VPS preflight

- Source base2ecc54f9 plus isolated binary source SHA256`60d909ebca3e3a8b9fc57d110f6c617f479e136c18ebf7ba6893e1956b845a83`.
- ARM64 release binary SHA256`8f3a464c7499ce0fa5842fc12f79e2b41b5c0694c92889e48d883294c12ef436`; isolated private build, not installed into the live service.
- Six CPU boundary tests pass: preflight isolation; global/job stop handling; execution-stop/control changes; scope/time validation; dangling stop; blocked execution before queue/credential access. SDK/core dependencies are unchanged. Release build passes offline/locked; one existing unrelated SDK warning remains.
- At05:47:40UTC both existing official services are inactive/MainPID0. Preflight against the actual237-exchange control reports `readBoundaryCurrentlyOpen=false`, `networkDisabledPresent=true`, `executionStopPresent=true`, `writesEnabled=false`, zero API calls, zero credential loads and no queue opening. Control/STOP/original financial-journal inode/size/mtime observations match before and after. This is not a new full financial-ledger hash or a live-account reconciliation.

The first isolated build failed because this SDK disables Chrono's clock feature; the corrected code uses the SDK clock. Its source/logs remain preserved and uncredited. A premature upload check also failed its checksum before extraction; the completed reviewed archive was subsequently verified. No failed attempt reached production, credentials or an account client.

Private prepared invocation on the VPS:

```sh
/home/archon/moloch-history-read-probe-20261020-001/target/release/sig-history-read \
  --plan /home/archon/moloch-history-read-probe-20261020-002/probe-plan.json
```

This command has already run in preflight mode. Adding `--execute-read-only` is **not** permission to clear stops. Enabling historical data access must preserve the live execution STOP and all unknown-acceptance/backoff state. A successful probe would permit separately audited historical recovery under the human's data-only authorization; it would not establish an eligible corpus, untouched test, profitable model or trading activation.
