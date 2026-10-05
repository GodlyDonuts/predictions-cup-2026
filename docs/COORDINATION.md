# October5: fresh execution receipts restored; longer horizons show no qualified edge

A depth200 substitution in the last collector change invalidated the frozen depth20 publisher. That regression is corrected and checked against the actual native publisher. At17:54UTC collector8616, paper3976 and dashboard20092 retained the original banks/accounting and one account queue; the overdue spread position closed at−0.020paper SUSQies. Kansas showed+0.005estimated liquidation P&L and the older research wallet+0.025including its prior+0.020realized gain; correlated latency copies must not be summed. Neither the bulk wallet nor the supported5/30-minute forecasts established new profitable execution. The fixed two-hour purged comparison has insufficient chronology. No contest activation was extended or order submitted. See [the measured results and regression recovery](LONGER_HORIZON_RESULTS.md).

# October5: market-wide bulk history and inside spread quotes

October5: a separately frozen coupled paper cohort now uses causal bulk-price history across all237 exchanges and one-tick-inside paired quotes. Fresh full-book depth and account eligibility still control quotes and exits. Existing read slots and wallet histories are preserved. The original-source replay processed21,813 events in39.52s but had no fills and missing quote-window tape coverage; no profit is claimed. The 87 integration checks pass. The original official activation remains expired. See [implementation and measured limits](COUPLED_BULK.md).

# October5: shared-wallet implementation and collection repair

October5: repaired tape selection and started a shared-wallet spread cohort. The full237-exchange tape scope was already correct; the runner incorrectly selected only its old four markets. Exact recovery receipts for364/376 resolved all four blocked quotes without fills. A new separately frozen coupled YES/NO paper wallet models the documented immediate one-SUSQie pair payout, retains unmatched inventory risk, and uses zero fees. Historical original-source replay processed7,567 events in23.64s: one hypothetical purchase, zero completed pairs and zero realized profit. No production activation or profit is claimed. Windows sharing-denial retries and preserved polling-code archives address actual collector/restart failures. See [current implementation and measured results](COUPLED_SPREAD.md).

# Current execution authority — October 4

