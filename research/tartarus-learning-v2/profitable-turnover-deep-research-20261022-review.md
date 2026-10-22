# Implementation review of the requested ChatGPT research

The human-requested browser research completed October22. Its verbatim Markdown export and SHA provenance are retained alongside this review. This is external analysis, not execution authority or proof of profit.

The actionable recommendation is cash-funded rotation: include the economic cost of selling existing exposure in the whole-plan expected change, execute only the current funded leg, then use actual reconciled proceeds and a fresh analysis for any next leg. This agrees with our earlier finite six-leg comparison; that comparison is not an executable order or evidence of recurring liquidity.

## Corrections before implementation

- The report's 0.21 rotations/minute,80% completion,25bp edge and throughput/profit intervals are stipulated assumptions, not measured opportunities or forecasts. They cannot establish the one-fill/minute requirement.
- Four actual filled orders and18.73 FIFO-realized gain do not calibrate a future edge per unit of turnover. Most orders bought new inventory, while the realized gain belongs to a specific older-cost sale. The immutable funnel audit covers15 POSTs from15:12 through16:16; restoration time must be distinguished from active time.
- Current-price joint scenarios are uncalibrated assumptions. The retained dataset and model builder explicitly state that same-party conditional external mappings do not prove contract/resolution equivalence. The report's exact-contract phrasing cannot override these source facts.
- Server-side net funding for an atomic mixed SELL/BUY bundle remains unproved. Gross-cash admission must not be removed on the strength of pseudocode. Sequential actual-proceeds funding is the directly defensible implementation route.
- The human's fixed train/evolution/independent-test calendar remains authoritative. The report's suggested dates cannot create missing early three-venue data, declare previously inspected data untouched, or relabel an engineering dataset as an independent test.
- The suggested100-SUSQie canary budget,1.5s age bound and20–30bp floor are research suggestions, not current policy changes. Do not add arbitrary new restrictions or renew source clocks from this document.

## Concrete integration boundary

`moloch/native/src/reviewed_rank_campaign.rs` already models funding SELLs, real FIFO, partial-fill failure branches and conditional later legs for rank objectives. It explicitly rejects monetary prerequisites (`RANK_TARGET_GAIN`), requiring a monetary action itself to have positive modeled expected change. This is the precise source boundary for a monetary-rotation extension; changing only worker cash admission would be incomplete.

The extension must evaluate the entire conditional rotation under every declared execution model, expose the first leg's negative or mixed expectation honestly, retain actual-cash admission for the one physical leg, and retain partial-fill/unavailable-replacement branches. Every later physical order needs a new actual account snapshot, original fresh books and a new review. Never credit modeled future proceeds or count a conditional replacement as realized profit. Existing original approval bridges, monetary caps, single worker and unknown acceptance must survive any native identity change.

The file-only preparer/publisher must emit the matching objective and contingent plan; the supervisor must recognize a completed funding leg and reprice residual opportunities. A plan must not repeatedly sell the same inventory on an expired review. Existing cancellation/terminal reconciliation in the worker remains the physical execution path.

Current R97 is already installed and restoring. It should be allowed to recover and provide fresh execution evidence before another replacement; no additional worker or restart was created for this research review.

## Primary-source checks

Direct official pages inspected October22:

- [Markets & Trading](https://sig.thesuper.market/docs/markets-and-trading): ordinary limits can rest; opposite-position purchases can net shares with immediate pair payout credited toward the purchase. This is economically equivalent to closing existing exposure and is not free profit.
- [Changelog](https://sig.thesuper.market/docs/changelog): busy-database responses use503/Retry-After; older unfilled canceled orders can become archived; outcome corrections are separate signed settlement adjustments. These lifecycle rules remain relevant to reconciliation.
- [FAQ](https://sig.thesuper.market/docs/faq): SUSQies are nonredeemable competition points.

The verbatim export retains its original ChatGPT citation identifiers, which are resolved in the linked browser report. Independent primary-source checking and actual source/runtime evidence take precedence over its estimates or pseudocode.
