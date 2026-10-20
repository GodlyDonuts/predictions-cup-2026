# Exact replay diagnosis and verified local history — October 20, 21:50 UTC

GPU027 remains the single GPU learner. Its second training cohort committed 270 full-episode optimizer updates, bringing the run to 21,555 decisions and 342 updates. This is development training on an already exposed October 19 calendar, not independent qualification. The first evaluated member remains negative: ordinary marked ROI -5.3605825%, stress -25.9327%. No positive independent ROI is established.

## Reproducing the original training actions

The CPU diagnostic replayed all 4,495 recorded decisions in GPU027's first eight-world training cohort. Every original observation tensor, clock, reward and end-of-batch financial check passed. It used the original frozen simulator at `7c2e3bcbd255b62236eec5340685cac39bc9b7d5`, the sealed episode manifest and exact original configuration bytes. It ran no policy inference, optimizer or account request.

The first diagnostic attempt failed closed because the saved experiment JSON sorts object keys, while historical Kalshi and Polymarket targets were registered in dictionary insertion order. Identical configuration values therefore produced a different target-row order. Replaying with the exact original raw configuration restored the recorded observation hash. Future registrations now sort mapping entries explicitly; a regression checks reversed dictionary order for both venues. Fourteen historical binding/neural checks pass. The active frozen GPU027 source is unchanged.

The completed fill attribution found 270 fills: 157 with an absent actor book and 113 with an invalid actor book. At fill time, 246 had no cached external reference, 19 had Kalshi and five had Polymarket. Aggregate paper marked P&L across the eight separate worlds was -78,202.3575 SUSQies: -78,152.3675 from fill-time midpoint impacts and -49.99 from the remaining mark changes. These are simulated midpoint quantities, not executable liquidation or realized alpha. They diagnose the initial founder rollout before its optimizer pass; they are not results for the later updated checkpoint. Fill-time cached information also does not establish the precise information used when the original decision was made.

The checked diagnostic additionally counts attempted read endpoints, action/result families, stream state and cached book state before each replayed action. A second CPU replay is measuring these details without changing recorded behavior. Requested reads alone do not establish successful receipts.

SIG's cached October 19 OpenAPI requires REST reconciliation on initialization/reconnect, token refresh, revision gaps and `resyncRequired`, plus periodic reconciliation. The simulator's baseline requirement was therefore retained. More permissive treatment would misrepresent these documented obligations. No action family, model decision domain or realistic API limit was removed in this change.

## Longer original history

All 174 archived original files are now extracted locally and verified against their member SHA256 hashes: 18,408,765,787 original bytes. The offset database and its manifest are also verified. A 100,000-observation / 1,000-trade normalized prefix matches the server output byte-for-byte. The redundant ARM semantic pass was deliberately stopped at 21:38:15.984741 UTC; its partial output and all original files remain preserved.

At the latest recorded local check, 80,000 of 148,181 original envelopes were normalized. This includes 1,993,932 fit books and 944,712 search books, with original gaps retained. Normalization is still running; this partial progress is not a complete dataset or training admission. The source covers October 15–19 with documented empty intervals, not the entire competition. External recorded journals still require their own complete validation and integration.

The independent October 21–25 calendar remains reserved and unopened. The current October 19 development weights cannot become an eligible pre-October-17 training lineage by renaming them. Full historical coverage, complete daily-growth scoring and independently profitable four-day performance remain unfinished.

The existing data-only SIG capture was freshly verified on segment 8, reader 956783. Execution STOP SHA256 remains `1b1b24f1379d9e3a11c13991afd588d085dc16d99c392eda2a3920769c0b2043`. This work submitted no contest order and changed no production financial state.

Evidence: [exact training attribution](population-027-sealed-train-attribution-20261020.json), [local extraction and parity](original-breadth-local-verification-20261020.json), [GPU027 configuration and lineage](population-027-live-20261020.json).
