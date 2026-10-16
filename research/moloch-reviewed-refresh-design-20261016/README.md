# Reviewed-policy refresh without replaying the financial bank

Design only, source `90fbd456`, October16. No code, review, Control, process,
account client or live state was changed. PID534982 was reported restoring by
the execution owner; this document does not independently verify fresh trading.

The smallest useful first implementation updates reviews of **already managed
exchanges only**, on the existing Session/Transport thread between operations.
It keeps the original Control and ProductionAuthority pin, appends one durable
fundamental-policy generation, then replaces the in-memory review authority.
It requires one checked implementation rollout initially; subsequent approved
forecast revisions need not reopen/replay the native financial journal.

## Why editing the existing files is wrong

`worker.rs:1576–1581,1624–1626` loads Control and fundamental Settings once, then
opens the existing sidecar and binds managed exchanges. There is no run-loop
Settings loader. `fundamental_policy.rs:476–506` holds immutable path/inode/size/
hash FilePins for ReviewAuthority. Rewriting that authority fails validation;
editing a review path does not install a new review in the running policy map.

`physical_decision():877–903` freezes review hash, trusted-set hash, original
authority descriptor, exact sidecar tail, bank/head/quote/ledger/request/permit,
scope, source TTLs and owner. `PhysicalDecision::authorize():589–607` rechecks
the held authority and sidecar pins. A stale token must not survive a policy
generation change. Do not relax FilePin, mutate a held authority, reload Control,
or swap trust after an intent/attempt has already begun.

Existing `Cohorts::install():1128–1152` and `apply(review):959–965` already check
run/bank, strictly newer review time, exact exchange/market/contract/tournament/
side/origin and current review validity. A filled/existing cohort cannot extend
its original hold or change objective horizon. Its original entry binding/hash,
FIFO basis, first-fill index, archive and exit bindings remain historical.
Existing pending entries can require cancellation under a new review; a refresh
does not make them new intents or rearm a cohort.

## Approval boundary: a required input, not a JSON claim

Use one optional immutable bootstrap setting, omitted by default, for a
`reviewRefresh` channel: fixed absolute inbox/approval locations, protocol
version, initial settings/authority digests, allowed existing exchange set and
strict file/size/count bounds. Omission preserves current behavior and review
serialization. This setting authorizes the **mechanism**, not every future hash.

For every generation the execution owner must separately approve the exact
descriptor and new review allowlist. Merely finding a pending file, a self hash,
`executionOwnerReviewed:true`, a source hash, calibration flag or increasing
generation is not proof of that approval. Do not let forecast collection, model
exporters, the worker or other source producers publish approval records.

The current deployment uses the same Unix account for operator and worker.
Private file permissions and SHA pins detect changes but do not cryptographically
distinguish those actors. A claimed root-owned directory must not be presented
as UID0 separation: here “root” means the execution owner/orchestrator. Reuse an
existing independently authenticated operator approval facility if one is
actually available. Otherwise document the manual operator publication trust
boundary explicitly and require the owner to approve/publish each exact
generation; do not claim an autonomous self-approval-resistant signing system.
Adding a signing credential or a separate privileged approval service is a
separate authorization/implementation decision, not a hidden dependency created
by this design. If independent approval cannot be established under the selected
threat model, keep restart-based review deployment; hashes alone cannot solve it.

## Exact immutable transition packet

One new descriptor names version, unique update ID, generation+1,
expected-current generation/settings/authority digests, original Control hash,
run/bank/account-scope/tournament identity, new Settings digest and absolute
immutable artifact path/hash/length references. It includes the exact new
authority allowlist, review/source files, proposal time, application deadline,
and the separately approved descriptor digest/approval witness reference.

Define Settings canonicalization explicitly; do not confuse sorted JSON Value
hashing with Rust typed `ReviewInput` serialization. Use actual Rust
`review_sha256()` for review allowlist entries. Preserve all previous approved
hashes for attribution; revocation is an explicit current review field or
separate acquisition deny, not deletion of history. A hash remaining in a
historical allowlist does not make its expired/superseded review current.

The first version forbids scope additions/removals, side/origin/contract changes,
new initialization/adoption/rearm, financial limits above the separately
approved descriptor caps, native policy changes, source-clock renewal and
cohort horizon extension. Forecast p, exception assumptions, reviewed risk caps,
revocation and supported execution mode changes are allowed only as explicitly
approved review revisions. Existing required source kinds/original byte hashes,
actual source lifetimes and objective-specific timing/deadline model checks all
remain. This is not permission to manufacture current observations.

## Serialized application and durability

1. At a bounded run-loop checkpoint before dispatch, inspect at most one pending
   descriptor. Check Control/STOP/owner and actual Transport quiet state: no
   active HTTP, unknown, poison or manual review; ordinary/package unresolved
   attempts must be resolved. No background thread, new client or queue command.
   Do not interleave refresh with any existing prepared request, activation wait,
   pending physical token or SDK call. The sole worker owns this checkpoint.
