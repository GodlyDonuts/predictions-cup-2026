# Original-reference entry-cap comparator

Offline only. This tool includes the original median kernel unchanged and
compares two independent one-tick wallets on identical sealed public-book,
account-eligibility and bulk-price source prefixes:

- `one_tick_native_control`: the exact original strict bulk-history kernel.
- `one_tick_original_reference_entry_cap`: after the original kernel creates a
  new BUY proposal, freeze its limit at
  `min(original ask + 0.005, original causal median bid - 0.005)` (also retaining
  the native maximum-price bound). Never recompute that reference at follow-up.

For median 0.500 and original ask 0.495, the second arm reserves 0.495 instead
of 0.500. A later ask of 0.500 cannot fill it; a qualifying later ask of 0.495
can. An improved later price is booked at its actual observed level. Original
3-second request delay, later publication sequence and consumed-version guard,
individual-level quantity at least 100, one-share sizing, 120-second expiry, account eligibility, stops,
targets, holding time and cash/FIFO checks remain unchanged. No trend filter is
applied. Lower caps cannot create a fill at the proposal receipt.

Original proposal admission still uses the conservative ask-plus-tick funding
test. Only an admitted new intent gets the lower reservation; the comparator
does not introduce funding-only admissions. It records every new proposal's
immutable original median, ask, cap, source identity and clocks. A carried
intent, owned lot or exit is never repriced by this adapter. Cases must not be
summed, and absent later evidence is not proof of a real resting-order outcome.
The unchanged kernel may use a later authoritative receipt with the same raw
book version for its first fill; consumed depth cannot fill again on that version.

Build/test from `backend-rust` with the existing local Rust toolchain:

```text
cargo test --offline --manifest-path median-entry-cap-replay/Cargo.toml
cargo build --offline --release --manifest-path median-entry-cap-replay/Cargo.toml
median-entry-cap-replay/target/release/baal-median-entry-cap-replay.exe ABSOLUTE_CONFIG.json
```

The config uses the same sealed-source shape as the causal-v2 trend reader:

```json
{
  "publicSource": {"path": "ABSOLUTE_public-books.jsonl", "bytes": 123, "sha256": "64 hex"},
  "accountSource": {"path": "ABSOLUTE_account-eligibility.jsonl", "bytes": 123, "sha256": "64 hex"},
  "priceSource": {"path": "ABSOLUTE_market-prices.jsonl", "bytes": 123, "sha256": "64 hex"},
  "universeBank": "ABSOLUTE_original-median-bank.json",
  "output": "ABSOLUTE_NEW_result.json",
  "experimentCutoff": "2026-10-06T07:37:20.792Z"
}
```

The registered future cutoff is in
`data/median-entry-cap-experiment-plan-20261006.json`. Earlier rows only prime
history; both future wallets start with 100 cash and no orders, fills or lots.
New proposals require both request and publication strictly after cutoff.
Omitting the cutoff creates a historical development comparison, never a
prospective profitability claim. All hash chains and exact prefixes are
validated; each book selects its own causally available account snapshot.
Output is create-new only. No API client, runtime owner or deployment is added.

Version 2 additionally validates the original bank policy and universe hash,
unique market/exchange mappings, exact sanitized source schemas, independently
monotone account projection clocks, scoped unique exclusion market IDs, and
bulk-price receipt monotonicity, quote values and missing-ID coverage. Account
eligibility contains market IDs only; exchange IDs are checked in bulk quotes
against the bank mapping. Version 1 outputs remain pre-hardening artifacts;
economic parity requires a fresh version 2 run on their identical sealed inputs.
