# Executable-side paired-price diagnostic

The fixed chronological diagnostic uses the captured bulk best bid and ask, never last prices or tournament marks. For two discovered opposing-party winner contracts, quoted acquisition of one NO share per leg is

`C = (1 - bid_1 + tick) + (1 - bid_2 + tick)`.

Quoted sale proceeds are

`L = (1 - ask_1 - tick) + (1 - ask_2 - tick)`.

Both legs must have observed request/receipt clocks within a10-second combined window and20-second age. The fixed screen starts at `C <= 0.980`, waits for both later requests after two seconds, and tests `L - C >= 0.010`, otherwise records the first cofresh quote after30 minutes. Fees are zero. It never treats a stale leg, a current account snapshot, later contract review or an absent depth level as historical execution.

The October5 sealed scan processed17,736 original hash-verified records covering75.863 hours and117 title-discovered pairs in6.239 seconds. It found815 entry quote signals,810 closed quote signals,12 profitable quote exits and798 timeout exits. No pair had positive net closed quote margins; summed margins were−21.405 across overlapping independent screens. This sum is **not an account loss or funded portfolio return**. Bulk quotes contain no depth, sequential fills, actual order acceptance or failed-hedge outcomes. These are exploratory diagnostics, not profitability evidence or a reason to force trades.

The result rejects this specific fast30-minute reversion screen as a candidate for scaling. It does not prove every mean-reversion horizon fails or that holding reviewed pairs through genuine settlement is unprofitable. Conditional ordinary settlement floor and prompt executable sale proceeds answer different questions. For a0.990-cost pair, ordinary payout1 leaves0.010 before exceptional resolution risk; the exceptional zero-payout loss is the full0.990. Calendar time cannot book the ordinary payout. A title match alone does not verify the predicates.

The new real Delaware screen learns both exact roots before fresh adjacent depth20 receipts. At22:40:33UTC its whole packet skew was4.286seconds, cost0.990 with adverse entry ticks, eligible one-percent depths88 and176, and zero fills/orders. The old standalone packet's25.195-second gap was censored. Root expiry must still hold when the final response arrives. This separate screen does not become an ordinary publisher receipt or adopt account holdings.

Next economic work is later exact entry and exit depth, forecast-to-holding-horizon alignment and a capital-constrained untouched-data policy. Keep original frozen experiments and liabilities. Do not repeat tuned historical scores as fresh validation or count correlated latency copies as independent profit.

Run `node data/paired-price-reversion-20261005.mjs` for the accelerated scan and `node --test data/paired-price-reversion-20261005.test.mjs` for quote/clock correctness. The run seals a complete original prefix, verifies the full sequence/hash chain and saves source hash, frontier and all pair results.
