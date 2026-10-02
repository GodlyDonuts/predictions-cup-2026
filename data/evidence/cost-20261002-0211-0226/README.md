# Reproducible derived cost audit

This immutable market-only export contains 64 accepted book decisions observed between 02:11 and 02:25:30.455 UTC on October 2. It is an exploratory reproducibility sample, not a profitability experiment. Account records, exclusions, raw responses and credentials are omitted. The original capture and frozen banks remain local and unchanged. This derived dataset cannot independently verify the unshared original journal or its filtering.

Run from the repository root:

```sh
node live-paper/cost-audit.mjs --dataset data/evidence/cost-20261002-0211-0226/dataset.json --config data/evidence/cost-20261002-0211-0226/config.json --compact > reproduced.json
```

The output must match `expected.json` exactly, including its trailing newline. `manifest.json` supplies exact byte hashes for the data, config and output, plus hashes of all transitive code dependencies with CRLF normalized to LF. Use the commit containing this bundle; the manifest's base commit identifies preceding code, while the added audit module is bound by its own hash.

Four scenarios use the same delayed observed prices, sampled depth, quantities, risk limits, FIFO, warmup and censor rules: zero additional explicit fee/zero extra tick, 10 bps fee only, one extra adverse tick only, and both stresses. Zero additional explicit fee is a baseline supported by historical ledger reconciliation for those recorded transactions; future undocumented fees remain uncertain. Observed next-snapshot price movement already occurs in every scenario and is separate from the extra tick penalty.

Each scenario retains all 64 outcomes: 40 warmup, 4 entry unfilled, 4 completed, 9 open exit-censored and 7 entry-censored. All four completed blind probes lost even in the zero-additional-fee baseline. Four correlated completions do not establish that ML signals are unprofitable. These are independent flat-wallet probes using preserved v1 label admission, not actual ML trades; their results cannot be summed as portfolio P&L. Open quantities and unavailable liquidation values remain explicit. This is implementation evidence, not a ranking or trade recommendation.
