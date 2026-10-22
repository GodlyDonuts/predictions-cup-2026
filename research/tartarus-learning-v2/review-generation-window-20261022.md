# Keep a published review on one source generation

The original1051 review was rejected because the source updater committed another generation between research capture and execution. A manual updater pause allowed the next review to pass ownership, but its later physical packet expired. R103 separately addresses measured preparation costs. This change addresses the generation race without another account-worker restart.

## Coordination behavior

The source updater and checked owner publisher share a local `generation-publication.lock`. This coordinates source/review publication only; it is not an account lock, distributed lease or order capability. Existing financial and physical checks remain authoritative.

Before publishing a managed review, the publisher requires the source updater's existing request to be reconciled. The nominee's generation must match both the source state and the exact worker's current owner status. A proposal that became stale during research is declined **before** owner approval is written. It needs fresh inputs; its old clocks are not renewed.

Before issuing a new source-generation request, the updater checks whether an exact, currently approved managed review uses that generation. It delays the new request only until the earliest original nomination expiry, approval expiry, source deadline or five minutes after the original review timestamp. Public feeds and the account worker continue; no decision, quantity or account-rate limit is added. Expired, unapproved, future-dated and differently bound reviews do not delay updates.

The shared lock covers final source-request issuance and review publication, closing the gap between that check and sending a reload. Already-issued reloads retain their original pending state and reconciliation path. An uncertain reply is not converted into rejection or retried. The protocol coordinates these two cooperating tools; independent tools still face the worker's original generation checks.

## Verification and use

[Exact source hashes and qualification](review-generation-window-qualified-20261022.json) record30 passing VPS checks:8 review/real-POSIX-lock cases,18 updater checks and4 publisher checks. Publisher tests verify that a changed generation, unresolved reload or substituted nomination cannot reach the existing approval-writing path. They mock that path rather than claiming real approvals or fills. The Linux publisher CLI loads with its actual dependencies. A read-only live probe confirmed that the existing expired review does not delay source updates and that its old generation cannot be published against the restoring R103 worker.

The [helper-only deployment](tools/deploy-review-generation-window-009-20261022.py) retains the original four scopes, source clocks, policies, PID/binary binding and refresh interval. It changes only the helper/module paths and source revision; the worker stays running. The installed publisher lives alongside the pinned coordination module and its existing reservation-projection dependency. Use the deployment artifact's `publisherPath` and pass `--source-reference-root` for managed reviews. Older inactive publishers are retained as historical code.

Qualification does not establish a successful new trade. A fresh post-restoration review must still pass the actual original source, account, book, ownership, funding and physical submission checks.
