# Rust ARM64 VPS deployment

The deployed service uses native Rust for collection, realtime normalization,
paper accounting, durable recovery and offline replay. No Node runtime is
installed or used by the VPS. Historical `.mjs` files remain reference material
for the original Windows experiments and migration parity tests; they are not
part of the deployed release.

The current release is `~/apps/moloch/current`. Run `~/apps/moloch/check` over SSH
for an offline audit of the disabled deployment and the recovered paper wallet.
The service invokes `current/bin/moloch-collector run`. It is disabled and inactive.
`deployment.json` disables network and trading, `runtime.env` sets
`MOLOCH_NETWORK_ENABLED=0`, and both NETWORK_DISABLED and EXECUTION_STOP are
present. NETWORK_ENABLED is absent. No SIG credentials were transferred.

The Rust collector uses one serialized client and a persistent rolling cap of
100 read admissions/minute with 600ms minimum spacing, endpoint pacing and
Retry-After/backoff. Actual throughput falls below that cap when the server or
network requires it. Realtime-token authentication is separately paced. There
are no contest-order endpoints. One account-wide budget must cover every key
and host; a local OS file lock does not enforce exclusivity across hosts.

The paper bank at `shared/paper-bank` was initialized from the audited closed
historical wallet: 60 fills, 151 retired orders, 99,849.720000 cash and
-150.280000 realized P&L. This is paper accounting, not contest-account cash.
The original expired entry deadline remains unchanged. Recovery checks preserve
the full history and reject source-prefix changes, truncation and policy mismatch.

Available executables in `current/bin`:

- `moloch-ready`: offline disabled-deployment and financial-session audit.
- `moloch-collector run`: guarded paper collection; currently rejects activation.
- `moloch-session ingest DIR SEED_OR_DASH POLICY SOURCE_ID INPUT_JSONL`:
  durable offline ingestion with duplicate-prefix detection.
- `moloch-paper [--seed SNAPSHOT] [--journal] EVENTS POLICY NEW_REPORT`:
  standalone exact-decimal financial replay.
- `moloch-compare JOURNAL SNAPSHOT CONFIG NEW_REPORT`: equal-condition strategy
  diagnostics; optional continuationEpisodes carry each variant's full state
  through later sealed journals without resetting cash, orders or inventory.
- `moloch-source`, `moloch-seed`, `moloch-replay`: receipt normalization,
  legacy-wallet audit and accelerated replay.

See `moloch/native/RUST_MIGRATION.md` and `moloch/results/rust-*.json` for checked
source hashes, test evidence and limitations. Strategy diagnostics are
retrospective and do not establish production profitability. No automatic
activation or entry-deadline renewal follows from a positive paper result.
