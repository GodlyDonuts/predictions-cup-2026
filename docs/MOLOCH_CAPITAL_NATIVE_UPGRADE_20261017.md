# Moloch capital/native upgrade handoff — October 17, 2026

This is a bounded execution-owner handoff snapshot, not a new live account observation or deployment approval. The documentation task performed no SSH, account requests, builds, control changes or production actions.

| State | Evidence at handoff |
|---|---|
| Checked **R16 candidate** | Source `7c11cd1c49579deb71ab98afe77114ccc6798f12`; execution owner reports **892 passing checks**, optimized ARM64 binary prefix `fb303ba7`, native identity prefix `7a74f8c3`. These prefixes are abbreviated identifiers, not full artifact hashes. |
| **Current production** | Sole PID **584167**, binary prefix `33005b45`, native identity family `b5`. R16 has **not** been deployed. |
| Financial compatibility | Offline replay of the actual approximately **1.13 GB** financial prefix is **running**, with no result asserted here. Passing source tests/builds do not substitute for this original-prefix check and the required identity bridge. |
| Prospective models | The proposed **eleven-review generation** is separate from both binary validation and production. It is not active at this handoff; a candidate file or model point does not establish approval, current flatness, available depth or a fill. |

The owner-reported production ledger has **265 actual fill rows**, cash **51,175.350000**, owned cost **44,268.335000**, and realized PnL **−136.230000**. The four new BUY logical orders account for **24 fill rows / 28,885 shares / 19,587.500000 principal**: exchange 865 **2,849.240000**, 872 **2,579.220000**, 885 **7,050.000000**, and 993 **7,109.040000**. These are acquisitions, **not new realized profit**; cash, inventory principal and realized PnL remain separate.

## Quantity permission is cumulative; every order remains bounded

R16 permits an explicitly reviewed fundamental cohort `maximumQuantity` up to **1,000,000 shares**. It does not change the current native policy's **10,000-share per-order** limit. A new reservation still takes the minimum of the remaining reviewed quantity, native per-order quantity, genuine depth and available funded principal. Even a one-million-share review cannot bypass these limits or create a one-million-share physical order.

Optional `sameSideTopUps` version 1 permits at most **16 entries in total**, including the original entry. Omission preserves the legacy behavior. Each next entry requires all preceding counted entries to have their original ACK-backed, complete terminal/durable closed evidence; actual native quantities, fills and FIFO lots must agree. No exit may have begun. Mixed/inherited origin, opposite holdings, competing reservations, protected packages and unresolved physical liabilities block this path. Every new entry has a distinct stable qid; an ACK or cancellation ACK alone is not closure.

Principal limits cover **held cohort cost plus reservations plus the proposed new cost**, rather than renewing a fresh budget on every top-up. The current **10,000 principal/market** and effective **95,579.915000 global committed-capital** limits are not increased by this candidate. Reservation also subtracts existing reservations from actual native cash and remaining global commitment. Current account ownership, source/rule review, entry window, fees, rate limits and final physical authority still apply. No borrowing, future settlement proceeds or unrealized gain funds an entry.

Depth is an independent capacity constraint. Current same-side liquidation depth bounds acquisition quantity; `boundedLimitMayRest` additionally bounds it by present acquisition depth through the unchanged limit. This does not promise a same-price exit, future fill or profitable liquidation. At sixteen entries and the present per-order limit, the entry-count limit can bind long before the million-share review ceiling.

## Transient pledge audit and proved no-network lifecycle

The native audit replaces repeated historical order scans for every held wallet key with one temporary accumulation of active SELL pledges and opposite BUY quantities. It retains the original share-pledge equation and wallet-key error ordering, with checked range refusal. **Expired or cancel-pending orders that are not retired still count**; only retired orders are excluded. The map is rebuilt during audit, never persisted as financial state, and neither skips audit/fsync nor releases a reservation. This targets repeated audit work; no maximum-speed or measured production improvement is established by the source change.

Fundamental abandonment of an SDK-proved pre-admission/no-network attempt requires the genuine retained original SDK proof, original intent/attempt/error binding, current-process proof hydration, original native retirement and fresh exact facts. Serialized proof JSON, queue absence, an error string or `mayHaveExecuted=false` alone is insufficient. Contradictory ACK/binding/fill evidence is not erased. Original attempted/error/abort rows remain immutable; the qid remains attempted and **cannot be retried**. A separately eligible future intent does not fabricate an ACK, terminal, fill, cash credit or profit for the abandoned one.

## Original sources and retained validation

One original source may now be up to **16 MiB**, while the **aggregate source bytes per loaded review remain 16 MiB**. Original hashes, held descriptors/path metadata, source clocks, types, contract root and valuation-objective checks remain intact. The complete California General certification is **14,833,345 bytes**; with its selected CTM original and exact node, the review sources total **15,016,579 bytes**. This fits without splitting, extracting or relabeling the original PDF. This size support does not approve the forecast or renew its source time.

Source provenance is the frozen candidate above: quantity/audit change `2aa789d9`, source-bound change `4f9cf863`, and proved-abandonment change `fb0b7bba`, followed by focused fixture corrections through `7c11cd1c`. The owner retains the actual R16 check/build logs and artifact manifests; **892 is the owner's executed-check count, not a documentation rerun**. Preserve earlier failed fixture logs and earlier production/prefix proofs with their original source identities. They are historical evidence, not a claim that the new native identity has replayed the financial prefix. Deployment remains the owner's separate decision after genuine prefix/bridge, artifact, control and model-generation review. None of these changes establishes calibrated forecast probabilities, positive future PnL or guaranteed capital deployment.
