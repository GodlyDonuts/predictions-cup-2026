# Offline live-prefix copy verifier

`moloch-verify-live-copy ABSOLUTE_FROZEN_COPY` replays a private, independently copied bank using the existing consuming `Session::open_live` path. It creates no API client, events, credits, bridge approval or production lease. Session acquires a **copy-local** writer lock and republishes its projections; success requires their bytes to remain identical. No all-or-none crash guarantee is introduced.

The orchestrator first copies the stopped authoritative boundary and installs its genuine next bridge proposal **on the copy**. This helper never creates or approves a bridge. Existing Session validation checks every original journal/source row and every applicable identity bridge, including financial parity at each bridge boundary. No bank files are deleted, repaired, truncated or reseeded.

Required private marker `.moloch-offline-prefix-copy.json`:

```json
{"kind":"moloch_offline_prefix_copy_v1","copyOnly":true,"accountRequests":0,"originalHead":{},"manifestSha256":"RAW_SHA256","reportSha256":"RAW_SHA256","prefixBytes":0,"prefixSha256":"RAW_SHA256","bridgeSha256":"RAW_SHA256"}
```

`originalHead` is the exact sealed original head object. All hashes cover original raw file bytes; `bridgeSha256` covers `engine-upgrade-v1.json` after the orchestrator installs its genuine proposal. Only same-engine synthetic fixtures may use null with no bridge file. Marker mode must be0600, directory not group/world writable, both owned by the current Unix UID. “Root-created” means the authorized orchestrator, not uid0. The marker is a copy provenance gate, not cryptographic trading authorization or a distributed lock.

The helper rejects relative paths, symlink ancestors/children, production `shared/live-production-v1` ancestry and hardlinked source files. It compares original sealed scope and all financial report, order, fill, source-cursor and contest-binding projections. After dropping Session it rehashes manifest/head/report/events/bridge/marker and requires byte equality. Output is a parity proof only; it cannot authorize execution or installation on the original bank. A hostile concurrent writer outside these ownership guards is out of scope: the operator must retain an immutable private copy and compare its stopped source boundary separately before installing anything.

Source-only handoff initially; no actual frozen-bank replay or timing improvement has yet been measured for this binary. Compiler and synthetic tests will be recorded after the exclusive build slot is available. Native core and engine identity inputs are untouched.
