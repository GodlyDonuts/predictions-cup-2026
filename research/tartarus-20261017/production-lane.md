# Tartarus production integration: candidate source, not deployed proof

At October 18, 00:50 UTC, migrated worker735621 is running and restoring.
Its actual executable/control and held owner descriptor were verified. Fresh
trading and profitable Tartarus execution have not been established. R39 has completed ARM64 qualification and independent artifact verification;
it is not evidence that the service has migrated or that a campaign can win.
The original incident was recovered without HTTP at 00:30:45 UTC. Its STOP was archived during the controlled00:42:25 restart. See [build evidence](checked-production-lane/build-verification.json).

## Physical execution path

`moloch/live-runtime/src/worker.rs::rank_opportunity` consumes a separately
owner-approved nomination inside the existing serialized account worker.
It does not create another account client. The nomination is a declared
whole-account rank-risk alternative, separate from the positive-alpha lane.
Negative monetary expectation remains explicit.

The native evaluator covers every held side, original FIFO refund principal,
cash and modeled terminal debt. It records no-fill, partial-fill and complete
first-leg outcomes. A rotation prerequisite can include at most 128 conditional
replacement tranches, each bounded by the physical order-size limit, with
explicit future liquidity and completion assumptions. Those projections grant
no authority to place the replacement orders. Confirmed fills, actual cash and
a fresh review are required for the next physical leg.

The worker validates original contract roots, current collateral and account
facts, current executable depth, native reservation, activation, source clocks,
owner file pins and a process-local physical decision before ordinary SDK
dispatch. Existing STOP, ownership, shared pacing, permanent idempotency and
unknown-acceptance controls still apply. An ACK is not a fill or profit.

## Owner inputs

The optional immutable Control field `rankCampaign` names `nominationPath`,
`ownerApprovalPath` and `originalsDirectory` beneath the runtime root's private
`rank-campaign-owner` directory. The approval independently binds the nomination
raw SHA256, run, account scope and approval deadline. Original sources use
hash-named `.bin` files. Merely writing nomination JSON does not approve it.

The complete nomination schema is documented at the start of
`moloch/live-runtime/src/rank_campaign.rs`. Initial owner-path opt-in during an
engine rollout preserves the original fundamental generation and financial
Control fields. It does not renew entries or change limits. Subsequent input
mutation or approval expiry invalidates the held physical capability.

Eligible ordinary BUY drafts that were never submitted can yield their modeled
cash reservations to this lane. Attempted or acknowledged orders, physical
holdings and managed cohorts cannot be released by that mechanism.

## What still needs evidence

- Complete ARM64 runtime qualification and real-journal financial parity.
- Resolve the exact retained incident with the source-bound no-HTTP proof;
  preserve the failed intent and original error, and never retry that intent.
- Install and observe the sole migrated worker, then reconcile current account
  facts through it. Dated portfolio exports cannot substitute for this step.
- Supply current economically reviewed whole-account scenarios, probability
  assumptions, incomplete-field stresses, debt assumptions and executable
  actions. The retained 23-contract registry leaves missing models explicit.
- Observe actual campaign decisions, official acknowledgments, reconciled fills
  and account returns. Tests, source hashes, hypothetical target hits and trade
  counts do not establish calibrated first-place probability or profitability.

Current integration work and checked evidence are tracked in issue #173;
the original execution incident and source-bound proof are tracked in #174.

## Acquisition continuation follow-up

R39 can review a funding sale but requires direct BUYs to improve immediate
threshold hits. With a10,000-share physical cap and a distant tournament target,
early acquisition tranches cannot satisfy that condition. The follow-up adds an
explicit `acquisition_prerequisite`, preserving zero immediate gain plus strictly
positive declared conditional target improvement. Current BUY funding, the first
BUY in cumulative committed/market/count limits, partial loss, unavailable future
liquidity and fresh reapproval remain binding. This follow-up is not deployed.
Existing fundamental cohort targets remain excluded from the rank lane; ownership
of those cohorts must be reconciled before any broader rotation handoff.