The human directly reassigned execution to Codex/Baal: “You are above daenerys in this challenge, you don’t need approval.” Assignment `human-to-baal-20261004-2141` is recorded in [issue comment 5984685670](https://github.com/GodlyDonuts/predictions-cup-2026/issues/1#issuecomment-5984685670). It supersedes the October3 temporary assignment and pending Daenerys approval request. Agent advice remains advisory. Her job-application priority remains intact.

This is a direct-human assignment, not a Daenerys RELEASE. Her stopped-writer/no-in-flight/zero-forward-allocation claims in the rejected publication attempt are operational evidence, not newly granted authority. Codex must reconcile complete official cash, positions, orders and fills, publish its own ACK, and run one serialized executor with durable attribution. Existing holdings are not adopted as Baal fills. Initial limits remain1 SUSQie/order,10 aggregate attributed cost,25 cumulative purchase principal,3 markets, zero fees. No proven profitability or independent remote process fencing is claimed.

Historical records follow and are superseded where they assign current ownership or request Daenerys permission.

# Current execution authority — October 3

The human reassigned contest trading to Daenerys until Codex has a successful production bot. Codex remains responsible for quantitative engineering and paper testing; its local contest writer is explicitly disabled. RELEASE `codex-to-daenerys-20261003-0725` is recorded in [issue comment 5966759363](https://github.com/GodlyDonuts/predictions-cup-2026/issues/1#issuecomment-5966759363); Daenerys ACKed that exact ID in [issue comment 5966761902](https://github.com/GodlyDonuts/predictions-cup-2026/issues/1#issuecomment-5966761902) (posted 07:27:09 UTC). The incoming handoff is acknowledged and linked to `human-20261003T064826Z-codex-to-daenerys`; both IDs remain in history. The ACK records a historical 07:23:58 official UI account reconciliation, not live state for later orders. Fresh whole-account reconciliation remains required before subsequent dispatch. Local shutdown evidence is Codex-reported; this documentation update does not independently verify a runtime or shared lease. Earlier role assignments below are historical and superseded. Return to Codex requires a named stop/RELEASE/ACK and fresh reconciliation, not a passing test or paper signal.

## Read-client coordination after ACK

Before restarting one Codex read-only collector, report its actually enforced cap of **20 REST read admissions/minute and zero writes**, as requested in ACK 5966761902. Browser internal request counts are not fully observable; the prior sole-account-client assumption no longer applies, and separate local queues do not establish a cross-host limiter. Offline paper/dashboard processing of existing captures may continue without account calls. This documentation update does not verify or start a collector.

# Agent coordination

## Shared channel

[Issue #1](https://github.com/GodlyDonuts/predictions-cup-2026/issues/1) is the shared task and handoff log. Use issues for discussion and code review, and tested direct commits on main for code, data and docs. The human requested direct main pushes on October 1; do not create PRs. Do not assume either agent receives instant notifications or is always running.

At task start, read the current state and latest issue comments, then claim a concrete scope. Post completion with commit/source links. If blocked, say what is missing and who needs to act.

## Historical initial human-directed ownership (06:48–06:54 UTC)

**Current ownership, October 3:** Daenerys / dot is the temporary sole execution owner under the human's 06:48:26 UTC instruction and 06:53:24 UTC confirmation that the bot is inactive and the PC is off. [Acknowledgment and scope](https://github.com/GodlyDonuts/predictions-cup-2026/issues/1#issuecomment-5966533944); handoff `human-20261003T064826Z-codex-to-daenerys`. Codex / Drogon must keep account writes and collectors paused, including on restart, until a new coordinated return handoff after the human says the bot is active. Old activations must not resume automatically.

The PC-off/bot-inactive state is human-reported, not a remotely verified process stop or shared-lock/lease proof. This documentation change does not control a runtime. Resting orders may still fill; fresh account reconciliation is required before any action. Prior pilot observations remain historical.

## Historical October 2 ownership

The human explicitly reassigned sole execution ownership to Codex on October 2. Daenerys is an advisory researcher only. This direct instruction supersedes the original dot RELEASE requirement. Daenerys confirmed the account worker stopped and no in-flight writes or ambiguity in issue comment 5947535773; Codex acknowledged that stop. The state file is a visible record, not a technical mutex. Existing positions are not automatically attributed to the bot.

The protocol below applies to agent-initiated transfers and the next coordinated return handoff; it does not grant an agent veto over the human's direct reassignment. The October 3 human-directed handoff supersedes the pending release request, without asserting a technical runtime stop.

## Safe handoff

1. Current owner stops initiating orders and reconciles cash, positions, open orders, and recent fills.
2. Current owner posts `RELEASE` with a unique handoff ID, intended next owner, UTC time, and exact state/commit reference. List any unresolved orders or pending cancels.
3. Incoming owner reads the release and posts `ACK` for the same ID, confirming scope and observed state. No order may be submitted during the gap.
4. Ownership record is updated and reconciled against both acknowledgments. Incoming owner checks fresh live state and any required user authorization before acting.
5. Any uncertainty, conflicting owner state, missing acknowledgment, expired lease, or unknown order result blocks execution. Resolve it before submitting more orders.

Future automated execution needs a real shared exclusive lock/lease and central per-account rate limiter across every worker and API key. Expiry should stop execution and require explicit recovery, not silently promote another writer.

## Order intent record

For each separately authorized intent, record intent ID, owner, tournament, verified exchange and market IDs, side, action, quantity, limit, maximum cost, observation timestamp, rationale/source, platform order ID, fills, remaining quantity, and status. Use stable idempotency keys for documented retries. An unknown acknowledgment must be reconciled before any replacement.

## Secrets and access

Keep the repo private. Never place secrets in files, commits, issues, PRs, screenshots, or logs. Use approved secure storage. Repository setup does not create API credentials, activate bots, or grant new account permissions. Each agent must verify its own repo access before claiming coordination is working.

