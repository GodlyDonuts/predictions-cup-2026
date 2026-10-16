# Actual maker exit-window audit — October16

Read-only research under [task156](https://github.com/GodlyDonuts/predictions-cup-2026/issues/156). The analyzer reads an exact frozen native bank prefix once, verifies every original compact row seal and chain, and reuses the existing actual-economics FIFO analyzer to require exact fills/positions/cash/cost/fee/realized parity. It does not replay the financial engine, query an account, or change policy.

Actual inherited, ordinary and explicitly forecast-tagged inventory remain separate. Exit attribution uses recorded bank order, genuine actual-fill IDs for private deduplication, original execution/observation clocks, and actual native exit reasons. The public output contains only aggregates and source hashes.

The60/180/300/600second labels use the first valid subsequent original book within30seconds of the fixed horizon. Source time must reach the horizon; a late receipt of an old book is excluded. REST request age, source age and observer age must be within10seconds; stream subscription age is not treated as book age. NO liquidation uses the complement of YES asks and checks full depth. Missing depth, missing packets, unobserved fills and end-of-snapshot censoring are separate from zero return.

One-tick opportunities mean displayed full-size liquidation bids at least0.005 above the actual acquisition price after the fill was known. They are hypothetical taker opportunities, not passive maker fills, and cannot be summed across overlapping cohorts that share depth. Public trade rows have no proved aggressor or own-queue identity, so they never create an actual fill or claimed profit.

Run tests with `python -m unittest discover -s research/maker-exit-window-20261016 -p test_analyze.py -v`. The CLI accepts `--boundary ORIGINAL_BOUNDARY_JSON --dispatch ORIGINAL_DISPATCH --breadth FROZEN_BREADTH_JSONL --output AGGREGATE_JSON`; repeat `--breadth` for explicitly retained captures. Use reduced process priority. The input boundary pins the frozen directory, report SHA, bank prefix bytes/hash/head and original dispatch head. No output may overwrite an input.

Actual results and the bounded policy hypothesis will be added after the pinned private-source run; no strategy change is authorized by this report.
