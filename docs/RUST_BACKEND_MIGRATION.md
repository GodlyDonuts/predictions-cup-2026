# Rust backend migration — October 5

Three isolated Rust components are implemented and checked. They are not a replacement for the complete live backend yet. The active collector, authenticated API response validation, source publication, frozen strategy hosts and dashboard retain their existing bindings.

| Component | Ported behavior | Checked evidence |
| --- | --- | --- |
| `backend-rust/dual-no-engine` | Generic exact-receipt paired-NO reducer, bank predicates, integer funded cash/FIFO/reservations and reports; one persistent JSONL worker | 405 actual sealed journal records; 809 complete state/report comparisons match Node. Ten additional adverse policy continuations match. Twenty same-history replays: Node 2.822 seconds, Rust 1.845 seconds including process and serialization, 1.53×. |
| `backend-rust/account-queue` | Durable serialized admission, completion pacing, shared rolling caps, Retry-After, scoped lock and immutable intent bindings | 18 native tests and four offline comparisons against the actual Node ReadClient. Rust deliberately enforces the inclusive 60/24 admission boundary more conservatively. No HTTP or credential handling is ported. |
| `backend-rust/journal-stream-v2` | One-pass original source SHA and canonical chain verification, bounded memory, persistent worker, Windows snapshot locking and file identity checks | Five native and seven Node checks. Actual 8.806 GB sealed source: original hash-plus-native restore 29.411 seconds; new one-pass 26.963 seconds, 1.09×. Source digest, chain tail and last-state bytes match. |

Sources are generic by supplied bank and scope. State/market selection is still configuration and research input. No state-specific trading rule is embedded in the new dual-NO engine. Existing deployment scopes remain narrow; this does not claim automated whole-market coverage.

These are component performance comparisons, not a measured percentage of the live backend or an end-to-end order latency benchmark. The journal gain is modest. Serialized API admission and remote response times remain external constraints. Temporary sealed benchmark copies were deleted; original production journals and frozen banks were not rewritten.

All constructed adverse cases are correctness evidence only. Actual paired paper replay still reports zero realized profit. Ordinary conditional settlement surplus remains separate from liquidatable cash; calendar time does not manufacture settlement. No contest activation or order is introduced by building these components.

Next integration: bind long-lived workers to exact binary/source hashes, original bank and journal frontiers; verify replacement, rollback, malformed-output and crash behavior; preserve current owner/STOP/activation and account reconciliation validators. An adapter must compare original source/prefix/tail identities before restoring an append interface. Admission booleans from the CLI are not authority and must come from existing trusted validators. Compiled binaries, journals and `target/` stay out of the repository.
