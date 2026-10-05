# Generic dual-NO paper reducer in Rust

This standalone crate ports the exact-receipt v2 paper reducer, review predicate checks, integer cash/FIFO/reservation accounting and reports. Market IDs, state, office, race and contract roots are supplied in the original bank; no Michigan or other state is embedded in the engine. It does not request market data, submit orders, modify frozen banks or replace the active collector.

Build from `backend-rust` so its existing Windows linker configuration applies:

```powershell
cargo build --offline --release --manifest-path dual-no-engine/Cargo.toml
cargo test --offline --manifest-path dual-no-engine/Cargo.toml
```

The executable accepts one JSON request per stdin line and returns one JSON response. Keep one process alive for a cohort rather than spawning per book. Operations are `init` with `bank`; `restore` with `bank,state`; `advance` with `event`; `batch` with chronological `events`; and `report` with `at`. Rejected commands return an `error` and leave the previous valid state intact. Batch changes are committed only when every event succeeds. No durability or ownership is inferred from this in-memory interface: an integrating host must retain its immutable source frontier, bank/binary binding and durable journal before publishing an advance.

Bank validation and its canonical SHA-256 binding are cached in the worker while the bank is privately immutable. Every successful event still checks funded cash, FIFO realized accounting, covered sales, inventory conservation, full two-leg reservations and cumulative purchase limits. All money uses integer micro-SUSQies. Reports keep realized cash, estimated executable liquidation, and conditional ordinary settlement surplus separate. A clock advancing past the election does not book a payout.

Run `node data/benchmark-dual-no-rust-20261005.mjs` at repository root for complete as-recorded state/report comparisons against a sealed v2 paper journal and a same-event chronological throughput comparison. It includes orphan, account exclusion, stale/future account, fractional depth, fixed limit, source boundary and expiry cases. Those constructed cases prove correctness only. Benchmark results are saved to `data/dual-no-rust-benchmark-20261005.json`; compare the actual measured speed rather than assuming a Rust implementation is faster.

The external capacity admission attestation remains an input matching the Node reducer. The canonical book-capacity evaluator, source publication/journal host and official executor are not ported by this crate. Production adoption requires a separately bound continuation that preserves existing inventory and journal identity; this port is currently offline.
