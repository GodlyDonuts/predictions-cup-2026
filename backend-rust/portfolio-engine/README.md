# Native funded strategy engines

This crate runs the funded settlement portfolio, original funded settlement V1, canonical dual-NO capacity, and both original dual-NO policies in Rust. Native settlement decisions execute in-process; there is no Node evaluator, subprocess per valuation, JavaScript ledger or network client in the library.

`PortfolioEngine::new(bank)` starts only a fresh independently funded paper wallet. `PortfolioEngine::restore(bank,state)` checks exact funded invariants, but the integrating service must additionally replay the original source and journal to prove attribution. `apply(row,available_at,account)` returns a new state only after the whole transition succeeds. `apply_with_entry_permission(...,false)` prevents new buy intents while retaining previously attributed pending obligations and processing held exits under the original policy. The integrating journal must preserve that permission flag. None of these methods adopts official account holdings or submits orders.

Supported bank modes are `funded_settlement_portfolio_paper_v2` and the separately preserved historical `funded_settlement_value_paper_v1`. V1's original bank hash, named group cap, cumulative budget and report schema stay unchanged. Generic V2 event groups derive from exact reviewed race/stage/state/office/date predicates. Research, account clocks, later-request latency, one-percent depth, one adverse entry/exit tick, zero fees, full cash/FIFO/reservations, event caps and probability-mass coherence remain enforced.

The **separate prospective** `funded_settlement_portfolio_receipt_baseline_v3` bank requires `schemaVersion:3` and `receipt_fill_policy()`: original fill-policy fields plus `fillAdverseTicks:0` and `liquidationAdverseTicks:0`. Native admission, hard order limit, one-tick protective cushion, reserved maximum cost, original independent requests/publication/version/latency gates, one-percent integer depth, cash/event constraints and zero fees remain unchanged. Only later simulated paid prices and quote-based liquidation use observed raw eligible receipt levels. An observed adverse move is charged once through that later price; a price beyond the original fixed limit never fills. A missing/insufficient eligible depth level does not create a share.

V3 reports identify schema3 and the distinct fill model, record raw observed price versus protective order limit/depth, and separate raw/one-tick/two-tick residual liquidation sensitivity. All these liquidation cases are estimated and `realized:false`. Their tick stress is not a platform fee. Research expires at its original time; the historical KS assessment expires **2026-10-06T04:56:38.550Z** and must not be retimed. A new bank needs a new future source cutoff, immutable compiled Rust source identity and independent cash/history. No original V1/V2 fills, prices, journals or bank IDs are rewritten.

Additional bank metadata such as `appendBackend`, `nativeBackend`, `experiment` and a compiled Rust `codeIdentity` is permitted and included in the original primary `digest(bank)`. V3 has a new report schema; clients must not assume it is a V2 report. Point forecasts remain expected-value assumptions rather than confidence bounds or guaranteed near-term gains.

Public API:

```rust
PortfolioEngine::new(bank: serde_json::Value)
PortfolioEngine::restore(bank: Value, state: Value)
engine.apply(row: &Value, available_at: &str, account: Option<&Value>)
engine.apply_with_entry_permission(row, available_at, account, allow_new_entries)
engine.state()
engine.report(at: &str)
engine.bank()
dual_capacity(review: &Value, policy: &Value, books: &[Value], account: Option<&Value>, at: &str)
import_history(bank, checkpoint_path, journal_bytes, journal_sha,
               public_source_path, public_bytes, public_sha)
```

`dual_no` reexports the original standalone exact-receipt V2 Rust library. `dual_v1` preserves the original stressed V1 fill and liquidation semantics, original bank mode/hash and report/source schema. Both expose `create`, `assert_state`, `advance`, and `report` using `Value` inputs. Their ordinary settlement surplus remains conditional and is never realized cash.

`js_stringify` and `canonical_js_stringify` implement JavaScript enumeration: numeric object keys first in numeric order, ordinary keys in their original insertion order, and JavaScript number rendering. `digest` uses those bytes for original portfolio identities and state hashes. `canonical_hash` is a separate sorted-key predicate/dual-bank hash. Never substitute one identity scheme for the other. `forecast_metadata` explicitly distinguishes expected-value points from confidence bounds while preserving original research clocks and experiment metadata.

Build from `backend-rust` so its Windows linker configuration applies:

```powershell
cargo test --offline --manifest-path portfolio-engine/Cargo.toml
cargo build --offline --release --manifest-path portfolio-engine/Cargo.toml
```

The CLI is a persistent offline JSONL worker supporting `init`, `restore`, `advance`, `batch`, `import`, `report`, `capacity`, `dual_init`, `dual_advance`, and `dual_report`. Portfolio advances optionally declare `allowNewEntries`; invalid nonboolean values reject. Errors leave the prior engine state intact. Financial journal durability, account ownership and execution authorization belong to the integrating service.

`import_history` streams and fully verifies both original byte prefixes. It validates the actual native publication scope, canonical record hash chain, clocks, depth and versions, then reproduces every original funded checkpoint from the exact original rows/account snapshots. It supports V1's distinct checkpoint-only genesis/batch envelope and V2's full-state source batches. Evidence separates the wallet's consumed publication frontier from the independently verified public snapshot frontier. It never fills the gap with current account data or adopts official holdings. The integrating service must seed an explicit continuation rather than append a common envelope into V1's different journal format.

For the authoritative value-overlay importer, `import_historical_base_for_overlay(...,opening)` verifies the actual original bank-file SHA, bank identity, exact base journal bytes/SHA, original stats hash, source/financial state anchor and complete wallet report. It returns evidence with `historicalNotCurrent:true` and `authoritativeOverlayApplied:false`. The caller must replay and validate the complete authoritative overlay before exposing current state. This narrow verification path never permits the obsolete base holdings to appear independently.

Run from repository root:

```powershell
node --test backend-rust/portfolio-engine/test.mjs
node backend-rust/portfolio-engine/parity.mjs
node backend-rust/portfolio-engine/dual-v1-parity.mjs
node backend-rust/portfolio-engine/settlement-v1-parity.mjs
node backend-rust/portfolio-engine/import-parity.mjs
```

The parity drivers read sealed originals and their frozen Node graphs solely as independent reference implementations. They verify every original recorded financial checkpoint and complete native state/report outputs. They never modify a current bank, journal, source, runtime configuration or activation. Native correctness fixtures exercise reservations, failed/missing account evidence, depth, clocks, boundaries, event coherence, ownership-independent entry drains and covered exits; constructed profits are correctness assertions, not strategy evidence.

Original source-probability/stepDip families and operator valuation overlays have their own state/journal formats. They are not silently mapped into this portfolio, and remain unsupported until separately ported and verified. In particular, the original Texas base journal is historical: its authoritative value overlay sold the position. The importer rejects that base when an overlay marker **or persistent overlay opening/journal** exists, even if the marker was removed. It cannot resurrect the obsolete held inventory or count the base and overlay as independent wallets. Existing journals/holdings must remain preserved. The active V2 stress policy is unchanged; an exact-receipt alternative requires a separately declared future cohort rather than retroactively changing paid costs or manufacturing profitability.
