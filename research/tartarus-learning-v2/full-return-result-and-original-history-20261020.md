# Full-return result and longer original SIG history — October 20

GPU025's first closed TRAIN cohort completed15,809 decisions and committed176 optimizer updates using complete episode return targets. The completed member002 development evaluation remains negative: ordinary marked ROI -12.217655%, stressed -9.01708%,61/29fills respectively, zero realized FIFO profit. This is still the exposed October19 partial-history proxy, with a stale1030mark in the ordinary case, not multi-day daily-growth scoring or independent ROI. See population-025-first-selection-20261020.json. A paired CPU before/after evaluation is running to assess the effect of training on common development cases; the CUDA result alone cannot establish improvement over an earlier candidate with a different seed.

The next inherited candidate continues on the sole GPU learner16052. No production financial state changed.

## Original history discovery

The VPS primary source directory contains175 `breadth-*.jsonl` files, including original captures whose filenames date October15–19 and three prefix artifacts. These span roughly18GB of stored original frame bytes, far beyond the short locally compiled calendar. Sampled records use the official SIG realtime producer format: event/hash/previous/sequence envelopes, receiver timestamps, stream-reset records and markets_batch payloads with books and trades. File names and samples do not establish complete or usable replay coverage.

A new read-only auditor verifies each whole original file's byte hash, row hashes, contiguous sequence/hash links and receiver clocks, then inventories raw book-header occurrences and scope. It quarantines failures and excludes their counts from aggregate evidence. It does not normalize depth, remove duplicates, prove global continuity, reconstruct fills or admit training data. Existing source files, execution controls and account processes are unchanged; it makes zero account requests. The exclusive cutoff is October21,00:00UTC.

The first audit001 incorrectly required a64-character previous hash on initial rows. The native producer starts sequence1 with an empty previous string. Corrected checked sourcef3de91bd matches that convention; five CPU tests pass, including a specific empty-root regression. Audit001 is preserved as superseded diagnostic output, not evidence that the sources are corrupt.

Corrected audit002 runs as VPS PID952701 in `/home/archon/moloch-breadth-inventory-20261020-002`; status and per-file results are under `inventory/`. At the first check,19whole files (1.49GB) had passed with no quarantines. Completion and semantic replay admission remain pending. The auditor source SHA256 is `da1dde923e8583bb14a23ee0da65acf47cd7ebd220df355176d9efcd5930e84b`.

The important next step is to establish exact dates, gaps, source-version consistency and contract lifecycle coverage from this larger corpus, then compile causal replay without loading the whole corpus into memory. Public historical candles/trades still remain information-only. No independent-test prices were used.
