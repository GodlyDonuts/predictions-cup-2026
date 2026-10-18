**Production observation — October 18, 08:35 UTC: R43 is installed; original financial replay is in progress.** Sole PID 778730/startTicks 28213733 matches checked source `67e34365`, binary `fcb092dc`, unchanged native engine `85d4c4c5` and control `205a48b0`. R42 completed replay but stopped at 08:02:47 on a realtime-token request rejection, before fresh trading. R43 narrowly recovers recognised structured token transients through the existing charged retry budget and records redacted endpoint diagnostics. Deployment independently preserved the original financial prefix, all 387 fill tokens/FIFO, original permanent key and quiet SDK state; no bridge, risk or daily-window change. Lossless inactive SDK archival recovered 25.8 GB with decompression/hash verification. New-process fresh trading, additional fills and Tartarus profitability remain unverified. [Checked release and deployment proof](checked-production-lane/r43-token-recovery-and-worker-start.json).

**Production observation — October18,07:33UTC: recovery complete; sole R42 worker started and restoring.** Actual PID773025/startTicks27836490 started07:31:50 on checked source4b1602ab/binarybbd4a497/control034d50e9 with executable and arguments verified. Corrected helper27531c74 reconciled original18533 exposure without retry or invented venue ID. Independent audit verified the full original1,897,831,408-byte financial prefix, all387 exact fill tokens/FIFO/financial fields and permanent key; SDK unknown count0, native574173. The original STOP was archived only after verification. Worker startup replay/current preflight remain; fresh trading and new profits are not yet verified. Controls differ only in the approved binary binding and service paths; daily windows and risk remain. [Original recovery/start proof](checked-production-lane/r42-recovery-and-worker-start.json). Research separately covers139 conditional references and holding-preserving shared-model candidate subsets; no Tartarus campaign or positive whole-account ROI established.

**October18,04:01UTC: the combined managed-continuation release is checked, not deployed.** Source d37fde43, ARM64 binary6b072cef/native eb384dc2 passes native121/library261/ownership5/original fundamental132/worker199 and release. All295 archive members/281 source inputs/274 unchanged parent pins and ELF/native identity are independently verified. [Original verification](checked-production-lane/managed-continuation-build-verification.json); [behavior and remaining work](managed-continuation.md). Current R39 worker742716 remains verified live after its GET503 recovery, with385original fills/FIFO preserved. New-rank financial-prefix replay/upgrade, current official liabilities/depth and a feasible reviewed campaign remain outstanding; no Tartarus profit is established.

# Tartarus production integration: candidate source, not deployed proof

At October 18, 01:44 UTC, migrated worker735621 is verified trading and realtime healthy.
Its actual executable/control and held owner descriptor were verified. R39 progress reports1601 acknowledgments,1598 terminal orders and382 fills. No new fill or profitable Tartarus execution is established. R39 has completed ARM64 qualification and independent artifact verification;
The running release is not evidence that a campaign can win.
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

## Migration evidence and remaining work

- R39 ARM64 qualification and real-journal financial parity are complete; the
  acquisition-continuation follow-up has also passed qualification; deployment
  and fresh-account verification remain separate.
- The exact retained incident is resolved with the source-bound no-HTTP proof.
  The failed intent, original error and permanent key remain; no retry occurred.
- The sole migrated worker is installed and observed restoring. Fresh account
  reconciliation and the queued packet remain pending; dated exports cannot
  substitute for this step.
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
early acquisition tranches cannot satisfy that condition. The checked follow-up adds an
explicit `acquisition_prerequisite`, preserving zero immediate gain plus strictly
positive declared conditional target improvement. Current BUY funding, the first
BUY in cumulative committed/market/count limits, partial loss, unavailable future
liquidity and fresh reapproval remain binding. This follow-up passes13rank/117native/261library/191worker checks and an
optimized ARM64 release; its293-member/279-input artifact is independently
verified. It is not deployed. [Build evidence](checked-production-lane/acquisition-build-verification.json).
Existing fundamental cohort targets remain excluded from the rank lane; ownership
of those cohorts must be reconciled before any broader rotation handoff.

## External reference continuity

The Windows Kalshi collector and supervisor were positively absent after their
last23:30UTC receipt. The existing frozen release was restored at00:59UTC; no
credential was transferred. The same20-read/second limiter and private VPS
loopback route are working. [Observed restoration](checked-production-lane/kalshi-restoration.json)
records actual throughput, original response preservation and the stopped-data
gap. A23-contract reference packet preserves source bytes and receipt times;
all23 REST records lack venue publication timestamps. Settlement equivalence
and pricing authority remain unverified. The Polymarket public collector remains
separate reference data. Neither feed throughput nor a quote is profit evidence.


## Read-only account observation while offers remain open

The actual01:37 request failed before any account read because the allocation packet requires all physical orders to be terminal. Maintaining live offers can therefore starve research. Candidate 826ff09c adds `acquisitionMode: original_account_observation_four_v1` to the same private one-shot request mechanism. It passes195 worker checks and the ARM64 release build; production R39 does not support it yet.

