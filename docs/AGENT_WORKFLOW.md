# Agent collaboration workflow

## Compact inbox — October 12

Start with the body of [issue #10](https://github.com/GodlyDonuts/predictions-cup-2026/issues/10), not the full history of issue #1. The old board remains searchable evidence. Do not use the all-pages fetch_issue_comments connector for routine board polling.

With Node 22+ and an already authenticated GitHub CLI (or existing GH_TOKEN), run from this repository:

```powershell
node coordination/inbox.mjs init --session Rhaegal-task-name --since 2026-10-12T00:00:00Z
node coordination/inbox.mjs poll --session Rhaegal-task-name
node coordination/inbox.mjs show COMMENT_ID --session Rhaegal-task-name
node coordination/inbox.mjs ack COMMENT_ID --session Rhaegal-task-name
```

Choose the initial timestamp explicitly after reading the compact summary and relevant ownership evidence. Earlier history is not acknowledged or scanned automatically. Every chat uses its own session. GH_PATH can point to the installed CLI executable. Never paste tokens into a prompt, file, or command line.

Each successful poll fetches only updated comments since the last check across this repository. Output is capped at eight previews of 800 characters; all remaining unread comments stay queued. Use show for full text and acknowledge exact IDs only after handling them. Acknowledged comments edited later reappear. Failed requests/pages do not advance the cursor. Local session locks prevent simultaneous consumers on one device; they do not lock task scopes across hosts.

Use AGENT_INBOX_DIR to choose a persistent, private, device-local directory outside the repository. Do not share cursors between agents or commit inbox state. State includes comment contents, never credentials. If no changes exist, output is NO_NEW_MESSAGES. Avoid feeding unchanged checks to a model: ordinary software can check without model tokens. No background poller, idle-agent relay, or automatic wakeup is currently active. If CLI authentication is unavailable, read the compact index/task bodies via the connector and report that live incremental checks are unavailable.

Maintain the compact index with current task links and blockers when relevant; omit historical logs. Separate task issues prevent the index becoming another giant conversation.

## Shared board and task threads

[Issue #1](https://github.com/GodlyDonuts/predictions-cup-2026/issues/1) is the team board for agent registration, cross-task dependencies, scope conflicts, and execution handoffs. Use one separate issue per concrete task; keep its questions, decisions, progress, and completion together. Link each task from issue #1. Existing tasks may keep their issue numbers. Read AGENTS.md and docs/COORDINATION.md first; existing execution ownership and direct-main publication rules still apply.

## Identity

Use a stable device/agent name and a unique session ID, for example Rhaegal/20261012-coordination. Prefix every comment with [agent/session]. Multiple concurrent chats on one device must use distinct session IDs. Register in issue #1 with name, session, role, task link, and availability. Rhaegal is the human-selected name for this device; Baal, Drogon, and Daenerys are historical or independently assigned identities, not aliases to silently rewrite. A device name is not execution authority. Keep commit email associated with the human's GitHub account; do not replace it with an invented agent email.

## Claim before editing

1. Read the latest board comments, the task thread, and relevant open tasks.
2. Post CLAIM in the task thread: exact paths or resource scope, expected output, dependencies, base commit if available, and next check-in time in UTC. Link the claim in issue #1.
3. Re-read comments before starting. If claims overlap, request an explicit ACK from the existing owner and agree on a split or named handoff before editing shared scope. For simultaneous conflicting claims, the first posted claim has provisional priority; later claimants pause and resolve the conflict. The first claimant must also recheck for conflicts.
4. Expand scope only after posting the additional paths and resolving any new overlap.
5. Claims are cooperative records, not atomic locks. Silence, a missed check-in, or a stale claim does not authorize taking another agent's scope. Ask for RELEASE/ACK or a direct human reassignment.

Use isolated task checkouts where available. Never share a mutable working directory or branch across simultaneous agents. This local project currently has no .git directory; do not initialize it or copy another agent's uncommitted work as part of setup. Repository policy requires checked direct commits to main, not PRs. Before publication, refresh remote content, preserve unrelated changes, use expected-head/blob checks, and re-read after a conflict instead of overwriting. Serialize overlapping publication through the task thread.

## Messages and status

Use CLAIM, PROGRESS, QUESTION, BLOCKED, HANDOFF, ACK, RELEASE, or DONE. Each message includes agent/session, UTC timestamp, task link, scope, and a concrete next action. Address messages explicitly, for example "To: Baal/session-id"; sharing a GitHub login means an @mention cannot distinguish devices. Do not impersonate another agent or acknowledge on its behalf.

For HANDOFF, include a unique handoff ID, recipient, changed files, commit/source links, checks run, remaining work, and unresolved risks. Recipient ACKs the same ID before taking the scope. DONE links verified commits/results, checks, limitations, and releases the claimed scope. Update the task body status and owner session without deleting its conversation history. Issue #1 keeps cross-task summaries; task threads keep detailed work.

## Reading and delivery

Read the compact index body and unread inbox delta at start, before editing shared scope or publishing, at major milestones, and before completion. Read task comments only for relevant work; never reread the entire historical board. Report blockers promptly. Comments persist but do not wake idle agents automatically. This setup creates no watcher, scheduler, relay, distributed lock, or guarantee of message delivery; running agents must check, and the human may need to resume an idle agent.

## Trading and shared services

Task claims and collaboration messages grant no new account permissions. Existing execution-owner, RELEASE/ACK, reconciliation, execution-stop, and account-budget requirements remain controlling. Do not start another collector or writer, adopt another agent's holdings, or renew an activation through this workflow. Runtime exclusivity still requires an actually enforced shared lock/lease, not GitHub comments.

Never post credentials, tokens, raw account data, or unrelated personal information. Preserve immutable experiment histories and source evidence.
