# Causal funded V6/V7 replay entry point

This runner compares the frozen original wallet and the separate inventory-risk candidate on **identical original rule/book receipts**, in recorded chronological order. It reports cash, realized results, depth-aware liquidation marks, remaining inventory/reservations and exact capital-time integrals. It does not pick parameters, update source probabilities, create synthetic observation clocks, collect market data, or activate trading.

The current local evidence contains the historical Maine bank and a derived wallet projection, but not its original full receipt chronology. `availability.json` records that limitation. No real-data policy performance is claimed. Checkpoints retaining only a wallet and source hash cannot reconstruct the original rules receipt and are rejected.

## Original receipt input

Use JSONL with one full `{book,rules}` sample per line, or `{sample:{book,rules}}`. Preserve original API request/response clocks, exact scope, source version, price ladders and rule root. Do not insert guessed clocks or generated market snapshots. Rows must be on or after the bank's original cutoff and chronologically ordered. Same-version conflicting depth is rejected. Source provenance is recorded as supplied evidence and remains independently unverified by this offline tool.

The runner verifies the bank hash and its complete frozen code manifest before execution. On Windows, it may reconstruct LF source bytes from CRLF files **only when the reconstructed bytes match the exact recorded SHA256**. It materializes those verified bytes in an isolated temporary tree; neither bank nor worktree source is modified. Any content mismatch fails. The new V7 source is separately hashed in the candidate bank and result.

```powershell
node research/viserion-causal-replay-20261012/replay.mjs --bank data/maine-passive-harvest-paper-20261012-bank.json --receipts ORIGINAL-RECEIPTS.jsonl --holding-ms DECLARED_MILLISECONDS --adverse-ticks DECLARED_TICKS --output REPLAY-RESULT.json
node --test research/viserion-causal-replay-20261012/replay.test.mjs
```

Declare the holding horizon and adverse threshold before examining outcomes. The example placeholders intentionally do not nominate live settings. Each policy starts at the original bank boundary, fully funded, with its own custody/depth/accounting state. No future reference-value revisions are imported.

Capital-hours integrate prior owned principal and buy reservations over elapsed recorded time; exact integer micro-SUSQie-milliseconds and share-milliseconds are included. Decimal capital-hour displays floor to six places. Integration ends at the final original receipt; it does not assume unresolved inventory becomes flat afterward. Liquidation marks stay null when post-entry evidence is missing or depth is incomplete.

Six checks cover exact frozen-byte restoration/content mismatch, original-clock chronology, same-version depth conflict, streamed funded losses/capital-time integrals, rejection of missing/checkpoint-only evidence, and full CLI frozen-prefix/output-preservation behavior. The CLI hashes and reads a bounded original byte prefix, then rechecks its hash before writing a new output file. It refuses to overwrite evidence. The test inputs are expressly synthetic mechanism fixtures, not market results. No network requests, orders or automatic source-file exports occur.

Next: obtain the full original matched receipt archive from Rhaegal through approved coordination, run both policies without tuning on that sample, and freeze any prospective comparison under a separately declared bank. The existing consumer records source hashes and wallet checkpoints; those alone do not establish a reproducible receipt archive.
