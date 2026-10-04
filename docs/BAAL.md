# Baal

Baal is the human's chosen bot name. The local Next.js dashboard at http://127.0.0.1:3000 displays the official account separately from counterfactual paper wallets. Naming changes do not rename historical strategy banks or change execution ownership.

As of October 4, 00:59 UTC, the maker paper engine had one modeled NO purchase at 0.200 in market 381, no completed exit and zero realized P&L. The join-quote wallet had no fills. This is a local strict price-through model, not a competition fill or evidence of profitability. Added fees are zero; latency, spread, depth and adverse exit prices still affect the economics.

Maker host V3 and core V2 correct two reviewed accounting defects: absence of a qualifying trade requires contiguous fully paginated coverage of the quote's active interval with sufficient projection checkpoints; rejected account/stale books cannot supply liquidation marks. Incomplete tape windows restart from their original first page instead of advancing past missing coverage. The paired admission host considers both actual books only once the coherent pair is available at the consumer clock.

V1 and V2 code, banks and journals are preserved. V3 continues V2's exact wallet cash, fills, inventory and pending quotes through a bank-bound opening-state snapshot and verified predecessor journal prefix. It cancels modeled quote intervals at the transition, clears old marks/coverage and preserves late pre-cancel print handling. Its totals include inherited accounting; predecessor and successor balances must never be added or represented as independent new profits. The original probability wallets also remain unchanged.

The three persistent services are one read-only collector, the original probability paper process and the Next.js dashboard. The collector retains its coordinated 20-read/minute allocation, serialized admission and persistent backoff. Codex competition activation remains disabled; Daenerys remains the contest execution owner under the existing named handoff. No new contest orders were submitted for this milestone.

Validation: 56 focused maker/core/host/tape/frontend/capacity checks; production Next.js build; live cohort, frozen-prefix and dashboard verification. Synthetic accounting cases establish correctness only. Next work is subsequent entry/exit evidence and useful source-backed opportunities, without forcing trades or scaling an unproven strategy.