2. Clear `ProductionAuthority::ready` and discard ephemeral fundamental hints,
   plans and tokens. Existing durable intents remain unchanged. Capture current
   fundamental generation/tail and bank/dispatch identities. Load all bounded
   immutable files once through existing descriptor pins. Verify independent
   approval, expected generation, current scope, allowlist, every review/source
   and all existing revision constraints against a staged sidecar policy view.
   No official HTTP is necessary to install an assumption; execution still
   needs its later genuine seven-fact/current-root/account/book proof.
3. Recheck owner/STOP/quiet state and original current generation/tail. Append
   **one** fsynced fundamental sidecar `review_generation_v1` row containing the
   descriptor/approval/source references, new Settings/authority digest, and
   complete verified review revisions. Validate the entire candidate before
   writing. Do not call `install()` repeatedly for several reviews and describe
   that as an atomic generation: a crash could expose only a prefix.
4. After durable success only, replace the in-memory ReviewAuthority and current
   policy view together. Acquire a new pin on the same sidecar descriptor/tail;
   old held file bytes are never overwritten. Append/fsync an acknowledgment
   naming the applied descriptor/generation, without deleting the proposal.
   If acknowledgment alone fails, the durable generation remains authoritative;
   restart recognizes it exactly, never reapplies or rolls back the generation.
5. Resume ordinary reconciliation and existing policy-driven cancellations/
   covered exits. New entries still need fresh account/contract/book facts,
   exact current policy/token and original reservation/intent/physical checks.
   Refresh must not itself submit, cancel, release funding, create a native
   order, remove a STOP, erase an unknown, or retry an old logical key.

Invalid/unapproved/expired pending descriptors produce bounded rejection
diagnostics with no policy change; current policy continues its existing expiry
semantics. A sidecar I/O/possible partial-write failure poisons the policy writer
and fails closed through the existing durable-failure path. Never silently fall
back to an old generation after a possibly committed update.

## Restart and historical proof

On startup, original Control Settings are the **bootstrap**, not instructions to
reinstall old reviews over a later recorded generation. Replay the fundamental
sidecar, verify its accepted generation chain and original approval/artifacts,
then load the latest committed authority/reviews. Otherwise today's
`open_inner()` can attempt an older still-current bootstrap review and correctly
reject it as a non-increasing revision. Missing/mutated accepted-generation
artifacts block that restoration; do not infer approval from the latest file in
a directory, reset the sidecar or downgrade to generation0.

Old physical intents remain bound to their original review and source hashes.
Restoring genuine old ACKs/fills/terminal evidence must still use the existing
original log/archived binding paths; a current review revision cannot rewrite
their attribution or reject valid venue accounting just to preserve eligibility.
After crash-before-commit, old generation remains current if the existing
durability contract proves that state. Crash-after-commit-before-swap restores
the new generation. Torn tails retain the existing fail-closed recovery policy;
no automatic truncation or replay of a financial action.

## Concrete implementation split and limits

- Add a small `review_refresh.rs` descriptor/approval loader and opaque
  `PreparedRefresh`, no authority from caller JSON. Expose `prepare(current,...)
  -> PreparedRefresh` and `apply_at_checkpoint(...) -> AppliedGeneration`.
- Add an optional omitted Settings refresh bootstrap plus a fundamental-sidecar
  generation record/staged validator and active-authority restoration. Reuse
  existing original-source/revision validation; do not relax old record parsing.
- Add one worker checkpoint, preferably before due reconciliation/dispatch with
  no local prepared context. Keep Control/ProductionAuthority native/financial
  identity unchanged. No native Session or financial journal replay is required
  for an already-managed review update.

Do not add a new managed exchange in this first patch. Current startup also
calls `require_fundamental_exchanges` and installs source/control-bound native
managed-entry exclusions (`worker.rs:406+,1624+`). That transition, competing
ordinary quotes, attribution and package protection need their own checked
integration. A smaller refresh of existing NC/DE/MA/OR forecasts avoids those
scope changes. A schema/strategy/native-engine change still requires normal
checked rollout; a mailbox cannot make a new executable safe.

Meaningful checks: genuine source/allowlist approval and forged witness denial;
wrong expected generation/run/bank/Control; revoked/expired original source;
same source body with renewed clock; old token after switch; active HTTP/unknown/
STOP; no intent rewrite; pending old ACK and later fills; filled cohort hold/
objective unchanged; multi-review atomic record; crash at each write/swap/ack
boundary; duplicate accepted descriptor; restart bootstrap older than accepted
generation; missing historical approval/artifacts; omitted-option old behavior.
No checks, runtime speedup or authenticated approval facility are claimed
implemented by this design. It removes the reason to replay a financial bank
for an assumption revision only after those pieces are genuinely checked.
