# RestSource immutable-fact copy comparison

Source-only candidate for [task167](https://github.com/GodlyDonuts/predictions-cup-2026/issues/167). Base `1a3e6803f7e13082348a7eb65d2ab296329fcff0`; production change `9db1ace9b2c638119cc5f1dcb68b82c7e88043f1`. No build, benchmark result, deployment, latency improvement or profitability is claimed by this handoff.

`RestSource::accept`, `broadcast` and `seed_trade_facts` stage a transactional clone. The candidate shares only `facts: Arc<BTreeMap<...>>`, using `Arc::make_mut` on a genuinely new fact insertion. Existing facts remain immutable. Scope/book/tape/revision copying, conflict and clock validation, outputs and publication order remain unchanged. A rejected candidate drops its private insertion; no history is pruned. A new-print transaction still copies the map when shared. This does not optimize Engine/history/report work.

`Session::engine_identity` at this base does not include `source.rs`; none of its financial-identity inputs changes. The candidate still needs independent exact-source checks and a reviewed binary rollout. It grants no execution authority.

## Private input

The harness takes one source-hash-pinned private JSON file. Raw trade/receipt/source data stay outside Git. Preserve an external extraction manifest linking the recovered original seed/scopes, receipt/frame bytes and their original source lineage. `provenance` must document reconstruction completeness and explicitly identify synthetic cases; a recovered fact history plus a few priming receipts is not a complete checkpoint of every live cached book.

```json
{
  "schemaVersion": 1,
  "tournamentId": "original tournament",
  "startedAt": "original source start UTC",
  "scopes": [{"marketId": "original", "exchangeId": "original"}],
  "seedEvents": ["original normalized native trade objects"],
  "prime": ["Step objects"],
  "cases": [
    {"name": "book_only", "steps": [], "followup": []},
    {"name": "duplicate_trade", "steps": [], "followup": []},
    {"name": "new_trade", "steps": [], "followup": []},
    {"name": "insert_then_conflict", "steps": [], "followup": [],
     "rollbackProbe": {"exchangeId": "synthetic probe scope", "id": "synthetic new ID"}}
  ],
  "provenance": {
    "syntheticCases": ["new_trade", "insert_then_conflict"],
    "sourceStateCompleteness": "describe original history and recovered prime coverage",
    "originalInputs": ["private original hashes, clocks and extraction references"]
  }
}
```

The strings above describe objects; they are not runnable fixtures. Each Step is one of:

```json
{"kind":"receipt","receipt":{},"expect":"ok"}
```

```json
{"kind":"broadcast","event":"markets_batch","payload":{},"receivedAt":"original UTC","expect":"ok"}
```

`expect` is either `ok` or the exact source error code. Errors do not terminate the case; following steps run against the unchanged published source. Inputs are bounded to256MiB,500,000 seed events,1,000 scopes,4,096 prime steps and64 steps/follow-up steps per case. At least10,000 original seed events are required for the actual-history benchmark. Original scope/receipt/source clocks are never renewed to make a fixture pass.

All four named cases are mandatory:

- `book_only`: original successful reads/broadcasts emit at least one valid book and no trade. Record which transport path was actually exercised. A blocked empty broadcast is insufficient.
- `duplicate_trade`: original successful transactions emit trades whose exact exchange/ID already exists in the original seed or was actually emitted by the parity-checked common prime. Normalizer duplicate/conflict checks still run. `knownKeysAfterPrime` is reported separately; the original seed count is not changed.
- `new_trade`: explicitly synthetic copied transaction emits a previously absent ID. Its follow-up changes that ID's price and must fail `SOURCE_TRADE_FACT_CONFLICT`. The same follow-up must succeed from the untouched baseline, proving successful candidate mutation did not leak to its shared owner.
- `insert_then_conflict`: one explicitly synthetic transaction inserts `rollbackProbe` before a seeded conflicting trade and fails with a fact/clock conflict. For a REST page, the harness obtains the **normalizer's sorted** trade order from original `receipt_events`; merely placing rows earlier in an unsorted page is insufficient. The follow-up must successfully introduce the same probe at a different price. It must match a follow-up from the untouched baseline, proving rollback. Broadcast cases use the original batch shape and ordering.

A common prime must respect each later original receipt's timestamp. Do not prime a later book and then replay an older original trade receipt. REST trades can exercise fact normalization while recovery is blocked; a valid book case must actually complete the genuine recovery conditions. Original delivery revision/sequence/expiry metadata cannot be reconstructed from normalized native books and called original wire data.

## Offline build/run by the execution owner

In a **new private offline project**, copy this exact file as `bench.rs`, the original base source as `old_source.rs`, and the candidate as `new_source.rs`. Do not edit either source for compilation. Keep those files beside the harness; its `#[path]` bindings never accept an input-selected code path. Include `bench.rs` itself for its compiled byte hash. Example manifest (replace only the local dependency path):

```toml
[package]
name = "moloch-source-cow-check"
version = "0.0.0"
edition = "2024"

[[bin]]
name = "source-cow-bench"
path = "bench.rs"

[dependencies]
moloch-replay = { path = "/PRIVATE/CHECKED/moloch/native" }
serde_json = "1"
chrono = { version = "0.4", default-features = false, features = ["std"] }
sha2 = "0.10"
reqwest = { version = "0.12", default-features = false, features = ["blocking", "rustls-tls"] }
```

Resolve only from the existing offline cache; retain the resulting lockfile and exact dependency/source manifest. No network client is constructed or used. With the isolated project/target and no competing compiler:

```sh
CARGO_BUILD_JOBS=1 CARGO_TARGET_DIR=/PRIVATE/OWN-TARGET nice -n 15 cargo test --offline --bin source-cow-bench
CARGO_BUILD_JOBS=1 CARGO_TARGET_DIR=/PRIVATE/OWN-TARGET nice -n 15 cargo build --offline --release --bin source-cow-bench
nice -n 15 /PRIVATE/OWN-TARGET/release/source-cow-bench /PRIVATE/input.json INPUT_SHA256 OLD_SOURCE_SHA256 NEW_SOURCE_SHA256 > /PRIVATE/result.json
```

After the first offline resolution, use `--locked` too. Require successful exit and retain stderr; an empty/partial stdout file is not a result. The test binary contains two new transaction/owner-isolation checks and the unchanged tests included from both source versions; report actual executed counts.

## Measurement and interpretation

The harness seeds/initializes/prime-checks both versions once. For each case it checks exact ordered original outputs/errors and follow-up parity, runs two warm-ups, then20 matched rounds with alternating execution order. Each round clones the same primed baseline **outside** its counted/timed region. It measures the transaction's own clone/normalization/output creation; comparisons, hashing, follow-up, setup and owner/result destruction are excluded for both versions. Results report min/median/max integer nanoseconds, allocation/reallocation calls and requested bytes. Even-count medians are the integer average of the central two observations.

The allocator counts allocation/zeroed-allocation requested sizes and the **new** requested size for each realloc, including failed requests. This is allocation traffic, not live/peak heap, RSS or physical I/O. `factJsonProjectionBytes` serializes first-seen six-field fact records plus keys; it is an auditable payload-size projection, not Rust object-layout or heap measurement. Input/source/harness/provenance and exact output hashes bind each report without publishing raw events.

A material reduction here establishes only this source-adapter component's benefit for the documented recovered state/cases. It excludes Session Engine staging, per-event planning/audit, durable report/hash/serialization/fsync, stream queueing, breadth capture and HTTP. Compare those separately on the same frozen state before attributing a whole-worker delay or promising a production gain. A REST-only book result does not measure the broadcast branch; no missing original WS witness may be fabricated to claim that coverage.
