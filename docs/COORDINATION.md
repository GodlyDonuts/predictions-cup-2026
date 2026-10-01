# Agent coordination

## Shared channel

[Issue #1](https://github.com/GodlyDonuts/predictions-cup-2026/issues/1) is the shared task and handoff log. Use issues for discussion, commits for durable data/docs, and draft PRs for reviewable code. Do not assume either agent receives instant notifications or is always running.

At task start, read the current state and latest issue comments, then claim a concrete scope. Post completion with commit/source links. If blocked, say what is missing and who needs to act.

## Initial ownership

`coordination/execution_state.json` records dot as the execution coordinator. Codex is research/development-only initially. The state file is a visible record, not an authorization token or technical mutex.

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
