# Production rotation prerequisite — October 21, 02:15 UTC

The human requested substantially more trading and accepted greater contest risk. The existing sole production worker remains live. At 02:11:54 UTC PID968525 reported healthy realtime, 396 fills and 1660 orders/terminal orders. These counts did not increase after restoration. No new trade or return is claimed.

The last complete official reconciliation had 403.36 cash and 94,296.23 marked holdings. Approximately 99.6% of marked account wealth was already invested. Increasing buying alone cannot deploy another 70,000; inventory rotation and exits must release capital. Current marked wealth was 94,699.59 versus the original 100,000. The exact original-cost bridge remains in [the restoration report](production-restoration-20261021.md). This is not a claim that the ML model caused those losses.

## Confirmed defect and correction

The existing worker's research snapshot request made four serialized account reads, then failed `TARTARUS_ORIGINAL_PROFILE_CHANGED`. The October15 retained seed was a `/tournaments/midterm-elections` response, whose `id` identifies the tournament. The fresh `/account?tournamentId=...` response identifies the user. Comparing those different namespaces rejected the unchanged account. The original retained owner-capital record independently binds the current profile to this run, bank manifest and tournament. [Retained diagnosis](legacy-seed-profile-diagnostic-20261021.json).

`seed_identity.rs` preserves the original profile-seed path and adds an explicit legacy tournament-seed path. The latter requires the original successful scoped tournament endpoint, matching tournament ID/slug and seed timestamp, exact original cash, and the independent durable owner record's profile/run/manifest/tournament fields. Missing or changed anchors reject the export. The existing caller still verifies the retained receipt digest. The original seed, FIFO costs and ledger are not rewritten.

Two regression tests cover both seed types and negative cases for profile, run, manifest, tournament, slug, cash, clock, endpoint, status and missing independent identity. Both pass against the deployed transport library. The full isolated R57 worker also type checks. The actual new Rust validator successfully checked the original October15 seed receipt against the existing worker's October21 02:13:04 account receipt and durable owner record, with zero network requests. [Actual retained-source check](legacy-seed-profile-retained-check-20261021.json), [check scope](legacy-seed-profile-fix-checks-20261021.json).

The ARM64 release also built successfully, reusing the unchanged native/SDK libraries. Both changed compiled Rust sources match checked GitHub main `4ac9a2f8` after line-ending normalization. [Actual release verification](legacy-seed-profile-release-20261021.json).

This change is an export prerequisite, not a trading strategy or execution approval. It has **not been deployed**. Production remains on checked R57; no additional account client, risk increase, order or fill resulted from these checks. The rank lane separately reports `RANK_CAMPAIGN_NOT_OWNER_APPROVED`: a fresh portfolio campaign still requires its own current source-bound assessment. An expired packet is not reused as a current opportunity.

## Learning and throughput

Sole CUDA learner27732 continues population032. At 02:12:06 it had 16,302 decisions, two completed full two-day worlds and zero new optimizer updates; optimization follows collection of the sealed cohort. No independently profitable checkpoint is established or deployed. Protected October21–25 evaluation data is not used for this training or selection.

Production CPU and storage observations also show a throughput concern: the original worker had used approximately one full CPU core and written 33.27GB since its current start. Available VPS storage was 15.43GB at 02:11 UTC. These are process/storage measurements, not proof that disk or queue serialization alone causes the low order count. The isolated SDK-v2 prototype is not a deployed fix. No live queue files were removed or active history compacted by this task.
