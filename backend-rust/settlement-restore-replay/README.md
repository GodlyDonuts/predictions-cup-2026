# Offline funded settlement restoration

One native invocation reuses the existing exact Rust portfolio reducer to replay
every original funded checkpoint. It constructs no service, API client, writer,
lease or daemon. It never initializes a bank, replaces a journal, adopts official
holdings, or attaches the verified state to a live host.

Generic V2 restoration uses `portfolio-engine::import_history`. V1 with an
authoritative operator revision must supply the exact original opening, original
base journal prefix, operator journal prefix and original operator code. Its
historical base is verified separately and remains historical. The original
`Overlay` reducer then replays every operator state hash. The utility avoids
redundant reconstruction of an opening already covered by base import evidence;
the complete final public prefix is independently verified and must contain the
exact sequence/hash frontier used by the overlay. Missing overlay information or
an older public prefix fails closed.

Build/test from `backend-rust` with the configured existing toolchain:

```text
cargo test --offline --manifest-path settlement-restore-replay/Cargo.toml --target-dir baal-service/target
cargo build --offline --release --manifest-path settlement-restore-replay/Cargo.toml --target-dir baal-service/target
baal-service/target/release/baal-settlement-restore-replay.exe ABSOLUTE_CONFIG.json
```

All config paths must be absolute. Config has `bank:{path,sha256}`, `bankHash`
(the original insertion-order digest), `journal:{path,bytes,sha256}`,
`publicSource:{path,bytes,sha256}`, `reportAt`, a fresh create-new `output`,
`sourcePins:[{path,sha256}]`, and optional `archivedReport:{path,sha256}`.
The latter requires exact canonical equality of every original wallet report,
including cash, costs, fills, pending reservations, inventory and liquidation.

For authoritative V1 overlays additionally provide:

```json
{
  "overlay": {
    "opening": {"path": "ABSOLUTE_opening.json", "sha256": "64 hex"},
    "operatorJournal": {"path": "ABSOLUTE_operator_journal.jsonl", "bytes": 123, "sha256": "64 hex"},
    "baseJournal": {"path": "ABSOLUTE_original_journal.jsonl", "bytes": 123, "sha256": "64 hex"},
    "operatorCode": {"path": "ABSOLUTE_original_operator.mjs", "sha256": "64 hex"}
  }
}
```

`journal` must equal `overlay.operatorJournal`; `baseJournal` uses the immutable
opening's original byte/hash anchor. Seal the operator prefix before sealing the
public prefix so source progression cannot make the selected public frontier
older than the selected operator state. Append-only source growth after sealing
is not consumed. File replacement and exact prefix/canonical/chain/checkpoint
failures reject the whole result. Source pins and bank bytes are rechecked after
replay; no persisted cache weakens full-source verification.

Measured real KS evidence: 1,490 exact checkpoints on a 127.77 MB journal and
84.43 MB public prefix; original archived wallets matched (99.645 cash and one
356 share at .355 for each correlated latency case). Native restoration took
37.795 seconds with zero per-book subprocesses. The frozen host's captured cold
startup took 294.420 seconds, but its frontier was slightly earlier: the 7.8x
ratio is an operational comparison rather than an identical-head slow-host run.
These are accounting/correctness results, never profitability evidence.

Any future host integration needs separate original-bank/source/head/policy
binding, explicit code-revision evidence, exclusive append ownership, STOP checks
and reconciliation before consuming new events. This utility does none of those
live actions and must not be treated as a runtime activation.
