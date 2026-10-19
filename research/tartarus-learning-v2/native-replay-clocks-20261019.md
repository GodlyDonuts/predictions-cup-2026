# Original producer time and laptop archive time

The expanded fit tape has a median 6.706 seconds between original book receipt
and laptop archive receipt. Decomposing the original timestamp fields gives a
median 1.854 seconds for producer-to-laptop archive copying and a median 4.152
seconds between original book receipt and producer processing. Their respective
95th percentiles are 4.941 seconds and 24.858 seconds; medians/percentiles are not
additive. [Fit-only decomposition](native-clock-decomposition-001.json).

An archive-copy timestamp is relevant provenance but differs from the original
producer's information time. The compiler now offers an explicit
`producer_processed` clock for a new retrospective development cohort. It keeps
the first laptop receipt and byte-chain identity alongside each selected native
record. Original producer processing must not precede original event receipt,
regress, or occur after laptop receipt. Heldout role filtering still precedes
interpretation of prices. An explicit retrospective protocol is required;
existing receiver-clock cohorts retain their original interpretation.

The separate cohort uses fit 09:00–09:45 UTC and selection 09:50–10:00 UTC on
October 19. These are previously available development periods, not new unseen
test days. It passes the same temporal coverage check, with 36 observed fit SIG
contracts, 232 Kalshi markets, 156 Polymarket markets and 73 distinct official
engine-match prints. Source files were copied from the already sealed expanded
dataset and checked against its prefix hashes; no collector request was added.
[Counts and immutable source identities](producer-clock-development-data-001.json).

This does not prove physical exchange-time matching parity. Producer processing,
original network receipt, publisher time and actual exchange matching time remain
distinct; publisher time can be absent. The compiler's new axis removes archive
copying from one retrospective assumption, not all processing or execution
uncertainty. Calibrating hidden exchange updates and counterfactual API delivery
against original execution evidence remains required before profitability claims.
Phase 011 continues on its original, source-pinned receiver-clock tape as an
engineering control; it has not silently switched data or clocks mid-run.

The combined raw-action, replay, compiler and flow checks pass 106 tests,
including producer-clock selection, retained receiver provenance, preserved
default behavior and exclusion of a heldout book with an intentionally
uninterpretable price. Neither clock choice grants production authority.
