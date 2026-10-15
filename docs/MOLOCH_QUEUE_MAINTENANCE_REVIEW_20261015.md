# Queue capacity maintenance review — October 15

[Task #128](https://github.com/GodlyDonuts/predictions-cup-2026/issues/128), Viserion/queue-maintenance-review-20261015, independently reviewed parent source `b8a2af78` and corrective follow-ups `bee51224` and `223e3148`. The review is read-only for the SDK and queue core. Parent [#120](https://github.com/GodlyDonuts/predictions-cup-2026/issues/120) owns implementation, compilation, queue recovery and production.

## Finding

The bounded local retry logic is sound: only `EXPLICIT_COMPACTION_REQUIRED` and `RECOVERY_COMPACTION_REQUIRED` trigger one preserving compaction and one replay of the identical **local queue command**. It does not repeat HTTP, recreate an intent, change an idempotency key, clear unknown acceptance or reset the rate budget. Follow-up223e3148 addresses the directory-durability finding for the production Unix path; no blocking source finding remains on that path. Compilation/tests and a healthy deployed cycle are separate evidence, owned by #120; reading the source does not establish those outcomes.

V14's reported09:25:31UTC failure occurred before cancellation HTTP with queue activeNone/unknown{} and an active journal of67,104,068 bytes near the64MiB cap. The existing every20cycles compaction policy can be missed across restarts. That observation motivates local storage maintenance; it does not prove that a cancellation happened or eliminate later reconciliation.

## Why retry is bounded and preserves state

- `Store::command` applies ordinary operations to a cloned candidate. `Store::persist` checks the serialized record against the active journal cap before append, fsync, sequence advance or assigning the candidate to `self.state`. A normal cap rejection therefore has not admitted, network-fenced, completed or reconciled anything durably or in the original in-memory state.
- The wrapper retains the same `Store`. A rejected post-HTTP completion can compact the existing active, network-started write and then durably record its original completion. Reopening instead would conservatively recover that write as unknown and would not be an equivalent local completion retry.
- Recovery-at-cap is deliberately different: `Store::open` has already installed the crash-conservative recovered state in memory and marked `recovery_compaction_required`. Compaction must checkpoint that recovered state, including unknown bindings, before retrying the originally requested operation. An unknown write still blocks a new write afterward.
- Existing checked compaction preserves the full state, anchored sequence/previous hash, archived source length/SHA256 and prior checkpoint hash. Rate timestamps, completion gap, cooldown, read profile, manual-review flag, active request, permanent intent/key bindings and unknowns remain. It does not create a new scope or lease.
- The wrapper explicitly excludes the original `compact` operation, matches only the two exact capacity error strings, calls the raw compact method and raw original command, and does not recurse. A record that remains too large for a fresh journal fails after one generation; there is no maintenance loop.
- Lease loss, poisoned durability, source-journal changes, checkpoint/manifest failures and ordinary admission/authorization errors propagate. An error in compaction prevents the original command replay. The original manual CLI/Store command semantics are unchanged; only SDK `Transport::command` opts in.

The physical transport still rechecks authority/permit/STOP after the local network-fence command and immediately before HTTP. Compaction does not grant additional execution authority. If local completion cannot become durable, existing completion-uncertainty handling still stops the transport.

## Clock-regression edge found and corrected

An ordinary clock-regression command sets manual review on its candidate and tries to persist that poison. If the cap rejects that append, the caller sees the capacity error before the clock error. Compaction using the regressed timestamp would itself fail its clock check, leaving the poison unrecorded.

Follow-up `bee51224` uses `max(original_command.now, store.state.last_clock)` for the maintenance command only. The original command is retried unchanged, fails again as a clock regression, and now durably preserves manual review. Maintenance itself does not advance the queue's state clock. The reviewed regression checks the error, in-memory poison and restart persistence.

## Directory-durability qualification

The reused queue core synchronizes checkpoint, journal and manifest file data, but the reviewed `compact_with_hook` does not synchronize the containing directory after the manifest rename. File synchronization alone does not guarantee durable directory entries; Linux documents the separate directory operation in [fsync(2)](https://man7.org/linux/man-pages/man2/fsync.2.html). Process-crash fault tests do not test this power-loss boundary. Inference from that source and the current implementation: a rolled-back existing manifest could select an older generation even after an intent was durably appended to the new generation.

Follow-up223e3148 retains the unchanged core and synchronizes the queue directory on Unix immediately after raw compaction, BEFORE retrying any original command. The explicit SDK compact path also pins the directory before returning success. A pin failure poisons the same Store and returns `COMPACTION_DURABILITY_FAILURE`; no original local command is replayed. Subsequent commands fail closed on the poisoned Store. Two injected-error tests exercise automatic and explicit compaction. The non-Unix branch deliberately keeps a directory no-op and therefore does not establish this power-loss guarantee on those platforms; production is Unix.

## Test review and remaining verification

The parent candidate contains nine focused maintenance tests covering admission at cap; network-marker compaction plus crash recovery; post-HTTP completion without a second network admission; recovery-cap unknown preservation; lease/durability errors without maintenance; clock poison at cap; a single record that cannot fit even after one rotation; and failed directory pins in automatic and explicit compaction. Existing queue-core tests independently cover active-write compaction, unknown/intents/key retention, source anchors, compaction fault points, ownership loss, manifest/checkpoint integrity and restart recovery. Parent subsequently confirmed102 SDK and78 runtime checks plus the locked release build passed with223e3148; V15 was observed healthy at09:50:18UTC. The reviewer did not run account requests or mutate the live queue.

Useful further integration checks are a direct first-command admission after recovery-at-cap (unknown remains blocking), a completion whose preserved outcome is unknown, and full state equality including nonzero cooldown/read profile across maintenance. These are coverage recommendations, not evidence of a source blocker in the reviewed bounded wrapper. No account client, HTTP call, queue mutation or live process was run by this review task.
