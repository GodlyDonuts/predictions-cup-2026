# Typed queue v2 SDK adapter — October 15

[Task #138](https://github.com/GodlyDonuts/predictions-cup-2026/issues/138) adapts the SDK to the checked [isolated v2 prototype](MOLOCH_QUEUE_V2_PROTOTYPE_20261015.md). This is source under review, not a production migration. No worker, authority, financial engine, bank, credential, account process or production queue was changed.

`AccountAdmission` keeps a strict v1/v2 backend enum. An empty directory and an existing schema1 generation continue through the original v1 reader. Only an existing schema2 manifest selects the v2 reader, which then checks the generation guard, scope, checkpoint chain, migration ancestry, active tail and exclusive lease. Detection never creates or migrates a v2 queue. Missing, malformed, unsupported or changed selection fails; there is no fallback from a selected v2 failure to an older journal or new directory. The original v1 reader still rejects schema2.

## Public contract

| API | Behavior |
|---|---|
| `Transport::admission_summary()` / `AccountAdmission::summary()` | Typed bounded snapshot of the last durable state, including mandatory `summarySchemaVersion`, authoritative `globalUnknownCount` and `hasUnknown`. No full permanent-map clone/serialization or added queue event. |
| `Transport::admission_lookup_intent(id)` / `AccountAdmission::lookup_intent(id)` | One exact immutable binding and one exact unknown binding, with the global count/boolean. v2 uses the core lookup; v1 reads only the requested entries. |
| Legacy `Transport::admission_state()` / raw `status` | Original v1 result remains available. On v2, returns `V2_TYPED_SUMMARY_REQUIRED`. It never manufactures empty maps or presents a summary as full state. |
| `Transport::admission_diagnostic_state()` / raw `diagnostic_status` | Explicit O(N) full-state diagnostic for either backend, using the original status clock transition. |
| `configure_reads(cap,gap)` | v1 retains fresh-only setup. A migrated v2 queue must already contain the identical immutable profile; mismatch or absent profile fails without resetting it. |
| `seed_legacy_headroom` | Preserves the original fresh-v1-only boundary. Explicitly migrated v2 queues cannot be reseeded. |

Typed summaries require the schema, scope, head, explicit active/profile fields, permanent counts and global uncertainty fields. Count/boolean agreement and both permanent-index counts are checked. An empty exact lookup says nothing about other unknowns. Exact unknown reconciliation retains the permanent intent/key reservations after clearing its one unknown.

Normal Transport requests inspect the typed snapshot; writes also use the exact intent lookup. Core admission remains responsible for the original clock/manual-review/active/global-unknown/immutable-key error ordering. An early SDK duplicate/unknown rejection must not hide a regressed clock before its durable manual-review transition. The independent SDK global-unknown guard also prevents physical dispatch if admission ever contradicts the authoritative nonempty global snapshot. Reconciliation verifies the supplied binding against the exact unknown lookup and global count before invoking existing authority and the durable core command. Reads needed for reconciliation remain possible while unknown writes block new submissions.

The snapshot timestamp is `state.last_clock`, the last durable operation. Querying the new summary/lookup does not advance that clock or prune rate arrays. Admission, network, completion and explicit diagnostics retain their durable clock behavior. v1 still writes its existing full snapshots for those queue commands; v2 uses bounded deltas. No idempotency key is evicted and no active/unknown state is replaced from summary fields.

## Maintenance and integration boundary

Capacity maintenance still retries only the identical rejected **local** queue command once after preserving compaction. It recognizes only `EXPLICIT_COMPACTION_REQUIRED` and the v1 recovery-cap condition. It uses an admissible compaction clock while preserving the original command and its clock. Directory-pin failure poisons the selected backend before replay. Review/lease/format/poison failures do not trigger maintenance or HTTP retry. v2 compaction additionally retains its core source/manifest fences and pins. Visibility changes in the core expose only existing ownership/summary/poison controls and the cap field used by synthetic tests; transition semantics are unchanged.

The existing live worker and recovery CLI are intentionally outside this task. Until separately adapted to typed summary and exact lookup, their legacy status call fails closed on a migrated v2 queue. Publishing this SDK source does not permit or perform migration. Actual production adoption still needs stopped-owner/exact-prefix proof, physical capacity/restart testing, prior-binary refusal verification and measured resident memory. The current512MiB service limit has not been validated for the projected permanent history; the2M-binding format ceiling is not a memory guarantee.

## Verification

The new source is currently uncompiled. Planned focused checks cover missing/inconsistent summary fields, unchanged empty/v1 selection, explicit v2/old-reader refusal, unrelated global unknowns, exact recovery and retained bindings, active-write restart,429 backoff, immutable read profiles, rejected reseeding, preserving capacity maintenance, failed pin, malformed selected generations and bounded no-append snapshots with10k retained bindings. Loopback-only Transport tests exercise a normal v2 read and place, global unknown blocking before credentials/HTTP, exact recovery and a duplicate intent without another HTTP request. No live API is called by these tests.
