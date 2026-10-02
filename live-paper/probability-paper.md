# Source-backed probability paper adapter

`probability-paper.mjs` implements a separate research-driven paper policy using the existing delayed snapshot, depth, fixed-point FIFO and risk engine. It makes no API requests and submits no contest orders. Existing ML banks and collector processes are unchanged.

Create a named frozen cohort with `createProbabilityCohort(completeJournalPrefix, config, {name, identities, research, policy})`. The result binds bank/config/research hashes, original source prefix hash, sequence and prospective cutoff. Persist that bank and its initial state separately; advance only with `advanceProbability(state, completeJournal, bank)`. Source changes, bank changes and replaying processed records cannot create duplicate fills. `probabilityReport` reports only the policy wallet, decisions and explicit uncertainty; internal benchmark accounting is excluded from that report.

Each identity maps a market ID to its verified YES exchange ID and SHA256 of the exact settlement predicate. Each research record requires these exact fields:

```text
schemaVersion: 1
id, marketId, exchangeId, side: YES, predicateHash
recordedAt, sourceAt, expiresAt: UTC timestamp strings
lower, upper: six-decimal probability strings between zero and one
method: subjective_range or calibrated_model
settlementReviewed: boolean
flags: predicate_uncertain, settlement_uncertain, source_conflict, superseded
sources: array of {url: public HTTPS reference without credentials/query/fragment,
                    contentHash: SHA256 of the referenced evidence}
```

The frozen record must already be known by the cohort cutoff. No future research revisions are silently inserted into a running cohort; introduce a new named cohort. Research must be known before the snapshot request begins, remain unexpired at receipt, fit the configured maximum source age and exactly match the contract binding. Any unresolved flag or absent review blocks entry. Source links/hashes, predicates, calibration labels and review booleans are supplied attestations; the adapter does not authenticate them.

Policy requires `maxResearchAgeMs` (1 second to 24 hours), `maxHoldMs` (1 second to 7 days) and positive six-decimal `minimumEdge`. Entry selection compares the supplied lower bound with observed ask plus the named fee/tick assumption. This is a settlement expected-value heuristic, not a forecast of a sale price at `maxHoldMs`. Subsequent exit prices can be lower, and horizon/stop-loss exits use sampled bids. No settlement payout or fee schedule is invented.

Pending buys are revoked before delayed fills if research expires, weakens below the edge threshold, changes revision or an account/source outage invalidates eligibility. Exits of held inventory retain existing risk/horizon logic and cannot become buys without renewed evidence. Excluded real holdings/orders cannot create accepted policy books. Insufficient depth or reserve/exposure room can leave an intent unfilled. Open and partially exited inventory remains visible through conservative valuation and FIFO accounting.

This turn adds and tests the adapter; no research cohort has been activated. Authentic current forecasts, exact reviewed predicates and source evidence are required before freezing a prospective cohort. Synthetic correctness tests are separate from any performance claim.
