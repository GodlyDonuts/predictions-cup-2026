# Moloch permit-expiry incident — October 16

Production automatically stopped at03:33:45UTC after the Paper3 distribution diagnostic scanned historical native orders on the physical dispatch path. The original ordinary q3411 attempt was recorded03:33:28.927; SDK returned PERMIT_EXPIRED_OR_INVALID with no HTTP status/order ID and mayHaveExecuted=false. Its permit expired before admission. This was a production regression; the human directed continuous trading while development proceeds separately.

The incident fix indexes native orders once instead of cloning and scanning them for each historical owned order. The restoration control disables the optional execution distribution and execution model diagnostics; makerPolicy was already off. The fundamental review, ownership, account queue, daily windows,40,000 total/10,000 per-market limits and native financial engine are unchanged. Fresh final seven-receipt preflight now follows forecast reservation activation, addressing the separate stale-book deferral of q3410.

Recovery uses an opaque SDK proof of the exact original request, permit, attempt and local error. It verifies the complete original V1 queue generation, positive last-clock watermark before the attempt, permanent intent/key absence, and quiet global state. It durably archives original SDK sources before recording a distinct aborted_before_admission dispatch event. JSON alone grants no authority; startup reverifies original archives. The original intent remains attempted, cannot be retried, and receives no fabricated ACK or closure. Native retirement additionally requires the unchanged unfilled quote, no physical binding and no acknowledgment. Other unknown outcomes still stop dispatch.

At03:55:04.935UTC the production original proof succeeded, recording dispatch sequence6192 for q3411 with zero network requests and no financial change. The original stopped bank149916 and its nine native identity bridges remain intact. Only SDK-proved no-network attempts are removed from forecast physical-liability gates; tagged pending forecast lifecycle stays exclusive until separately resolved.

Checked on ARM64:6 SDK proof tests,10 dispatch tests,34 worker tests and78 forecast integration tests. These cover genuine original archives, restart revalidation, tampering, unrelated unknown acceptance and forecast liveness. Release build and actual restoration evidence are recorded separately as they complete. Do not interpret these correctness checks as profitability or fresh trading.

Known follow-up: a crash between SDK archive fsync and dispatch-abort append requires an idempotent verified existing-archive recovery path. Current incident completed both steps; no history is overwritten to recover that window. Broader Paper3 research remains incomplete.

[Incident and source milestones](https://github.com/GodlyDonuts/predictions-cup-2026/issues/145).