This mode uses the same worker/SDK/shared limiter for Account, Collateral, complete Positions and complete Open Orders. It retains original receipt bytes and timestamps, original request/control bytes, a separately labeled native projection, and known unclosed acknowledgments/intents. Cash and signed quantities must match the original native account, and source clocks, private account generation, control, owner descriptors and unknown-acceptance checks remain binding. No native event is admitted by the capture.

Output uses the distinct schema `tartarus_readonly_account_observation_v1` and directory `tartarus-account-observations`. It declares allocation qualification, open-order ownership reconciliation and execution authority false. Unfilled proceeds are never credited. The strict allocation packet and downstream campaign validators are unchanged; observation data must not be passed off as an admitted financial boundary. Research must model remaining orders/liabilities and reconcile again before any physical action.

Known resting orders now defer a rank allocation before costly preflight reads. An unresolved submission still raises its original fatal error. A cancellation acknowledgment is not terminal evidence. The request budget remains four durable attempts; old markers and the failed original request remain intact. Strict allocation requests wait for quiet without consuming an attempt.

The fundamental controller still owns ten held scopes containing roughly79,421.95 of the dated91,851.73 FIFO principal. This is accounting cost, not current liquidation value. Rank cannot silently take those scopes. Whole-account rotation and any controller transition require original basis/holdings preservation and reconciled outstanding offers; these economic/controller steps remain incomplete.


The controller issue is reproducible in current source: `Cohorts::cohort_lots` rejects any post-origin fill not registered as its own entry or exit (`FUNDAMENTAL_FOREIGN_FILL`); generation reload rejects removal of existing scopes; and production authority pins the required fundamental exchange set. Simply bypassing the rank target exclusion would risk a later accounting stop and competing reacquisition. A proper continuation must register the rank sale against original cohort FIFO and suspend conflicting acquisitions under an explicit owner-approved transition. No such transition has been installed.


## Dated execution diagnosis — October18 approximately01:50UTC

A read-only tail sample of the actual native journal contained46 exchange books, with very different observation ages; it is not a simultaneous all-market snapshot. The corresponding original bytes were retained privately at `/tmp/moloch-tartarus-depth-sample-20261018/books.original.ndjson`. No additional SIG request was made.

Recent original ACKs were SELL977 NO10000@0.95, SELL993 YES10000@0.72 and SELL872 YES6521@0.465, each showing `quantityTraded:0` in that original ACK. Later sample books showed977 NO bid0.92 (derived from YES ask0.08,454 shares),993 YES bid0.715 (4039 shares) and872 YES bid0.45 (936 shares). These are nearby observations, not synchronized order-status/fill evidence; top-level depth does not cover the entire offers. Some other sampled books were over20minutes old and cannot support a current trade.

The samples distinguish pending accounting-profit offers from executable rotation proceeds. They explain why additional ACKs alone need not free capital. Existing positions must be valued against surrendering their future payout and the feasible use of confirmed proceeds. Faster submission does not itself improve that decision. No modeled proceeds, anonymous liquidity or sampled quote is counted as an actual fill or ROI.


The observation/between-scope follow-up is independently verified at source826ff09c. [Build proof](checked-production-lane/observation-build-verification.json) checks all292 archive members and279 source pins, exact unchanged checked R40 inputs outside `worker.rs`,19 focused observation checks,3 rank-worker checks,195 total worker checks, release compilation and actual ARM64 ELF cc2d1cb2/native4fc90d44. The new capture and known-order handling are worker-only; no additional native behavior changed beyond the already checked acquisition prerequisite. These are source/build results, not deployment or a new financial-prefix replay. Production remains on R39. [Live process evidence](checked-production-lane/live-trading-observation.json) independently binds the actual process, executable/control hashes and fresh progress at01:46:45UTC.


## Actual filled existing-strategy exit — October18,01:58UTC

[Original-evidence verification](checked-production-lane/live-fifo-fill-verification.json) records real order46602282 / q18095:10,000 exchange1067 YES shares sold at0.33 at01:52:54.790, split761/600/8639 across three distinct official fill IDs. The complete original receipt was received01:53:31.493. Root independently reconciled the sealed current bank report, all382 unchanged prior fills, the original prior FIFO lots, the complete fill tape, cash and owned-cost changes.3,300 proceeds minus3,113.77 original principal equals186.23 realized gain; fee0. Current cash6,913.785/owned cost88,737.96/total native realized71.83. New-trading realized1.26 and inherited realized70.57 remain separately attributed. Exact original report, original receipt and prior upgrade/FIFO boundary bytes are retained privately under the directory named in the verification.

This is a fundamental-controller exit, not a Tartarus rank campaign. It does not establish current whole-account ROI, or prove that surrendering those shares improves first-place prospects. The newly confirmed cash is real; any replacement still requires current depth, actual funding and a reviewed continuation. Production was not stopped for the source work.
