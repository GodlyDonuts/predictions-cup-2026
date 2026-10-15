# Bounded queue v2 prototype — October 15

This is an isolated candidate for [task #131](https://github.com/GodlyDonuts/predictions-cup-2026/issues/131), approved by the execution owner after [the measured v1 amplification audit](MOLOCH_QUEUE_SCALABILITY_20261015.md). It does not migrate production or change SDK, worker, financial engine, account ownership, funding or rate limits. The existing v1 `Store` API, reader and command semantics remain available. New entry points select v2 explicitly. Production adoption requires independent review and a separate stopped-owner migration.

## Transition and durability contract

Normal v2 commands construct a small `State` projection with operational fields, the requested intent/key entries, the requested unknown entry and a real first unknown entry when any global unknown exists. Thus a request cannot hide an unrelated unknown from the global admission gate. `State::apply` remains the validation oracle, including its error ordering, clock pruning and manual-review behavior. At most two unknown entries and one entry from each permanent index are cloned. The full permanent maps are never cloned, hashed or serialized for a normal command.

The candidate records the command, resulting operational fields, new permanent bindings and explicit unknown insert/remove operations. It checks the size and tail cap before append. After append and file fsync, it checks the same lease before installing the staged changes in memory. Once append begins, any write, injected fsync-boundary or lease failure poisons the process; it cannot dispatch another command. Restart validates sequence/hash continuity and recomputes each bounded transition through the oracle. A partial final line is refused rather than repaired.

Rejected commands match v1 persistence rules: ordinary errors discard candidate clock/pruning changes; errors on a candidate already in manual review still persist that candidate. Clock regression persists permanent manual review. Unknown outcomes preserve exact immutable bindings. Every normal command reserves one maximum-size recovery row under the tail cap. Restart consumes that reserve to turn an active write into an unknown before any subsequent admission, preserving the original conservative recovery rules. No HTTP operation or retry exists in this module.

Full `status` remains an explicit diagnostic with the original `{state: fullState}` result and the same clock behavior. It is intentionally O(number of bindings). `summary` uses the same clock transition but returns bounded operational fields/counts with mandatory `summarySchemaVersion:1`, `globalUnknownCount` and `hasUnknown`. It is **not shape compatible** with the old full status. A future SDK/worker adapter must validate these typed fields and fail closed if missing; it must not interpret a missing `state.unknown` object as empty. `lookup_intent` uses `request` as the exact intent ID and returns the binding and unknown binding, plus the authoritative global counts. The direct lookup method checks the lease and does not scan the maps.

## Checkpoints and explicit bounds

Checkpoint output streams a header, each permanent binding once, each unknown once and a footer as individually hash-chained JSONL rows. The key index is reconstructed with duplicate and equality checks; it is not repeated on disk. Each row is at most64KiB. The tail retains the existing64MiB bound, with64KiB reserved for recovery. The v2 checkpoint cap is explicitly1GiB with at most2,000,000 permanent bindings; v1 retains its256MiB checkpoint cap. A legal v1 command with exceptionally long fields can exceed the new64KiB row format and is refused without admission. These are explicit prototype format limits, not a claim that every possible v1 input fits.

Streaming bounds serialization memory, not resident history. The two complete permanent indexes remain in memory. At the audit's hypothetical691,298 bindings, their heap footprint may exceed the current service's512MiB process limit even if the checkpoint fits. That process limit has not been changed. Peak RSS and startup/compaction time at the intended retained count, plus a reviewed memory budget, are required before production. The2M-binding bound is a format ceiling, not a demonstrated deployment capacity or twenty-day guarantee.

Checkpoint publication fsyncs the file, creates/syncs an empty successor tail and temporary manifest, verifies the source bytes/hash again, pins all directory entries and the generation guard, then atomically renames and directory-fsyncs the schema2 manifest. Failure poisons the writer. A first guard without its manifest makes both readers fail closed; an older manifest is never implicitly selected. Unix directory fsync is required. Unsupported platforms return `V2_DIRECTORY_FSYNC_UNSUPPORTED`; they do not pretend to provide equivalent durability.

All prior journals/checkpoints remain. No history, binding, archive, key or unknown is deleted, reset or compressed. Startup reads the current streamed checkpoint and bounded delta suffix; the original migration proof is carried through later checkpoints and remains bound while the suffix grows.

## Offline migration interface

Run only with the existing production owner stopped, no concurrent queue owner, and all actual in-flight requests reconciled. The queue's existing exclusive lease is held throughout proof verification and migration. No account client is created.

```text
baal-account-queue --offline-v1-proof DIRECTORY SCOPE TOKEN
baal-account-queue --offline-migrate-v2 DIRECTORY SCOPE TOKEN PROOF_FILE
baal-account-queue --offline-jsonl-v2 DIRECTORY SCOPE TOKEN
```

The first command emits a source proof, not a migration authorization. Preserve it in a private file. The proof binds scope, original head sequence/hash, exact full-state hash, source journal name/length/hash, current checkpoint hash and original manifest hash. Migration requires an exact match and no active request/recovery-compaction condition; it rejects stale or modified proofs. It creates a streamed checkpoint and an empty v2 tail, then pins a schema2 manifest. The unchanged v1 reader rejects that manifest with `MANIFEST_SCOPE_OR_PATH_FAILURE`. It must never be reselected to downgrade. Any future downgrade must export the **current** v2 head and every binding through a separately reviewed path.

The existing v1 open routine conservatively journals crash recovery before returning. Consequently, producing a proof on an unreconciled stopped queue can record conservative unknown recovery; it cannot establish that actual account reconciliation is complete. Review the old head and unknowns first. Wrong/stale proofs on a quiescent queue do not switch the manifest. Interrupted new-generation files remain as evidence. No automated migration/retry from the SDK is present.

The future adapter also needs reviewed typed operations for read-profile configuration, legacy-headroom seeding and maintenance. Existing SDK direct access to v1 `Store.state`, `dir`, `last_clock` and `poisoned` has not been redirected. The current worker/recovery CLI still require an integration change before v2 may be used by them.

## Verification status

Linux ARM64 verification on the exact candidate passed53 tests (30 unchanged v1 checks and23 v2 checks) and a locked offline release build. The build took32.46 seconds; the suite took3.28 seconds. A separate nice10 fixture was used; no production queue or account client was accessed. Tests exercise mixed read/write oracle equivalence, rejected paths and clock poison, both immutable indexes, global unknown blocking, backoff/retry overflow, read profiles, active-write recovery, constant delta size across permanent-map counts, streamed checkpoint integrity, exact migration proof and old-reader refusal, every generation switch stage, directory-pin failure, partial append, source mutation, tail-cap refusal, missing/corrupted/torn files and lease replacement. These establish tested semantics and failure behavior; they do not establish live performance, account profit or complete twenty-day capacity.
