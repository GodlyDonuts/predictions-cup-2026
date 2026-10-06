# Opposing-party NO execution work

Buying one Democratic NO and one Republican NO for the exact same ordinary election gives a minimum ordinary payout of one SUSQie: one wins when either party wins, and both win if neither wins. This relies on mutually exclusive Party Winner settlement. Administrative, cancelled, N/A or inconsistent outcomes can break that floor; it is not unconditional arbitrage.

The official read-only Alaska Senate capture at09:58:30UTC priced a one-share package at0.985 including one tick of protection per leg. Its10:30:27UTC successor priced0.990, with93/10 eligible shares under the1% depth fraction. These are expired source observations, not current offers or fills. The two historical capture plans preserve their source hashes, cheap-first order, total cost ceiling and exceptions. No pair orders were sent.

`execution/paired-dry-run.mjs` implements a durable one-pair lifecycle with2 SUSQies allocated, one share per leg, at most1 per order, full two-leg reservation, unknown-acceptance recovery and a second-leg limit derived from the actual first-leg cost. Failed hedges remain attributed orphan inventory. This strict draft uses existing exact-cash/realtime accounting helpers and is not a production writer.

`execution/no-rest-reconciliation.mjs` separately supports the documented side-relative REST NO fill convention. It verifies the original placement ACK, terminal order, complete fill pages, signed position change and complete scoped order history. A resting order's original zero-fill ACK is not rewritten when later fills arrive. Rounded tournament cash is represented by an explicit conservative plus/minus0.01 interval for funding and consistency; exact global cash and unknown global economic activity are not fabricated. Unexplained holdings, extra scoped orders, incomplete pages and pre-order fill clocks reject.

Sixty-five new-module checks passed; independent review reproduced and then verified fixes for truncated positions, missing terminal filled quantities and fill timestamps preceding server order creation. The existing stricter helper is unchanged. [Primary API contract evidence](../research/paired-rest-contract-20261006.json) records the actual public specification observation and exact schema pointers.

The next implementation joins this evidence reducer to the durable REST two-leg lifecycle and the sole collector's existing guarded transport/funding ledger. Passing correctness tests or a positive conditional settlement spread does not establish realized profit or activate a writer.
