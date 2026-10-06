# Native original probability-receipt engine

This Rust library replaces the retained Node economic reducers for `probability-receipt.mjs`, `probability-receipt-v2.mjs` and `probability-receipt-v3.mjs`. The deployed reducer/importer has no Node runtime, subprocess evaluator, API client, credential access or runtime activation. Its scope is the original probability cohorts; dual-NO and funded-settlement engines are separate native components.

The port preserves original bank IDs and opening states, original wallet/run IDs, both correlated1000/3000 ms latency cases, exact integer micro-SUSQie cash/fees/FIFO, delayed depth-limited fills, cumulative funding, pending intent metadata, cooldowns, research/account clocks, source barriers, receipt freshness, latched stop/timeout repricing and derived report fields. It neither adopts official account holdings nor resets a wallet while importing it. Original floating signal diagnostics use the same sequential arithmetic and JS number representation; money never passes through those floats.

The public API is:

```rust
Engine::new(bank_value)?;                       // V1, schema2
Engine::from_opening(bank_value, opening)?;     // V2/V3, schema3/4
Engine::restore(bank_value, verified_state)?;
engine.apply(&event)?;
engine.apply_with_entry_permission(&event, allow_new_entries)?;
engine.state();
engine.report(utc_timestamp)?;
```

`new` requires an opening for continuation schemas instead of inventing fresh capital. `restore` checks bank/config identity and reconstructs cash, fees, turnover, realized basis, quantities and FIFO lots from the entire retained fill ledger; a separately authenticated importer/source pin remains necessary. `state`, `bank`, `mode` and `kind` are available to the embedding service. `canonical`, `digest` and `canonical_hash` reproduce original `JSON.stringify` hashing, including numeric object-key enumeration, insertion order, IEEE parsing with `float_roundtrip`, and ryu-js number formatting. Default serde float parsing was found to change one recorded bank scalar by one ULP and is deliberately not used.

The original checkpoint journal stores hashes/frontiers, not full-state events. A genesis is `{sequence,previous,bankHash,kind:"genesis",stateHash}`. A batch additionally records `availableAt`, sanitized `accountEligibility`, `sourceBytes`, `sourceSequence`, `sourceHash` and `stateHash`. Each batch must be hydrated from the immutable original public-book rows between its source frontiers. A bare checkpoint cannot be used as a market observation.

`apply` accepts `{row,availableAt,accountEligibility}` for one original public record, or a hydrated `{rows:[...],availableAt,accountEligibility,stateHash?}` batch. Failed live reductions restore the prior in-memory state. `apply_with_entry_permission` applies a separate operator entry drain: it suppresses new buys while retaining already-attributed pending buys under their unchanged original research/funding/latency gates, held exits and FIFO. It does not alter bank research, produce fabricated exclusions, renew expiry or grant contest authority. The embedding service must persist the operator permission and actual source receipt separately.

`import::import_history(bank, opening, checkpoint_path, checkpoint_bytes, checkpoint_sha256, public_source_path, public_bytes, public_sha256)` returns `ImportedHistory {engine,evidence}`. It streams the original sealed source once, validates the sorted-canonical public chain, scope, sessions, clocks, book versions/depth and bank-prefix pins, hydrates each original checkpoint, and compares **every** committed original state hash. Original source/checkpoint files remain untouched. The caller must also pin raw bank/opening bytes and any recorded original code graph; opening data contains the original predecessor identity and accounting state and must not be supplied as an unauthenticated cache.

Evidence `sourceSequence/sourceHash/sourceBytes` identifies the last economically consumed legacy checkpoint. `publicSnapshotSequence/publicSnapshotHash` identifies the final verified public row; it can be later. `publicPrefixBytes/publicPrefixSha256` pins the complete sealed capture, while checkpoint byte/SHA/tail-hash fields pin the original economic journal. The embedding service must preserve these different frontiers and either consume a real later receipt or issue an explicit source boundary; it must not pretend merely verified later rows were already traded.

Build/test with `./build.ps1 -Test`. Eight portable checks cover exact money/JS hashing, latency and1% depth, FIFO conservation, entry drain, held take-profit, missing account data, rollback/conflicting observations, and integer fee rounding. These synthetic checks establish correctness only.

`sealed-parity.mjs` is an **offline test oracle**, not a runtime dependency. It loads the actual runtime archived JS source graph, verifies its recorded hashes, seals original checkpoint bytes and a public source prefix, checks every original economic checkpoint, then compares the native state and original JS report exactly. The native importer independently performs full public-source protocol validation. Unrelated book rows in the JS oracle use their exact `lastAvailableAt`-only effect, so timing ratios compare different verification work and are not production speedup claims.

The measured sealed run in `sealed-parity-results.json` matched30,030 original checkpoints over32,593 public rows/71,805,675 bytes:

- Original source-probability native V3:15,591 checkpoints; cash100.040000, realized+0.040000, four original hypothetical fills and flat inventory per latency case.
- Kansas V1:9,544 checkpoints; one1045:YES share at0.345000, cash99.655000 and zero realized profit per case.
- Original Texas V1:4,895 checkpoints; one884:YES share at0.295000, cash99.705000 and zero realized profit per case.

Exact original report hashes also matched for all three cohorts. Native imports took18.42s,11.76s and7.53s respectively, including sealed source validation. Separately, streaming canonical hashing and avoiding redundant import clones reduced the **same** Kansas sealed import from90.085s to11.327s with identical state/report/checkpoint hashes. These are engineering measurements and original local counterfactual positions, not contest fills or proof of a profitable policy. Correlated latency cases must never be summed.

The embedding Rust service must retain source/lease ownership, actual account reconciliation, source publication, immutable bank/opening pins, durable intent/order attribution, official API scope and any separately bounded human activation. This library alone grants none of those. No runtime services, account requests, orders, credentials, banks or original journals were changed during development.

Publish source, Cargo.lock, build script, this README, the offline oracle and concise parity result. Exclude `target/` and `parity-local/`, which contain local binaries and large sealed debugging/import artifacts. The original bank names and economics remain intact even when a new Rust service journal records a separately pinned imported opening.
