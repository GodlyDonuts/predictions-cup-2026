# R87 candidate: reuse verified cohort-event offsets

R85 is trading in production. Its first complete15-minute window produced one distinct first-filled order: SELL3,473 YES865 at0.88, with3,056.24 proceeds,3,013.55 original FIFO cost and42.69 realized net. Five receipts at one execution clock belong to that one physical order. Original fills, every new official receipt and remaining FIFO reconcile. The one-filled-order/minute objective remains unmet. [Completed cadence](r85-cadence-window-20261022.json), [financial audit](r85-turnover-window-20261022.json).

Median completed-scope spacing was118.7945seconds over six intervals. This remains too slow; the small, differing scope samples do not establish a general speedup from R83. The same-clock raw/native capture still shows freshness/coverage gaps, without proving a complete trade tape or a missed fill. [Scope measurement](r85-scope-progress-20261022.json), [coverage observations](r85-book-coverage-20261022.json).

One argument-redacted live stack, with a0.475-second attachment and confirmed detach, caught `committed_cohort_event` parsing JSON during cohort refresh. That function reads from the start of the2.69GB journal to find a selected retirement/terminal event. This single sample identifies work on the path, not its total runtime fraction. The pending20937 retirement was subsequently found and archived without financial mutation.

R87 retains a bounded in-memory index of original event offsets for the held Session, manifest and journal file. The first search still reads forward to the requested event, retaining earlier cohort offsets. Later hits reread the selected original row and reverify its exact event kind, order ID, prepared-event digest and membership in the Session's committed source references. Misses continue from the previously checked offset through newly required history or appended rows. The first matching event remains authoritative.

File replacement, truncation, unexpected metadata/head changes, oversized or torn rows, and altered source membership fail. Failed or uncertain scans discard the partial index. The100,000-entry storage bound falls back to the original parser. This cache changes no ledger, intent, queue state or execution authority.

The two new regression checks cover earlier-offset reuse, incremental tail reads, equality with the original parser, unchanged hits, missing events, altered bytes, truncation and inode replacement. All249 worker checks pass. The candidate also includes R86's final-entry projection scope. Native/runtime libraries, sizing, clocks and shared account limits remain unchanged. [Exact source/library/binary qualification](r87-cohort-index-qualified-20261022.json).

R87 is not deployed. R85 is kept operating while the next production change is assessed against actual fill behavior and current opportunity evidence. No whole-account profit or one-filled-order/minute success is claimed.
