# Fixed median trend comparison (offline only)

The crate includes `../median-prefix-replay/src/frozen_kernel.rs` unchanged.
It reports three independent funded wallets: the original one-tick prospective
bulk-history control, a 30-minute versus prior 30-minute trend gate, and a
15-minute versus prior 45-minute gate. It does not replace the historical
receipt-only baseline or select a winning alternative.

All retained points must be known before the original book request and within
the hour ending at actual availableAt. The latest eligible point is dropped.
The original minimum 20 points/30-minute span remains. Each trend segment
additionally needs five observations; otherwise new entry is denied with an
unknown-coverage count. Upper medians use timestamp-defined segments, so
uneven sampling changes observation weights rather than interval boundaries.
A decline of at least one .005 tick blocks only a new entry proposal. Pending
buys, delayed executable receipts, cash reservations, owned exits, stop priority,
depth, costs and FIFO accounting remain the original kernel's behavior.

Build/test from `backend-rust` using the existing configured toolchain:

```text
cargo test --offline --manifest-path median-trend-replay/Cargo.toml
cargo build --offline --release --manifest-path median-trend-replay/Cargo.toml
median-trend-replay/target/release/baal-median-trend-replay.exe ABSOLUTE_CONFIG.json
```

Config shape:

```json
{
  "publicSource": {"path": "ABSOLUTE_public-books.jsonl", "bytes": 123, "sha256": "64 hex"},
  "accountSource": {"path": "ABSOLUTE_account-eligibility.jsonl", "bytes": 123, "sha256": "64 hex"},
  "priceSource": {"path": "ABSOLUTE_market-prices.jsonl", "bytes": 123, "sha256": "64 hex"},
  "universeBank": "ABSOLUTE_original-median-bank.json",
  "output": "ABSOLUTE_NEW_result.json",
  "experimentCutoff": "2026-10-06T07:08:27.849Z"
}
```

Omit `experimentCutoff` for a full-window development comparison. Include the
independently preregistered cutoff for subsequent data: earlier source rows prime
history only, all wallets start with 100 and no imported fills/inventory. The
reader hashes exact sealed prefixes, validates original publication/account/
price chains, and never constructs an API client. Output is create-new only.

Results include candidate/rejection counts, actual counterfactual closed losses,
realized PnL, open attributed cost, pending reservations and depth-qualified final
liquidation (null when stale/unknown). Cases must not be summed. Development
profit, if any, is not a selected-strategy or production-profitability claim.
