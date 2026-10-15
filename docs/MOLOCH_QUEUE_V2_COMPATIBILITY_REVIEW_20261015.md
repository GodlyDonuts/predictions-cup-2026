# Queue v2: independent SDK and runtime compatibility review

Task: [#131](https://github.com/GodlyDonuts/predictions-cup-2026/issues/131). This is read-only design review of the existing v1 SDK consumers and the isolated v2 prototype. It authorizes no migration, account request or deployment. The prototype has not replaced the production queue.

## Required consumer changes before any selection

| Existing consumer | Compatibility requirement |
| --- | --- |
| `worker.rs` startup checks `state.unknown.as_object().is_some_and(nonempty)` | This fails open if a bounded summary omits the map. A typed summary must require its schema, scope and authoritative GLOBAL unknown count/boolean; missing/malformed/inconsistent fields must fail closed. |
| `worker.rs` embeds `Transport::admission_state()` in each status file | This currently issues `status`, serializes the full state and then serializes that result again into runtime status. Delta journal records alone do not remove this hot-path allocation. Add a distinct bounded-summary API for ordinary status. |
| `moloch-reconcile.rs` indexes `state.unknown[exact_intent]` | Use a scoped exact-binding lookup returning the immutable binding and exact unknown binding. A bounded preview cannot satisfy reconciliation evidence. Reconciliation must retain permanent intent/key reservations after unknown clearance. |
| `AccountAdmission::configure_reads` directly reads `Store.state.read_profile` and sequence | A v2 wrapper must enforce the identical immutable cap/gap contract, not infer it from a truncated summary. Missing profile on a nonfresh queue must not silently reset or replace the profile. |
| `seed_legacy_headroom` reads clock/rate arrays/active/maps and persists state | Preserve its fresh-queue-only precondition or explicitly reject it for a migrated v2 queue. Never seed an empty replacement history from operational-summary fields. |
| `command_with_maintenance` directly reads directory, poison and last clock | Provide explicit v2 maintenance/poison/clock APIs. Capacity recovery may replay only the rejected local command after durable compaction; it must never replay HTTP or construct a fresh admission for a failed completion. |

The SDK receives `admitted`, `wait_until` and completion `next` fields as dispatch controls. These result shapes need explicit compatibility tests alongside summary migration. Completion error must remain `COMPLETION_DURABILITY_UNCERTAIN`, poison the transport and prevent later HTTP. A summary response must never masquerade as a successful admission or completion.

## Global unknowns and sparse staging

The reviewed prototype uses an actual first unknown binding as a nonempty sentinel plus the command's requested unknown binding. This preserves the current core's global-empty gate while avoiding a clone of every unknown entry per command. It is valid only for the current access set: admission tests GLOBAL emptiness and reconciliation looks up one exact intent. Any future core rule querying all unknowns, their count or a cross-binding relation requires revisiting that projection and differential tests.

The bounded summary should require `summarySchemaVersion`, `globalUnknownCount` and `hasUnknown` with count/boolean agreement, alongside scope and operational state. Those values must describe the full retained state, not a preview. The core write-admission gate must independently remain global even if the SDK validates its summary correctly. Tests must include an unrelated unknown outside the requested binding and removal of one unknown while another remains.

## Error paths, crashes and clocks

The existing core durably poisons manual review on a regressed clock, even though the operation returns an error. A delta implementation must persist this error transition too. Capacity maintenance must compact using an admissible current clock while preserving the ORIGINAL regressed command for its one local replay; replacing its clock would erase the poison. Failed persistence or directory pin remains fatal.

An active write is conservatively unknown after restart even when `network_started` was false. The SDK can crash between durable admission, network fencing, final authority check, request transmission and completion persistence; neither a missing response nor a false local marker permits replacement. The retained write sample, next-admission time and immutable binding must survive. A crashed network read also retains its existing conservative twenty-one-second timeout headroom. Reads and writes share serialization and completion pacing; the v2 delta format cannot turn them into independent burst queues.

The prototype's `--offline-v1-proof` opens the v1 store. Opening may recover and durably append a crashed active request. It is a local/offline operation, but not necessarily a byte-read-only operation. It must run only after the prior worker is verified stopped, and its resulting complete state/source proof must be the actual migration source. An `active:none` state after opening does not prove there was no uncertain write; recovered unknowns remain controlling.

## Downgrade and source continuity

The current v1 store validates its generation guard and requires a schema1 manifest, so a schema2 selected manifest should reject rather than start from the old legacy file. Verify this behavior using every actual supported prior immutable runtime on an isolated queue copy. Do not assume an older binary predating generation guards is protected merely because the current v1 source is.

Migration must bind exact scope, full operational state and permanent intent/key/unknown sets, sequence/head, source journal name/bytes/hash and checkpoint ancestry under the existing exclusive owner. Keep source generations immutable and preserve interrupted-switch failure behavior. A new directory, missing manifest or rollback to a prior checkpoint is not an acceptable compatibility fallback. An agent issue or JSON ownership record does not replace the process/queue lock.

## Memory and performance limits

The reviewed v2 prototype streams checkpoint rows and writes bounded deltas, but still retains complete intent and key indexes in memory. Its two-million-binding count cap and one-GiB checkpoint cap do not establish compatibility with the production service's 512-MiB memory limit. Long intent/key strings and duplicate map-owned binding data increase that risk. Full-state `status` and full-state migration hashing add temporary allocations on top of the resident indexes.

Before SDK selection, measure ARM64 peak RSS for the intended retained-binding horizon, including restore, full proof capture, migration, summary, lookup, compaction and one new write under the real service limit. Establish a retention-safe capacity bound and fail closed before exceeding it; permanent idempotency bindings cannot be silently discarded to meet a memory target. Faster delta appends are useful evidence but are not a bounded-memory guarantee.

No runtime files were changed or migrated in this review. Prototype tests, SDK adaptation, production rollout and actual throughput evidence remain separate milestones.
